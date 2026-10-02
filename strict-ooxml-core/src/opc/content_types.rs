//! `[Content_Types].xml` parsing and the content-type index.
//!
//! Resolves a part path to its content type using `Default` (by extension) and
//! `Override` (by part name) declarations, with `Override` taking precedence
//! (stage task S1.6).

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;
use crate::limits::ResourceLimits;
use crate::opc::path::canonicalize_part_name;
use crate::part::PartId;
use crate::xml::escape::escape_attr_into;
use crate::xml::{XmlEvent, XmlReader};

/// Index of declared content types for a package.
#[derive(Clone, Debug, Default)]
pub struct ContentTypeIndex {
    defaults: HashMap<String, Arc<str>>,
    overrides: HashMap<PartId, Arc<str>>,
}

impl ContentTypeIndex {
    /// Creates an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses `[Content_Types].xml`.
    ///
    /// Takes ownership of the bytes so the UTF-8 path needs no copy. Unknown
    /// elements and attributes are ignored; malformed XML is an error.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`](crate::error::StrictError) on malformed XML or
    /// a resource-limit violation.
    pub fn parse(bytes: impl Into<Vec<u8>>, part: PartId, limits: &ResourceLimits) -> Result<Self> {
        let mut reader = XmlReader::from_vec(bytes.into(), part, limits)?;
        let mut index = Self::new();
        loop {
            match reader.next_event()? {
                XmlEvent::StartElement { name, attrs } => match name.local() {
                    "Default" => {
                        if let (Some(extension), Some(content_type)) = (
                            attr_value(&attrs, "Extension"),
                            attr_value(&attrs, "ContentType"),
                        ) {
                            index
                                .defaults
                                .insert(extension.to_ascii_lowercase(), Arc::from(content_type));
                        }
                    }
                    "Override" => {
                        if let (Some(part_name), Some(content_type)) = (
                            attr_value(&attrs, "PartName"),
                            attr_value(&attrs, "ContentType"),
                        ) {
                            let trimmed = part_name.trim_start_matches('/');
                            let id = canonicalize_part_name(trimmed)?;
                            index.overrides.insert(id, Arc::from(content_type));
                        }
                    }
                    _ => {}
                },
                XmlEvent::Eof => break,
                _ => {}
            }
        }
        Ok(index)
    }

    /// Registers a default content type for an extension.
    pub fn insert_default(&mut self, extension: &str, content_type: &str) {
        self.defaults
            .insert(extension.to_ascii_lowercase(), Arc::from(content_type));
    }

    /// Registers an override content type for a specific part.
    pub fn insert_override(&mut self, part: PartId, content_type: &str) {
        self.overrides.insert(part, Arc::from(content_type));
    }

    /// Returns the content type for a part, preferring an `Override`.
    #[must_use]
    pub fn content_type_for(&self, id: &PartId) -> Option<&str> {
        if let Some(content_type) = self.overrides.get(id) {
            return Some(content_type);
        }
        let extension = id.as_str().rsplit_once('.').map(|(_, ext)| ext)?;
        self.defaults
            .get(&extension.to_ascii_lowercase())
            .map(AsRef::as_ref)
    }

    /// Returns the number of declared content types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.defaults.len() + self.overrides.len()
    }

    /// Returns `true` if no content type is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty() && self.overrides.is_empty()
    }

    /// Iterates over all declared content-type strings.
    pub fn iter_content_types(&self) -> impl Iterator<Item = &str> {
        self.defaults
            .values()
            .chain(self.overrides.values())
            .map(AsRef::as_ref)
    }

    /// Serializes the index back to `[Content_Types].xml`.
    ///
    /// `Default` declarations come first, ordered by extension, then
    /// `Override` declarations ordered by part name. The ordering is what makes
    /// the output reproducible: the index is backed by hash maps, whose
    /// iteration order is not part of any contract (SC-1).
    #[must_use]
    pub fn write_xml(&self) -> String {
        let mut out = String::with_capacity(256 + 96 * (self.len()));
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        out.push_str(
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        );

        let mut defaults: Vec<(&str, &str)> = self
            .defaults
            .iter()
            .map(|(extension, content_type)| (extension.as_ref(), content_type.as_ref()))
            .collect();
        defaults.sort_unstable_by_key(|(extension, _)| *extension);
        for (extension, content_type) in defaults {
            out.push_str("<Default Extension=\"");
            escape_attr_into(&mut out, extension);
            out.push_str("\" ContentType=\"");
            escape_attr_into(&mut out, content_type);
            out.push_str("\"/>");
        }

        let mut overrides: Vec<(&str, &str)> = self
            .overrides
            .iter()
            .map(|(part, content_type)| (part.as_str(), content_type.as_ref()))
            .collect();
        overrides.sort_unstable_by_key(|(part, _)| *part);
        for (part, content_type) in overrides {
            out.push_str("<Override PartName=\"");
            escape_attr_into(&mut out, part);
            out.push_str("\" ContentType=\"");
            escape_attr_into(&mut out, content_type);
            out.push_str("\"/>");
        }

        out.push_str("</Types>\n");
        out
    }
}

fn attr_value<'a>(attrs: &'a [crate::xml::Attr], local: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.name.local() == local)
        .map(|attr| attr.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::ContentTypeIndex;
    use crate::limits::ResourceLimits;
    use crate::part::PartId;

    const XML: &[u8] = br#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.ms-word.document.main+xml"/>
</Types>"#;

    #[test]
    fn resolves_override_and_default() {
        let part = PartId::new("/[Content_Types].xml");
        let index = ContentTypeIndex::parse(XML, part, &ResourceLimits::default()).unwrap();
        assert_eq!(
            index.content_type_for(&PartId::new("/word/document.xml")),
            Some("application/vnd.ms-word.document.main+xml")
        );
        assert_eq!(
            index.content_type_for(&PartId::new("/word/styles.xml")),
            Some("application/xml")
        );
        assert_eq!(
            index.content_type_for(&PartId::new("/word/media/x.png")),
            None
        );
    }

    #[test]
    fn written_xml_is_ordered_and_reparses() {
        let mut index = ContentTypeIndex::new();
        // Inserted out of order on purpose: the index is hash-map backed.
        index.insert_override(
            PartId::new("/word/styles.xml"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        );
        index.insert_default("png", "image/png");
        index.insert_default(
            "rels",
            "application/vnd.openxmlformats-package.relationships+xml",
        );

        let xml = index.write_xml();
        let again = index.write_xml();
        assert_eq!(xml, again, "write_xml must be reproducible");

        let reparsed = ContentTypeIndex::parse(
            xml.as_bytes(),
            PartId::new("/[Content_Types].xml"),
            &ResourceLimits::default(),
        )
        .expect("reparse");
        assert_eq!(
            reparsed.content_type_for(&PartId::new("/word/styles.xml")),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml")
        );
        assert_eq!(
            reparsed.content_type_for(&PartId::new("/word/media/a.png")),
            Some("image/png")
        );

        let png = xml.find("Extension=\"png\"").expect("png default");
        let rels = xml.find("Extension=\"rels\"").expect("rels default");
        assert!(png < rels, "defaults must be ordered by extension");
    }
}
