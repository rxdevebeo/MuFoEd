//! Deterministic XML emission for Strict parts.
//!
//! The writer is a stack machine over a `String`: `start` opens an element,
//! attributes follow, `text` writes escaped character data, `end` closes. It
//! emits nothing it was not asked to emit — no attribute reordering, no
//! self-closing heuristics, no whitespace injection — because the acceptance
//! criterion is byte-identical output across runs (SC-1) and the independent
//! oracle for correctness is `roxmltree`, not our own pretty printer.

use std::fmt::Display;

/// The XML declaration written at the top of every part.
pub const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Strict `wordprocessingml/main`.
pub const NS_W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
/// Strict `officeDocument/relationships`.
pub const NS_R: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// Strict `drawingml/main`.
pub const NS_A: &str = "http://purl.oclc.org/ooxml/drawingml/main";
/// Strict `drawingml/wordprocessingDrawing`.
pub const NS_WP: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
/// Strict `drawingml/picture`.
pub const NS_PIC: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// Strict `officeDocument/math`.
pub const NS_M: &str = "http://purl.oclc.org/ooxml/officeDocument/math";
/// Microsoft `wordprocessingShape` — an extension, identical in both families.
pub const NS_WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
/// Microsoft `wordprocessingGroup` — an extension, identical in both families.
pub const NS_WPG: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";
/// Markup Compatibility and Extensibility.
pub const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// Microsoft Word 2010 wordml extensions (`w14:paraId`).
pub const NS_W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
/// Microsoft Word 2012 wordml extensions.
pub const NS_W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
/// The reserved `xml` prefix, needed for `xml:space`.
pub const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";

/// Builds a part's XML.
#[derive(Debug)]
pub struct XmlWriter {
    out: String,
    /// Byte offset of each open element's name inside `out`.
    open: Vec<usize>,
    /// For each open element, whether its start tag has been closed with `>`.
    ///
    /// An element whose tag is still open when `end()` arrives is written
    /// self-closing, so this is exactly "this element already has a child or
    /// character data". Attributes do not set it.
    tag_open: Vec<bool>,
    max_depth: usize,
    overflowed: bool,
    /// A start tag has been opened and still needs its closing `>`.
    pending_tag: bool,
    /// Byte offset of the root element's name, when [`start_root`](Self::start_root)
    /// opened one.
    root_start: Option<usize>,
    /// The namespace declarations the root was *offered*, filtered at finish time.
    root_declarations: Vec<(&'static str, &'static str)>,
    /// Every prefix the part actually wrote.
    ///
    /// A prefix declared and never used is the debt
    /// `strict-ooxml-write/tests/strict_conformance.rs` refuses; a prefix used and
    /// never declared is a part that does not parse. Both are decided here.
    used_prefixes: std::collections::BTreeSet<String>,
}

