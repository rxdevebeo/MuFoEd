//! A minimal `WordprocessingML` package, for either conformance family.
//!
//! [`DocxBuilder::strict`] and [`DocxBuilder::transitional`] produce the four
//! parts a reader needs — `[Content_Types].xml`, `_rels/.rels`,
//! `word/document.xml` and `word/_rels/document.xml.rels` — and every one of them
//! can be replaced, which is how a hostile test breaks exactly one thing.
//!
//! The OPC namespaces and the core-properties relationship type are the same in
//! both families (ISO/IEC 29500-2 has no Strict variant; ADR-0015).

use std::fmt::Write as _;

use crate::zip::{Method, ZipBuilder};

/// OPC namespaces and relationship types, identical in both families.
pub mod opc {
    /// Namespace of `[Content_Types].xml`.
    pub const CONTENT_TYPES_NS: &str =
        "http://schemas.openxmlformats.org/package/2006/content-types";
    /// Namespace of every `*.rels` part.
    pub const RELATIONSHIPS_NS: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships";
    /// Namespace of the core-properties part.
    pub const CORE_PROPERTIES_NS: &str =
        "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
    /// Relationship type of the core-properties part.
    pub const CORE_PROPERTIES_TYPE: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    /// Content type of a relationships part.
    pub const RELATIONSHIPS_CONTENT_TYPE: &str =
        "application/vnd.openxmlformats-package.relationships+xml";
    /// Content type of the main document part.
    pub const MAIN_DOCUMENT_CONTENT_TYPE: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
}

/// Conformance family of a synthetic package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// ISO/IEC 29500-1 Strict (`http://purl.oclc.org/ooxml/...`).
    Strict,
    /// ISO/IEC 29500-4 Transitional (`http://schemas.openxmlformats.org/...`).
    Transitional,
}

/// The markup namespaces of one family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Namespaces {
    /// `WordprocessingML` main.
    pub w: &'static str,
    /// Office document relationships (the `r:` attributes).
    pub r: &'static str,
    /// `DrawingML` main.
    pub a: &'static str,
    /// `DrawingML` word-processing drawing.
    pub wp: &'static str,
    /// `DrawingML` picture.
    pub pic: &'static str,
    /// Word-processing shape, as `strict-ooxml-wml` expects it.
    pub wps: &'static str,
    /// Word-processing group, as `strict-ooxml-wml` expects it.
    pub wpg: &'static str,
    /// Office Math.
    pub m: &'static str,
    /// Base of the office-document relationship types, ending in `/`.
    pub rel_base: &'static str,
}

/// Strict namespaces.
pub const STRICT: Namespaces = Namespaces {
    w: "http://purl.oclc.org/ooxml/wordprocessingml/main",
    r: "http://purl.oclc.org/ooxml/officeDocument/relationships",
    a: "http://purl.oclc.org/ooxml/drawingml/main",
    wp: "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
    pic: "http://purl.oclc.org/ooxml/drawingml/picture",
    wps: "http://purl.oclc.org/ooxml/drawingml/wordprocessingShape",
    wpg: "http://purl.oclc.org/ooxml/drawingml/wordprocessingGroup",
    m: "http://purl.oclc.org/ooxml/officeDocument/math",
    rel_base: "http://purl.oclc.org/ooxml/officeDocument/relationships/",
};

/// Transitional namespaces.
pub const TRANSITIONAL: Namespaces = Namespaces {
    w: "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    r: "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    a: "http://schemas.openxmlformats.org/drawingml/2006/main",
    wp: "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    pic: "http://schemas.openxmlformats.org/drawingml/2006/picture",
    wps: "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    wpg: "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    m: "http://schemas.openxmlformats.org/officeDocument/2006/math",
    rel_base: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
};

impl Family {
    /// The namespaces of this family.
    pub fn ns(self) -> &'static Namespaces {
        match self {
            Self::Strict => &STRICT,
            Self::Transitional => &TRANSITIONAL,
        }
    }

    /// An office-document relationship type of this family, by its last
    /// segment (`"styles"`, `"image"`, `"officeDocument"`...).
    pub fn rel_type(self, name: &str) -> String {
        format!("{}{name}", self.ns().rel_base)
    }
}

/// One relationship: `(Id, Type, Target, external)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rel {
    /// `Id`.
    pub id: String,
    /// `Type`, a full URI.
    pub type_uri: String,
    /// `Target`.
    pub target: String,
    /// `TargetMode="External"`.
    pub external: bool,
}

impl Rel {
    /// An internal relationship.
    pub fn new(
        id: impl Into<String>,
        type_uri: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            type_uri: type_uri.into(),
            target: target.into(),
            external: false,
        }
    }
}

/// Serializes a relationships part.
pub fn rels_xml(rels: &[Rel]) -> Vec<u8> {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"{}\">",
        opc::RELATIONSHIPS_NS
    );
    for rel in rels {
        let _ = write!(
            xml,
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            rel.id,
            rel.type_uri,
            rel.target,
            if rel.external {
                " TargetMode=\"External\""
            } else {
                ""
            }
        );
    }
    xml.push_str("</Relationships>");
    xml.into_bytes()
}

