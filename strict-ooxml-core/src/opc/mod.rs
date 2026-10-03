//! OPC package layer.
//!
//! Provides access to a `.docx` package: the ZIP reader, the
//! `[Content_Types].xml` index, the relationship graph, target-path
//! canonicalization and conformance detection (`TZ-STRICT-OOXML-RUST.md` §8;
//! stage task S1.8).

pub mod content_types;
pub mod path;
pub mod policy;
pub mod rels;
pub mod zip;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{LimitKind, Result, StrictError};
use crate::limits::ResourceLimits;
use crate::normalize::RawNormalizer;
use crate::ns::detect::{detect_conformance, ConformanceSignals};
use crate::ns::Conformance;
use crate::part::{Part, PartId, PartSource};
use content_types::ContentTypeIndex;
use rels::{parse_relationships, source_part_for_rels, Relationship, RelationshipGraph};
use zip::ZipArchive;

/// Absolute id of the content-types part.
pub const CONTENT_TYPES_PART: &str = "/[Content_Types].xml";

/// How the package should be interpreted with respect to Strict conformance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConformancePolicy {
    /// Accept Strict only; reject Transitional and Mixed packages.
    StrictOnly,
    /// Read Transitional packages through the configured normalizer.
    Normalize,
    /// Normalize where possible and continue, recording problems.
    Permissive,
}

/// Options controlling how a package is opened.
pub struct OpenOptions {
    /// Conformance policy (default [`ConformancePolicy::StrictOnly`]).
    pub conformance: ConformancePolicy,
    /// Resource limits (default [`ResourceLimits::default`]).
    pub limits: ResourceLimits,
    /// Optional raw normalizer, the Stage-6 extension point.
    ///
    /// Shared as an [`Arc`] so an opened [`Package`] can keep using it after
    /// `OpenOptions` is dropped.
    pub normalization: Option<Arc<dyn RawNormalizer>>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            conformance: ConformancePolicy::StrictOnly,
            limits: ResourceLimits::default(),
            normalization: None,
        }
    }
}

impl OpenOptions {
    /// Sets the conformance policy.
    #[must_use]
    pub fn conformance(mut self, policy: ConformancePolicy) -> Self {
        self.conformance = policy;
        self
    }

    /// Sets the resource limits.
    #[must_use]
    pub fn limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets the raw normalizer.
    #[must_use]
    pub fn normalization<N: RawNormalizer + 'static>(mut self, normalizer: N) -> Self {
        self.normalization = Some(Arc::new(normalizer));
        self
    }

    /// Installs a normalizer the caller keeps a handle to.
    ///
    /// [`Self::normalization`] moves the value into an `Arc`, which loses the
    /// caller's only way to read the loss report — and the report is the whole
    /// point of normalizing. This takes the `Arc` instead, so the caller reads
    /// the same instance the package will.
    #[must_use]
    pub fn shared_normalization(mut self, normalizer: Arc<dyn RawNormalizer>) -> Self {
        self.normalization = Some(normalizer);
        self
    }
}

/// An opened OPC package.
pub struct Package {
    zip: ZipArchive,
    content_types: ContentTypeIndex,
    rels: RelationshipGraph,
    parts: Vec<Part>,
    part_by_id: HashMap<PartId, usize>,
    limits: ResourceLimits,
    normalizer: Option<Arc<dyn RawNormalizer>>,
    conformance: Conformance,
    /// Whether a `RawNormalizer` has changed any part read so far (AUD-23 /
    /// ADR-0016): set the moment opening or reading touches one, so it is
    /// already meaningful right after `open_*` returns, not only once a
    /// caller has read every part.
    ///
    /// `AtomicBool` rather than `Cell<bool>` because `read_part` takes `&self`
    /// and `strict-ooxml-wml`'s `feature = "parallel"` path reads parts of the
    /// same `Package` from two `rayon::join` threads at once, which requires
    /// `Package: Sync`.
    was_normalized: AtomicBool,
    main_document: PartId,
}

impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package")
            .field("conformance", &self.conformance)
            .field(
                "was_normalized",
                &self.was_normalized.load(Ordering::Relaxed),
            )
            .field("parts", &self.parts.len())
            .field("main_document", &self.main_document)
            .finish_non_exhaustive()
    }
}

impl Package {
    /// Opens a package from any reader.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] for damaged archives, missing required parts,
    /// resource-limit violations or a conformance policy mismatch.
    pub fn open_reader<R: Read>(mut reader: R, options: &OpenOptions) -> Result<Self> {
        let limit = options.limits.max_compressed_input;
        let mut data = Vec::new();
        reader
            .by_ref()
            .take(limit.saturating_add(1))
            .read_to_end(&mut data)
            .map_err(StrictError::Io)?;
        if data.len() as u64 > limit {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::CompressedInput,
                limit,
                actual: data.len() as u64,
            });
        }
        Self::open_archive(Arc::new(data), options)
    }

    /// Opens a package from a filesystem path.
    ///
    /// # Errors
    ///
    /// See [`Package::open_reader`].
    pub fn open_path(path: impl AsRef<Path>, options: &OpenOptions) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(StrictError::Io)?;
        Self::open_reader(file, options)
    }

    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    fn open_archive(data: Arc<Vec<u8>>, options: &OpenOptions) -> Result<Self> {
        let zip = ZipArchive::new(data, &options.limits)?;
        let normalizer = options.normalization.as_deref();
        let mut was_normalized = false;

        let content_types_id = PartId::new(CONTENT_TYPES_PART);
        if zip.entry(&content_types_id).is_none() {
            return Err(StrictError::MissingPart(content_types_id));
        }
        let (content_types_bytes, touched) = apply_normalizer(
            normalizer,
            &content_types_id,
            read_part(&zip, &content_types_id, &options.limits)?,
        )?;
        was_normalized |= touched;
        let content_types =
            ContentTypeIndex::parse(content_types_bytes, content_types_id, &options.limits)?;

        // T0 signal (AUD-23 / ADR-0016): the relationship-type URI exactly as
        // written, read from the **raw** bytes before any normalizer sees
        // them. `.rels` parts are small and few, so parsing each one twice —
        // raw for the signal below, (possibly) normalized for the graph the
        // rest of the package resolves against — is cheap.
        let mut rels = RelationshipGraph::new();
        let mut raw_relationship_types: Vec<String> = Vec::new();
        for entry in zip.entries() {
            if !entry.id.as_str().ends_with(".rels") {
                continue;
            }
            // AUD-25: only `<dir>/_rels/<name>.rels` is a relationship part.
            // A stray `*.rels` elsewhere stays an ordinary part (and is not
            // attributed to the package root).
            let Some(source) = source_part_for_rels(&entry.id) else {
                continue;
            };
            let raw_bytes = read_part(&zip, &entry.id, &options.limits)?;
            raw_relationship_types.extend(
                parse_relationships(raw_bytes.clone(), &source, &options.limits)?
                    .into_iter()
                    .map(|rel| rel.raw_type),
            );
            let (bytes, touched) = apply_normalizer(normalizer, &entry.id, raw_bytes)?;
            was_normalized |= touched;
            rels.add(
                source.clone(),
                parse_relationships(bytes, &source, &options.limits)?,
            );
        }
        // AUD-24: every resolved target so far carries whatever casing its
        // own `Target` attribute used; rewrite them all to the spelling the
        // ZIP central directory actually has before anything (including
        // `locate_main_document` below) reads a resolved `PartId`.
        rels.rewrite_resolved_to_zip_spelling(&zip);

        let main_document = locate_main_document(&rels, &zip)?;
        check_main_content_type(
            &content_types,
            &main_document,
            options.conformance,
            normalizer,
        )?;

        let (conformance, detection_touched) = detect(
            &zip,
            &raw_relationship_types,
            &rels,
            &main_document,
            &options.limits,
            normalizer,
        )?;
        was_normalized |= detection_touched;
        policy::decide(
            options.conformance,
            conformance,
            options.normalization.is_some(),
        )?;

        let parts = build_parts(&zip, &content_types);
        let mut part_by_id = HashMap::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            part_by_id.insert(part.id.clone(), index);
        }

        Ok(Self {
            zip,
            content_types,
            rels,
            parts,
            part_by_id,
            limits: options.limits,
            normalizer: options.normalization.clone(),
            conformance,
            was_normalized: AtomicBool::new(was_normalized),
            main_document,
        })
    }

    /// Iterates over all parts of the package.
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.parts.iter()
    }

    /// Returns a part by id.
    #[must_use]
    pub fn part(&self, id: &PartId) -> Option<&Part> {
        self.part_by_id
            .get(id)
            .and_then(|&index| self.parts.get(index))
    }

    /// Returns the content type of a part.
    #[must_use]
    pub fn content_type(&self, id: &PartId) -> Option<&str> {
        self.content_types.content_type_for(id)
    }

    /// Returns the relationships declared by `from`.
    #[must_use]
    pub fn relationships(&self, from: &PartId) -> &[Relationship] {
        self.rels.relationships(from)
    }

    /// Resolves a relationship by source part and id.
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::UnresolvedRelationship`] if the id is unknown.
    pub fn resolve_relationship(&self, from: &PartId, rel_id: &str) -> Result<&Relationship> {
        self.rels.resolve(from, rel_id)
    }

    /// Parts reachable from `from` by following internal relationships
    /// (AUD-25), not including `from` itself.
    ///
    /// Breadth-first, cycle-safe (a visited set). Edge depth is bounded by
    /// [`ResourceLimits::max_rel_depth`](crate::limits::ResourceLimits::max_rel_depth);
    /// a hop that would land past the bound returns
    /// [`StrictError::LimitExceeded`] with [`LimitKind::RelationshipDepth`].
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::LimitExceeded`] when the relationship graph is
    /// deeper than the configured bound.
    pub fn reachable_parts(&self, from: &PartId) -> Result<Vec<PartId>> {
        let mut visited = HashSet::new();
        let mut out = Vec::new();
        let mut queue = VecDeque::new();
        visited.insert(from.clone());
        queue.push_back((from.clone(), 0u32));
        while let Some((part, depth)) = queue.pop_front() {
            for rel in self.relationships(&part) {
                let Some(target) = rel.resolved.as_ref() else {
                    continue;
                };
                if !visited.insert(target.clone()) {
                    continue;
                }
                let next = depth.saturating_add(1);
                if next > self.limits.max_rel_depth {
                    return Err(StrictError::LimitExceeded {
                        kind: LimitKind::RelationshipDepth,
                        limit: u64::from(self.limits.max_rel_depth),
                        actual: u64::from(next),
                    });
                }
                out.push(target.clone());
                queue.push_back((target.clone(), next));
            }
        }
        Ok(out)
    }

    /// Returns the package's T0 (pre-normalization) conformance.
    ///
    /// Detected from raw signals — root namespaces read without projecting
    /// through the normalizer's registry, relationship types read from the
    /// `.rels` bytes before any normalizer touches them — so this never
    /// changes based on whether a normalizer happened to be installed
    /// (AUD-23 / ADR-0016). Use [`Package::was_normalized`] to find out
    /// whether the parts actually read so far came out Strict anyway.
    #[must_use]
    pub fn conformance(&self) -> Conformance {
        self.conformance
    }

    /// Returns whether a configured `RawNormalizer` has changed any part of
    /// this package read so far.
    ///
    /// `false` for a package opened with no normalizer, or with one that
    /// never had anything to rewrite. Already meaningful immediately after
    /// `open_*` returns: opening always reads `[Content_Types].xml`, every
    /// `.rels` part and the handful of parts conformance detection looks at,
    /// so a Transitional package normalized on the way in is already
    /// reflected here before a caller reads anything else (AUD-23 /
    /// ADR-0016).
    #[must_use]
    pub fn was_normalized(&self) -> bool {
        self.was_normalized.load(Ordering::Relaxed)
    }

    /// The [`NormalizationReport`](crate::normalize::NormalizationReport) from
    /// the installed normalizer, if any (AUD-31).
    ///
    /// `None` when no normalizer is installed or the normalizer keeps no
    /// report (`NoopNormalizer`). The caller that knows package-level
    /// detection should set `conformance_detected` on the returned value.
    #[must_use]
    pub fn normalization_report(&self) -> Option<crate::normalize::NormalizationReport> {
        self.normalizer
            .as_ref()
            .and_then(|normalizer| normalizer.report())
    }

    /// Returns the main document part id.
    ///
    /// # Errors
    ///
    /// Reserved for future use; currently always `Ok`.
    pub fn main_document_part(&self) -> Result<&PartId> {
        Ok(&self.main_document)
    }

    /// Returns the part source for this package.
    #[must_use]
    pub fn source(&self) -> &dyn PartSource {
        self
    }

    /// Reads a part's bytes fully, applying the configured raw normalizer and
    /// verifying size and CRC-32.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] for a missing part, a corrupt stream or a
    /// resource-limit violation.
    pub fn read_part(&self, id: &PartId) -> Result<Vec<u8>> {
        let (bytes, touched) = apply_normalizer(
            self.normalizer.as_deref(),
            id,
            read_part(&self.zip, id, &self.limits)?,
        )?;
        if touched {
            self.was_normalized.store(true, Ordering::Relaxed);
        }
        Ok(bytes)
    }
}

