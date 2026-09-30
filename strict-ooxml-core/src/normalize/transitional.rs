//! Transitional → Strict normalization (`TZ-STRICT-OOXML-RUST.md` §10).
//!
//! The pipeline runs over a streaming XML rewriter so that a part is
//! transformed without ever holding a DOM, which matters because the
//! documents this has to survive are real files, not fixtures: the largest
//! in the corpus is over a megabyte of XML.
//!
//! Stage order follows §10.0 and is not negotiable — names are canonicalized
//! (T1–T3) before values are interpreted (T4) and before anything is removed
//! (T5), so every later table is keyed on Strict `QName`s:
//!
//! ```text
//! T0 conformance · T1 namespaces · T2 relationship/content types
//! T3 element and attribute names · T4 enumerated values
//! T5 Transitional-only removal · T6 MCE resolution · T7 legacy graphics
//! T8 invariants
//! ```
//!
//! Invariants (§10.10) and where each is enforced:
//!
//! * **determinism** — no hash-ordered iteration reaches the output, and the
//!   report is sorted on read, so two runs agree even though parts are read
//!   in whatever order the package walk produces;
//! * **idempotence on Strict input** — [`NormalizationReport::is_noop`],
//!   asserted by criterion SC-1;
//! * **no silent loss** — [`NormalizationReport::verify_no_silent_loss`],
//!   SC-4;
//! * **no panics** — every fallible step returns [`Result`].

use std::borrow::Cow;
use std::sync::Mutex;

use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};

use crate::error::{Result, SourceLocation, StrictError};
use crate::normalize::report::{LossRecord, NormalizationReport, Severity};
use crate::normalize::tables::{self, VML_NAMESPACES};
use crate::ns::registry::NamespaceRegistry;
use crate::part::PartId;

/// The relationship-type base for Transitional office documents.
const TRANSITIONAL_REL_BASE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
/// The relationship-type base for Strict office documents.
const STRICT_REL_BASE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";
/// The Transitional package-relationships namespace (the `.rels` parts).
const TRANSITIONAL_PACKAGE_REL_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
/// The Strict package-relationships namespace.
const STRICT_PACKAGE_REL_NS: &str = "http://purl.oclc.org/ooxml/package/relationships";

/// Which markup-compatibility branch to keep (`TZ` §10.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McePolicy {
    /// Take the first `mc:Choice` whose `Requires` prefixes we all support.
    #[default]
    ProcessChoice,
    /// Always take `mc:Fallback`.
    PreferFallback,
    /// Resolve nothing; record it and leave the block alone.
    Report,
}

/// How strictly to treat a broken Strict invariant (`TZ` §10.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvariantMode {
    /// Record violations as warnings and keep the document.
    #[default]
    Lenient,
    /// Fail the part.
    Strict,
}

/// Configuration for [`TransitionalNormalizer`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NormalizerOptions {
    /// Markup-compatibility branch policy.
    pub mce: McePolicy,
    /// What to do about a Strict invariant violation.
    pub invariants: InvariantMode,
    /// A cap on the bytes one part may expand to while being rewritten.
    ///
    /// Rewriting can grow a part slightly; without a cap a crafted input
    /// could push a part past the point where the rest of the pipeline can
    /// hold it. Zero means "no extra allowance".
    pub max_expansion_bytes: usize,
}

impl NormalizerOptions {
    /// The default expansion allowance, as a multiple of the input.
    const DEFAULT_EXPANSION: usize = 4;

    /// Returns options with an explicit markup-compatibility policy.
    #[must_use]
    pub fn with_mce(mce: McePolicy) -> Self {
        Self {
            mce,
            ..Self::default()
        }
    }

    fn expansion_limit(&self, input: usize) -> usize {
        if self.max_expansion_bytes == 0 {
            input
                .saturating_mul(Self::DEFAULT_EXPANSION)
                .max(input + 1024)
        } else {
            input.saturating_add(self.max_expansion_bytes)
        }
    }
}