/// Wraps `body` in a `w:document` that declares `w r a wp pic wps wpg m` for
/// `family`.
pub fn document_xml(family: Family, body: &str) -> Vec<u8> {
    let ns = family.ns();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"{}\" xmlns:r=\"{}\" xmlns:a=\"{}\" xmlns:wp=\"{}\" xmlns:pic=\"{}\" xmlns:wps=\"{}\" xmlns:wpg=\"{}\" xmlns:m=\"{}\">\
<w:body>{body}</w:body></w:document>",
        ns.w, ns.r, ns.a, ns.wp, ns.pic, ns.wps, ns.wpg, ns.m
    )
    .into_bytes()
}

/// Wraps `inner` in a root element `root` (`"w:styles"`, `"w:hdr"`...) that
/// declares the same prefixes as [`document_xml`].
pub fn part_xml(family: Family, root: &str, inner: &str) -> Vec<u8> {
    let ns = family.ns();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<{root} xmlns:w=\"{}\" xmlns:r=\"{}\" xmlns:a=\"{}\" xmlns:wp=\"{}\" xmlns:pic=\"{}\" xmlns:wps=\"{}\" xmlns:wpg=\"{}\" xmlns:m=\"{}\">{inner}</{root}>",
        ns.w, ns.r, ns.a, ns.wp, ns.pic, ns.wps, ns.wpg, ns.m
    )
    .into_bytes()
}

/// Builds a `.docx`.
///
/// Parts are written in this order: `[Content_Types].xml`, `_rels/.rels`, the
/// main document, its relationships, then the extra parts in insertion order.
/// A part added with [`part`](Self::part) under one of the four default names
/// replaces the default instead of duplicating it; [`raw_entry`](Self::raw_entry)
/// never replaces anything.
#[derive(Debug, Clone)]
pub struct DocxBuilder {
    family: Family,
    document: Vec<u8>,
    content_types: Option<Vec<u8>>,
    root_rels: Vec<Rel>,
    root_rels_override: Option<Vec<u8>>,
    document_rels: Vec<Rel>,
    document_rels_override: Option<Vec<u8>>,
    overrides: Vec<(String, String)>,
    parts: Vec<(String, Vec<u8>)>,
    method: Method,
}

const DOCUMENT_PART: &str = "word/document.xml";
const DOCUMENT_RELS_PART: &str = "word/_rels/document.xml.rels";
const ROOT_RELS_PART: &str = "_rels/.rels";
const CONTENT_TYPES_PART: &str = "[Content_Types].xml";

impl DocxBuilder {
    /// A Strict package with an empty body.
    pub fn strict() -> Self {
        Self::new(Family::Strict)
    }

    /// A Transitional package with an empty body.
    pub fn transitional() -> Self {
        Self::new(Family::Transitional)
    }

    /// A package of `family` with an empty body.
    pub fn new(family: Family) -> Self {
        Self {
            family,
            document: document_xml(family, ""),
            content_types: None,
            root_rels: vec![Rel::new(
                "rId1",
                family.rel_type("officeDocument"),
                DOCUMENT_PART,
            )],
            root_rels_override: None,
            document_rels: Vec::new(),
            document_rels_override: None,
            overrides: vec![(
                format!("/{DOCUMENT_PART}"),
                opc::MAIN_DOCUMENT_CONTENT_TYPE.to_owned(),
            )],
            parts: Vec::new(),
            method: Method::Stored,
        }
    }

    /// The family this builder was created for.
    pub fn family(&self) -> Family {
        self.family
    }

    /// Sets the content of `w:body`.
    #[must_use]
    pub fn body(mut self, body: &str) -> Self {
        self.document = document_xml(self.family, body);
        self
    }