impl Default for XmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlWriter {
    /// Creates a writer with the default depth budget (256, matching
    /// `ResourceLimits::max_xml_depth`).
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_depth(256)
    }

    /// Creates a writer with an explicit element-nesting budget.
    ///
    /// The budget is enforced at [`XmlWriter::finish`], not on every `start`:
    /// the input is an already-parsed DOM whose depth the parser has already
    /// bounded, so a violation is a programming error in the writer, not a
    /// condition of the input, and there is nothing to recover from mid-element.
    #[must_use]
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            out: String::with_capacity(4096),
            open: Vec::new(),
            tag_open: Vec::new(),
            max_depth,
            overflowed: false,
            pending_tag: false,
            root_start: None,
            root_declarations: Vec::new(),
            used_prefixes: std::collections::BTreeSet::new(),
        }
    }

    /// Closes a start tag whose `>` has not been written yet.
    ///
    /// Attributes belong to the tag `start` opened, so the tag cannot be closed
    /// until the caller moves on to content, a child or the closing tag. Every
    /// method that ends the attribute run calls this first, which is why
    /// `<w:p><w:r/></w:p>` comes out right without the caller tracking it.
    fn close_tag(&mut self) {
        if self.pending_tag {
            self.out.push('>');
            self.pending_tag = false;
            if let Some(open) = self.tag_open.last_mut() {
                *open = true;
            }
        }
    }

    /// Writes the XML declaration.
    pub fn declaration(&mut self) {
        self.out.push_str(DECLARATION);
    }

    /// Opens an element. `name` carries its prefix, for example `w:p`.
    ///
    /// The name is not copied: only its offset is kept, and
    /// [`XmlWriter::end`] reads it back out of the buffer. A 500-page document
    /// opens hundreds of thousands of elements and none of them should cost an
    /// allocation.
    pub fn start(&mut self, name: &str) {
        self.close_tag();
        if self.open.len() >= self.max_depth {
            self.overflowed = true;
        }
        self.used_prefixes.insert(prefix_of(name));
        self.out.push('<');
        self.open.push(self.out.len());
        self.tag_open.push(false);
        self.out.push_str(name);
        self.pending_tag = true;
    }

    /// Starts the root element of a part and declares the namespaces it **may** use.
    ///
    /// The declarations are filtered at [`finish`](Self::finish) down to the ones
    /// the part actually used, which is the only place that can know: a header
    /// that holds a drawing needs `wp`/`a`/`pic` and a header that does not must
    /// not declare them, and no per-part table gets that right for both without
    /// every part listing every vocabulary it might contain.
    ///
    /// That asymmetry is not cosmetic. A part that declares a prefix it never uses
    /// is the unused-declaration debt `strict-ooxml-write/tests/strict_conformance.rs`
    /// refuses; a part that uses a prefix it did not declare **does not parse**, and
    /// the reader that finds out is one step removed from the writer that broke it.
    pub fn start_root(&mut self, name: &str, namespaces: &[(&'static str, &'static str)]) {
        self.declaration();
        // Before `start`, not after: `start` records the name's prefix as used, and
        // clearing afterwards threw away the root's own prefix — which is how
        // `word/fontTable.xml` came out with `<w:fonts>` and no `xmlns:w`.
        self.used_prefixes.clear();
        self.start(name);
        self.root_start = Some(self.open.last().copied().unwrap_or(0));
        self.root_declarations = namespaces.to_vec();
    }

    /// Writes an attribute. `name` carries its prefix when it has one.
    pub fn attr(&mut self, name: &str, value: impl Display) {
        self.used_prefixes.insert(prefix_of(name));
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        escape_attr_into(&mut self.out, &value.to_string());
        self.out.push('"');
    }

    /// Writes an attribute when the value is present.
    pub fn attr_opt(&mut self, name: &str, value: Option<impl Display>) {
        if let Some(value) = value {
            self.attr(name, value);
        }
    }

    /// Writes a `w:`-namespaced attribute when the value is present.
    pub fn attr_w_opt(&mut self, local: &str, value: Option<impl Display>) {
        self.attr_opt(&format!("w:{local}"), value);
    }

    /// Writes a `w:`-namespaced attribute unconditionally.
    pub fn attr_w(&mut self, local: &str, value: impl Display) {
        self.attr(&format!("w:{local}"), value);
    }

    /// Writes an `m:`-namespaced attribute when the value is present.
    pub fn attr_m_opt(&mut self, local: &str, value: Option<impl Display>) {
        self.attr_opt(&format!("m:{local}"), value);
    }

    /// Writes an `m:`-namespaced attribute unconditionally.
    pub fn attr_m(&mut self, local: &str, value: impl Display) {
        self.attr(&format!("m:{local}"), value);
    }

    /// Writes an `r:`-namespaced attribute when the value is present.
    pub fn attr_r_opt(&mut self, local: &str, value: Option<impl Display>) {
        self.attr_opt(&format!("r:{local}"), value);
    }

    /// Writes escaped character data.
    pub fn text(&mut self, value: &str) {
        self.close_tag();
        escape_text_into(&mut self.out, value);
    }

    /// Closes the innermost element.
    ///
    /// An element that received no content is written self-closing
    /// (`<w:sz w:val="32"/>`) rather than as an empty pair, which is both what
    /// every real producer emits and what the project's own reader's
    /// `skip_element` expects: it costs the reader a round trip through
    /// `skip_element` for the explicit form and shows up as a difference
    /// between a written part and the part it was parsed from.
    ///
    /// Closing more elements than were opened is a no-op: the writer is used
    /// from straight-line code where the balance is checked by the tests, and a
    /// `debug_assert` cannot fire in a release build a caller relies on.
    pub fn end(&mut self) {
        let Some(name_start) = self.open.pop() else {
            return;
        };
        self.tag_open.pop();
        if self.pending_tag {
            self.pending_tag = false;
            self.out.push_str("/>");
            return;
        }
        // The name ends at the first delimiter after it: a space starts the
        // attributes, `>` ends the tag. Scanning for `>` alone would swallow the
        // attributes into the name.
        let name_end = self.out[name_start..]
            .find([' ', '>', '/'])
            .map_or(self.out.len(), |offset| name_start + offset);
        let name = self.out[name_start..name_end].to_owned();
        self.out.push_str("</");
        self.out.push_str(&name);
        self.out.push('>');
    }

    /// Writes a self-closing element.
    pub fn empty(&mut self, name: &str) {
        self.close_tag();
        self.used_prefixes.insert(prefix_of(name));
        self.out.push('<');
        self.out.push_str(name);
        self.out.push_str("/>");
    }

    /// Writes a self-closing element with one attribute.
    pub fn empty_attr(&mut self, name: &str, attr: &str, value: impl Display) {
        self.close_tag();
        self.used_prefixes.insert(prefix_of(name));
        self.used_prefixes.insert(prefix_of(attr));
        self.out.push('<');
        self.out.push_str(name);
        self.out.push(' ');
        self.out.push_str(attr);
        self.out.push_str("=\"");
        escape_attr_into(&mut self.out, &value.to_string());
        self.out.push_str("\"/>");
    }

    /// Writes a self-closing element with one `w:`-namespaced attribute.
    pub fn empty_attr_w(&mut self, name: &str, local: &str, value: impl Display) {
        self.empty_attr(name, &format!("w:{local}"), value);
    }

    /// Returns `true` when an element was still open.
    #[must_use]
    pub fn has_open_elements(&self) -> bool {
        !self.open.is_empty()
    }

    /// Returns `true` when the innermost open element has content or attributes.
    ///
    /// Written as `start` + nothing leaves the tag pending and the element
    /// self-closing, so `<wp:positionH/>` is what comes out; a caller that has to
    /// fill a required child needs to know that before calling `end`.
    #[must_use]
    pub fn has_content(&self) -> bool {
        self.tag_open.last().copied().unwrap_or(false)
    }

    /// Returns the nesting depth reached.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.open.len()
    }

    /// Consumes the writer, returning the XML text.
    ///
    /// # Errors
    ///
    /// Returns [`WriteError::DepthExceeded`] when the document nested deeper
    /// than the budget, and [`WriteError::Unbalanced`] when an element was left
    /// open.
    pub fn finish(mut self) -> Result<String, WriteError> {
        if self.overflowed {
            return Err(WriteError::DepthExceeded(self.max_depth));
        }
        if !self.open.is_empty() {
            return Err(WriteError::Unbalanced(self.open.len()));
        }
        self.close_tag();
        self.write_root_declarations();
        let mut out = self.out;
        out.push('\n');
        Ok(out)
    }

    /// Inserts the root's `xmlns:*` declarations, filtered to what the part used.
    ///
    /// Written here rather than in [`start_root`](Self::start_root) because the
    /// answer is not known until the part is finished, and the insertion point is
    /// unambiguous: the declarations go **immediately after the root element's
    /// name**, which is where `close_tag` already computes the boundary — the
    /// first character that is a space, a `>` or a `/`. Inserting before the `>`
    /// instead looks equivalent and is not: a part with no children writes a
    /// self-closing root, `<w:fonts/>`, and there the `>` is preceded by a `/`,
    /// so the declarations land between the two and the part stops being XML.
    fn write_root_declarations(&mut self) {
        if self.root_declarations.is_empty() {
            return;
        }
        let Some(at) = self.root_start else {
            return;
        };
        let mut declarations = String::new();
        for (prefix, uri) in &self.root_declarations {
            if !self.used_prefixes.contains(*prefix) {
                continue;
            }
            declarations.push_str(" xmlns:");
            declarations.push_str(prefix);
            declarations.push_str("=\"");
            escape_attr_into(&mut declarations, uri);
            declarations.push('"');
        }
        if declarations.is_empty() {
            return;
        }
        let name_end = self.out[at..]
            .find([' ', '>', '/'])
            .map_or(self.out.len(), |offset| at + offset);
        self.out.insert_str(name_end, &declarations);
    }
}