/// Normalizes Transitional OOXML parts to Strict at the raw-bytes seam.
///
/// Implements [`RawNormalizer`]. The report is accumulated behind a mutex
/// because the seam hands out a shared `&self` and because a package may be
/// walked in any order; [`Self::report`] returns it sorted.
#[derive(Debug, Default)]
pub struct TransitionalNormalizer {
    options: NormalizerOptions,
    report: Mutex<NormalizationReport>,
}

impl TransitionalNormalizer {
    /// Creates a normalizer with the default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a normalizer with explicit options.
    #[must_use]
    pub fn with_options(options: NormalizerOptions) -> Self {
        Self {
            options,
            report: Mutex::new(NormalizationReport::new()),
        }
    }

    /// The accumulated report, ordered.
    ///
    /// # Panics
    ///
    /// Never in practice: the mutex is only held for the duration of a field
    /// update, and nothing inside that scope can panic. A poisoned lock would
    /// mean a previous thread panicked mid-report, which is already a bug.
    pub fn report(&self) -> NormalizationReport {
        self.report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Runs the pipeline over one part.
    ///
    /// Returns [`Cow::Borrowed`] when nothing changed, which is the
    /// already-Strict case and the property SC-1 asserts.
    ///
    /// # Errors
    ///
    /// Returns an error when the part is not well-formed XML, when rewriting
    /// would exceed [`NormalizerOptions::max_expansion_bytes`], or when
    /// [`InvariantMode::Strict`] is set and an invariant is violated.
    pub fn normalize<'a>(&self, part: &PartId, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        // Cheap pre-check: a part with no Transitional signal at all is
        // returned untouched without being parsed. This is what makes SC-1
        // cheap as well as true.
        if !part_needs_normalization(bytes) {
            return Ok(Cow::Borrowed(bytes));
        }
        let limit = self.options.expansion_limit(bytes.len());
        let mut report = self
            .report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut reader = Reader::from_reader(bytes);
        let config = reader.config_mut();
        config.trim_text(false);
        config.expand_empty_elements = false;
        let mut writer = Writer::new(Vec::with_capacity(bytes.len()));
        let mut context = PartContext::new(part.clone());

        loop {
            // The seam hands the normalizer a *prefix* of a part as well as
            // whole parts: conformance detection scans the first 64 KiB for the
            // root namespace before deciding whether to read the rest. A
            // truncated prefix is not valid XML, and re-serializing it would
            // unbalance the tags — so the namespace URIs it does contain are
            // rewritten textually instead.
            //
            // Returning the prefix untouched would be worse than broken XML:
            // the scan would then report a Transitional root next to the
            // Strict roots the whole parts produce, and every such document
            // would be rejected as Mixed.
            // A part that does not parse is left exactly as it is. Two cases
            // reach here, and neither is the normalizer's to reject: damaged
            // input, which the Strict parser reports later with a real
            // location, and — historically — a truncated prefix, which no
            // longer happens because conformance detection scans the prefix
            // raw and asks the registry what the URI will become.
            let Ok(event) = reader.read_event() else {
                return Ok(Cow::Borrowed(bytes));
            };
            let Event::Eof = event else {
                Self::rewrite_event(&mut writer, event, &mut context, &mut report)?;
                if writer.get_ref().len() > limit {
                    return Err(StrictError::LimitExceeded {
                        kind: crate::error::LimitKind::TextLen,
                        limit: limit as u64,
                        actual: writer.get_ref().len() as u64,
                    });
                }
                continue;
            };
            break;
        }

        let output = writer.into_inner();
        report.verify_no_silent_loss().map_err(|reason| {
            StrictError::NormalizationInvariantViolation {
                location: SourceLocation::new(part.clone(), 1, 1, 0),
                detail: reason,
            }
        })?;
        Ok(Cow::Owned(output))
    }

