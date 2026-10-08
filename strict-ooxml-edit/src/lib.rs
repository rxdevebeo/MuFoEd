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
        clippy::unreachable
    )
)]
use std::collections::HashSet;
use std::ops::Range;
use strict_ooxml_wml::model::{
    Block, Document, Inline, Paragraph, Run, RunContent, RunProperties, Space, StyleId, StyleType,
    TextNode, TriState, Underline,
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
    /// The paragraph contains structured or tracked content.
    UnsupportedContent,
    /// The scalar range is reversed or out of bounds.
    InvalidRange,
    /// Text contains XML-invalid or structural control characters.
    InvalidText,
    /// A style reference is missing or has the wrong kind.
    UnknownStyle,
    /// Existing body identity or section invariants are inconsistent.
    InvalidModel,
    /// A configured resource or revision limit was exceeded.
    LimitExceeded,
    /// There is nothing to undo or redo.
    EmptyHistory,
}
impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::StaleRevision => "the revision is stale; re-read the document and retry",
            Self::InvalidParagraph => "the address does not name an editable paragraph",
            Self::UnsupportedContent => {
                "the paragraph holds structured or tracked content this edit cannot change"
            }
            Self::InvalidRange => "the range is reversed or past the end of the text",
            Self::InvalidText => "the text holds a character XML or a paragraph cannot carry",
            Self::UnknownStyle => "the style does not exist or is of the wrong type",
            Self::InvalidModel => "the document model breaks an identity or section invariant",
            Self::LimitExceeded => "an edit, history or nesting limit was exceeded",
            Self::EmptyHistory => "there is nothing to undo or redo",
        })
    }
}
impl std::error::Error for EditError {}
#[derive(Clone, Copy, Debug)]
/// Bounds for history and text processing.
pub struct EditLimits {
    /// Maximum retained undo transactions, zero disables history.
    pub history_transactions: usize,
    /// Maximum scalar count of an edited paragraph.
    pub paragraph_scalars: usize,
}
impl Default for EditLimits {
    fn default() -> Self {
        Self {
            history_transactions: 100,
            paragraph_scalars: 1_000_000,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Describes changed logical blocks, without inventing physical page numbers.
pub struct ChangeSet {
    /// Revision after the operation.
    pub revision: u64,
    /// Sorted changed body block indices.
    pub paragraphs: Vec<usize>,
    /// Consumers must recalculate layout for these changes.
    pub invalidate_layout: bool,
    /// Parser support metadata is no longer current.
    pub invalidate_support: bool,
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
                            return Err(EditError::UnknownStyle);
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

fn plain_runs(paragraph: &Paragraph, max: usize) -> Result<Vec<(Run, String)>, EditError> {
    if paragraph.revision.is_some() {
        return Err(EditError::UnsupportedContent);
    }
    let mut count = 0_usize;
    let mut runs = Vec::new();
    for inline in &paragraph.inlines {
        let Inline::Run(run) = inline else {
            return Err(EditError::UnsupportedContent);
        };
        if run.revision.is_some() {
            return Err(EditError::UnsupportedContent);
        }
        let mut text = String::new();
        for content in &run.content {
            let RunContent::Text(node) = content else {
                return Err(EditError::UnsupportedContent);
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
        prefix.push(text_run(template, replacement.to_owned()));
    }
    prefix.extend(suffix);
    paragraph.inlines = prefix;
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
        return Err(EditError::InvalidModel);
    }
    for (boundary, section) in boundaries.iter().zip(&document.sections) {
        if **boundary != section.properties {
            return Err(EditError::InvalidModel);
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
                        return Err(EditError::InvalidModel);
                    }
                }
                if p.props
                    .style
                    .as_ref()
                    .is_some_and(|id| document.styles.get(id).is_none())
                {
                    return Err(EditError::UnknownStyle);
                }
                for inline in &p.inlines {
                    if let Inline::Run(run) = inline {
                        if run
                            .props
                            .style
                            .as_ref()
                            .is_some_and(|id| document.styles.get(id).is_none())
                        {
                            return Err(EditError::UnknownStyle);
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
