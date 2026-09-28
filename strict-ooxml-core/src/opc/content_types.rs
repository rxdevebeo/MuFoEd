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
}