/// Applies an optional raw normalizer to a part's bytes.
///
/// Returns the bytes to use (unchanged if there was no normalizer or it left
/// the part alone) and whether the normalizer actually changed them —
/// callers fold that into [`Package::was_normalized`].
fn apply_normalizer(
    normalizer: Option<&dyn RawNormalizer>,
    part: &PartId,
    bytes: Vec<u8>,
) -> Result<(Vec<u8>, bool)> {
    match normalizer {
        None => Ok((bytes, false)),
        Some(normalizer) => match normalizer.normalize_part(part, &bytes)? {
            Cow::Borrowed(_) => Ok((bytes, false)),
            Cow::Owned(owned) => Ok((owned, true)),
        },
    }
}

impl PartSource for Package {
    fn open_part(&self, id: &PartId) -> Result<Box<dyn Read + '_>> {
        if self.normalizer.is_none() {
            return Ok(Box::new(self.zip.open_reader(id, &self.limits)?));
        }
        // A normalizer transforms whole parts, so materialize the normalized
        // bytes to keep `open_part` consistent with `read_part`.
        Ok(Box::new(std::io::Cursor::new(self.read_part(id)?)))
    }
}

/// Builds the part table from ZIP entries and the content-type index.
fn build_parts(zip: &ZipArchive, content_types: &ContentTypeIndex) -> Vec<Part> {
    zip.entries()
        .iter()
        .map(|entry| {
            let compression = entry.compression;
            Part {
                id: entry.id.clone(),
                content_type: content_types.content_type_for(&entry.id).map(Arc::from),
                compressed_size: entry.compressed_size,
                uncompressed_size: entry.uncompressed_size,
                compression,
            }
        })
        .collect()
}