    /// Replaces the bytes of `word/document.xml` entirely (broken XML, foreign
    /// root, other namespaces...).
    #[must_use]
    pub fn document_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.document = bytes.into();
        self
    }

    /// Adds a relationship from the main document part.
    #[must_use]
    pub fn rel(mut self, id: &str, type_name: &str, target: &str) -> Self {
        let type_uri = self.family.rel_type(type_name);
        self.document_rels.push(Rel::new(id, type_uri, target));
        self
    }

    /// Adds a relationship from the main document part with a full type URI.
    #[must_use]
    pub fn rel_uri(mut self, rel: Rel) -> Self {
        self.document_rels.push(rel);
        self
    }

    /// Adds a package-level relationship (in `_rels/.rels`), after the default
    /// `officeDocument` one.
    #[must_use]
    pub fn root_rel(mut self, rel: Rel) -> Self {
        self.root_rels.push(rel);
        self
    }

    /// Replaces `_rels/.rels` entirely.
    #[must_use]
    pub fn root_rels_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.root_rels_override = Some(bytes.into());
        self
    }

    /// Replaces `word/_rels/document.xml.rels` entirely.
    #[must_use]
    pub fn document_rels_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.document_rels_override = Some(bytes.into());
        self
    }

    /// Replaces `[Content_Types].xml` entirely.
    #[must_use]
    pub fn content_types_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.content_types = Some(bytes.into());
        self
    }

    /// Declares an `Override` in the generated `[Content_Types].xml`.
    /// `part_name` is written as given, so it should start with `/`.
    #[must_use]
    pub fn content_type(mut self, part_name: &str, content_type: &str) -> Self {
        self.overrides
            .push((part_name.to_owned(), content_type.to_owned()));
        self
    }

    /// Adds a part, or replaces one of the four default parts if `name` is one
    /// of theirs. The bytes are not checked.
    #[must_use]
    pub fn part(mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        match name {
            DOCUMENT_PART => self.document = bytes,
            DOCUMENT_RELS_PART => self.document_rels_override = Some(bytes),
            ROOT_RELS_PART => self.root_rels_override = Some(bytes),
            CONTENT_TYPES_PART => self.content_types = Some(bytes),
            _ => self.parts.push((name.to_owned(), bytes)),
        }
        self
    }

    /// Adds a part whose content is `inner` wrapped in `root` (see [`part_xml`]).
    #[must_use]
    pub fn part_xml(self, name: &str, root: &str, inner: &str) -> Self {
        let bytes = part_xml(self.family, root, inner);
        self.part(name, bytes)
    }

    /// Adds a ZIP entry as is, never replacing a default part — for
    /// duplicates and case variants.
    #[must_use]
    pub fn raw_entry(mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Self {
        self.parts.push((name.to_owned(), bytes.into()));
        self
    }

    /// Stores every entry uncompressed (the default).
    #[must_use]
    pub fn stored(mut self) -> Self {
        self.method = Method::Stored;
        self
    }

    /// Deflates every entry.
    #[must_use]
    pub fn deflated(mut self) -> Self {
        self.method = Method::Deflated;
        self
    }

    /// The generated `[Content_Types].xml`.
    pub fn content_types_xml(&self) -> Vec<u8> {
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"{}\">\
<Default Extension=\"rels\" ContentType=\"{}\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>\
<Default Extension=\"gif\" ContentType=\"image/gif\"/>",
            opc::CONTENT_TYPES_NS,
            opc::RELATIONSHIPS_CONTENT_TYPE
        );
        for (part, content_type) in &self.overrides {
            let _ = write!(
                xml,
                "<Override PartName=\"{part}\" ContentType=\"{content_type}\"/>"
            );
        }
        xml.push_str("</Types>");
        xml.into_bytes()
    }

    /// Writes the package.
    pub fn build(&self) -> Vec<u8> {
        let content_types = self
            .content_types
            .clone()
            .unwrap_or_else(|| self.content_types_xml());
        let root_rels = self
            .root_rels_override
            .clone()
            .unwrap_or_else(|| rels_xml(&self.root_rels));
        let document_rels = self
            .document_rels_override
            .clone()
            .unwrap_or_else(|| rels_xml(&self.document_rels));
        let mut zip = ZipBuilder::new()
            .method(self.method)
            .entry(CONTENT_TYPES_PART, content_types)
            .entry(ROOT_RELS_PART, root_rels)
            .entry(DOCUMENT_PART, self.document.clone())
            .entry(DOCUMENT_RELS_PART, document_rels);
        for (name, bytes) in &self.parts {
            zip = zip.entry(name.clone(), bytes.clone());
        }
        zip.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    #[test]
    fn strict_package_names_strict_namespaces_and_standard_opc() {
        let bytes = DocxBuilder::strict().body("<w:p/>").build();
        assert!(contains(&bytes, STRICT.w));
        assert!(contains(
            &bytes,
            "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument"
        ));
        assert!(contains(&bytes, opc::RELATIONSHIPS_NS));
        assert!(!contains(&bytes, "purl.oclc.org/ooxml/package"));
        assert!(!contains(&bytes, TRANSITIONAL.w));
    }

    #[test]
    fn transitional_package_names_transitional_namespaces() {
        let bytes = DocxBuilder::transitional().build();
        assert!(contains(&bytes, TRANSITIONAL.w));
        assert!(contains(
            &bytes,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"
        ));
        assert!(!contains(&bytes, "purl.oclc.org"));
    }

    #[test]
    fn a_default_part_is_replaced_not_duplicated() {
        let bytes = DocxBuilder::strict()
            .part("word/document.xml", b"<broken".to_vec())
            .build();
        let count = bytes
            .windows(b"word/document.xml".len())
            .filter(|window| *window == b"word/document.xml")
            .count();
        // Local header, central directory, the Override in content types and
        // the target in `_rels/.rels`.
        assert_eq!(count, 4);
        assert!(contains(&bytes, "<broken"));
    }

    #[test]
    fn raw_entry_duplicates() {
        let bytes = DocxBuilder::strict()
            .raw_entry("WORD/DOCUMENT.XML", b"x".to_vec())
            .build();
        assert!(contains(&bytes, "WORD/DOCUMENT.XML"));
        assert!(contains(&bytes, "word/document.xml"));
    }
}