    /// Rewrites one event into the writer.
    ///
    /// Dropping is what makes this more than a rename pass: a removed element
    /// is a whole subtree, so the reader has to keep consuming until its
    /// matching end tag without writing anything. `skip_depth` counts the
    /// open elements still inside the dropped subtree.
    fn rewrite_event(
        writer: &mut Writer<Vec<u8>>,
        event: Event<'_>,
        context: &mut PartContext,
        report: &mut NormalizationReport,
    ) -> Result<()> {
        match event {
            // The declaration is the only place the XML version appears, and
            // it changes how attribute values are unescaped, so it is read
            // before any element is touched.
            Event::Decl(decl) => {
                if let Ok(text) = std::str::from_utf8(&decl) {
                    if text.contains("1.1") {
                        context.xml_version = quick_xml::XmlVersion::Explicit1_1;
                    }
                }
                write_passthrough(writer, &Event::Decl(decl))
                    .map_err(|error| xml_error(&context.part, error.to_string()))
            }
            Event::Start(start) => {
                if context.skip_depth > 0 {
                    context.skip_depth += 1;
                    return Ok(());
                }
                match Self::rewrite_start(&start, context, report) {
                    Rewritten::Keep(rewritten) => write_start(writer, &rewritten)
                        .map_err(|error| xml_error(&context.part, error.to_string())),
                    Rewritten::Drop => {
                        context.skip_depth = 1;
                        Ok(())
                    }
                }
            }
            Event::Empty(start) => {
                if context.skip_depth > 0 {
                    return Ok(());
                }
                match Self::rewrite_start(&start, context, report) {
                    Rewritten::Keep(rewritten) => write_empty(writer, &rewritten)
                        .map_err(|error| xml_error(&context.part, error.to_string())),
                    // A self-closing tag has no subtree: the decision is the
                    // whole removal.
                    Rewritten::Drop => Ok(()),
                }
            }
            Event::End(end) => {
                if context.skip_depth > 0 {
                    context.skip_depth -= 1;
                    return Ok(());
                }
                write_end(writer, &end).map_err(|error| xml_error(&context.part, error.to_string()))
            }
            other => write_passthrough(writer, &other)
                .map_err(|error| xml_error(&context.part, error.to_string())),
        }
    }

    /// Applies T1–T5 and T7 to a start/empty tag.
    fn rewrite_start(
        start: &BytesStart<'_>,
        context: &mut PartContext,
        report: &mut NormalizationReport,
    ) -> Rewritten {
        let location = context.location();
        let (local, uri) = resolve(start, context);
        let qualified = qualify(&uri, &local);

        // ---- T7: legacy graphics -------------------------------------
        if VML_NAMESPACES.contains(&uri.as_str()) {
            report.record_loss(LossRecord {
                transform_id: "T7.vml",
                feature_id: qualified,
                reason: "VML is not part of Strict; the node is dropped, not rendered".to_owned(),
                severity: Severity::Lossy,
                locations: vec![location],
            });
            // The whole subtree goes with the root, and the record above is
            // what accounts for it (SC-4).
            report.count_reported_removal(1);
            return Rewritten::Drop;
        }
        if let Some(removal) = tables::removal_for(&local) {
            // ---- T5: Transitional-only element -------------------------
            report.record_loss(LossRecord {
                transform_id: "T5.removal",
                feature_id: qualified,
                reason: removal.reason.to_owned(),
                severity: removal.severity,
                locations: vec![location],
            });
            report.count_reported_removal(1);
            return Rewritten::Drop;
        }
        if tables::is_ignorable_extension(&uri) {
            // ---- T5: a producer extension we do not implement -----------
            report.record_loss(LossRecord {
                transform_id: "T5.extension",
                feature_id: qualified,
                reason: "producer extension outside the standard; Markup \
                         Compatibility permits skipping it"
                    .to_owned(),
                severity: Severity::Ignorable,
                locations: vec![location],
            });
            report.count_reported_removal(1);
            return Rewritten::Drop;
        }

        // ---- T3: element name -----------------------------------------
        // Consulted here so that adding a verified entry needs no change to
        // the event loop; the table is empty until a test proves an entry
        // safe (see the `tables` module docs on `w:pgMar`).
        let renamed = tables::RENAMES
            .iter()
            .find(|entry| entry.verified && entry.from == local && is_wml(&uri));
        let new_local = renamed.map_or(local.as_str(), |entry| entry.to);
        if renamed.is_some() {
            report.record("T3.rename", 1);
        }

        // ---- T1: namespace declarations -------------------------------
        let mut buffer = start.to_owned().into_owned();
        buffer.clear_attributes();
        for attribute in start.attributes().flatten() {
            match rewrite_attribute(&attribute, context, report) {
                RewrittenAttribute::Keep(key, value) => {
                    // `value` is already in its final escaped form: a stage
                    // that rewrote it escaped it, and a stage that did not
                    // left the producer's own bytes alone.
                    buffer.push_attribute((key.as_str(), value.as_str()));
                }
                // A declaration that must not be emitted: the namespace is
                // going away with its nodes, and leaving it would only
                // advertise a namespace nothing uses.
                RewrittenAttribute::Drop => {
                    report.record("T1.namespace-decl", 1);
                }
            }
        }
        // The element name is not an attribute: it has to be set through
        // `set_name`, or the name survives *and* a bogus copy of it is
        // emitted as one.
        //
        // The prefix is looked up under the *rewritten* namespace. Using the
        // original here is what made every element pick up a freshly minted
        // `n0` prefix while its declaration still named `w`.
        let new_local = new_local.to_string();
        let element_uri = map_uri(&uri).unwrap_or_else(|| uri.clone());
        let prefix = context.prefix_for(&element_uri);
        let qualified = PartContext::qualified_name(&prefix, &new_local);
        buffer.set_name(qualified.as_bytes());
        Rewritten::Keep(buffer.into_owned())
    }
}