/// Locates the main document part through the package-root relationship.
fn locate_main_document(rels: &RelationshipGraph, zip: &ZipArchive) -> Result<PartId> {
    use rels::RelType;
    let root = PartId::new("/");
    let target = rels
        .relationships(&root)
        .iter()
        .find(|rel| rel.rel_type == RelType::OfficeDocument)
        .and_then(|rel| rel.resolved.clone())
        .ok_or_else(|| {
            StrictError::InvalidZip("package has no officeDocument relationship".to_owned())
        })?;
    if zip.entry(&target).is_none() {
        return Err(StrictError::MissingPart(target));
    }
    Ok(target)
}

/// `WordprocessingML` main-part content types accepted by AUD-26.
const MAIN_CONTENT_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
    "application/vnd.ms-word.document.macroEnabled.main+xml",
    "application/vnd.ms-word.template.macroEnabled.main+xml",
];

/// Validates the main document's content type (AUD-26).
///
/// Under [`ConformancePolicy::StrictOnly`] an unexpected MIME is a hard error.
/// Under `Normalize`/`Permissive` with a normalizer, the open continues and the
/// normalizer records `T2.content-type`. Without a normalizer the open still
/// continues (the inspect path under `Permissive`).
fn check_main_content_type(
    content_types: &ContentTypeIndex,
    main: &PartId,
    policy: ConformancePolicy,
    normalizer: Option<&dyn RawNormalizer>,
) -> Result<()> {
    let content_type = content_types.content_type_for(main).unwrap_or("");
    if MAIN_CONTENT_TYPES.contains(&content_type) {
        return Ok(());
    }
    if policy == ConformancePolicy::StrictOnly {
        return Err(StrictError::UnexpectedContentType {
            part: main.clone(),
            content_type: content_type.to_owned(),
        });
    }
    if let Some(normalizer) = normalizer {
        normalizer.note_unexpected_main_content_type(main, content_type);
    }
    Ok(())
}

