//! Streaming, namespace-aware XML reader.
//!
//! Wraps the low-level `quick-xml` tokenizer with a namespace stack and hard
//! safety limits; DTD and external entity processing are disabled up front
//! (ADR-0002, ADR-0003). The reader owns its decoded buffer, so events are
//! owned values rather than borrows of the input (stage tasks S1.9–S1.10).

pub mod escape;
pub mod ns_stack;
pub mod qname;
pub mod safety;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

use crate::error::{Result, SourceLocation, StrictError};
use crate::limits::ResourceLimits;
use crate::part::PartId;
use ns_stack::NsStack;
use qname::QName;

/// An attribute of a start element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    /// Resolved attribute name.
    pub name: QName,
    /// Unescaped attribute value.
    pub value: String,
}

/// A single XML pull event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XmlEvent {
    /// An opening element with its attributes.
    StartElement {
        /// Resolved element name.
        name: QName,
        /// Element attributes (namespace declarations excluded).
        attrs: Vec<Attr>,
    },
    /// A closing element.
    EndElement {
        /// Resolved element name.
        name: QName,
    },
    /// Character data with entity references expanded.
    Text(String),
    /// Unescaped character data from a CDATA section.
    CData(String),
    /// End of the document.
    Eof,
}

/// Owned, encoding-aware, namespace-resolving XML reader.
pub struct XmlReader {
    part: PartId,
    data: Vec<u8>,
    pos: usize,
    event_start: usize,
    limits: ResourceLimits,
    ns: NsStack,
    depth: u32,
    text_total: u64,
    open_names: Vec<String>,
    pending_end: Option<QName>,
    scanned: usize,
    line: u32,
    column: u32,
    event_line: u32,
    event_col: u32,
}

/// Owned intermediate produced while the underlying event borrow is alive.
enum Parsed {
    Start {
        raw: String,
        attrs: Vec<(String, String)>,
        empty: bool,
    },
    End {
        raw: String,
    },
    Text(String),
    CData(String),
    Eof,
    Skip,
}

