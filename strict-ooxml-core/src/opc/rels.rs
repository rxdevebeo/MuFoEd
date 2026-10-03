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
    /// Any other relationship type, carrying the raw URI.
    Other(String),
}

impl RelType {
    /// Maps a raw relationship-type URI to its normalized form.
    #[must_use]
    pub fn from_uri(uri: &str) -> Self {
        let suffix = uri.rsplit('/').next().unwrap_or(uri);
        match suffix {
            "officeDocument" => Self::OfficeDocument,
            "styles" => Self::Styles,
            "numbering" => Self::Numbering,
            "settings" => Self::Settings,
            "theme" => Self::Theme,
            "fontTable" => Self::FontTable,
            "font" => Self::Font,
            "image" => Self::Image,
            "hyperlink" => Self::Hyperlink,
            "header" => Self::Header,
            "footer" => Self::Footer,
            "footnotes" => Self::Footnotes,
            "endnotes" => Self::Endnotes,
            _ => Self::Other(uri.to_owned()),
        }
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
#[must_use]
pub fn strict_type_uri(rel_type: &RelType) -> String {
    const BASE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";
    match rel_type {
        RelType::OfficeDocument => format!("{BASE}officeDocument"),
        RelType::Styles => format!("{BASE}styles"),
        RelType::Numbering => format!("{BASE}numbering"),
        RelType::Settings => format!("{BASE}settings"),
        RelType::Theme => format!("{BASE}theme"),
        RelType::FontTable => format!("{BASE}fontTable"),
        RelType::Font => format!("{BASE}font"),
        RelType::Image => format!("{BASE}image"),
        RelType::Hyperlink => format!("{BASE}hyperlink"),
        RelType::Header => format!("{BASE}header"),
        RelType::Footer => format!("{BASE}footer"),
        RelType::Footnotes => format!("{BASE}footnotes"),
        RelType::Endnotes => format!("{BASE}endnotes"),
        // An unknown kind keeps the URI it was parsed with: rewriting it into
        // the Strict base would invent a relationship type that does not exist.
        RelType::Other(uri) => uri.clone(),
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
        Relationship, RelationshipGraph, TargetMode,
    };
    use crate::limits::ResourceLimits;
    use crate::part::PartId;

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
