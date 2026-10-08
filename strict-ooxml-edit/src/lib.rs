//! Transactional editing over the existing WML document. See `docs/EDITING_PLAN.md`.
// Never-crash (docs/WORDCRAFT_ADOPTION_2026-10-07.md §4.2): library paths
// return errors or degrade with a report; tests may still unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]
use std::collections::HashSet;
use std::ops::Range;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_wml::model::{
    Block, Color, Document, Fonts, HalfPoints, Highlight, Inline, Paragraph, Run, RunContent,
    RunProperties, Space, StyleId, StyleType, TextNode, TriState, Underline, VertAlign,
};

mod structured;
pub use structured::{Address, Container, Edit, EditFailure, Editor, Story};
mod operations;
pub use operations::{
    IdentityChange, MoveReport, OperationError, OperationLimits, Operations, ReplacePolicy,
    ReplaceReport, SearchHit, SearchQuery, SearchResult, SearchScope, TextQuery, TextSlice,
};
#[cfg(feature = "save")]
mod save;
#[cfg(feature = "save")]
pub use save::{SaveError, SavePolicy, SavedDocument};
#[cfg(feature = "visual")]
mod visual;
#[cfg(feature = "visual")]
pub use visual::{
    scalar_to_utf16, utf16_to_scalar, DrawingPosition, TextPosition, VisualError, VisualMap,
    VisualRect,
};

