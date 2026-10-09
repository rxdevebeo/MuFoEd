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

use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

use crate::control;
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
    /// One tokenizer for the whole part. Recreating `quick_xml::Reader` on a
    /// mid-document slice that began with UTF-8 `EF BB BF` made quick-xml treat
    /// U+FEFF text as a stream BOM (R01): the character disappeared and the
    /// next three bytes were read twice. A persistent reader runs BOM detection
    /// only once, at the start of the already-normalized buffer.
    reader: Reader<Cursor<Vec<u8>>>,
    /// Scratch buffer for [`Reader::read_event_into`].
    scratch: Vec<u8>,
    pos: usize,
    event_start: usize,
    limits: ResourceLimits,
    ns: NsStack,
    depth: u32,
    text_total: u64,
    /// Start and empty-element tags read so far in this part, against
    /// `max_xml_elements`.
    elements: u64,
    /// Attribute bytes (names plus raw values) read so far in this part,
    /// against `max_text_len` but separately from `text_total`.
    attr_total: u64,
    open_names: Vec<String>,
    pending_end: Option<QName>,
    /// Whether the document's single root element has been opened.
    ///
    /// XML 1.0 §2.1 gives a well-formed document exactly one top-level element.
    /// The reader enforced the nesting but never the singleton, so a part with two
    /// roots parsed as if the first one closed the document - and a part with none
    /// parsed as an empty one, which is how a truncated `fontTable.xml` became a
    /// font table with no fonts instead of an error.
    root_seen: bool,
    /// Whether that root element has been closed; after it, only misc may follow.
    root_closed: bool,
    scanned: usize,
    line: u32,
    column: u32,
    event_line: u32,
    event_col: u32,
    /// Events returned so far, for spacing the [`control`] checkpoints.
    events: u32,
    /// Whether this part is the one the installed control's progress measures.
    tracked: bool,
}