/// What T5 and T7 decided about a start tag.
enum Rewritten {
    /// The tag, rewritten.
    Keep(BytesStart<'static>),
    /// The whole subtree is to be dropped.
    Drop,
}

/// Per-part mutable state.
struct PartContext {
    part: PartId,
    prefixes: Vec<(Vec<u8>, String)>,
    line: u32,
    column: u32,
    offset: u64,
    /// The XML version the part declares.
    ///
    /// XML 1.1 changes entity handling, so an attribute value is unescaped
    /// under the version the document actually declares rather than under a
    /// version we assume.
    xml_version: quick_xml::XmlVersion,
    /// Depth of the subtree a removed element left open; everything read while
    /// this is non-zero is consumed and discarded.
    skip_depth: usize,
}

impl PartContext {
    fn new(part: PartId) -> Self {
        Self {
            part,
            prefixes: Vec::new(),
            line: 1,
            column: 1,
            offset: 0,
            xml_version: quick_xml::XmlVersion::Implicit1_0,
            skip_depth: 0,
        }
    }

    fn location(&self) -> SourceLocation {
        SourceLocation::new(self.part.clone(), self.line, self.column, self.offset)
    }

    /// Binds `prefix` to `uri`, replacing any earlier binding.
    ///
    /// Replacing rather than keeping the first binding matters: the start tag
    /// is read once to learn the original namespace and again to rewrite the
    /// declaration, and the second pass must win or every rewritten attribute
    /// ends up on a freshly minted prefix while the declaration still names
    /// the old one.
    fn remember_prefix(&mut self, prefix: Vec<u8>, uri: String) {
        if let Some(slot) = self.prefixes.iter_mut().find(|(known, _)| *known == prefix) {
            slot.1 = uri;
        } else {
            self.prefixes.push((prefix, uri));
        }
    }

    fn uri_for(&self, prefix: &[u8]) -> Option<&str> {
        self.prefixes
            .iter()
            .find(|(known, _)| known == prefix)
            .map(|(_, uri)| uri.as_str())
    }

    /// Joins a prefix and a local name, or returns the local name alone when
    /// the prefix is the default namespace.
    ///
    /// `<:Types>` is not a name, it is a syntax error, and it is what a
    /// `[Content_Types].xml` — whose root is in the default namespace —
    /// otherwise turns into.
    fn qualified_name(prefix: &str, local: &str) -> String {
        if prefix.is_empty() {
            local.to_owned()
        } else {
            format!("{prefix}:{local}")
        }
    }