/// Gathers T0 conformance signals and detects the package conformance.
///
/// Returns the detected [`Conformance`] and whether `normalizer`, if any,
/// changed any of the parts this function reads — folded into
/// [`Package::was_normalized`] by the caller.
///
/// `rels` (the graph built from *normalized* bytes) is used only to find
/// *which part* is `styles.xml`/`numbering.xml`/etc.: normalization rewrites
/// a relationship's `Type`, never its `Target`, so the resolved [`PartId`] is
/// the same either way. The namespace signal itself always comes from
/// [`raw_root_namespace`], which never projects through the normalizer's
/// registry (AUD-23 / ADR-0016 — this is the fix for the regression where a
/// normalizer installed for a *different* package made every package detect
/// as `Strict`).
fn detect(
    zip: &ZipArchive,
    raw_relationship_types: &[String],
    rels: &RelationshipGraph,
    main_document: &PartId,
    limits: &ResourceLimits,
    normalizer: Option<&dyn RawNormalizer>,
) -> Result<(Conformance, bool)> {
    use rels::RelType;
    let mut namespaces: Vec<String> = Vec::new();
    let mut touched = false;

    let mut probe = |id: &PartId| -> Result<()> {
        let (ns, this_touched) = raw_root_namespace(zip, id, limits, normalizer)?;
        if let Some(ns) = ns {
            namespaces.push(ns);
        }
        touched |= this_touched;
        Ok(())
    };

    probe(main_document)?;
    let root = PartId::new("/");
    for rel in rels.relationships(&root) {
        let part = match rel.rel_type {
            RelType::Styles
            | RelType::Numbering
            | RelType::Settings
            | RelType::FontTable
            | RelType::Theme => rel.resolved.as_ref(),
            _ => None,
        };
        if let Some(part) = part {
            probe(part)?;
        }
    }
    // Also inspect the main part's own related sub-parts.
    for rel in rels.relationships(main_document) {
        if matches!(
            rel.rel_type,
            RelType::Styles | RelType::Numbering | RelType::Settings
        ) {
            if let Some(part) = &rel.resolved {
                probe(part)?;
            }
        }
    }

    let signals = ConformanceSignals {
        namespaces: namespaces.iter().map(String::as_str).collect(),
        relationship_types: raw_relationship_types.iter().map(String::as_str).collect(),
    };
    Ok((detect_conformance(&signals)?, touched))
}

/// How many bytes of a part to stream when looking for its root namespace.
const ROOT_NS_PREFIX_BYTES: usize = 64 * 1024;