impl XmlReader {
    /// Creates a reader over one part's bytes.
    ///
    /// The input is decoded from UTF-8 (optionally with a BOM), UTF-16 (BOM), or
    /// a legacy encoding named in an XML declaration, then tokenized.
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::InvalidXml`] if the encoding is unsupported.
    pub fn new(bytes: &[u8], part: PartId, limits: &ResourceLimits) -> Result<Self> {
        Self::from_vec(bytes.to_vec(), part, limits)
    }

    /// Creates a reader taking ownership of an already-decoded UTF-8 buffer.
    ///
    /// This avoids copying the part bytes on the common UTF-8 path (a BOM, if
    /// present, is discarded in place; UTF-16/legacy input is transcoded).
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::InvalidXml`] if the encoding is unsupported.
    pub fn from_vec(data: Vec<u8>, part: PartId, limits: &ResourceLimits) -> Result<Self> {
        let data = normalize_encoding(data, &part)?;
        Ok(Self {
            part,
            data,
            pos: 0,
            event_start: 0,
            limits: *limits,
            ns: NsStack::new(),
            depth: 0,
            text_total: 0,
            open_names: Vec::new(),
            pending_end: None,
            scanned: 0,
            line: 1,
            column: 1,
            event_line: 1,
            event_col: 1,
        })
    }

    /// Returns the part this reader is decoding.
    #[must_use]
    pub fn part(&self) -> &PartId {
        &self.part
    }

    /// Returns the source location of the most recently returned event.
    #[must_use]
    pub fn last_event_location(&self) -> SourceLocation {
        self.location()
    }

    /// Advances to the next event.
    ///
    /// Comments, processing instructions and the XML declaration are skipped;
    /// DOCTYPE is rejected.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] on malformed XML, a forbidden construct, an
    /// unbound prefix or a resource-limit violation.
    pub fn next_event(&mut self) -> Result<XmlEvent> {
        loop {
            if let Some(name) = self.pending_end.take() {
                self.ns.pop_scope();
                self.depth = self.depth.saturating_sub(1);
                return Ok(XmlEvent::EndElement { name });
            }
            match self.read_parsed()? {
                Parsed::Start { raw, attrs, empty } => {
                    return self.open_element(raw, attrs, empty);
                }
                Parsed::End { raw } => return self.close_element(&raw),
                Parsed::Text(text) => return self.push_text(text, false),
                Parsed::CData(text) => return self.push_text(text, true),
                Parsed::Eof => return Ok(XmlEvent::Eof),
                Parsed::Skip => {}
            }
        }
    }

    /// Reads one low-level event and copies it into an owned `Parsed` value.
    fn read_parsed(&mut self) -> Result<Parsed> {
        self.event_start = self.pos;
        self.advance_to(self.event_start);
        self.event_line = self.line;
        self.event_col = self.column;
        let start = self.pos;
        let (event, consumed) = {
            let mut reader = Reader::from_reader(&self.data[start..]);
            reader.config_mut().check_end_names = false;
            reader.config_mut().allow_unmatched_ends = true;
            let event = reader
                .read_event()
                .map_err(|error| self.invalid_xml(format!("{error}"), start))?;
            let position = reader.buffer_position();
            let consumed = usize::try_from(position)
                .map_err(|_| self.invalid_xml("position overflow".to_owned(), start))?;
            (event, consumed)
        };
        self.pos = start + consumed;
        match event {
            Event::Start(e) => self.parse_start(&e, false),
            Event::Empty(e) => self.parse_start(&e, true),
            Event::End(e) => {
                let raw = self.raw_name(e.name().as_ref(), start)?;
                Ok(Parsed::End { raw })
            }
            Event::Text(t) => {
                let text = t
                    .xml10_content()
                    .map_err(|error| self.invalid_xml(format!("{error}"), start))?
                    .into_owned();
                Ok(Parsed::Text(text))
            }
            Event::GeneralRef(reference) => {
                Ok(Parsed::Text(self.resolve_reference(&reference, start)?))
            }
            Event::CData(c) => {
                let text = std::str::from_utf8(&c.into_inner())
                    .map_err(|error| self.invalid_xml(format!("{error}"), start))?
                    .to_owned();
                Ok(Parsed::CData(text))
            }
            Event::DocType(_) => Err(StrictError::InvalidXml {
                location: self.location(),
                detail: "DOCTYPE declarations are not allowed".to_owned(),
            }),
            Event::Comment(_) | Event::PI(_) | Event::Decl(_) => Ok(Parsed::Skip),
            Event::Eof => Ok(Parsed::Eof),
        }
    }

    /// Owns a start/empty tag's name and attributes.
    fn parse_start(
        &self,
        element: &quick_xml::events::BytesStart<'_>,
        empty: bool,
    ) -> Result<Parsed> {
        let raw = self.raw_name(element.name().as_ref(), self.event_start)?;
        let mut attrs = Vec::new();
        for attribute in element.attributes() {
            let attribute = attribute
                .map_err(|error| self.invalid_xml(format!("{error}"), self.event_start))?;
            let key = std::str::from_utf8(attribute.key.as_ref())
                .map_err(|error| self.invalid_xml(format!("{error}"), self.event_start))?
                .to_owned();
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|error| self.invalid_xml(format!("{error}"), self.event_start))?
                .into_owned();
            attrs.push((key, value));
        }
        Ok(Parsed::Start { raw, attrs, empty })
    }

    fn open_element(
        &mut self,
        raw: String,
        attrs: Vec<(String, String)>,
        empty: bool,
    ) -> Result<XmlEvent> {
        safety::check_attributes(attrs.len(), &self.limits)?;
        let location = self.location();
        self.ns.push_scope();
        let mut resolved = Vec::with_capacity(attrs.len());
        for (key, value) in attrs {
            match key.as_str() {
                "xmlns" => self.ns.declare(None, &value),
                _ => {
                    if let Some(prefix) = key.strip_prefix("xmlns:") {
                        self.ns.declare(Some(prefix.to_owned()), &value);
                    } else {
                        resolved.push((key, value));
                    }
                }
            }
        }
        self.depth = self.depth.saturating_add(1);
        safety::check_depth(self.depth, &self.limits)?;
        let name = self.resolve_name(&raw, false, &location)?;
        let mut out = Vec::with_capacity(resolved.len());
        for (key, value) in resolved {
            let attr_name = self.resolve_name(&key, true, &location)?;
            out.push(Attr {
                name: attr_name,
                value,
            });
        }
        if empty {
            self.pending_end = Some(name.clone());
        } else {
            self.open_names.push(raw);
        }
        Ok(XmlEvent::StartElement { name, attrs: out })
    }

    fn close_element(&mut self, raw: &str) -> Result<XmlEvent> {
        let matches = self.open_names.last().is_some_and(|open| open == raw);
        if !matches {
            return Err(StrictError::InvalidXml {
                location: self.location(),
                detail: format!("unmatched end tag </{raw}>"),
            });
        }
        let location = self.location();
        let name = self.resolve_name(raw, false, &location)?;
        self.ns.pop_scope();
        self.open_names.pop();
        self.depth = self.depth.saturating_sub(1);
        Ok(XmlEvent::EndElement { name })
    }

    fn push_text(&mut self, text: String, cdata: bool) -> Result<XmlEvent> {
        self.text_total = self.text_total.saturating_add(text.len() as u64);
        safety::check_text(self.text_total, &self.limits)?;
        if cdata {
            Ok(XmlEvent::CData(text))
        } else {
            Ok(XmlEvent::Text(text))
        }
    }

    /// Resolves a raw `prefix:local` name against the current namespace scope.
    fn resolve_name(&self, raw: &str, attribute: bool, location: &SourceLocation) -> Result<QName> {
        if let Some((prefix, local)) = raw.split_once(':') {
            if !self.ns.is_declared(prefix) {
                return Err(StrictError::UnboundPrefix {
                    location: location.clone(),
                    prefix: prefix.to_owned(),
                });
            }
            let ns = self.ns.resolve(Some(prefix));
            Ok(QName::new(ns, Some(prefix.to_owned()), local.to_owned()))
        } else if attribute {
            // Unprefixed attributes are never in a namespace.
            Ok(QName::new(None, None, raw.to_owned()))
        } else {
            let ns = self.ns.resolve(None);
            Ok(QName::new(ns, None, raw.to_owned()))
        }
    }

    /// Resolves a character or predefined entity reference.
    ///
    /// Custom (DTD-declared) entities are rejected because DTD processing is
    /// forbidden.
    fn resolve_reference(
        &self,
        reference: &quick_xml::events::BytesRef<'_>,
        offset: usize,
    ) -> Result<String> {
        if reference.is_char_ref() {
            let ch = reference
                .resolve_char_ref()
                .map_err(|error| self.invalid_xml(format!("{error}"), offset))?
                .ok_or_else(|| {
                    self.invalid_xml("invalid character reference".to_owned(), offset)
                })?;
            return Ok(ch.to_string());
        }
        let name = reference
            .decode()
            .map_err(|error| self.invalid_xml(format!("{error}"), offset))?;
        let ch = match name.as_ref() {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            other => {
                return Err(self.invalid_xml(
                    format!("entity reference '&{other};' is not allowed"),
                    offset,
                ))
            }
        };
        Ok(ch.to_string())
    }

    fn raw_name(&self, bytes: &[u8], offset: usize) -> Result<String> {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| self.invalid_xml(format!("invalid name: {error}"), offset))
    }

    fn invalid_xml(&self, detail: String, _offset: usize) -> StrictError {
        StrictError::InvalidXml {
            location: self.location(),
            detail,
        }
    }

    /// Advances the cached line/column up to `target`, scanning each byte once.
    ///
    /// Across a whole document every byte is scanned at most once, which keeps
    /// location bookkeeping amortized O(1) per event.
    fn advance_to(&mut self, target: usize) {
        let target = target.min(self.data.len());
        if target <= self.scanned {
            return;
        }
        for &byte in &self.data[self.scanned..target] {
            if byte == b'\n' {
                self.line = self.line.saturating_add(1);
                self.column = 1;
            } else {
                self.column = self.column.saturating_add(1);
            }
        }
        self.scanned = target;
    }

    /// Returns the cached location of the current event (O(1)).
    fn location(&self) -> SourceLocation {
        SourceLocation::new(
            self.part.clone(),
            self.event_line,
            self.event_col,
            self.event_start as u64,
        )
    }
}