/// How many events pass between two [`control::checkpoint`] calls.
const CHECKPOINT_EVERY: u32 = 256;

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
        let mut reader = Reader::from_reader(Cursor::new(data));
        reader.config_mut().check_end_names = false;
        reader.config_mut().allow_unmatched_ends = true;
        Ok(Self {
            reader,
            scratch: Vec::new(),
            pos: 0,
            event_start: 0,
            limits: *limits,
            ns: NsStack::new(),
            depth: 0,
            text_total: 0,
            elements: 0,
            attr_total: 0,
            open_names: Vec::new(),
            pending_end: None,
            root_seen: false,
            root_closed: false,
            scanned: 0,
            line: 1,
            column: 1,
            event_line: 1,
            event_col: 1,
            events: 0,
            tracked: control::is_tracked(&part),
            part,
        })
    }

    /// Borrowed view of the normalized UTF-8 part bytes.
    fn data(&self) -> &[u8] {
        self.reader.get_ref().get_ref()
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
    /// The stream is checked as XML 1.0 requires, not merely as a token stream:
    /// the end of the input with an element still open, a second top-level
    /// element and a missing root are all errors rather than a clean `Eof`.
    /// A truncated part is not a shorter part.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] on malformed XML, a forbidden construct, an
    /// unbound prefix or a resource-limit violation.
    pub fn next_event(&mut self) -> Result<XmlEvent> {
        self.events = self.events.wrapping_add(1);
        if self.events % CHECKPOINT_EVERY == 0 {
            control::checkpoint()?;
            if self.tracked {
                control::advance(self.reader.buffer_position());
            }
        }
        loop {
            if let Some(name) = self.pending_end.take() {
                self.close_scope();
                return Ok(XmlEvent::EndElement { name });
            }
            match self.read_parsed()? {
                Parsed::Start { raw, attrs, empty } => {
                    return self.open_element(raw, attrs, empty);
                }
                Parsed::End { raw } => return self.close_element(&raw),
                Parsed::Text(text) => return self.push_text(text, false),
                Parsed::CData(text) => return self.push_text(text, true),
                Parsed::Eof => return self.end_of_document(),
                Parsed::Skip => {}
            }
        }
    }

    /// Ends the stream, refusing to report a truncated document as a complete
    /// one.
    ///
    /// `Eof` is idempotent: once the input is exhausted and the document is well
    /// formed there is nothing left to check, and a reader polled past the end
    /// gets `Eof` again rather than an error it has already been told about.
    fn end_of_document(&self) -> Result<XmlEvent> {
        if !self.root_seen {
            return Err(self.invalid_xml("no root element".to_owned(), 0));
        }
        // `pending_end` is drained at the top of `next_event`, so a document that
        // ended inside an empty element still has that element on `open_names`
        // only if it was never closed; the check is kept for the general case.
        let unclosed = self.open_names.len() + usize::from(self.pending_end.is_some());
        if unclosed > 0 {
            let innermost = self
                .open_names
                .last()
                .map_or_else(String::new, Clone::clone);
            return Err(self.invalid_xml(
                format!("unexpected end of document: {unclosed} unclosed element(s), innermost <{innermost}>"),
                0,
            ));
        }
        Ok(XmlEvent::Eof)
    }

    /// Rejects anything that is not the single root element's opening tag or the
    /// misc that may precede it.
    fn open_element(
        &mut self,
        raw: String,
        attrs: Vec<(String, String)>,
        empty: bool,
    ) -> Result<XmlEvent> {
        if self.depth == 0 {
            if self.root_closed {
                return Err(self.invalid_xml("content after the root element".to_owned(), 0));
            }
            self.root_seen = true;
        }
        // The element count and the attribute count and bytes were checked in
        // `own_start`, before the attribute strings were allocated.
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
        self.close_scope();
        self.open_names.pop();
        Ok(XmlEvent::EndElement { name })
    }

    /// Leaves one element: pops its namespace scope and records that the document
    /// root is behind us once nothing is open.
    fn close_scope(&mut self) {
        self.ns.pop_scope();
        self.depth = self.depth.saturating_sub(1);
        if self.depth == 0 {
            self.root_closed = true;
        }
    }

    fn push_text(&mut self, text: String, cdata: bool) -> Result<XmlEvent> {
        // XML 1.0 §2.8: the prolog holds only misc (comments, PIs, whitespace),
        // so character data before the root is as malformed as after it.
        if !self.root_seen && !text.trim().is_empty() {
            return Err(self.invalid_xml("content before the root element".to_owned(), 0));
        }
        if self.root_closed && !text.trim().is_empty() {
            return Err(self.invalid_xml("content after the root element".to_owned(), 0));
        }
        self.text_total = self.text_total.saturating_add(text.len() as u64);
        safety::check_text(self.text_total, &self.limits)?;
        if cdata {
            Ok(XmlEvent::CData(text))
        } else {
            Ok(XmlEvent::Text(text))
        }
    }

    /// Reads one low-level event and copies it into an owned `Parsed` value.
    fn read_parsed(&mut self) -> Result<Parsed> {
        self.event_start = self.pos;
        self.advance_to(self.event_start);
        self.event_line = self.line;
        self.event_col = self.column;
        let start = self.pos;
        let err_location = SourceLocation::new(
            self.part.clone(),
            self.event_line,
            self.event_col,
            self.event_start as u64,
        );
        let invalid = |detail: String| -> StrictError {
            StrictError::InvalidXml {
                location: err_location.clone(),
                detail,
            }
        };

        self.scratch.clear();
        let event = self
            .reader
            .read_event_into(&mut self.scratch)
            .map_err(|error| invalid(format!("{error}")))?;
        let position = self.reader.buffer_position();
        // Copy out of `scratch` before any further `&self` use: the event
        // borrows that buffer for its lifetime.
        let parsed = match event {
            Event::Start(e) => Self::own_start(
                &e,
                false,
                &self.limits,
                &mut self.elements,
                &mut self.attr_total,
                &invalid,
            )?,
            Event::Empty(e) => Self::own_start(
                &e,
                true,
                &self.limits,
                &mut self.elements,
                &mut self.attr_total,
                &invalid,
            )?,
            Event::End(e) => {
                let raw = std::str::from_utf8(e.name().as_ref())
                    .map(str::to_owned)
                    .map_err(|error| invalid(format!("invalid name: {error}")))?;
                Parsed::End { raw }
            }
            Event::Text(t) => {
                let text = t
                    .xml10_content()
                    .map_err(|error| invalid(format!("{error}")))?
                    .into_owned();
                Parsed::Text(text)
            }
            Event::GeneralRef(reference) => {
                Parsed::Text(Self::decode_reference(&reference, &invalid)?)
            }
            Event::CData(c) => {
                let text = std::str::from_utf8(&c.into_inner())
                    .map_err(|error| invalid(format!("{error}")))?
                    .to_owned();
                Parsed::CData(text)
            }
            Event::DocType(_) => {
                return Err(invalid("DOCTYPE declarations are not allowed".to_owned()));
            }
            Event::Comment(_) | Event::PI(_) | Event::Decl(_) => Parsed::Skip,
            Event::Eof => Parsed::Eof,
        };
        self.pos = usize::try_from(position)
            .map_err(|_| invalid(format!("position overflow at {start}")))?;
        Ok(parsed)
    }

    /// Owns a start/empty tag's name and attributes.
    ///
    /// Every budget is charged before the allocation it bounds: the element
    /// count before the name is copied, and for each attribute the attribute
    /// count and the raw bytes of its name and value before either is copied.
    /// The count used to be checked in `open_element`, after a `String` pair had
    /// been allocated for every attribute. The raw value is charged rather than
    /// the normalized one because normalization never makes it longer.
    fn own_start(
        element: &quick_xml::events::BytesStart<'_>,
        empty: bool,
        limits: &ResourceLimits,
        elements: &mut u64,
        attr_total: &mut u64,
        invalid: &dyn Fn(String) -> StrictError,
    ) -> Result<Parsed> {
        *elements = elements.saturating_add(1);
        safety::check_elements(*elements, limits)?;
        let raw = std::str::from_utf8(element.name().as_ref())
            .map(str::to_owned)
            .map_err(|error| invalid(format!("invalid name: {error}")))?;
        let mut attrs = Vec::new();
        for attribute in element.attributes() {
            let attribute = attribute.map_err(|error| invalid(format!("{error}")))?;
            safety::check_attributes(attrs.len().saturating_add(1), limits)?;
            let raw_len = attribute
                .key
                .as_ref()
                .len()
                .saturating_add(attribute.value.len());
            *attr_total = attr_total.saturating_add(raw_len as u64);
            safety::check_attribute_bytes(*attr_total, limits)?;
            let key = std::str::from_utf8(attribute.key.as_ref())
                .map_err(|error| invalid(format!("{error}")))?
                .to_owned();
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|error| invalid(format!("{error}")))?
                .into_owned();
            attrs.push((key, value));
        }
        Ok(Parsed::Start { raw, attrs, empty })
    }

    /// Resolves a character or predefined entity while the tokenizer still
    /// borrows the scratch buffer.
    fn decode_reference(
        reference: &quick_xml::events::BytesRef<'_>,
        invalid: &dyn Fn(String) -> StrictError,
    ) -> Result<String> {
        if reference.is_char_ref() {
            let ch = reference
                .resolve_char_ref()
                .map_err(|error| invalid(format!("{error}")))?
                .ok_or_else(|| invalid("invalid character reference".to_owned()))?;
            return Ok(ch.to_string());
        }
        let name = reference
            .decode()
            .map_err(|error| invalid(format!("{error}")))?;
        let ch = match name.as_ref() {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            other => {
                return Err(invalid(format!(
                    "entity reference '&{other};' is not allowed"
                )));
            }
        };
        Ok(ch.to_string())
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
        let len = self.data().len();
        let target = target.min(len);
        if target <= self.scanned {
            return;
        }
        let from = self.scanned;
        let mut line = self.line;
        let mut column = self.column;
        for &byte in self.data().get(from..target).unwrap_or_default() {
            if byte == b'\n' {
                line = line.saturating_add(1);
                column = 1;
            } else {
                column = column.saturating_add(1);
            }
        }
        self.line = line;
        self.column = column;
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
        let head = bytes.get(..head_len).unwrap_or_default();
        if let Some(label) = sniff_declared_encoding(head) {
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
    let (decl, _) = text.split_once("?>")?;
    let (_, after) = decl.split_once("encoding")?;
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let (value, _) = after.strip_prefix(quote)?.split_once(quote)?;
    Some(value.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

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

    /// Drains `xml`, returning the error the first failure produced.
    fn read_error(xml: &[u8]) -> StrictError {
        let mut reader = XmlReader::new(xml, part(), &ResourceLimits::default()).unwrap();
        loop {
            match reader.next_event() {
                Ok(XmlEvent::Eof) => panic!("expected an error, the document parsed"),
                Ok(_) => {}
                Err(error) => return error,
            }
        }
    }

    /// The `detail` of an `InvalidXml` error.
    fn detail(error: StrictError) -> String {
        match error {
            StrictError::InvalidXml { detail, .. } => detail,
            other => panic!("expected InvalidXml, got {other:?}"),
        }
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

    // AUD-04: the end of the input is checked as XML, not accepted as a token.

    #[test]
    fn end_of_document_with_open_elements_is_an_error_naming_them() {
        let detail = detail(read_error(b"<a><b>"));
        assert!(detail.contains("2 unclosed"), "{detail}");
        assert!(detail.contains("<b>"), "{detail}");
        assert!(detail.starts_with("unexpected end of document"), "{detail}");
    }

    #[test]
    fn a_second_root_element_is_an_error() {
        let detail = detail(read_error(b"<a/><b/>"));
        assert!(
            detail.contains("content after the root element"),
            "{detail}"
        );
    }

    #[test]
    fn non_whitespace_text_after_the_root_is_an_error() {
        let detail = detail(read_error(b"<a/>text"));
        assert!(
            detail.contains("content after the root element"),
            "{detail}"
        );
    }

    #[test]
    fn whitespace_comments_and_pis_may_follow_the_root() {
        let events = read_all(b"<a/>  <!--c--><?pi?>  \n ");
        assert!(matches!(events.last(), Some(XmlEvent::Eof)), "{events:?}");
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, XmlEvent::Text(text) if !text.trim().is_empty())),
            "{events:?}"
        );
    }

    // Audit 2026-10-09 §3.2: the prolog holds misc only.

    #[test]
    fn non_whitespace_text_before_the_root_is_an_error() {
        let detail = detail(read_error(b"text<a/>"));
        assert_eq!(detail, "content before the root element");
    }

    #[test]
    fn cdata_before_the_root_is_an_error() {
        let detail = detail(read_error(b"<![CDATA[x]]><a/>"));
        assert_eq!(detail, "content before the root element");
    }

    #[test]
    fn a_reference_before_the_root_is_an_error() {
        // Whether the tokenizer or the prolog check refuses it, it is refused.
        let detail = detail(read_error(b"&amp;<a/>"));
        assert!(!detail.is_empty());
    }

    #[test]
    fn whitespace_comments_and_pis_may_precede_the_root() {
        let events = read_all(b"<?xml version=\"1.0\"?>\r\n  <!--c-->\n<?pi?>\t<a/>");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, XmlEvent::StartElement { .. })),
            "{events:?}"
        );
        assert!(matches!(events.last(), Some(XmlEvent::Eof)), "{events:?}");
    }

    #[test]
    fn a_utf8_bom_before_the_root_is_still_accepted() {
        let mut xml = vec![0xEF, 0xBB, 0xBF];
        xml.extend_from_slice(b"<?xml version=\"1.0\"?>\n<a/>");
        let events = read_all(&xml);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, XmlEvent::StartElement { .. })),
            "{events:?}"
        );
    }

    #[test]
    fn an_empty_document_has_no_root() {
        assert_eq!(detail(read_error(b"")), "no root element");
    }

    #[test]
    fn a_declaration_alone_has_no_root() {
        assert_eq!(detail(read_error(b"<?xml?>")), "no root element");
    }

    #[test]
    fn the_unclosed_element_is_reported_on_every_later_call() {
        let mut reader = XmlReader::new(b"<a><b>", part(), &ResourceLimits::default()).unwrap();
        let mut errors = Vec::new();
        for _ in 0..3 {
            let mut error = None;
            loop {
                match reader.next_event() {
                    Ok(XmlEvent::Eof) => break,
                    Ok(_) => {}
                    Err(StrictError::InvalidXml { detail, .. }) => {
                        error = Some(detail);
                        break;
                    }
                    Err(other) => panic!("unexpected {other:?}"),
                }
            }
            errors.push(error.expect("every call past the truncation is an error"));
        }
        for message in &errors {
            assert!(message.contains("2 unclosed"), "{message}");
        }
    }

    #[test]
    fn a_well_formed_document_still_reports_eof_repeatedly() {
        let mut reader =
            XmlReader::new(b"<a><b/></a>", part(), &ResourceLimits::default()).unwrap();
        let mut eofs = 0;
        for _ in 0..8 {
            if reader.next_event().unwrap() == XmlEvent::Eof {
                eofs += 1;
            }
        }
        // four events for `<a><b/></a>`, and every later call is `Eof`.
        assert_eq!(eofs, 4);
    }

    #[test]
    fn a_truncated_cdata_section_is_an_error() {
        let detail = detail(read_error(b"<a><![CDATA[unterminated"));
        assert!(detail.contains("CDATA"), "{detail}");
    }

    // --- Dependency-regression coverage (quick-xml RUSTSEC-2026-0194 / #970 / #977 / #980) ---
    //
    // We use `Reader` (not `NsReader`) and own the namespace stack + limits, so the
    // NsReader-specific advisories do not apply directly. These tests lock the
    // wrapper contracts that keep us safe if those paths ever change.

    #[test]
    fn enforces_attribute_limit() {
        // Nine attributes with a budget of eight → LimitExceeded, not a panic.
        let mut xml = String::from("<a");
        for i in 0..9 {
            let _ = write!(xml, " a{i}=\"{i}\"");
        }
        xml.push_str("/>");
        let limits = ResourceLimits {
            max_xml_attributes_per_elem: 8,
            ..ResourceLimits::default()
        };
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::LimitExceeded {
                kind: crate::error::LimitKind::XmlAttributesPerElement,
                ..
            })
        ));
    }

    /// Audit 2026-10-09 §3.1: the attribute count is checked while the
    /// attributes are read, so the error names the first one past the budget
    /// rather than the total the element declared.
    #[test]
    fn attribute_limit_stops_at_the_first_attribute_past_the_budget() {
        let mut xml = String::from("<a");
        for i in 0..100 {
            let _ = write!(xml, " a{i}=\"{i}\"");
        }
        xml.push_str("/>");
        let limits = ResourceLimits {
            max_xml_attributes_per_elem: 8,
            ..ResourceLimits::default()
        };
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::LimitExceeded {
                kind: crate::error::LimitKind::XmlAttributesPerElement,
                limit: 8,
                actual: 9,
            })
        ));
    }

    /// Hostile-input research 2026-10-09 §2.1: elements are counted per part.
    #[test]
    fn enforces_element_limit() {
        let limits = ResourceLimits {
            max_xml_elements: 3,
            ..ResourceLimits::default()
        };
        // Exactly three elements (one of them empty) parse.
        let mut reader = XmlReader::new(b"<a><b/><c></c></a>", part(), &limits).unwrap();
        while reader.next_event().unwrap() != XmlEvent::Eof {}
        // A fourth is refused.
        let mut reader = XmlReader::new(b"<a><b/><c/><d/></a>", part(), &limits).unwrap();
        let error = loop {
            match reader.next_event() {
                Ok(XmlEvent::Eof) => panic!("expected the element limit"),
                Ok(_) => {}
                Err(error) => break error,
            }
        };
        assert!(
            matches!(
                error,
                StrictError::LimitExceeded {
                    kind: crate::error::LimitKind::XmlElements,
                    limit: 3,
                    actual: 4,
                }
            ),
            "{error:?}"
        );
    }

    /// Hostile-input research 2026-10-09 §2.2: attribute bytes are budgeted.
    #[test]
    fn enforces_attribute_byte_budget() {
        let limits = ResourceLimits {
            max_text_len: 64,
            ..ResourceLimits::default()
        };
        let value = "x".repeat(40);
        // One element with two 40-byte values: 2 * (2 + 40) > 64.
        let xml = format!("<a k1=\"{value}\" k2=\"{value}\"/>");
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::LimitExceeded {
                kind: crate::error::LimitKind::TextLen,
                ..
            })
        ));
        // The budget is per part, across elements, and not shared with text:
        // 60 bytes of text plus 42 bytes of attributes both fit.
        let text = "t".repeat(60);
        let xml = format!("<a k1=\"{value}\">{text}</a>");
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        while reader.next_event().unwrap() != XmlEvent::Eof {}
        let xml = format!("<a k1=\"{value}\"><b k2=\"{value}\"/></a>");
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        assert!(matches!(
            reader.next_event(),
            Ok(XmlEvent::StartElement { .. })
        ));
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::LimitExceeded {
                kind: crate::error::LimitKind::TextLen,
                ..
            })
        ));
    }

    #[test]
    fn many_distinct_attributes_finish_quickly() {
        // RUSTSEC-2026-0194: pre-0.41 `attributes()` was O(N²) on distinct names.
        // We are on 0.41.0 and also cap N; this pins both layers.
        let n = 512u32;
        let mut xml = String::from("<a");
        for i in 0..n {
            let _ = write!(xml, " a{i}=\"{i}\"");
        }
        xml.push_str("/>");
        let limits = ResourceLimits {
            max_xml_attributes_per_elem: n,
            ..ResourceLimits::default()
        };
        let started = std::time::Instant::now();
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        match reader.next_event().expect("start") {
            XmlEvent::StartElement { attrs, .. } => assert_eq!(attrs.len(), n as usize),
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "attribute scan took {:?}; quadratic duplicate-check may have regressed",
            started.elapsed()
        );
    }

    #[test]
    fn many_xmlns_declarations_count_toward_the_attribute_limit() {
        // Sibling of RUSTSEC-2026-0195: NsReader allocated unboundedly for xmlns.
        // Our wrapper treats xmlns as attributes, so the same budget applies.
        let mut xml = String::from("<a");
        for i in 0..20 {
            let _ = write!(xml, " xmlns:p{i}=\"urn:{i}\"");
        }
        xml.push_str("/>");
        let limits = ResourceLimits {
            max_xml_attributes_per_elem: 16,
            ..ResourceLimits::default()
        };
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::LimitExceeded {
                kind: crate::error::LimitKind::XmlAttributesPerElement,
                ..
            })
        ));
    }

    #[test]
    fn duplicate_attribute_names_are_rejected() {
        let mut reader =
            XmlReader::new(br#"<a x="1" x="2"/>"#, part(), &ResourceLimits::default()).unwrap();
        assert!(matches!(
            reader.next_event(),
            Err(StrictError::InvalidXml { .. })
        ));
    }

    #[test]
    fn deep_nesting_with_per_level_xmlns_hits_our_depth_limit() {
        // quick-xml #977/#980 target NsReader depth/quadratic resolve. We never
        // use NsReader; depth is ours. A document that nests past the budget
        // must be LimitExceeded, not a panic or corrupted scope.
        let depth = 40u32;
        let mut xml = String::new();
        for i in 0..depth {
            let _ = write!(xml, "<e{i} xmlns:p{i}=\"urn:{i}\">");
        }
        for i in (0..depth).rev() {
            let _ = write!(xml, "</e{i}>");
        }
        let limits = ResourceLimits {
            max_xml_depth: 32,
            ..ResourceLimits::default()
        };
        let mut reader = XmlReader::new(xml.as_bytes(), part(), &limits).unwrap();
        let mut saw_limit = false;
        loop {
            match reader.next_event() {
                Ok(XmlEvent::Eof) => break,
                Ok(_) => {}
                Err(StrictError::LimitExceeded {
                    kind: crate::error::LimitKind::XmlDepth,
                    ..
                }) => {
                    saw_limit = true;
                    break;
                }
                Err(other) => panic!("unexpected {other:?}"),
            }
        }
        assert!(saw_limit, "expected XmlDepth LimitExceeded");
    }

    /// Collects the exact Unicode scalar values of every text/CDATA event.
    fn text_code_points(xml: &[u8]) -> Vec<u32> {
        let mut reader = XmlReader::new(xml, part(), &ResourceLimits::default()).unwrap();
        let mut out = Vec::new();
        loop {
            match reader.next_event().unwrap() {
                XmlEvent::Text(text) | XmlEvent::CData(text) => {
                    out.extend(text.chars().map(u32::from));
                }
                XmlEvent::Eof => break,
                _ => {}
            }
        }
        out
    }

    /// R01: a mid-document U+FEFF (UTF-8 `EF BB BF`) is character data, not a
    /// stream BOM. Recreating `quick_xml::Reader` on a slice that begins with
    /// those bytes used to drop the FEFF and re-read the last three text bytes.
    #[test]
    fn text_node_leading_feff_preserves_exact_code_points() {
        // U+FEFF + தமிழ் (Tamil) — the audit fixture's payload.
        let expected: Vec<u32> = vec![0xFEFF, 0x0BA4, 0x0BAE, 0x0BBF, 0x0BB4, 0x0BCD];
        let mut xml = b"<t xml:space=\"preserve\">".to_vec();
        xml.extend_from_slice("\u{FEFF}\u{0BA4}\u{0BAE}\u{0BBF}\u{0BB4}\u{0BCD}".as_bytes());
        xml.extend_from_slice(b"</t>");
        assert_eq!(text_code_points(&xml), expected);
    }

    #[test]
    fn text_node_feff_in_middle_and_end_is_kept() {
        let xml = "<t>a\u{FEFF}b\u{FEFF}</t>".as_bytes();
        assert_eq!(
            text_code_points(xml),
            vec![u32::from('a'), 0xFEFF, u32::from('b'), 0xFEFF]
        );
    }

    #[test]
    fn text_node_feff_only_is_kept() {
        let xml = "<t>\u{FEFF}</t>".as_bytes();
        assert_eq!(text_code_points(xml), vec![0xFEFF]);
    }

    #[test]
    fn document_utf8_bom_is_stripped_but_text_feff_remains() {
        let mut xml = vec![0xEF, 0xBB, 0xBF];
        xml.extend_from_slice("<t>\u{FEFF}x</t>".as_bytes());
        assert_eq!(text_code_points(&xml), vec![0xFEFF, u32::from('x')]);
    }

    #[test]
    fn text_node_leading_feff_with_ascii_cyrillic_and_supplementary() {
        // ASCII, Cyrillic, and a supplementary plane emoji after a leading FEFF.
        let text = "\u{FEFF}AБ\u{1F600}";
        let expected: Vec<u32> = text.chars().map(u32::from).collect();
        let xml = format!("<t>{text}</t>");
        assert_eq!(text_code_points(xml.as_bytes()), expected);
    }

    /// Inverse control for R01: the pre-fix pattern (new `Reader` on each
    /// remaining slice) still corrupts a leading U+FEFF by dropping it and
    /// re-reading the last three text bytes on the next event. Kept so a
    /// regression to slice recreation cannot silently pass the positive tests.
    #[test]
    fn inverse_recreated_slice_reader_corrupts_leading_feff() {
        use quick_xml::events::Event;
        use quick_xml::reader::Reader;

        let mut xml = b"<t>".to_vec();
        xml.extend_from_slice("\u{FEFF}\u{0BA4}\u{0BAE}\u{0BBF}\u{0BB4}\u{0BCD}".as_bytes());
        xml.extend_from_slice(b"</t>");
        let mut pos = 3; // payload after `<t>`
        let mut joined = String::new();
        for _ in 0..8 {
            let mut reader = Reader::from_reader(&xml[pos..]);
            reader.config_mut().check_end_names = false;
            reader.config_mut().allow_unmatched_ends = true;
            let event = reader.read_event().unwrap();
            let consumed = usize::try_from(reader.buffer_position()).unwrap();
            pos += consumed;
            match event {
                Event::Text(text) => joined.push_str(&text.xml10_content().unwrap()),
                Event::End(_) | Event::Eof => break,
                _ => {}
            }
        }
        let points: Vec<u32> = joined.chars().map(u32::from).collect();
        assert_eq!(
            points,
            vec![0x0BA4, 0x0BAE, 0x0BBF, 0x0BB4, 0x0BCD, 0x0BCD],
            "inverse signature: FEFF lost and last Tamil sign duplicated"
        );
        // Production reader must not share that signature.
        assert_eq!(
            text_code_points(&xml),
            vec![0xFEFF, 0x0BA4, 0x0BAE, 0x0BBF, 0x0BB4, 0x0BCD]
        );
    }
}