/// Outcome of scanning a part prefix for its root namespace.
enum RootNamespace {
    /// The root element was seen; carries its namespace URI (if any).
    Found(Option<String>),
    /// The prefix ended before the root element was complete.
    Incomplete,
}

/// Returns a part's root-element namespace **exactly as written** (T0, never
/// projected through the normalizer's registry), and whether `normalizer`
/// would change the part's bytes at all.
///
/// Without a normalizer this streams a bounded prefix and falls back to the
/// whole part only when the prefix is inconclusive (rework R7) — a 64 KiB
/// prefix cut lands mid-tag by construction, but `scan_root_namespace` only
/// needs to see the root's own start tag, which is always within it in
/// practice.
///
/// With a normalizer, the whole part is read unconditionally: the second
/// return value asks the normalizer whether it would rewrite this part at
/// all, and a normalizer parses and rewrites whole, well-formed XML, which a
/// mid-tag prefix is not. The namespace scan runs on those same raw bytes —
/// before whatever the normalizer would do to them — so the signal stays T0
/// either way.
fn raw_root_namespace(
    zip: &ZipArchive,
    id: &PartId,
    limits: &ResourceLimits,
    normalizer: Option<&dyn RawNormalizer>,
) -> Result<(Option<String>, bool)> {
    if zip.entry(id).is_none() {
        return Ok((None, false));
    }
    if let Some(normalizer) = normalizer {
        let bytes = read_part(zip, id, limits)?;
        let touched = matches!(normalizer.normalize_part(id, &bytes)?, Cow::Owned(_));
        let ns = match scan_root_namespace(bytes, id, limits)? {
            RootNamespace::Found(uri) => uri,
            RootNamespace::Incomplete => None,
        };
        return Ok((ns, touched));
    }
    let prefix = read_prefix(zip, id, limits, ROOT_NS_PREFIX_BYTES)?;
    match scan_root_namespace(prefix, id, limits) {
        Ok(RootNamespace::Found(uri)) => Ok((uri, false)),
        Ok(RootNamespace::Incomplete) | Err(_) => {
            let bytes = read_part(zip, id, limits)?;
            match scan_root_namespace(bytes, id, limits)? {
                RootNamespace::Found(uri) => Ok((uri, false)),
                RootNamespace::Incomplete => Ok((None, false)),
            }
        }
    }
}

/// Scans for the first start element and returns its namespace.
fn scan_root_namespace(
    bytes: Vec<u8>,
    id: &PartId,
    limits: &ResourceLimits,
) -> Result<RootNamespace> {
    let mut reader = crate::xml::XmlReader::from_vec(bytes, id.clone(), limits)?;
    loop {
        match reader.next_event()? {
            crate::xml::XmlEvent::StartElement { name, .. } => {
                return Ok(RootNamespace::Found(
                    name.ns.map(|ns| ns.as_str().to_owned()),
                ));
            }
            crate::xml::XmlEvent::Eof => return Ok(RootNamespace::Incomplete),
            _ => {}
        }
    }
}

/// Streams up to `limit` bytes of a part without materializing the rest.
fn read_prefix(
    zip: &ZipArchive,
    id: &PartId,
    limits: &ResourceLimits,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut reader = zip.open_reader(id, limits)?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    while buffer.len() < limit {
        let want = (limit - buffer.len()).min(chunk.len());
        let read = reader.read(&mut chunk[..want]).map_err(map_read_error)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(buffer)
}

/// Reads a part fully, mapping stream errors to package errors.
fn read_part(zip: &ZipArchive, id: &PartId, limits: &ResourceLimits) -> Result<Vec<u8>> {
    let mut reader = zip.open_reader(id, limits)?;
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).map_err(map_read_error)?;
    Ok(buffer)
}

/// Maps a stream error into a package-level error.
fn map_read_error(error: io::Error) -> StrictError {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
            StrictError::InvalidZip(error.to_string())
        }
        _ => StrictError::Io(error),
    }
}