/// Normalizes owned input bytes into UTF-8, honouring BOMs and an XML
/// declaration. Returns the same buffer unchanged on the UTF-8 fast path.
fn normalize_encoding(mut bytes: Vec<u8>, part: &PartId) -> Result<Vec<u8>> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
        return Ok(bytes);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (decoded, _) = encoding_rs::UTF_16LE.decode_with_bom_removal(&bytes);
        return Ok(decoded.into_owned().into_bytes());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (decoded, _) = encoding_rs::UTF_16BE.decode_with_bom_removal(&bytes);
        return Ok(decoded.into_owned().into_bytes());
    }
    if bytes.starts_with(b"<?xml") {
        let head_len = bytes.len().min(256);
        if let Some(label) = sniff_declared_encoding(&bytes[..head_len]) {
            let lowered = label.to_ascii_lowercase();
            if lowered != "utf-8" && lowered != "utf8" {
                let encoding =
                    encoding_rs::Encoding::for_label(label.as_bytes()).ok_or_else(|| {
                        StrictError::InvalidXml {
                            location: SourceLocation::new(part.clone(), 1, 1, 0),
                            detail: format!("unsupported XML encoding: {label}"),
                        }
                    })?;
                let (decoded, _) = encoding.decode_with_bom_removal(&bytes);
                return Ok(decoded.into_owned().into_bytes());
            }
        }
    }
    Ok(bytes)
}