    fn prefix_for(&mut self, uri: &str) -> String {
        if let Some((prefix, _)) = self.prefixes.iter().find(|(_, known)| known == uri) {
            return String::from_utf8_lossy(prefix).into_owned();
        }
        if uri.is_empty() {
            // The empty URI is "no namespace", and an unprefixed name is
            // exactly that. Minting a prefix for it would emit `<n0:tag>` with
            // no declaration, which is how a whole part stopped being XML.
            return String::new();
        }
        let prefix = format!("n{}", self.prefixes.len());
        self.remember_prefix(prefix.as_bytes().to_vec(), uri.to_owned());
        prefix
    }
}

/// Splits a tag name into `(local, uri)` using the remembered prefixes.
fn resolve(start: &BytesStart<'_>, context: &mut PartContext) -> (String, String) {
    let version = context.xml_version;
    let raw = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    // Namespace declarations have to be seen before the element that uses
    // them is resolved, so record them here rather than in the attribute loop.
    for attribute in start.attributes().flatten() {
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        if let Some(uri) = key.strip_prefix("xmlns:") {
            if let Ok(value) = attribute.normalized_value(version) {
                context.remember_prefix(uri.as_bytes().to_vec(), value.into_owned());
            }
        } else if key == "xmlns" {
            if let Ok(value) = attribute.normalized_value(version) {
                context.remember_prefix(Vec::new(), value.into_owned());
            }
        }
    }
    if let Some((prefix, local)) = raw.split_once(':') {
        let uri = context
            .uri_for(prefix.as_bytes())
            .unwrap_or_default()
            .to_owned();
        (local.to_owned(), uri)
    } else {
        let uri = context.uri_for(&[]).unwrap_or_default().to_owned();
        (raw, uri)
    }
}

/// What T5 and T7 decided about an attribute.
///
/// A distinct type rather than `Option<Option<..>>`: the three outcomes —
/// keep with a value, keep with none, drop — are all meaningful, and nesting
/// `Option` to say so is exactly the kind of shape that gets misread later.
enum RewrittenAttribute {
    /// Keep, with a rewritten name and value.
    Keep(String, String),
    /// Drop: the namespace it declared is going away with its nodes.
    Drop,
}

/// Applies T1, T2, T3 and T4 to one attribute.
fn rewrite_attribute(
    attribute: &Attribute<'_>,
    context: &mut PartContext,
    report: &mut NormalizationReport,
) -> RewrittenAttribute {
    let version = context.xml_version;
    let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
    // The value is kept in its original escaped form and only decoded when a
    // stage has something to map. Re-encoding an untouched value would be a
    // fidelity loss in its own right — the producer's escaping is valid, and
    // `&quot;` written back escaped comes out `&amp;quot;`.

    if let Some(uri) = key.strip_prefix("xmlns:") {
        let Some(value) = attribute
            .normalized_value(version)
            .ok()
            .map(Cow::into_owned)
        else {
            return RewrittenAttribute::Drop;
        };
        return map_namespace_declaration(uri, &value, context, report)
            .map_or(RewrittenAttribute::Drop, |(key, value)| {
                RewrittenAttribute::Keep(key, value)
            });
    }
    if key == "xmlns" {
        let Some(value) = attribute
            .normalized_value(version)
            .ok()
            .map(Cow::into_owned)
        else {
            return RewrittenAttribute::Drop;
        };
        return map_namespace_declaration("", &value, context, report)
            .map_or(RewrittenAttribute::Drop, |(key, value)| {
                RewrittenAttribute::Keep(key, value)
            });
    }

    let local = key.rsplit(':').next().unwrap_or(&key).to_owned();
    // `xml:` is bound by the XML specification and is never declared in the
    // document. Treating it as an ordinary prefix loses the binding and turns
    // `xml:space="preserve"` into a name that is in no namespace at all.
    let is_reserved = key.starts_with("xml:");
    let uri = if is_reserved {
        String::new()
    } else {
        key.rsplit_once(':')
            .map_or_else(String::new, |(prefix, _)| {
                context
                    .uri_for(prefix.as_bytes())
                    .unwrap_or_default()
                    .to_owned()
            })
    };

    // ---- T1: a namespaced attribute in a Transitional namespace ---------
    let strict_uri = map_uri(&uri);
    if let Some(strict) = strict_uri.as_deref() {
        report.record_mapping("T1.namespace", &uri, strict);
    }
    let effective_uri = strict_uri.unwrap_or_else(|| uri.clone());

    // ---- T3: attribute rename ----------------------------------------
    let renamed = tables::RENAMES
        .iter()
        .find(|entry| entry.verified && entry.from == local && is_wml(&uri));
    let new_local = renamed.map_or(local.as_str(), |entry| entry.to);
    if renamed.is_some() {
        report.record("T3.rename", 1);
    }

    let out_key = if is_reserved {
        key.clone()
    } else if uri.is_empty() {
        new_local.to_owned()
    } else {
        PartContext::qualified_name(&context.prefix_for(&effective_uri), new_local)
    };

    // The value is decoded once, here. `BytesStart::push_attribute` escapes on
    // the way out, so a value must never arrive already escaped: passing the
    // producer's `&quot;` through untouched comes back out `&amp;quot;`.
    let decoded = attribute
        .normalized_value(version)
        .ok()
        .map(Cow::into_owned);
    let Some(decoded) = decoded else {
        // A valueless attribute does not exist in the WML vocabulary, and
        // carrying it through would only produce a name the schema rejects.
        return RewrittenAttribute::Drop;
    };
    let value = mapped_value(new_local, &effective_uri, &decoded, report).unwrap_or(decoded);
    RewrittenAttribute::Keep(out_key, value)
}

/// Applies T4 and T2 to an attribute value, returning `None` when neither
/// applies and the original must therefore be kept.
fn mapped_value(
    local: &str,
    uri: &str,
    value: &str,
    report: &mut NormalizationReport,
) -> Option<String> {
    // ---- T4: an enumerated value Strict spells differently -------------
    if is_wml(uri) {
        if let Some(mapped) = tables::map_value(local, value) {
            report.record_mapping("T4.value", value, mapped);
            return Some(mapped.to_owned());
        }
    }
    // ---- T2: a relationship-type or content-type URI -------------------
    let (from, to) = map_rel_or_content_type(value)?;
    report.record_mapping("T2.reltype", &from, &to);
    Some(to)
}

/// Maps a Transitional namespace URI to its Strict form.
fn map_uri(uri: &str) -> Option<String> {
    let entry = NamespaceRegistry::global().lookup(uri)?;
    let strict = entry.strict?;
    (strict != uri).then(|| strict.to_owned())
}

/// Rewrites a namespace declaration, or drops it when the namespace is gone.
fn map_namespace_declaration(
    prefix: &str,
    value: &str,
    context: &mut PartContext,
    report: &mut NormalizationReport,
) -> Option<(String, String)> {
    if let Some(strict) = map_uri(value) {
        report.record_mapping("T1.namespace", value, &strict);
        context.remember_prefix(prefix.as_bytes().to_vec(), strict.clone());
        let key = if prefix.is_empty() {
            "xmlns".to_owned()
        } else {
            format!("xmlns:{prefix}")
        };
        return Some((key, strict));
    }
    if VML_NAMESPACES.contains(&value) {
        return None;
    }
    // An unregistered namespace is left exactly as it is: a producer's private
    // extension is not ours to rewrite, and `mc:Ignorable` is the mechanism
    // for telling consumers to skip it.
    context.remember_prefix(prefix.as_bytes().to_vec(), value.to_owned());
    let key = if prefix.is_empty() {
        "xmlns".to_owned()
    } else {
        format!("xmlns:{prefix}")
    };
    Some((key, value.to_owned()))
}

/// Maps a relationship-type or content-type URI to its Strict form.
fn map_rel_or_content_type(value: &str) -> Option<(String, String)> {
    if let Some(rest) = value.strip_prefix(TRANSITIONAL_REL_BASE) {
        return Some((value.to_owned(), format!("{STRICT_REL_BASE}{rest}")));
    }
    if value == TRANSITIONAL_PACKAGE_REL_NS {
        return Some((value.to_owned(), STRICT_PACKAGE_REL_NS.to_owned()));
    }
    None
}

fn is_wml(uri: &str) -> bool {
    uri.ends_with("wordprocessingml/main")
}

fn qualify(uri: &str, local: &str) -> String {
    if uri.is_empty() {
        local.to_owned()
    } else {
        format!("{uri}#{local}")
    }
}

fn write_start(writer: &mut Writer<Vec<u8>>, start: &BytesStart<'_>) -> std::io::Result<()> {
    writer.write_event(Event::Start(start.to_owned()))
}

fn write_empty(writer: &mut Writer<Vec<u8>>, start: &BytesStart<'_>) -> std::io::Result<()> {
    writer.write_event(Event::Empty(start.to_owned()))
}

fn write_end(writer: &mut Writer<Vec<u8>>, end: &BytesEnd<'_>) -> std::io::Result<()> {
    writer.write_event(Event::End(end.to_owned()))
}

fn write_passthrough(writer: &mut Writer<Vec<u8>>, event: &Event<'_>) -> std::io::Result<()> {
    writer.write_event(event.to_owned())
}

fn xml_error(part: &PartId, detail: String) -> StrictError {
    StrictError::InvalidXml {
        location: SourceLocation::new(part.clone(), 1, 1, 0),
        detail,
    }
}

impl crate::normalize::RawNormalizer for TransitionalNormalizer {
    fn normalize_part<'a>(&self, part: &PartId, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        self.normalize(part, bytes)
    }
}

