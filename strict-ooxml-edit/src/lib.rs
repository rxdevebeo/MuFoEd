//! Transactional editing over the existing WML document. See `docs/EDITING_PLAN.md`.
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ops::Range;
use strict_ooxml_wml::model::{
    Block, Document, Inline, Paragraph, Run, RunContent, RunProperties, Space, StyleId, StyleType,
    TextNode, TriState, Underline,
};

mod structured;
pub use structured::{Address, Container, Edit, Editor, Story};
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
        write!(f, "{self:?}")
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
pub struct EditSession<'a> {
    document: &'a mut Document,
    revision: u64,
    limits: EditLimits,
    undo: VecDeque<Vec<ParagraphChange>>,
    redo: Vec<Vec<ParagraphChange>>,
    support_stale: bool,
}
struct ParagraphChange {
    index: usize,
    before: Paragraph,
    after: Paragraph,
}
impl<'a> EditSession<'a> {
    /// Starts a session with exclusive access to the existing document.
    pub fn new(document: &'a mut Document, limits: EditLimits) -> Result<Self, EditError> {
        validate_body(document)?;
        Ok(Self {
            document,
            revision: 0,
            limits,
            undo: VecDeque::new(),
            redo: Vec::new(),
            support_stale: false,
        })
    }
    /// Reads the live document without bypassing the command protocol.
    pub fn document(&self) -> &Document {
        self.document
    }
    /// Monotonic revision used for optimistic command validation.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Whether parser support metadata needs recomputation.
    pub fn support_is_stale(&self) -> bool {
        self.support_stale
    }
    /// Atomically executes a batch; offsets follow preceding commands.
    pub fn transact(
        &mut self,
        expected: u64,
        commands: &[Command],
    ) -> Result<ChangeSet, EditError> {
        self.check_revision(expected)?;
        let mut candidates = BTreeMap::<usize, Paragraph>::new();
        for command in commands {
            let (index, range) = match command {
                Command::ReplaceText {
                    paragraph, range, ..
                }
                | Command::Format {
                    paragraph, range, ..
                } => (*paragraph, range),
            };
            if let std::collections::btree_map::Entry::Vacant(entry) = candidates.entry(index) {
                entry.insert(paragraph(self.document, index)?.clone());
            }
            let candidate = candidates
                .get_mut(&index)
                .ok_or(EditError::InvalidParagraph)?;
            let runs = plain_runs(candidate, self.limits.paragraph_scalars)?;
            let total: usize = runs.iter().map(|(_, text)| text.chars().count()).sum();
            if range.start > range.end || range.end > total {
                return Err(EditError::InvalidRange);
            }
            match command {
                Command::ReplaceText { text, .. } => {
                    if text.chars().any(|ch| {
                        !strict_ooxml_core::xml::escape::is_xml_char(ch)
                            || matches!(ch, '\n' | '\r' | '\t')
                    }) {
                        return Err(EditError::InvalidText);
                    }
                    let length = total
                        .checked_sub(range.end - range.start)
                        .and_then(|n| n.checked_add(text.chars().count()))
                        .ok_or(EditError::LimitExceeded)?;
                    if length > self.limits.paragraph_scalars {
                        return Err(EditError::LimitExceeded);
                    }
                    replace_text(candidate, &runs, range, text);
                }
                Command::Format { patch, .. } => {
                    if let Some(Some(id)) = &patch.character_style {
                        if !self
                            .document
                            .styles
                            .get(id)
                            .is_some_and(|s| s.style_type == StyleType::Character)
                        {
                            return Err(EditError::UnknownStyle);
                        }
                    }
                    format_text(candidate, &runs, range, patch);
                }
            }
        }
        let mut changes = Vec::new();
        for (index, after) in candidates {
            let before = paragraph(self.document, index)?;
            if before != &after {
                changes.push(ParagraphChange {
                    index,
                    before: before.clone(),
                    after,
                });
            }
        }
        if changes.is_empty() {
            return Ok(self.change_set(Vec::new()));
        }
        let next_revision = self.next_revision()?;
        let indices = changes.iter().map(|c| c.index).collect();
        for change in &changes {
            self.document.body.blocks[change.index] = Block::Paragraph(change.after.clone());
        }
        self.revision = next_revision;
        self.support_stale = true;
        self.redo.clear();
        if self.limits.history_transactions > 0 {
            self.undo.push_back(changes);
            if self.undo.len() > self.limits.history_transactions {
                self.undo.pop_front();
            }
        }
        Ok(self.change_set(indices))
    }
    /// Restores the exact paragraphs before the latest transaction.
    pub fn undo(&mut self, expected: u64) -> Result<ChangeSet, EditError> {
        self.check_revision(expected)?;
        if self.undo.is_empty() {
            return Err(EditError::EmptyHistory);
        }
        let next_revision = self.next_revision()?;
        let changes = self.undo.pop_back().ok_or(EditError::EmptyHistory)?;
        let indices = changes.iter().map(|c| c.index).collect();
        for change in &changes {
            self.document.body.blocks[change.index] = Block::Paragraph(change.before.clone());
        }
        self.redo.push(changes);
        self.revision = next_revision;
        self.support_stale = true;
        Ok(self.change_set(indices))
    }
    /// Reapplies the exact paragraphs from the latest undone transaction.
    pub fn redo(&mut self, expected: u64) -> Result<ChangeSet, EditError> {
        self.check_revision(expected)?;
        if self.redo.is_empty() {
            return Err(EditError::EmptyHistory);
        }
        let next_revision = self.next_revision()?;
        let changes = self.redo.pop().ok_or(EditError::EmptyHistory)?;
        let indices = changes.iter().map(|c| c.index).collect();
        for change in &changes {
            self.document.body.blocks[change.index] = Block::Paragraph(change.after.clone());
        }
        self.undo.push_back(changes);
        self.revision = next_revision;
        self.support_stale = true;
        Ok(self.change_set(indices))
    }
    fn check_revision(&self, expected: u64) -> Result<(), EditError> {
        if expected == self.revision {
            Ok(())
        } else {
            Err(EditError::StaleRevision)
        }
    }
    fn next_revision(&self) -> Result<u64, EditError> {
        self.revision.checked_add(1).ok_or(EditError::LimitExceeded)
    }
    fn change_set(&self, paragraphs: Vec<usize>) -> ChangeSet {
        let changed = !paragraphs.is_empty();
        ChangeSet {
            revision: self.revision,
            paragraphs,
            invalidate_layout: changed,
            invalidate_support: changed,
        }
    }
}

fn paragraph(document: &Document, index: usize) -> Result<&Paragraph, EditError> {
    match document.body.blocks.get(index) {
        Some(Block::Paragraph(p)) => Ok(p),
        _ => Err(EditError::InvalidParagraph),
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
