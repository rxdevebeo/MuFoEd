//! `.rels` relationship parsing and the relationship graph.
//!
//! Handles package-level (`_rels/.rels`) and part-level relationships,
//! including `TargetMode="External"` which is recorded but never fetched
//! (stage task S1.7).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::error::{Result, StrictError};
use crate::limits::ResourceLimits;
use crate::opc::path::resolve_target;
use crate::part::PartId;
use crate::xml::escape::escape_attr_into;
use crate::xml::{XmlEvent, XmlReader};

/// The OPC package-relationships namespace, written into every `.rels` part
/// this crate produces (AUD-20 / ADR-0015: one URI for both families).
pub const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Standard OPC core-properties relationship type (both families; ADR-0015 /
/// AUD-20: the openxmlformats vocabulary is family-neutral, so there is no
/// separate `purl.oclc.org` spelling to rewrite to).
pub const PACKAGE_CORE_PROPERTIES_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
/// Standard OPC thumbnail relationship type (both families; ADR-0015 / AUD-20).
pub const PACKAGE_THUMBNAIL_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";

/// Transitional relationship-type base (ECMA-376 Part 1 §15).
///
/// Exported so `normalize::transitional::map_rel_or_content_type` (T2) can
/// recognize "a Transitional officeDocument relationship type with no table
/// row" without a second copy of this literal.
pub const TRANSITIONAL_OFFICE_BASE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
/// Strict relationship-type base (ECMA-376 5th edition Part 4).
pub const STRICT_OFFICE_BASE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";

/// One row of the AUD-22 relationship-type table: the exact Transitional and
/// Strict URIs for one `WordprocessingML` relationship kind, and the
/// [`RelType`] it maps to.
///
/// `strict` is `None` for exactly one row, `stylesWithEffects`: Word's
/// 2007-compatible shadow of `styles.xml`, which Strict has no part for at
/// all. Every other row's `strict` differs from its `transitional` only in
/// spelling (`extended-properties` ↔ `extendedProperties`) or, for the two OPC
/// package rows, not at all (AUD-20: one URI serves both families).
pub struct RelTypeRow {
    /// Full Transitional relationship-type URI.
    pub transitional: &'static str,
    /// Full Strict relationship-type URI, or `None` when Strict has none.
    pub strict: Option<&'static str>,
    /// The [`RelType`] this row's URIs map to.
    kind: RelType,
}

const fn row(transitional: &'static str, strict: &'static str, kind: RelType) -> RelTypeRow {
    RelTypeRow {
        transitional,
        strict: Some(strict),
        kind,
    }
}

const fn row_no_strict(transitional: &'static str, kind: RelType) -> RelTypeRow {
    RelTypeRow {
        transitional,
        strict: None,
        kind,
    }
}

/// Shorthand for a row whose Transitional and Strict URIs are
/// `TRANSITIONAL_BASE`/`STRICT_BASE` plus the same suffix.
macro_rules! office_row {
    ($suffix:literal, $kind:expr) => {
        row(
            concat!(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
                $suffix
            ),
            concat!(
                "http://purl.oclc.org/ooxml/officeDocument/relationships/",
                $suffix
            ),
            $kind,
        )
    };
}

/// The relationship-type table (AUD-22): one row per `WordprocessingML`
/// relationship kind from ECMA-376 Part 1 §15 and Part 4, keyed by the exact
/// full URI rather than a URI's last path segment. [`RelType::from_uri`] and
/// [`strict_type_uri`] both read this table, and so does
/// `normalize::transitional::map_rel_or_content_type` (T2) — a single table
/// used by all three is the fix for the bug this table replaces: a
/// `rsplit('/')`-based match made `http://evil.example/officeDocument`
/// indistinguishable from the real relationship type, and a prefix-substring
/// T2 rewrite mangled `extended-properties` into `extended-properties`
/// instead of `extendedProperties`.
pub const REL_TYPES: &[RelTypeRow] = &[
    office_row!("officeDocument", RelType::OfficeDocument),
    office_row!("styles", RelType::Styles),
    // Word writes `stylesWithEffects` under a Microsoft vendor namespace, not
    // the officeDocument base, and Strict declares no part for it at all: it
    // is the 2007-compatible shadow of `styles.xml` kept for older clients.
    // Normalizing it would have to invent a URI that names nothing, which is
    // why `strict` is `None` and why the `Known` payload is the only URI
    // there is rather than a Strict one.
    row_no_strict(
        "http://schemas.microsoft.com/office/2007/relationships/stylesWithEffects",
        RelType::Known("http://schemas.microsoft.com/office/2007/relationships/stylesWithEffects"),
    ),
    office_row!("numbering", RelType::Numbering),
    office_row!("settings", RelType::Settings),
    office_row!("webSettings", RelType::WebSettings),
    office_row!("fontTable", RelType::FontTable),
    office_row!("font", RelType::Font),
    office_row!("theme", RelType::Theme),
    office_row!(
        "themeOverride",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/themeOverride")
    ),
    office_row!("image", RelType::Image),
    office_row!("hyperlink", RelType::Hyperlink),
    office_row!("header", RelType::Header),
    office_row!("footer", RelType::Footer),
    office_row!("footnotes", RelType::Footnotes),
    office_row!("endnotes", RelType::Endnotes),
    office_row!("comments", RelType::Comments),
    office_row!("customXml", RelType::CustomXml),
    office_row!(
        "customXmlProps",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/customXmlProps")
    ),
    office_row!("glossaryDocument", RelType::GlossaryDocument),
    office_row!(
        "attachedTemplate",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/attachedTemplate")
    ),
    office_row!(
        "subDocument",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/subDocument")
    ),
    office_row!(
        "aFChunk",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/aFChunk")
    ),
    office_row!(
        "oleObject",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/oleObject")
    ),
    office_row!(
        "package",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/package")
    ),
    office_row!(
        "chart",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/chart")
    ),
    office_row!(
        "chartUserShapes",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/chartUserShapes")
    ),
    office_row!(
        "diagramData",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/diagramData")
    ),
    office_row!(
        "diagramLayout",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/diagramLayout")
    ),
    office_row!(
        "diagramQuickStyle",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/diagramQuickStyle")
    ),
    office_row!(
        "diagramColors",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/diagramColors")
    ),
    office_row!(
        "control",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/control")
    ),
    office_row!(
        "frame",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/frame")
    ),
    office_row!(
        "printerSettings",
        RelType::Known("http://purl.oclc.org/ooxml/officeDocument/relationships/printerSettings")
    ),
    // The two whose Transitional and Strict suffixes differ in spelling, not
    // just base: this is the row T2 used to get wrong by rewriting the base
    // and keeping the suffix (`extended-properties` stayed `extended-properties`
    // instead of becoming `extendedProperties`).
    row(
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/extendedProperties",
        RelType::ExtendedProperties,
    ),
    row(
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/customProperties",
        RelType::CustomProperties,
    ),
    // OPC package-level types (AUD-20 / ADR-0015): family-neutral, so the
    // Transitional and Strict columns hold the identical URI.
    row(
        PACKAGE_CORE_PROPERTIES_REL,
        PACKAGE_CORE_PROPERTIES_REL,
        RelType::CoreProperties,
    ),
    row(
        PACKAGE_THUMBNAIL_REL,
        PACKAGE_THUMBNAIL_REL,
        RelType::Thumbnail,
    ),
];

/// Identifier of a relationship, unique within the `.rels` part that defines
/// it (the `Id` attribute of a `Relationship` element).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelId(Arc<str>);

impl RelId {
    /// Creates a relationship identifier from its raw `Id` value.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the raw relationship id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for RelId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Where a relationship target lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetMode {
    /// Target is a part inside the package.
    Internal,
    /// Target is an external resource; it is never resolved or fetched.
    External,
}

/// Normalized relationship type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelType {
    /// Office document main part.
    OfficeDocument,
    /// Styles part.
    Styles,
    /// Numbering part.
    Numbering,
    /// Settings part.
    Settings,
    /// Theme part.
    Theme,
    /// Font table part.
    FontTable,
    /// An embedded font binary (`word/fonts/*.ttf` or `*.odttf`).
    ///
    /// Separate from [`FontTable`](Self::FontTable) because the two are relations
    /// of **different parts**: the font table part is related from
    /// `word/document.xml`, and each font binary is related from
    /// `word/_rels/fontTable.xml.rels`. A normalization that recognized one and not
    /// the other left every embedded font unreachable, which is how 16 font
    /// binaries in 2 corpus documents disappeared without a word in the report.
    Font,
    /// Image part.
    Image,
    /// Hyperlink.
    Hyperlink,
    /// Header part.
    Header,
    /// Footer part.
    Footer,
    /// Footnotes part.
    Footnotes,
    /// Endnotes part.
    Endnotes,
    /// Web settings part.
    WebSettings,
    /// Comments part.
    Comments,
    /// Extended (application) document properties, `docProps/app.xml`.
    ExtendedProperties,
    /// Custom document properties, `docProps/custom.xml`.
    CustomProperties,
    /// Core document properties, `docProps/core.xml` (AUD-20: a package-level,
    /// family-neutral OPC type, not an officeDocument-base one).
    CoreProperties,
    /// Package thumbnail, `docProps/thumbnail.*` (AUD-20: package-level,
    /// family-neutral).
    Thumbnail,
    /// Custom XML part.
    CustomXml,
    /// Glossary document part.
    GlossaryDocument,
    /// A [`REL_TYPES`] row not promoted to its own variant (chart, diagram\*,
    /// `oleObject`, `package`, etc.), carrying the Strict URI — or, for
    /// `stylesWithEffects`, the only URI the row has, since Strict declares no
    /// equivalent at all.
    Known(&'static str),
    /// A relationship type with no row in [`REL_TYPES`] at all, carrying the
    /// raw URI exactly as read.
    Other(String),
}

impl RelType {
    /// Maps a raw relationship-type URI to its normalized form.
    ///
    /// Matches the **whole** URI against both columns of [`REL_TYPES`] — never
    /// a URI's last path segment, which is how `http://evil.example/officeDocument`
    /// used to be indistinguishable from the real relationship type: any URI
    /// ending in `/officeDocument` classified as [`Self::OfficeDocument`], no
    /// matter whose authority wrote the rest of it. A URI absent from the
    /// table is [`Self::Other`], not a guess.
    #[must_use]
    pub fn from_uri(uri: &str) -> Self {
        for candidate in REL_TYPES {
            if candidate.transitional == uri || candidate.strict == Some(uri) {
                return candidate.kind.clone();
            }
        }
        Self::Other(uri.to_owned())
    }
}

/// A single OPC relationship.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    /// Relationship id, unique within its `.rels` part.
    pub id: String,
    /// Normalized relationship type.
    pub rel_type: RelType,
    /// Raw relationship-type URI.
    pub raw_type: String,
    /// Raw target string.
    pub target: String,
    /// Whether the target is internal or external.
    pub target_mode: TargetMode,
    /// Resolved target part for internal relationships.
    pub resolved: Option<PartId>,
}

