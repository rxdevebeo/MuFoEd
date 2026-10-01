//! Inline-level model: runs, text, fields, breaks and hyperlinks.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;

use super::block::SdtContainer;
use super::drawing::Drawing;
use super::math::{MathExpression, MathParagraph};
use super::props::RunProperties;
use super::values::{BreakKind, FieldCharType, Space};

/// A text node with its `xml:space` handling (`w:t`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextNode {
    /// The text content.
    pub text: String,
    /// `xml:space` semantics.
    pub space: Space,
}

/// A symbol character (`w:sym`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// Symbol font name.
    pub font: Arc<str>,
    /// Character code point.
    pub character: char,
}

/// A field character (`w:fldChar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldChar {
    /// Character kind.
    pub kind: FieldCharType,
    /// Preserve formatting during update.
    pub dirty: bool,
}

/// Content of a run (`w:r`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunContent {
    /// Text (`w:t`).
    Text(TextNode),
    /// Tab (`w:tab`).
    Tab,
    /// Break (`w:br`).
    Break(BreakKind),
    /// Carriage return (`w:cr`).
    CarriageReturn,
    /// Inline drawing (`w:drawing`).
    Drawing(Drawing),
    /// Field instruction text (`w:instrText`).
    InstrText(String),
    /// Field character (`w:fldChar`).
    FieldChar(FieldChar),
    /// Footnote reference (`w:footnoteReference`); rendering is Stage 5.
    FootnoteRef(u32),
    /// Endnote reference (`w:endnoteReference`); rendering is Stage 5.
    EndnoteRef(u32),
    /// The number marker inside a footnote/endnote body (`w:footnoteRef`/
    /// `w:endnoteRef`); replaced by the note's number when rendered.
    NoteRef,
    /// Symbol (`w:sym`).
    Symbol(Symbol),
    /// Last rendered page break (`w:lastRenderedPageBreak`).
    LastRenderedPageBreak,
    /// Non-breaking hyphen (`w:noBreakHyphen`).
    NoBreakHyphen,
    /// Soft hyphen (`w:softHyphen`).
    SoftHyphen,
    /// Unknown content preserved for reporting.
    Opaque(OpaqueInline),
}

/// A run (`w:r`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// Run properties.
    pub props: RunProperties,
    /// Run content.
    pub content: Vec<RunContent>,
    /// Source location of the run.
    pub location: SourceLocation,
}

/// A hyperlink (`w:hyperlink`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hyperlink {
    /// Relationship id (`r:id`), for external/internal targets.
    pub rel_id: Option<RelId>,
    /// Document anchor (`w:anchor`).
    pub anchor: Option<Arc<str>>,
    /// Tooltip (`w:tooltip`).
    pub tooltip: Option<Arc<str>>,
    /// Child inlines.
    pub inlines: Vec<Inline>,
    /// Source location.
    pub location: SourceLocation,
}

/// A simple field (`w:fldSimple`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Field instruction (`w:instr`).
    pub instruction: Option<Arc<str>>,
    /// Cached field result content.
    pub inlines: Vec<Inline>,
    /// Source location.
    pub location: SourceLocation,
}

/// Identifier of a bookmark (`w:bookmarkStart`/`w:bookmarkEnd`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookmarkId(Arc<str>);

impl BookmarkId {
    /// Creates a bookmark id.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A `w:bookmarkStart`: the id that pairs it with its end, and the name.
///
/// Two attributes, two jobs, and the model used to keep only one of them. `w:id`
/// is what pairs the start with its `w:bookmarkEnd`; `w:name` is what a
/// `w:hyperlink/@w:anchor` and a `REF` field point at - it is the bookmark's
/// identity to everything outside the pair. Writing the id alone made the element
/// schema-invalid (`CT_Bookmark` makes `w:name` `use="required"`, `XS-20`) and
/// would have broken every internal link, so the name is carried.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Bookmark {
    /// `w:id`, pairing the start with its end.
    pub id: BookmarkId,
    /// `w:name`, the bookmark's name in the document.
    pub name: Arc<str>,
}

impl Bookmark {
    /// Creates a bookmark from its id and name.
    pub fn new(id: impl Into<Arc<str>>, name: impl Into<Arc<str>>) -> Self {
        Self {
            id: BookmarkId::new(id),
            name: name.into(),
        }
    }
}

/// Identifier of a comment range mark (`w:commentRangeStart` and friends).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommentId(Arc<str>);

impl CommentId {
    /// Creates a comment id.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An unknown inline element preserved for the support report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueInline {
    /// Namespace URI (empty when none).
    pub namespace: Arc<str>,
    /// Local name.
    pub local: Arc<str>,
    /// Attributes of the element.
    pub attributes: Vec<(Arc<str>, Arc<str>)>,
    /// Source location.
    pub location: SourceLocation,
}

impl OpaqueInline {
    /// Returns the qualified name (`prefix:local` style, namespace in parentheses).
    #[must_use]
    pub fn feature_id(&self) -> String {
        if self.namespace.is_empty() {
            self.local.to_string()
        } else {
            format!("w:{}", self.local)
        }
    }
}

/// Inline-level content of a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    /// Run.
    Run(Run),
    /// Hyperlink.
    Hyperlink(Hyperlink),
    /// Simple field.
    Field(Field),
    /// Drawing.
    Drawing(Drawing),
    /// Explicit break between runs.
    Break(BreakKind),
    /// Tab.
    Tab,
    /// Inline structured document tag.
    SdtInline(SdtContainer),
    /// Bookmark start, with the id and the name.
    BookmarkStart(Bookmark),
    /// Bookmark end.
    BookmarkEnd(BookmarkId),
    /// Comment range start (comment bodies are Stage 5).
    CommentRangeStart(CommentId),
    /// Comment range end.
    CommentRangeEnd(CommentId),
    /// Comment reference.
    CommentReference(CommentId),
    /// Footnote reference (rendering is Stage 5).
    FootnoteRef(u32),
    /// Endnote reference (rendering is Stage 5).
    EndnoteRef(u32),
    /// An inline formula (`m:oMath`, Stage 5C).
    Math(MathExpression),
    /// A display formula (`m:oMathPara`, Stage 5C).
    MathParagraph(MathParagraph),
    /// Unknown inline element.
    Opaque(OpaqueInline),
}

impl Inline {
    /// Returns the run, if this inline is one.
    #[must_use]
    pub fn as_run(&self) -> Option<&Run> {
        match self {
            Self::Run(run) => Some(run),
            _ => None,
        }
    }

    /// Returns the formula, if this inline is an inline `m:oMath`.
    #[must_use]
    pub const fn as_math(&self) -> Option<&MathExpression> {
        match self {
            Self::Math(expression) => Some(expression),
            _ => None,
        }
    }

    /// Returns the display formula, if this inline is an `m:oMathPara`.
    #[must_use]
    pub const fn as_math_paragraph(&self) -> Option<&MathParagraph> {
        match self {
            Self::MathParagraph(paragraph) => Some(paragraph),
            _ => None,
        }
    }
}
