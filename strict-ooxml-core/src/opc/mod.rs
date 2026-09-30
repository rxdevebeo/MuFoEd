//! OPC package layer.
//!
//! Provides access to a `.docx` package: the ZIP reader, the
//! `[Content_Types].xml` index, the relationship graph, target-path
//! canonicalization and conformance detection (`TZ-STRICT-OOXML-RUST.md` §8;
//! stage task S1.8).

pub mod content_types;
pub mod path;
pub mod rels;
pub mod zip;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

use crate::error::{LimitKind, Result, SourceLocation, StrictError};
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
    main_document: PartId,
}

impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package")
            .field("conformance", &self.conformance)
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

        let content_types_id = PartId::new(CONTENT_TYPES_PART);
        if zip.entry(&content_types_id).is_none() {
            return Err(StrictError::MissingPart(content_types_id));
        }
        let content_types_bytes = apply_normalizer(
            normalizer,
            &content_types_id,
            read_part(&zip, &content_types_id, &options.limits)?,
        )?;
        let content_types =
            ContentTypeIndex::parse(content_types_bytes, content_types_id, &options.limits)?;

        let mut rels = RelationshipGraph::new();
        for entry in zip.entries() {
            if entry.id.as_str().ends_with(".rels") {
                let bytes = apply_normalizer(
                    normalizer,
                    &entry.id,
                    read_part(&zip, &entry.id, &options.limits)?,
                )?;
                let source = source_part_for_rels(&entry.id);
                rels.add(
                    source.clone(),
                    parse_relationships(bytes, &source, &options.limits)?,
                );
            }
        }

        let main_document = locate_main_document(&rels, &zip)?;

        let conformance = detect(
            &zip,
            &content_types,
            &rels,
            &main_document,
            &options.limits,
            normalizer,
        )?;
        enforce_policy(conformance, options, &main_document)?;

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

    /// Returns the detected conformance of the package.
    #[must_use]
    pub fn conformance(&self) -> Conformance {
        self.conformance
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
        apply_normalizer(
            self.normalizer.as_deref(),
            id,
            read_part(&self.zip, id, &self.limits)?,
        )
    }
}

/// Applies an optional raw normalizer to a part's bytes.
fn apply_normalizer(
    normalizer: Option<&dyn RawNormalizer>,
    part: &PartId,
    bytes: Vec<u8>,
) -> Result<Vec<u8>> {
    match normalizer {
        None => Ok(bytes),
        Some(normalizer) => {
            let replacement: Option<Vec<u8>> = match normalizer.normalize_part(part, &bytes)? {
                Cow::Borrowed(_) => None,
                Cow::Owned(owned) => Some(owned),
            };
            match replacement {
                Some(owned) => Ok(owned),
                None => Ok(bytes),
            }
        }
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

/// Gathers conformance signals and detects the package conformance.
fn detect(
    zip: &ZipArchive,
    content_types: &ContentTypeIndex,
    rels: &RelationshipGraph,
    main_document: &PartId,
    limits: &ResourceLimits,
    normalizer: Option<&dyn RawNormalizer>,
) -> Result<Conformance> {
    use rels::RelType;
    let mut namespaces: Vec<String> = Vec::new();
    if let Some(ns) = root_namespace(zip, main_document, limits, normalizer)? {
        namespaces.push(ns);
    }
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
            if let Some(ns) = root_namespace(zip, part, limits, normalizer)? {
                namespaces.push(ns);
            }
        }
    }
    // Also inspect the main part's own related sub-parts.
    for rel in rels.relationships(main_document) {
        if matches!(
            rel.rel_type,
            RelType::Styles | RelType::Numbering | RelType::Settings
        ) {
            if let Some(part) = &rel.resolved {
                if let Some(ns) = root_namespace(zip, part, limits, normalizer)? {
                    namespaces.push(ns);
                }
            }
        }
    }

    let relationship_types: Vec<&str> = rels.iter().map(|rel| rel.raw_type.as_str()).collect();
    let signals = ConformanceSignals {
        namespaces: namespaces.iter().map(String::as_str).collect(),
        relationship_types,
        content_types: Some(content_types),
    };
    detect_conformance(&signals)
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

/// Returns the namespace URI a part's root element has **after** any
/// configured normalization, if any.
///
/// Only a bounded prefix of the part is decompressed; the whole part is read
/// solely as a fallback when the prefix is inconclusive (rework R7).
///
/// A 64 KiB prefix cut lands mid-tag by construction and is not valid XML, so
/// it is never handed to a rewriter. What the caller needs from a prefix is
/// only "which namespace will this part be", and the registry answers that
/// exactly: with a normalizer installed a Transitional URI maps to its Strict
/// twin, and without one nothing is mapped. Either way the answer is a single
/// consistent family, which is what keeps detection from seeing Strict and
/// Transitional in the same pass.
fn root_namespace(
    zip: &ZipArchive,
    id: &PartId,
    limits: &ResourceLimits,
    normalizer: Option<&dyn RawNormalizer>,
) -> Result<Option<String>> {
    if zip.entry(id).is_none() {
        return Ok(None);
    }
    let project = |uri: String| {
        if normalizer.is_none() {
            return uri;
        }
        match crate::ns::registry::NamespaceRegistry::global().lookup(&uri) {
            Some(entry) => entry.strict.map_or_else(|| uri.clone(), str::to_owned),
            None => uri,
        }
    };
    let prefix = read_prefix(zip, id, limits, ROOT_NS_PREFIX_BYTES)?;
    match scan_root_namespace(prefix, id, limits) {
        Ok(RootNamespace::Found(uri)) => Ok(uri.map(project)),
        Ok(RootNamespace::Incomplete) | Err(_) => {
            let bytes = apply_normalizer(normalizer, id, read_part(zip, id, limits)?)?;
            match scan_root_namespace(bytes, id, limits)? {
                RootNamespace::Found(uri) => Ok(uri.map(project)),
                RootNamespace::Incomplete => Ok(None),
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

/// Applies the conformance policy to a detected conformance.
fn enforce_policy(
    conformance: Conformance,
    options: &OpenOptions,
    main_document: &PartId,
) -> Result<()> {
    match conformance {
        // `Mixed` is T0 observing a package that has *not* been normalized
        // yet, so under a normalizing policy it is not a contradiction, it is
        // the expected starting state: a Transitional document whose parts
        // have been rewritten to different degrees. Normalization resolves it
        // by mapping every registered URI to one family. Rejecting it here
        // would make the normalizing policy unable to open a single real
        // document, which is what it exists for.
        Conformance::Mixed if options.normalization.is_some() => Ok(()),
        Conformance::Mixed => Err(StrictError::MixedConformance {
            detail: "both Strict and Transitional signals were detected".to_owned(),
        }),
        Conformance::Transitional => match options.conformance {
            ConformancePolicy::StrictOnly => Err(StrictError::TransitionalNotSupported {
                location: SourceLocation::new(main_document.clone(), 1, 1, 0),
            }),
            ConformancePolicy::Normalize if options.normalization.is_none() => {
                Err(StrictError::Unsupported(
                    "Normalize policy requires a RawNormalizer (implemented in Stage 6)".to_owned(),
                ))
            }
            ConformancePolicy::Normalize | ConformancePolicy::Permissive => Ok(()),
        },
        Conformance::Strict | Conformance::Unknown => Ok(()),
    }
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