/// Extracts the value of an `encoding="..."` pseudo-attribute from a decl.
fn sniff_declared_encoding(head: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(head).ok()?;
    let decl_end = text.find("?>")?;
    let decl = &text[..decl_end];
    let at = decl.find("encoding")?;
    let after = &decl[at + "encoding".len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::{XmlEvent, XmlReader};
    use crate::error::StrictError;
    use crate::limits::ResourceLimits;
    use crate::part::PartId;

    fn part() -> PartId {
        PartId::new("/word/document.xml")
    }

    fn read_all(xml: &[u8]) -> Vec<XmlEvent> {
        let mut reader = XmlReader::new(xml, part(), &ResourceLimits::default()).unwrap();
        let mut out = Vec::new();
        loop {
            let event = reader.next_event().unwrap();
            let eof = event == XmlEvent::Eof;
            out.push(event);
            if eof {
                break;
            }
        }
        out
    }

    #[test]
    fn resolves_namespaces_and_prefixes() {
        let xml =
            br#"<w:document xmlns:w="urn:w" xmlns="urn:default"><w:body w:val="1"/></w:document>"#;
        let events = read_all(xml);
        match &events[0] {
            XmlEvent::StartElement { name, .. } => {
                assert_eq!(name.prefix.as_deref(), Some("w"));
                assert_eq!(name.ns.as_ref().unwrap(), "urn:w");
                assert_eq!(name.local(), "document");
            }
            other => panic!("unexpected {other:?}"),
        }
        // The empty body element is expanded into start+end.
        match &events[1] {
            XmlEvent::StartElement { name, attrs } => {
                assert_eq!(name.ns.as_ref().unwrap(), "urn:w");
                assert_eq!(attrs[0].name.local(), "val");
                assert_eq!(attrs[0].name.ns.as_ref().unwrap(), "urn:w");
                assert_eq!(attrs[0].value, "1");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(events[2], XmlEvent::EndElement { .. }));
        assert!(matches!(events[3], XmlEvent::EndElement { .. }));
    }

    #[test]
    fn default_namespace_does_not_apply_to_attributes() {
        let xml = br#"<a xmlns="urn:d" val="x"/>"#;
        let events = read_all(xml);
        match &events[0] {
            XmlEvent::StartElement { attrs, .. } => {
                assert!(attrs[0].name.ns.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_prefix() {
        let xml = b"<w:document/>";
        let mut reader = XmlReader::new(xml, part(), &ResourceLimits::default()).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::UnboundPrefix { .. })
        ));
    }

    #[test]
    fn rejects_doctype() {
        let xml = br#"<!DOCTYPE foo [ <!ENTITY x "y"> ]><a/>"#;
        let mut reader = XmlReader::new(xml, part(), &ResourceLimits::default()).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::InvalidXml { .. })
        ));
    }

    #[test]
    fn enforces_depth_limit() {
        let xml = b"<a><b><c/></b></a>";
        let limits = ResourceLimits {
            max_xml_depth: 2,
            ..ResourceLimits::default()
        };
        let mut reader = XmlReader::new(xml, part(), &limits).unwrap();
        assert!(matches!(
            reader.next_event(),
            Ok(XmlEvent::StartElement { .. })
        ));
        assert!(matches!(
            reader.next_event(),
            Ok(XmlEvent::StartElement { .. })
        ));
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn parses_cdata_and_text() {
        let xml = b"<a>hi<![CDATA[<raw>]]></a>";
        let events = read_all(xml);
        assert_eq!(events[1], XmlEvent::Text("hi".to_owned()));
        assert_eq!(events[2], XmlEvent::CData("<raw>".to_owned()));
    }

    #[test]
    fn resolves_predefined_and_rejects_custom_entities() {
        let events = read_all(b"<a>x &amp; y</a>");
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                XmlEvent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "x & y");

        let mut reader =
            XmlReader::new(b"<a>&bogus;</a>", part(), &ResourceLimits::default()).unwrap();
        assert!(matches!(
            reader.next_event(),
            Ok(XmlEvent::StartElement { .. })
        ));
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::InvalidXml { .. })
        ));
    }

    #[test]
    fn locations_track_lines_and_columns() {
        let xml = b"<a>\n  <b/>\n  <c>text</c>\n</a>";
        let mut reader = XmlReader::new(xml, part(), &ResourceLimits::default()).unwrap();
        let mut starts = Vec::new();
        let mut end_a = None;
        loop {
            let event = reader.next_event().unwrap();
            match &event {
                XmlEvent::StartElement { name, .. } => {
                    starts.push((name.local().to_owned(), reader.last_event_location()));
                }
                XmlEvent::EndElement { name } if name.local() == "a" => {
                    end_a = Some(reader.last_event_location());
                }
                XmlEvent::Eof => break,
                _ => {}
            }
        }
        assert_eq!(starts[0].0, "a");
        assert_eq!(
            (
                starts[0].1.line,
                starts[0].1.column,
                starts[0].1.byte_offset
            ),
            (1, 1, 0)
        );
        assert_eq!(starts[1].0, "b");
        assert_eq!(
            (
                starts[1].1.line,
                starts[1].1.column,
                starts[1].1.byte_offset
            ),
            (2, 3, 6)
        );
        assert_eq!(starts[2].0, "c");
        assert_eq!(
            (
                starts[2].1.line,
                starts[2].1.column,
                starts[2].1.byte_offset
            ),
            (3, 3, 13)
        );
        let end_a = end_a.expect("end of <a>");
        assert_eq!((end_a.line, end_a.column), (4, 1));
    }

    #[test]
    fn xml_space_attribute_is_in_the_xml_namespace() {
        let xml = br#"<a xml:space="preserve"/>"#;
        let events = read_all(xml);
        match &events[0] {
            XmlEvent::StartElement { attrs, .. } => {
                assert_eq!(attrs[0].name.prefix.as_deref(), Some("xml"));
                assert_eq!(
                    attrs[0].name.ns.as_ref().unwrap(),
                    crate::xml::ns_stack::XML_NS
                );
                assert_eq!(attrs[0].name.local(), "space");
                assert_eq!(attrs[0].value, "preserve");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decodes_utf16_le_with_bom() {
        let text = "<?xml version=\"1.0\"?><a>hi</a>";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let events = read_all(&bytes);
        assert!(matches!(events[0], XmlEvent::StartElement { .. }));
    }
}