/// Graph of relationships grouped by their source part.
///
/// The package root is modelled as the part id `/`.
#[derive(Clone, Debug, Default)]
pub struct RelationshipGraph {
    by_source: HashMap<PartId, Vec<Relationship>>,
}

impl RelationshipGraph {
    /// Creates an empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the relationships declared by one `.rels` part.
    pub fn add(&mut self, source: PartId, relationships: Vec<Relationship>) {
        self.by_source
            .entry(source)
            .or_default()
            .extend(relationships);
    }

    /// Returns the relationships declared by `from` (empty if none).
    #[must_use]
    pub fn relationships(&self, from: &PartId) -> &[Relationship] {
        self.by_source.get(from).map_or(&[], Vec::as_slice)
    }

    /// Resolves a relationship by source part and id.
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::UnresolvedRelationship`] if the id is unknown.
    pub fn resolve(&self, from: &PartId, rel_id: &str) -> Result<&Relationship> {
        self.relationships(from)
            .iter()
            .find(|rel| rel.id == rel_id)
            .ok_or_else(|| {
                StrictError::UnresolvedRelationship(crate::opc::rels::RelId::new(rel_id))
            })
    }

    /// Iterates over every relationship in the graph.
    pub fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.by_source.values().flatten()
    }
}

/// Derives the source part that owns a `.rels` part.
///
/// `/word/_rels/document.xml.rels` → `/word/document.xml`;
/// `/_rels/.rels` → `/` (the package root).
#[must_use]
pub fn source_part_for_rels(rels_part: &PartId) -> PartId {
    let path = rels_part.as_str();
    if let Some((dir, file)) = path.rsplit_once("/_rels/") {
        let source_name = file.strip_suffix(".rels").unwrap_or(file);
        if dir.is_empty() {
            return PartId::new(format!("/{source_name}").as_str());
        }
        return PartId::new(format!("{dir}/{source_name}").as_str());
    }
    PartId::new("/")
}

/// Parses one `.rels` document into relationships resolved against `source`.
///
/// Takes ownership of the bytes so the UTF-8 path needs no copy.
///
/// # Errors
///
/// Returns a [`StrictError`] on malformed XML, an unsafe target or a
/// resource-limit violation.
pub fn parse_relationships(
    bytes: impl Into<Vec<u8>>,
    source: &PartId,
    limits: &ResourceLimits,
) -> Result<Vec<Relationship>> {
    let mut reader = XmlReader::from_vec(bytes.into(), source.clone(), limits)?;
    let mut out = Vec::new();
    loop {
        match reader.next_event()? {
            XmlEvent::StartElement { name, attrs } if name.local() == "Relationship" => {
                let id = attr_value(&attrs, "Id").unwrap_or_default().to_owned();
                let raw_type = attr_value(&attrs, "Type").unwrap_or_default().to_owned();
                let target = attr_value(&attrs, "Target").unwrap_or_default().to_owned();
                let external = attr_value(&attrs, "TargetMode") == Some("External");
                let target_mode = if external {
                    TargetMode::External
                } else {
                    TargetMode::Internal
                };
                let resolved = resolve_target(source, &target, external)?;
                out.push(Relationship {
                    id,
                    rel_type: RelType::from_uri(&raw_type),
                    raw_type,
                    target,
                    target_mode,
                    resolved,
                });
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn attr_value<'a>(attrs: &'a [crate::xml::Attr], local: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.name.local() == local)
        .map(|attr| attr.value.as_str())
}

/// The relationship type a serializer should emit for `rel_type`.
///
/// The reader accepts a Transitional URI, so a round trip through
/// [`parse_relationships`] followed by this function is what turns a parsed
/// Transitional graph back into Strict one (ISO/IEC 29500-1 §15.2). The writer
/// uses the *canonical* form, which is a single URI per relationship kind
/// rather than the six the Transitional family uses.
///
/// Reads the same [`REL_TYPES`] table [`RelType::from_uri`] does (AUD-22), so
/// the two can never disagree about what a kind's Strict spelling is.
#[must_use]
pub fn strict_type_uri(rel_type: &RelType) -> String {
    match rel_type {
        // An unknown kind keeps the URI it was parsed with: rewriting it into
        // the Strict base would invent a relationship type that does not exist.
        RelType::Other(uri) => uri.clone(),
        // `Known`'s payload is already the Strict URI (or the only URI there
        // is, for `stylesWithEffects`), so there is nothing to look up.
        RelType::Known(uri) => (*uri).to_owned(),
        _ => REL_TYPES
            .iter()
            .find(|candidate| candidate.kind == *rel_type)
            .and_then(|candidate| candidate.strict)
            .map(ToOwned::to_owned)
            // Unreachable for a non-`Known`, non-`Other` variant built by this
            // crate: every such variant has exactly one `REL_TYPES` row, and
            // that row's `strict` is always `Some` (the one row with `strict:
            // None` is `stylesWithEffects`, which is `Known`, not a named
            // variant). Falling back to an empty string rather than panicking
            // keeps this function infallible if that invariant is ever broken.
            .unwrap_or_default(),
    }
}

