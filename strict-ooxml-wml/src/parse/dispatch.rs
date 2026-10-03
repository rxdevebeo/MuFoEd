//! Data-driven element dispatch tables (STAGE-2 §4.2, ADR-0004).
//!
//! Instead of long `if`/`match` chains, the parser looks up each child element
//! in a static table. Elements absent from every table become `Opaque` nodes and
//! a `FeatureUse`, never an error.

/// Classification of a block-level (`w:body`/`w:tc`) child element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyKind {
    /// `w:p`.
    Paragraph,
    /// `w:tbl`.
    Table,
    /// `w:sdt`.
    Sdt,
    /// `w:altChunk`.
    AltChunk,
    /// `w:sectPr`.
    Section,
    /// `w:ins`.
    Inserted,
    /// `w:del`.
    Deleted,
    /// `w:moveTo`.
    MovedTo,
    /// `w:moveFrom`.
    MovedFrom,
    /// Transparent wrapper: children merge into the parent (`w:customXml`,
    /// `w:smartTag`, AUD-42).
    Transparent,
    /// Recognised and deliberately ignored (bookmarks, proof marks, ...).
    Ignored,
    /// Unknown.
    Opaque,
}

const BODY_TABLE: &[(&str, BodyKind)] = &[
    ("p", BodyKind::Paragraph),
    ("tbl", BodyKind::Table),
    ("sdt", BodyKind::Sdt),
    ("altChunk", BodyKind::AltChunk),
    ("sectPr", BodyKind::Section),
    ("ins", BodyKind::Inserted),
    ("del", BodyKind::Deleted),
    ("moveTo", BodyKind::MovedTo),
    ("moveFrom", BodyKind::MovedFrom),
    ("bookmarkStart", BodyKind::Ignored),
    ("bookmarkEnd", BodyKind::Ignored),
    ("proofErr", BodyKind::Ignored),
    ("permStart", BodyKind::Ignored),
    ("permEnd", BodyKind::Ignored),
    ("customXml", BodyKind::Transparent),
    ("smartTag", BodyKind::Transparent),
    ("oMath", BodyKind::Opaque),
    ("oMathPara", BodyKind::Opaque),
];

/// Classifies a block-level element by local name.
pub(crate) fn body_kind(local: &str) -> BodyKind {
    BODY_TABLE
        .iter()
        .find(|(name, _)| *name == local)
        .map_or(BodyKind::Opaque, |(_, kind)| *kind)
}

/// Classification of a paragraph child element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlineKind {
    /// `w:r`.
    Run,
    /// `w:hyperlink`.
    Hyperlink,
    /// `w:fldSimple`.
    Field,
    /// `w:drawing`.
    Drawing,
    /// `w:bookmarkStart`.
    BookmarkStart,
    /// `w:bookmarkEnd`.
    BookmarkEnd,
    /// `w:commentRangeStart`.
    CommentRangeStart,
    /// `w:commentRangeEnd`.
    CommentRangeEnd,
    /// `w:commentReference`.
    CommentReference,
    /// `w:footnoteReference`.
    FootnoteRef,
    /// `w:endnoteReference`.
    EndnoteRef,
    /// `w:sdt`.
    Sdt,
    /// `w:ins`.
    Inserted,
    /// `w:del`.
    Deleted,
    /// `w:moveTo`.
    MovedTo,
    /// `w:moveFrom`.
    MovedFrom,
    /// Transparent wrapper (`w:customXml`, `w:smartTag`, AUD-42).
    Transparent,
    /// Directional wrapper (`w:dir`, AUD-42).
    Dir,
    /// Bidirectional override (`w:bdo`, AUD-42).
    Bdo,
    /// Recognised and ignored.
    Ignored,
    /// Unknown.
    Opaque,
}

const INLINE_TABLE: &[(&str, InlineKind)] = &[
    ("r", InlineKind::Run),
    ("hyperlink", InlineKind::Hyperlink),
    ("fldSimple", InlineKind::Field),
    ("drawing", InlineKind::Drawing),
    ("bookmarkStart", InlineKind::BookmarkStart),
    ("bookmarkEnd", InlineKind::BookmarkEnd),
    ("commentRangeStart", InlineKind::CommentRangeStart),
    ("commentRangeEnd", InlineKind::CommentRangeEnd),
    ("commentReference", InlineKind::CommentReference),
    ("footnoteReference", InlineKind::FootnoteRef),
    ("endnoteReference", InlineKind::EndnoteRef),
    ("sdt", InlineKind::Sdt),
    ("ins", InlineKind::Inserted),
    ("del", InlineKind::Deleted),
    ("moveTo", InlineKind::MovedTo),
    ("moveFrom", InlineKind::MovedFrom),
    ("customXml", InlineKind::Transparent),
    ("smartTag", InlineKind::Transparent),
    ("dir", InlineKind::Dir),
    ("bdo", InlineKind::Bdo),
    ("proofErr", InlineKind::Ignored),
    ("permStart", InlineKind::Ignored),
    ("permEnd", InlineKind::Ignored),
];