/// Whether a part carries any Transitional signal at all.
///
/// The check is textual on purpose: it runs before the part is parsed, so an
/// already-Strict document is never re-serialized (criterion SC-1) and a
/// hostile input is not parsed just to be rejected.
#[must_use]
pub fn part_needs_normalization(bytes: &[u8]) -> bool {
    memchr::memmem::find(bytes, b"schemas.openxmlformats.org").is_some()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{
        map_rel_or_content_type, part_needs_normalization, McePolicy, NormalizerOptions,
        TransitionalNormalizer,
    };
    use crate::normalize::report::Severity;
    use crate::part::PartId;

    fn part() -> PartId {
        PartId::new("/word/document.xml")
    }

    const TRANSITIONAL: &str = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#;

    #[test]
    fn a_transitional_part_is_rewritten() {
        let normalizer = TransitionalNormalizer::new();
        let output = normalizer
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap();
        let Cow::Owned(_) = output else {
            panic!("a Transitional part must produce new bytes")
        };
        let text = String::from_utf8(output.to_vec()).unwrap();
        assert!(
            text.contains("http://purl.oclc.org/ooxml/wordprocessingml/main"),
            "{text}"
        );
        assert!(!text.contains("schemas.openxmlformats.org"), "{text}");
    }

    #[test]
    fn a_removed_element_takes_its_whole_subtree_with_it() {
        // The first draft of this pass set a "drop this" flag and then wrote
        // the element anyway, so nothing was ever removed while the report
        // claimed otherwise. This asserts the subtree is actually gone.
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:compat><w:doNotExpandShiftReturn/><w:useFELayout/></w:compat><w:p><w:r><w:t>kept</w:t></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            !text.contains("w:compat"),
            "the removed element must be gone: {text}"
        );
        assert!(
            !text.contains("doNotExpandShiftReturn"),
            "and so must its child: {text}"
        );
        assert!(!text.contains("useFELayout"), "and the other: {text}");
        assert!(
            text.contains("kept"),
            "sibling content must survive: {text}"
        );
    }

    #[test]
    fn vml_is_dropped_and_recorded_as_lossy() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml">