/// Serializes relationships into a `.rels` document.
///
/// Declarations are emitted in the given order — the caller owns determinism,
/// so this function does not sort. The document is parseable by
/// [`parse_relationships`].
///
/// The document namespace is the **Strict** one, `purl.oclc.org`. The
/// Transitional OPC namespace is still accepted by every reader in the wild,
/// which is exactly why writing it hid the problem: the file opened fine, the
/// conformance check passed (it classifies relationship *types*, not the `.rels`
/// namespace), and the package still needed a normalization pass to become
/// Strict. A written package has to be Strict on the first open — that is the
/// writer's first load-bearing property — so the namespace it declares is the
/// Strict one. [`parse_relationships`] accepts either.
#[must_use]
pub fn write_relationships(relationships: &[Relationship]) -> String {
    let mut out = String::with_capacity(128 + 160 * relationships.len());
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    out.push_str("<Relationships xmlns=\"");
    out.push_str(PACKAGE_REL_NS);
    out.push_str("\">");
    for relationship in relationships {
        out.push_str("<Relationship Id=\"");
        escape_attr_into(&mut out, &relationship.id);
        out.push_str("\" Type=\"");
        let uri = if relationship.raw_type.is_empty() {
            strict_type_uri(&relationship.rel_type)
        } else {
            relationship.raw_type.clone()
        };
        escape_attr_into(&mut out, &uri);
        out.push_str("\" Target=\"");
        escape_attr_into(&mut out, &relationship.target);
        out.push('"');
        if relationship.target_mode == TargetMode::External {
            out.push_str(" TargetMode=\"External\"");
        }
        out.push_str("/>");
    }
    out.push_str("</Relationships>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::{
        parse_relationships, source_part_for_rels, strict_type_uri, write_relationships, RelType,
        Relationship, RelationshipGraph, TargetMode, REL_TYPES,
    };
    use crate::limits::ResourceLimits;
    use crate::part::PartId;

    /// AUD-22: every row round-trips through `from_uri` in both columns, and
    /// agrees with `strict_type_uri` about what the Strict spelling is.
    ///
    /// This is the "verified" half of the table: a row that did not actually
    /// classify both its own URIs back to its own `kind` would be a comment,
    /// not a fact.
    #[test]
    fn every_table_row_round_trips_through_from_uri() {
        for entry in REL_TYPES {
            assert_eq!(
                RelType::from_uri(entry.transitional),
                entry.kind,
                "transitional URI {} did not classify as its own row's kind",
                entry.transitional
            );
            if let Some(strict) = entry.strict {
                assert_eq!(
                    RelType::from_uri(strict),
                    entry.kind,
                    "strict URI {strict} did not classify as its own row's kind"
                );
                assert_eq!(
                    strict_type_uri(&entry.kind),
                    strict,
                    "strict_type_uri disagrees with the table for {}",
                    entry.transitional
                );
            }
        }
    }

    /// No two *different* rows name the same URI in the same column — if they
    /// did, `from_uri` would silently return whichever row happened to come
    /// first. A row's own `transitional` and `strict` are allowed to be equal
    /// (the two OPC package rows: one URI serves both families), so the two
    /// columns are checked separately rather than pooled into one set.
    #[test]
    fn the_table_has_no_duplicate_uris_within_a_column() {
        let mut transitional_seen = std::collections::BTreeSet::new();
        let mut strict_seen = std::collections::BTreeSet::new();
        for entry in REL_TYPES {
            assert!(
                transitional_seen.insert(entry.transitional),
                "duplicate transitional URI {}",
                entry.transitional
            );
            if let Some(strict) = entry.strict {
                assert!(strict_seen.insert(strict), "duplicate strict URI {strict}");
            }
        }
    }

    /// The bug this table replaces: matching a URI's last path segment made
    /// any `.../officeDocument`-suffixed URI classify as the main document,
    /// no matter whose authority wrote the rest of it.
    #[test]
    fn an_unknown_authoritys_office_document_uri_is_other() {
        assert_eq!(
            RelType::from_uri("http://evil.example/officeDocument"),
            RelType::Other("http://evil.example/officeDocument".to_owned())
        );
    }

    /// A URI that merely contains a known suffix, rather than being one of the
    /// table's exact strings, must not match either — exercising the same
    /// "exact URI, not a fragment" rule the table enforces for every row.
    #[test]
    fn a_uri_with_a_known_suffix_but_wrong_base_is_other() {
        for suffix in ["styles", "numbering", "hyperlink"] {
            let uri = format!("http://evil.example/{suffix}");
            assert_eq!(RelType::from_uri(&uri), RelType::Other(uri));
        }
    }

    /// `stylesWithEffects` is the one row with no Strict column at all.
    #[test]
    fn styles_with_effects_has_no_strict_uri() {
        let entry = REL_TYPES
            .iter()
            .find(|entry| entry.transitional.ends_with("stylesWithEffects"))
            .expect("stylesWithEffects is in the table");
        assert_eq!(entry.strict, None);
        assert!(matches!(entry.kind, RelType::Known(_)));
    }

    /// The two OPC package rows (AUD-20): one URI serves both families.
    #[test]
    fn opc_package_rows_use_the_same_uri_for_both_columns() {
        for suffix in ["core-properties", "thumbnail"] {
            let entry = REL_TYPES
                .iter()
                .find(|entry| entry.transitional.ends_with(suffix))
                .unwrap_or_else(|| panic!("{suffix} is in the table"));
            assert_eq!(Some(entry.transitional), entry.strict);
        }
    }

    #[test]
    fn derives_source_part() {
        assert_eq!(
            source_part_for_rels(&PartId::new("/word/_rels/document.xml.rels")).as_str(),
            "/word/document.xml"
        );
        assert_eq!(
            source_part_for_rels(&PartId::new("/_rels/.rels")).as_str(),
            "/"
        );
    }

    #[test]
    fn parses_internal_and_external_relationships() {
        let xml = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="http://example.com" TargetMode="External"/>
</Relationships>"#;
        let source = PartId::new("/");
        let rels = parse_relationships(xml, &source, &ResourceLimits::default()).unwrap();
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].rel_type, RelType::OfficeDocument);
        assert_eq!(
            rels[0].resolved.as_ref().unwrap().as_str(),
            "/word/document.xml"
        );
        assert_eq!(rels[1].target_mode, TargetMode::External);
        assert!(rels[1].resolved.is_none());
    }

    #[test]
    fn graph_resolves_and_lists() {
        let xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/styles" Target="styles.xml"/></Relationships>"#;
        let source = PartId::new("/word/document.xml");
        let mut graph = RelationshipGraph::new();
        graph.add(
            source.clone(),
            parse_relationships(xml, &source, &ResourceLimits::default()).unwrap(),
        );
        assert_eq!(graph.relationships(&source).len(), 1);
        assert!(graph.resolve(&source, "rId1").is_ok());
        assert!(graph.resolve(&source, "rId9").is_err());
        assert_eq!(graph.iter().count(), 1);
        assert!(graph.relationships(&PartId::new("/none")).is_empty());
    }

    #[test]
    fn written_relationships_reparse_and_rewrite_to_strict() {
        let transitional = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="http://example.com" TargetMode="External"/></Relationships>"#;
        let source = PartId::new("/word/document.xml");
        let parsed =
            parse_relationships(transitional, &source, &ResourceLimits::default()).unwrap();

        // The writer emits the Strict twin of every Transitional type.
        let mut strict = parsed.clone();
        for relationship in &mut strict {
            relationship.raw_type = strict_type_uri(&relationship.rel_type);
        }
        let xml = write_relationships(&strict);
        assert!(xml.contains("http://purl.oclc.org/ooxml/officeDocument/relationships/styles"));
        assert!(!xml.contains("schemas.openxmlformats.org/officeDocument"));

        let again =
            parse_relationships(xml.as_bytes(), &source, &ResourceLimits::default()).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!(again[0].rel_type, RelType::Styles);
        assert_eq!(
            again[0].resolved.as_ref().unwrap().as_str(),
            "/word/styles.xml"
        );
        assert_eq!(again[1].target_mode, TargetMode::External);
    }

    #[test]
    fn attribute_values_are_escaped() {
        let relationship = Relationship {
            id: "rId&1".to_owned(),
            rel_type: RelType::Hyperlink,
            raw_type: strict_type_uri(&RelType::Hyperlink),
            target: "a\"b&c".to_owned(),
            target_mode: TargetMode::External,
            resolved: None,
        };
        let xml = write_relationships(&[relationship]);
        assert!(xml.contains("Id=\"rId&amp;1\""), "{xml}");
        assert!(xml.contains("Target=\"a&quot;b&amp;c\""), "{xml}");
    }
}