/// Only specified direct properties are changed. `Some(None)` clears a property.
#[derive(Clone, Debug, Default)]
pub struct FormatPatch {
    /// Direct bold state; `Absent` restores inheritance.
    pub bold: Option<TriState>,
    /// Direct italic state; `Absent` restores inheritance.
    pub italic: Option<TriState>,
    /// Underline value, or explicitly clear the direct value.
    pub underline: Option<Option<Underline>>,
    /// Character style reference, or explicitly clear it.
    pub character_style: Option<Option<StyleId>>,
    /// Direct strikethrough (`w:strike`).
    pub strike: Option<TriState>,
    /// Direct double strikethrough (`w:dstrike`).
    pub double_strike: Option<TriState>,
    /// Direct all caps (`w:caps`).
    pub caps: Option<TriState>,
    /// Direct small caps (`w:smallCaps`).
    pub small_caps: Option<TriState>,
    /// Font size for every script (`w:sz` and `w:szCs`, as Word sets them),
    /// or clear both.
    pub size: Option<Option<HalfPoints>>,
    /// Text colour (`w:color`), or clear it. Either drops a theme colour, which
    /// would otherwise win over the value set here.
    pub color: Option<Option<Color>>,
    /// Highlight (`w:highlight`), or clear it.
    pub highlight: Option<Option<Highlight>>,
    /// Run fonts (`w:rFonts`), or clear them.
    pub fonts: Option<Option<Fonts>>,
    /// Superscript or subscript (`w:vertAlign`), or clear it.
    pub vert_align: Option<Option<VertAlign>>,
}
/// A command of the deprecated [`EditSession`]; [`Editor`] takes [`Edit`].
///
/// Ranges use Unicode scalar offsets, and commands in a batch execute in order.
#[derive(Clone, Debug)]
pub enum Command {
    /// Insert, delete or replace plain text.
    ReplaceText {
        /// Index in the document body's block sequence.
        paragraph: usize,
        /// Half-open Unicode scalar range.
        range: Range<usize>,
        /// Replacement text.
        text: String,
    },
    /// Apply specified direct formatting to existing text.
    Format {
        /// Index in the document body's block sequence.
        paragraph: usize,
        /// Half-open Unicode scalar range.
        range: Range<usize>,
        /// Direct properties to modify.
        patch: FormatPatch,
    },
}
/// A rejected operation never changes the document or history.
///
/// [`Editor::transact_detailed`] adds which command failed and where.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// The caller used an obsolete revision.
    StaleRevision,
    /// The target is absent or is not a body paragraph.
    InvalidParagraph,
    /// The target holds content the edit cannot change; the detail says what.
    UnsupportedContent(Unsupported),
    /// The scalar range is reversed or out of bounds.
    InvalidRange,
    /// Text contains XML-invalid or structural control characters.
    InvalidText,
    /// A style reference is missing or has the wrong kind; it carries the id.
    UnknownStyle(StyleId),
    /// The model breaks, or the edit would break, the invariant named.
    InvalidModel(Invariant),
    /// A configured resource or revision limit was exceeded.
    LimitExceeded,
    /// There is nothing to undo or redo.
    EmptyHistory,
}
impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleRevision => {
                f.write_str("the revision is stale; re-read the document and retry")
            }
            Self::InvalidParagraph => {
                f.write_str("the address does not name an editable paragraph")
            }
            Self::UnsupportedContent(what) => {
                write!(
                    f,
                    "the paragraph holds content this edit cannot change: {what}"
                )
            }
            Self::InvalidRange => f.write_str("the range is reversed or past the end of the text"),
            Self::InvalidText => {
                f.write_str("the text holds a character XML or a paragraph cannot carry")
            }
            Self::UnknownStyle(id) => {
                write!(
                    f,
                    "style `{}` does not exist or is of the wrong type",
                    id.as_str()
                )
            }
            Self::InvalidModel(invariant) => {
                write!(f, "the edit would break a model invariant: {invariant}")
            }
            Self::LimitExceeded => f.write_str("an edit, history or nesting limit was exceeded"),
            Self::EmptyHistory => f.write_str("there is nothing to undo or redo"),
        }
    }
}
impl std::error::Error for EditError {}
/// What an edit met that it cannot change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unsupported {
    /// The paragraph or one of its runs carries a tracked insertion or deletion.
    TrackedChange,
    /// A run holds something other than text: a tab, a break, a field character, a drawing, a
    /// symbol.
    RunContent,
    /// The paragraph holds an inline that is not a plain run: a hyperlink, a field, an inline
    /// SDT, a bookmark, math.
    Inline,
    /// The drawing is kept as opaque markup.
    OpaqueDrawing,
    /// The address names a target of another kind than the command needs (not a run, not a
    /// shape, not a table...).
    Target,
    /// A cached field result, which the field's next update would overwrite.
    FieldResult,
}
impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TrackedChange => "a tracked change",
            Self::RunContent => "a run holding more than text",
            Self::Inline => "an inline that is not a plain run",
            Self::OpaqueDrawing => "an opaque drawing",
            Self::Target => "a target of another kind than the command needs",
            Self::FieldResult => "a cached field result",
        })
    }
}
/// The model invariant an edit found broken or would break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Invariant {
    /// A paragraph id, or a text id, appears twice in a story.
    DuplicateId,
    /// A paragraph has a text id but no paragraph id.
    TextIdWithoutParaId,
    /// The section list and the body's section breaks disagree.
    Sections,
    /// The edit would delete, join or move a section break.
    SectionBoundary,
    /// A bookmark start or end is unmatched or repeated.
    Bookmark,
    /// A comment range start or end is unmatched.
    Comment,
    /// Complex field characters are unbalanced, or a simple field holds unbalanced content.
    Field,
    /// An empty table or row, a cell not ending in a paragraph, or a span, grid or vertical
    /// merge mismatch.
    TableTopology,
    /// An unknown numbering instance or an invalid list level.
    Numbering,
    /// A `basedOn` cycle, or a cached style chain that disagrees with the styles.
    StyleChain,
    /// A header or footer reference names no header or footer part.
    HeaderFooterReference,
    /// A negative frame size.
    Frame,
    /// A non-positive grid column width.
    Grid,
    /// A footnote or endnote reference names no note.
    NoteReference,
    /// A picture names no media part, or a drawing extent is not positive.
    Drawing,
}
impl std::fmt::Display for Invariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DuplicateId => "a paragraph or text id used twice in a story",
            Self::TextIdWithoutParaId => "a text id without a paragraph id",
            Self::Sections => "the section list and the section breaks disagree",
            Self::SectionBoundary => "a section break deleted, joined or moved",
            Self::Bookmark => "a bookmark start or end without its pair",
            Self::Comment => "a comment range start or end without its pair",
            Self::Field => "unbalanced field characters",
            Self::TableTopology => "a table, row or cell of invalid shape",
            Self::Numbering => "an unknown numbering instance or an invalid list level",
            Self::StyleChain => "a style chain with a cycle or a stale cache",
            Self::HeaderFooterReference => "a header or footer reference without its part",
            Self::Frame => "a negative frame size",
            Self::Grid => "a non-positive grid column width",
            Self::NoteReference => "a note reference without its note",
            Self::Drawing => "a picture without its media or a drawing of non-positive size",
        })
    }
}
#[derive(Clone, Copy, Debug)]
/// Bounds for history and text processing.
pub struct EditLimits {
    /// Maximum retained undo transactions, zero disables history.
    pub history_transactions: usize,
    /// Maximum scalar count of an edited paragraph.
    pub paragraph_scalars: usize,
    /// The nesting budgets an edited model must stay within: those the
    /// document was opened with, so an edit can neither produce a model the
    /// reader would have refused nor be refused one it accepted.
    /// `StrictDocument::edit` fills it from the package.
    pub resource: ResourceLimits,
    /// Refuse text, format and split ranges whose ends fall inside a
    /// user-perceived character (an extended grapheme cluster: an emoji with
    /// a joiner, a flag, a letter and its combining mark) with
    /// [`EditError::InvalidRange`]. Off by default in this release; offsets
    /// are Unicode scalars either way.
    pub grapheme_boundaries: bool,
}
impl Default for EditLimits {
    fn default() -> Self {
        Self {
            history_transactions: 100,
            paragraph_scalars: 1_000_000,
            resource: ResourceLimits::default(),
            grapheme_boundaries: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
/// Describes changed logical blocks, without inventing physical page numbers.
pub struct ChangeSet {
    /// Revision after the operation.
    pub revision: u64,
    /// Sorted changed body block indices. [`Editor`] lists every body block
    /// whenever anything changed; [`blocks`](Self::blocks) is the precise form.
    pub paragraphs: Vec<usize>,
    /// The top-level block ranges that changed, in every story, in the
    /// coordinates after the operation; ordered by start within each story.
    /// Blocks outside them are the same blocks as before, shifted by the
    /// changes in front of them.
    pub blocks: Vec<BlockChange>,
    /// Consumers must recalculate layout for these changes.
    pub invalidate_layout: bool,
    /// Parser support metadata is no longer current.
    pub invalidate_support: bool,
}
/// One contiguous range of a story's top-level blocks that an operation
/// replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockChange {
    /// The story the blocks belong to.
    pub story: Story,
    /// The range in the document after the operation.
    pub range: Range<usize>,
    /// How many blocks stood there before; it differs from `range.len()` where
    /// blocks were inserted or removed.
    pub replaced: usize,
}
/// Exclusive borrow prevents untracked changes while history is active.
///
/// The body-paragraph subset of [`Editor`], kept for its callers: every command
/// is forwarded as an [`Edit::Text`] or [`Edit::Format`] on [`Address::body`],
/// so the two APIs cannot drift apart. The difference is the [`ChangeSet`],
/// which names exactly the body blocks that changed.
#[deprecated(
    since = "0.1.0",
    note = "use `Editor` with `Edit::Text` / `Edit::Format` on `Address::body`"
)]
pub struct EditSession<'a> {
    editor: Editor<'a>,
}
#[allow(deprecated)]
impl<'a> EditSession<'a> {
    /// Starts a session with exclusive access to the existing document.
    pub fn new(document: &'a mut Document, limits: EditLimits) -> Result<Self, EditError> {
        Ok(Self {
            editor: Editor::new(document, limits)?,
        })
    }
    /// Reads the live document without bypassing the command protocol.
    pub fn document(&self) -> &Document {
        self.editor.document()
    }
    /// Monotonic revision used for optimistic command validation.
    pub fn revision(&self) -> u64 {
        self.editor.revision()
    }
    /// Whether parser support metadata needs recomputation.
    pub fn support_is_stale(&self) -> bool {
        self.editor.support_is_stale()
    }
    /// Atomically executes a batch; offsets follow preceding commands.
    pub fn transact(
        &mut self,
        expected: u64,
        commands: &[Command],
    ) -> Result<ChangeSet, EditError> {
        let mut edits = Vec::with_capacity(commands.len());
        for command in commands {
            edits.push(match command {
                Command::ReplaceText {
                    paragraph,
                    range,
                    text,
                } => Edit::Text {
                    at: Address::body(*paragraph),
                    range: range.clone(),
                    text: text.clone(),
                },
                Command::Format {
                    paragraph,
                    range,
                    patch,
                } => {
                    // A character style must exist *as* a character style; the
                    // editor's model validation only asks that it exists.
                    if let Some(Some(id)) = &patch.character_style {
                        if !self
                            .document()
                            .styles
                            .get(id)
                            .is_some_and(|style| style.style_type == StyleType::Character)
                        {
                            return Err(EditError::UnknownStyle(id.clone()));
                        }
                    }
                    Edit::Format {
                        at: Address::body(*paragraph),
                        range: range.clone(),
                        patch: patch.clone(),
                    }
                }
            });
        }
        self.tracked(|editor| editor.transact(expected, &edits))
    }
    /// Restores the exact paragraphs before the latest transaction.
    pub fn undo(&mut self, expected: u64) -> Result<ChangeSet, EditError> {
        self.tracked(|editor| editor.undo(expected))
    }
    /// Reapplies the exact paragraphs from the latest undone transaction.
    pub fn redo(&mut self, expected: u64) -> Result<ChangeSet, EditError> {
        self.tracked(|editor| editor.redo(expected))
    }
    /// Runs `step` and narrows its change set to the body blocks that differ.
    fn tracked(
        &mut self,
        step: impl FnOnce(&mut Editor<'a>) -> Result<ChangeSet, EditError>,
    ) -> Result<ChangeSet, EditError> {
        let before = self.editor.document().body.blocks.clone();
        let mut change = step(&mut self.editor)?;
        let after = &self.editor.document().body.blocks;
        change.paragraphs = if before.len() == after.len() {
            before
                .iter()
                .zip(after)
                .enumerate()
                .filter_map(|(index, (old, new))| (old != new).then_some(index))
                .collect()
        } else {
            (0..after.len()).collect()
        };
        Ok(change)
    }
}

/// The paragraph's runs with their text, for an edit by scalar offsets.
///
/// Runs with a tracked change are included and count in the offsets - their
/// text is on the page, struck through or underlined - but stay whole: see
/// [`check_tracked`].
fn plain_runs(paragraph: &Paragraph, max: usize) -> Result<Vec<(Run, String)>, EditError> {
    let mut count = 0_usize;
    let mut runs = Vec::new();
    for inline in &paragraph.inlines {
        let Inline::Run(run) = inline else {
            return Err(EditError::UnsupportedContent(Unsupported::Inline));
        };
        let mut text = String::new();
        for content in &run.content {
            let RunContent::Text(node) = content else {
                return Err(EditError::UnsupportedContent(Unsupported::RunContent));
            };
            count = count
                .checked_add(node.text.chars().count())
                .ok_or(EditError::LimitExceeded)?;
            if count > max {
                return Err(EditError::LimitExceeded);
            }
            text.push_str(&node.text);
        }
        runs.push((run.clone(), text));
    }
    Ok(runs)
}
fn slice(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}
/// Refuses a range that would cut into or replace part of a run with a
/// tracked change: until the change is accepted or rejected the run is one
/// unit. An insertion at either edge of such a run is fine.
fn check_tracked(runs: &[(Run, String)], range: &Range<usize>) -> Result<(), EditError> {
    let mut offset = 0;
    for (run, text) in runs {
        let end = offset + text.chars().count();
        if run.revision.is_some() {
            let touches = if range.is_empty() {
                offset < range.start && range.start < end
            } else {
                range.start < end && offset < range.end
            };
            if touches {
                return Err(EditError::UnsupportedContent(Unsupported::TrackedChange));
            }
        }
        offset = end;
    }
    Ok(())
}
fn text_run(template: Option<&Run>, text: String) -> Inline {
    let mut run = template.cloned().unwrap_or(Run {
        props: RunProperties::default(),
        content: Vec::new(),
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    });
    run.content = vec![RunContent::Text(TextNode {
        text,
        space: Space::Preserve,
    })];
    Inline::Run(run)
}
fn replace_text(
    paragraph: &mut Paragraph,
    runs: &[(Run, String)],
    range: &Range<usize>,
    replacement: &str,
) {
    let old: String = runs.iter().map(|(_, text)| text.as_str()).collect();
    if slice(&old, range.start, range.end) == replacement {
        return;
    }
    let target = range.start.saturating_sub(1);
    let mut offset = 0;
    let template = runs.iter().find_map(|(run, text)| {
        let end = offset + text.chars().count();
        let found = if offset <= target && target < end {
            Some(run)
        } else {
            None
        };
        offset = end;
        found
    });
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    offset = 0;
    for (run, text) in runs {
        let len = text.chars().count();
        let end = offset + len;
        if len == 0 {
            if offset <= range.start {
                prefix.push(Inline::Run(run.clone()));
            } else {
                suffix.push(Inline::Run(run.clone()));
            }
            continue;
        }
        if end <= range.start {
            prefix.push(Inline::Run(run.clone()));
        } else if offset < range.start {
            prefix.push(text_run(Some(run), slice(text, 0, range.start - offset)));
        }
        if offset >= range.end {
            suffix.push(Inline::Run(run.clone()));
        } else if end > range.end {
            suffix.push(text_run(Some(run), slice(text, range.end - offset, len)));
        }
        offset = end;
    }
    if !replacement.is_empty() {
        let mut inserted = text_run(template, replacement.to_owned());
        // Typed next to a tracked run, the text is not part of that change.
        if let Inline::Run(run) = &mut inserted {
            run.revision = None;
        }
        push_merged(&mut prefix, inserted);
    }
    let mut suffix = suffix.into_iter();
    if let Some(first) = suffix.next() {
        push_merged(&mut prefix, first);
    }
    prefix.extend(suffix);
    paragraph.inlines = prefix;
}
/// Pushes `next`, merging it into the last inline when both are single-text
/// runs with the same properties.
///
/// Used at the edit's own seams only - the inserted text, and the first piece
/// after the range - so a keystroke extends its run and a deletion inside a
/// run joins the two halves again, instead of leaving a run per keystroke.
/// Runs away from the edit stay as the document had them.
fn push_merged(out: &mut Vec<Inline>, next: Inline) {
    if let (Some(Inline::Run(last)), Inline::Run(run)) = (out.last_mut(), &next) {
        if last.props == run.props && last.revision == run.revision {
            if let ([RunContent::Text(left)], [RunContent::Text(right)]) =
                (last.content.as_mut_slice(), run.content.as_slice())
            {
                left.text.push_str(&right.text);
                left.space = Space::Preserve;
                return;
            }
        }
    }
    out.push(next);
}
fn patch_properties(props: &mut RunProperties, patch: &FormatPatch) {
    if let Some(v) = patch.bold {
        props.bold = v;
    }
    if let Some(v) = patch.italic {
        props.italic = v;
    }
    if let Some(v) = &patch.underline {
        props.underline = *v;
    }
    if let Some(v) = &patch.character_style {
        props.style.clone_from(v);
    }
    for (slot, value) in [
        (&mut props.strike, patch.strike),
        (&mut props.double_strike, patch.double_strike),
        (&mut props.caps, patch.caps),
        (&mut props.small_caps, patch.small_caps),
    ] {
        if let Some(value) = value {
            *slot = value;
        }
    }
    if let Some(v) = &patch.size {
        props.size.clone_from(v);
        props.size_cs.clone_from(v);
    }
    if let Some(v) = &patch.color {
        props.color.clone_from(v);
        props.color_theme = None;
    }
    if let Some(v) = &patch.highlight {
        props.highlight.clone_from(v);
    }
    if let Some(v) = &patch.fonts {
        props.fonts.clone_from(v);
    }
    if let Some(v) = &patch.vert_align {
        props.vert_align.clone_from(v);
    }
}
fn format_text(
    paragraph: &mut Paragraph,
    runs: &[(Run, String)],
    range: &Range<usize>,
    patch: &FormatPatch,
) {
    if range.is_empty() {
        return;
    }
    let mut offset = 0;
    let mut out = Vec::new();
    for (run, text) in runs {
        let len = text.chars().count();
        let end = offset + len;
        let start = range.start.max(offset);
        let stop = range.end.min(end);
        let mut modified = run.clone();
        patch_properties(&mut modified.props, patch);
        if start >= stop || modified.props == run.props {
            out.push(Inline::Run(run.clone()));
        } else {
            if start > offset {
                out.push(text_run(Some(run), slice(text, 0, start - offset)));
            }
            out.push(text_run(
                Some(&modified),
                slice(text, start - offset, stop - offset),
            ));
            if stop < end {
                out.push(text_run(Some(run), slice(text, stop - offset, len)));
            }
        }
        offset = end;
    }
    paragraph.inlines = out;
}
fn validate_body(document: &Document) -> Result<(), EditError> {
    let mut ids = HashSet::new();
    validate_blocks(&document.body.blocks, document, &mut ids, 0)?;
    let boundaries: Vec<_> = document
        .body
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::Paragraph(p) = b {
                p.props.section.as_ref()
            } else {
                None
            }
        })
        .collect();
    if (!boundaries.is_empty() || !document.sections.is_empty())
        && document.sections.len() != boundaries.len() + 1
    {
        return Err(EditError::InvalidModel(Invariant::Sections));
    }
    for (boundary, section) in boundaries.iter().zip(&document.sections) {
        if **boundary != section.properties {
            return Err(EditError::InvalidModel(Invariant::Sections));
        }
    }
    Ok(())
}
fn validate_blocks(
    blocks: &[Block],
    document: &Document,
    ids: &mut HashSet<String>,
    depth: usize,
) -> Result<(), EditError> {
    if depth > 128 {
        return Err(EditError::LimitExceeded);
    }
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                if let Some(id) = &p.para_id {
                    if !ids.insert(id.as_str().to_ascii_uppercase()) {
                        return Err(EditError::InvalidModel(Invariant::DuplicateId));
                    }
                }
                if let Some(id) = p
                    .props
                    .style
                    .as_ref()
                    .filter(|id| document.styles.get(id).is_none())
                {
                    return Err(EditError::UnknownStyle(id.clone()));
                }
                for inline in &p.inlines {
                    if let Inline::Run(run) = inline {
                        if let Some(id) = run
                            .props
                            .style
                            .as_ref()
                            .filter(|id| document.styles.get(id).is_none())
                        {
                            return Err(EditError::UnknownStyle(id.clone()));
                        }
                    }
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        validate_blocks(&cell.blocks, document, ids, depth + 1)?;
                    }
                }
            }
            Block::SdtBlock(sdt) => validate_blocks(&sdt.blocks, document, ids, depth + 1)?,
            _ => {}
        }
    }
    Ok(())
}