/// The namespace prefix of a name, or the empty string when it has none.
///
/// `w:p` is `w`; `p` is nothing, and `a:b:c` is `a` because the second colon is
/// part of the local name as far as any consumer is concerned.
fn prefix_of(name: &str) -> String {
    name.split_once(':')
        .map_or_else(String::new, |(prefix, _)| prefix.to_owned())
}

/// Failures the writer itself can raise, independent of the DOM it walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WriteError {
    /// The document nested deeper than the configured budget.
    DepthExceeded(usize),
    /// An element was left open when the part was finished.
    Unbalanced(usize),
}

impl Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DepthExceeded(limit) => {
                write!(f, "XML nesting deeper than the writer budget of {limit}")
            }
            Self::Unbalanced(open) => write!(f, "{open} XML element(s) left open"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Escapes character data.
fn escape_text_into(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // A raw CR would be normalized to LF by an XML parser, so it is
            // written as a character reference to survive the round trip.
            '\r' => out.push_str("&#13;"),
            _ => out.push(ch),
        }
    }
}

/// Escapes an attribute value (written inside double quotes).
fn escape_attr_into(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WriteError, XmlWriter};

    #[test]
    fn writes_nested_elements() {
        let mut xml = XmlWriter::new();
        xml.declaration();
        xml.start("w:document");
        xml.attr("xmlns:w", "urn:w");
        xml.start("w:body");
        xml.empty_attr_w("w:p", "val", "1");
        xml.end();
        xml.end();
        let text = xml.finish().expect("balanced");
        assert_eq!(
            text,
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <w:document xmlns:w=\"urn:w\"><w:body><w:p w:val=\"1\"/></w:body></w:document>\n"
        );
    }

    #[test]
    fn escapes_text_and_attributes() {
        let mut xml = XmlWriter::new();
        xml.start("w:t");
        xml.text("a<b>&c\r\nd");
        xml.end();
        let text = xml.finish().expect("balanced");
        assert!(text.contains("a&lt;b&gt;&amp;c&#13;\nd"), "{text}");

        let mut xml = XmlWriter::new();
        xml.start("w:t");
        xml.attr("w:val", "q\"x'y\tz");
        xml.end();
        let text = xml.finish().expect("balanced");
        assert!(text.contains("w:val=\"q&quot;x&apos;y&#9;z\""), "{text}");
    }

    #[test]
    fn a_depth_overflow_is_reported() {
        let mut xml = XmlWriter::with_max_depth(2);
        xml.start("a");
        xml.start("b");
        xml.start("c");
        xml.end();
        xml.end();
        xml.end();
        assert_eq!(xml.finish(), Err(WriteError::DepthExceeded(2)));
    }

    #[test]
    fn an_unbalanced_element_is_reported() {
        let mut xml = XmlWriter::new();
        xml.start("w:p");
        assert!(xml.has_open_elements());
        assert_eq!(xml.finish(), Err(WriteError::Unbalanced(1)));
    }
}