/// Classifies a paragraph child element by local name.
pub(crate) fn inline_kind(local: &str) -> InlineKind {
    INLINE_TABLE
        .iter()
        .find(|(name, _)| *name == local)
        .map_or(InlineKind::Opaque, |(_, kind)| *kind)
}

/// Classification of a run (`w:r`) child element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunKind {
    /// `w:t`.
    Text,
    /// `w:delText`.
    DeletedText,
    /// `w:tab`.
    Tab,
    /// `w:br`.
    Break,
    /// `w:cr`.
    CarriageReturn,
    /// `w:drawing`.
    Drawing,
    /// `w:instrText`.
    InstrText,
    /// `w:fldChar`.
    FieldChar,
    /// `w:footnoteReference`.
    FootnoteRef,
    /// `w:endnoteReference`.
    EndnoteRef,
    /// `w:ptab`.
    Ptab,
    /// `w:commentReference`. The producer puts this inside a `w:r`, so it is
    /// dispatched here and NOT through `INLINE_TABLE` - which is why the inline
    /// entry for the same element was unreachable, and why a writer arm for it
    /// could never fire on real markup.
    CommentReference,
    /// `w:footnoteRef` / `w:endnoteRef` (the marker inside a note body).
    NoteRef,
    /// `w:sym`.
    Symbol,
    /// `w:lastRenderedPageBreak`.
    LastRenderedPageBreak,
    /// `w:noBreakHyphen`.
    NoBreakHyphen,
    /// `w:softHyphen`.
    SoftHyphen,
    /// `w:rPr`.
    RunProperties,
    /// `w:separator` / `w:continuationSeparator` (note separators).
    Separator,
    /// Unknown.
    Opaque,
}

const RUN_TABLE: &[(&str, RunKind)] = &[
    ("t", RunKind::Text),
    ("delText", RunKind::DeletedText),
    ("tab", RunKind::Tab),
    ("br", RunKind::Break),
    ("cr", RunKind::CarriageReturn),
    ("drawing", RunKind::Drawing),
    ("instrText", RunKind::InstrText),
    ("delInstrText", RunKind::InstrText),
    ("fldChar", RunKind::FieldChar),
    ("footnoteReference", RunKind::FootnoteRef),
    ("endnoteReference", RunKind::EndnoteRef),
    ("commentReference", RunKind::CommentReference),
    ("ptab", RunKind::Ptab),
    ("footnoteRef", RunKind::NoteRef),
    ("endnoteRef", RunKind::NoteRef),
    ("sym", RunKind::Symbol),
    ("lastRenderedPageBreak", RunKind::LastRenderedPageBreak),
    ("noBreakHyphen", RunKind::NoBreakHyphen),
    ("softHyphen", RunKind::SoftHyphen),
    ("rPr", RunKind::RunProperties),
    ("separator", RunKind::Separator),
    ("continuationSeparator", RunKind::Separator),
    ("object", RunKind::Opaque),
    ("pict", RunKind::Opaque),
    ("AlternateContent", RunKind::Opaque),
];

/// Classifies a run child element by local name.
pub(crate) fn run_kind(local: &str) -> RunKind {
    RUN_TABLE
        .iter()
        .find(|(name, _)| *name == local)
        .map_or(RunKind::Opaque, |(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use super::{body_kind, inline_kind, run_kind, BodyKind, InlineKind, RunKind};

    #[test]
    fn known_elements_dispatch() {
        assert_eq!(body_kind("p"), BodyKind::Paragraph);
        assert_eq!(body_kind("tbl"), BodyKind::Table);
        assert_eq!(body_kind("nope"), BodyKind::Opaque);
        assert_eq!(inline_kind("r"), InlineKind::Run);
        assert_eq!(inline_kind("hyperlink"), InlineKind::Hyperlink);
        assert_eq!(run_kind("t"), RunKind::Text);
        assert_eq!(run_kind("unknown"), RunKind::Opaque);
    }
}