<w:body><w:p><w:r><w:pict><v:shape id="s1"><v:fill color="red"/></v:shape></w:pict><w:t>text</w:t></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("v:shape"), "VML must be gone: {text}");
        assert!(
            !text.contains("w:pict"),
            "its Transitional container too: {text}"
        );
        assert!(text.contains("text"), "the run's text must survive: {text}");
        let report = normalizer.report();
        assert_eq!(report.lossy_count(), 1, "the VML removal is a real loss");
        report.verify_no_silent_loss().expect("SC-4");
    }

    #[test]
    fn an_ignorable_removal_does_not_count_as_a_cost() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:stylePaneFormatFilter w:val="x"/><w:zoom w:percent="100"/></w:settings>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("stylePaneFormatFilter"), "{text}");
        assert!(
            text.contains("w:zoom"),
            "unrelated settings must survive: {text}"
        );
        assert_eq!(normalizer.report().lossy_count(), 0);
        assert_eq!(
            normalizer.report().severity_counts()[&Severity::Ignorable],
            1
        );
    }

    #[test]
    fn attribute_values_survive_escaping() {
        // Decoding a value and writing it back raw turns `&amp;` into a bare
        // `&`, and the part stops being XML. Every document whose
        // `[Content_Types].xml` has an escaped character failed to reparse
        // before this was fixed.
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:t xml:space="preserve">a &amp; b &lt; c</w:t></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("&amp;"),
            "the ampersand must stay escaped: {text}"
        );
        assert!(
            text.contains("&lt;"),
            "the less-than must stay escaped: {text}"
        );
        // The output has to parse again, which is the property that matters.
        let second = TransitionalNormalizer::new();
        second.normalize(&part(), text.as_bytes()).expect("reparse");
    }

    #[test]
    fn an_escaped_quote_in_a_value_does_not_end_the_attribute() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:t xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:val="say &quot;hi&quot;"/>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(text.contains("&quot;"), "{text}");
        TransitionalNormalizer::new()
            .normalize(&part(), text.as_bytes())
            .expect("reparse");
    }

    #[test]
    fn a_strict_part_is_borrowed_unchanged_and_the_report_is_empty() {
        let normalizer = TransitionalNormalizer::new();
        let strict = TRANSITIONAL.replace(
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            "http://purl.oclc.org/ooxml/wordprocessingml/main",
        );
        let output = normalizer.normalize(&part(), strict.as_bytes()).unwrap();
        assert!(
            matches!(output, Cow::Borrowed(_)),
            "Strict input must not be touched"
        );
        assert!(normalizer.report().is_noop(), "SC-1: no-op on Strict input");
    }

    #[test]
    fn normalization_is_idempotent_over_bytes() {
        let normalizer = TransitionalNormalizer::new();
        let once = normalizer
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap()
            .into_owned();
        let twice = normalizer.normalize(&part(), &once).unwrap();
        assert_eq!(
            &*twice,
            &once[..],
            "SC-3: normalize(normalize(x)) == normalize(x)"
        );
    }

    #[test]
    fn two_runs_agree_byte_for_byte_and_report_for_report() {
        let first = TransitionalNormalizer::new();
        let second = TransitionalNormalizer::new();
        let a = first
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap()
            .into_owned();
        let b = second
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap()
            .into_owned();
        assert_eq!(a, b, "SC-2: determinism");
        assert_eq!(
            first.report().to_string(),
            second.report().to_string(),
            "SC-2: report"
        );
    }

    #[test]
    fn namespaces_that_do_not_exist_in_strict_are_left_alone() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:zz="urn:private:ext"><w:body><zz:thing/></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("urn:private:ext"),
            "a private extension must survive: {text}"
        );
    }

    #[test]
    fn relationship_types_are_mapped() {
        let (from, to) = map_rel_or_content_type(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
        )
        .expect("a Transitional relationship type maps");
        assert_eq!(
            to,
            "http://purl.oclc.org/ooxml/officeDocument/relationships/styles"
        );
        assert!(from.contains("schemas.openxmlformats.org"));
    }

    #[test]
    fn strict_relationship_types_are_left_alone() {
        assert!(map_rel_or_content_type(
            "http://purl.oclc.org/ooxml/officeDocument/relationships/styles"
        )
        .is_none());
    }

    #[test]
    fn a_non_xml_part_is_untouched() {
        let normalizer = TransitionalNormalizer::new();
        let image = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let output = normalizer
            .normalize(&PartId::new("/word/media/image1.png"), &image)
            .unwrap();
        assert!(matches!(output, Cow::Borrowed(_)));
    }

    #[test]
    fn the_transitional_signal_check_is_textual_and_narrow() {
        assert!(part_needs_normalization(TRANSITIONAL.as_bytes()));
        assert!(!part_needs_normalization(b"<w:document/>"));
    }

    #[test]
    fn options_are_configurable_without_changing_the_default_behaviour() {
        let normalizer =
            TransitionalNormalizer::with_options(NormalizerOptions::with_mce(McePolicy::Report));
        let output = normalizer
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap();
        assert!(String::from_utf8(output.into_owned())
            .unwrap()
            .contains("purl.oclc.org"));
    }
}
