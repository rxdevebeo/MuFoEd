//! The editor: revisions, transactions and the undo/redo history.
use crate::{BlockChange, ChangeSet, EditError, EditLimits};
use std::collections::VecDeque;
use strict_ooxml_wml::model::{Block, Document, ParaId, Paragraph};

use super::address::{addresses, mutate_story, read_paragraph, Address, Container, Story};
use super::apply::apply;
use super::command::{Edit, EditFailure};
use super::notes::split_shared_notes;
use super::validate::{validate, validate_patch};

/// Structural editor over the live WML document; `EditSession` is a body-only
/// view of it.
pub struct Editor<'a> {
    pub(crate) document: &'a mut Document,
    revision: u64,
    limits: EditLimits,
    undo: VecDeque<Snapshot>,
    redo: VecDeque<Snapshot>,
    history_byte_limit: usize,
    pub(crate) support_stale: bool,
    pub(crate) input_pipeline: strict_ooxml_core::pipeline::PipelineSummary,
    /// Whether every note reference is known to have a note of its own, so a
    /// transaction that touches no reference can skip the whole-document walk
    /// that would give one a copy.
    notes_settled: bool,
}
/// A replaced range of one story's top-level blocks.
#[derive(Clone)]
struct Patch {
    story: Story,
    start: usize,
    before: Vec<Block>,
    after: Vec<Block>,
}
/// What one history entry restores.
enum Change {
    /// The top-level blocks the transaction replaced, in the order it did.
    Blocks(Vec<Patch>),
    /// The whole document, for a transaction that gave note references their
    /// own notes.
    Whole {
        before: Box<Document>,
        after: Box<Document>,
    },
}
struct Snapshot {
    change: Change,
    bytes: usize,
}
impl<'a> Editor<'a> {
    /// Opens an exclusive session.
    pub fn new(document: &'a mut Document, limits: EditLimits) -> Result<Self, EditError> {
        validate(document, &limits.resource)?;
        let notes_settled = has_no_notes(document);
        Ok(Self {
            document,
            revision: 0,
            limits,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            history_byte_limit: 64 * 1024 * 1024,
            support_stale: false,
            input_pipeline: strict_ooxml_core::pipeline::PipelineSummary::new(),
            notes_settled,
        })
    }
    /// Reads the current document.
    pub fn document(&self) -> &Document {
        self.document
    }
    /// Current revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Whether support metadata must be refreshed before reporting.
    pub fn support_is_stale(&self) -> bool {
        self.support_stale
    }
    /// Carries previous normalization/conversion issues into every save result.
    #[must_use]
    pub fn with_pipeline(mut self, pipeline: strict_ooxml_core::pipeline::PipelineSummary) -> Self {
        self.input_pipeline = pipeline;
        self
    }
    /// Caps retained snapshot metadata/text in bytes (default 64 MiB).
    /// A transaction exceeding this cap is refused before changing the document.
    /// The estimate is the UTF-8 debug serialization of what a transaction
    /// replaced and what replaced it: the touched top-level blocks, or both whole
    /// documents for a transaction that gave note references their own notes.
    /// Media bytes remain external. Older undo states are evicted first.
    #[must_use]
    pub fn with_history_byte_limit(mut self, bytes: usize) -> Self {
        self.history_byte_limit = bytes;
        self.trim_history();
        self
    }
    /// Explicitly validates the current model and all editable stories.
    pub fn validate(&self) -> Result<(), EditError> {
        validate(self.document, &self.limits.resource)
    }
    /// Reads a paragraph without bypassing the command protocol.
    pub fn paragraph(&self, at: &Address) -> Result<&Paragraph, EditError> {
        read_paragraph(self.document, at)
    }
    /// Lists editable paragraph addresses, including table, SDT and shape text boxes.
    pub fn paragraphs(&self, story: &Story) -> Result<Vec<Address>, EditError> {
        addresses(self.document, story)
    }
    /// Finds an identified paragraph in a story, after structural changes.
    pub fn find(&self, story: &Story, id: &ParaId) -> Result<Address, EditError> {
        addresses(self.document, story)?
            .into_iter()
            .find(|a| {
                read_paragraph(self.document, a).is_ok_and(|p| {
                    p.para_id
                        .as_ref()
                        .is_some_and(|v| v.as_str().eq_ignore_ascii_case(id.as_str()))
                })
            })
            .ok_or(EditError::InvalidParagraph)
    }
    /// Executes an atomic command batch.
    ///
    /// [`transact_detailed`](Self::transact_detailed) says which command failed.
    pub fn transact(&mut self, revision: u64, commands: &[Edit]) -> Result<ChangeSet, EditError> {
        self.transact_detailed(revision, commands)
            .map_err(|failure| failure.error)
    }
    /// Executes an atomic command batch; a failure names the command and its
    /// target.
    pub fn transact_detailed(
        &mut self,
        revision: u64,
        commands: &[Edit],
    ) -> Result<ChangeSet, EditFailure> {
        self.check(revision).map_err(EditFailure::batch)?;
        if !self.notes_settled {
            return self.transact_whole(commands);
        }
        // Each command replaces a range of its story's top-level blocks; the
        // live document takes the commands one by one and every range is kept,
        // so a refusal anywhere restores exactly what was there.
        let mut patches = Vec::with_capacity(commands.len());
        for (index, command) in commands.iter().enumerate() {
            match apply_patched(self.document, command, self.limits) {
                Ok(patch) => patches.push(patch),
                Err(error) => {
                    restore(self.document, &patches);
                    return Err(EditFailure {
                        command: Some(index),
                        address: Some(command.address().clone()),
                        error,
                    });
                }
            }
        }
        let (bytes, mentions_notes) = inspect(&patches);
        // A copied or moved note reference may have to become a note of its
        // own, and that renumbers across the document: the whole-document path
        // takes such a transaction.
        if mentions_notes {
            restore(self.document, &patches);
            return self.transact_whole(commands);
        }
        // One patch (a keystroke, a format, a split) is checked where it can
        // have broken something; a batch, whose later commands may rework what
        // earlier ones did, gets the whole model checked.
        let checked = match patches.as_slice() {
            [patch] => validate_patch(
                self.document,
                &self.limits.resource,
                &patch.story,
                &patch.before,
                &patch.after,
            ),
            _ => validate(self.document, &self.limits.resource),
        };
        if let Err(error) = checked {
            restore(self.document, &patches);
            return Err(EditFailure::batch(error));
        }
        if patches.iter().all(|patch| patch.before == patch.after) {
            return Ok(self.changes(Vec::new()));
        }
        let Some(next) = self.revision.checked_add(1) else {
            restore(self.document, &patches);
            return Err(EditFailure::batch(EditError::LimitExceeded));
        };
        if self.limits.history_transactions > 0 && bytes > self.history_byte_limit {
            restore(self.document, &patches);
            return Err(EditFailure::batch(EditError::LimitExceeded));
        }
        let blocks = forward(&patches);
        self.record(Change::Blocks(patches), bytes, next);
        Ok(self.changes(blocks))
    }
    /// The transaction on a copy of the whole document, with note references
    /// given notes of their own.
    fn transact_whole(&mut self, commands: &[Edit]) -> Result<ChangeSet, EditFailure> {
        let mut candidate = self.document.clone();
        for (index, command) in commands.iter().enumerate() {
            apply(&mut candidate, command, self.limits).map_err(|error| EditFailure {
                command: Some(index),
                address: Some(command.address().clone()),
                error,
            })?;
        }
        split_shared_notes(&mut candidate).map_err(EditFailure::batch)?;
        validate(&candidate, &self.limits.resource).map_err(EditFailure::batch)?;
        if same_content(self.document, &candidate) {
            return Ok(self.changes(Vec::new()));
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or_else(|| EditFailure::batch(EditError::LimitExceeded))?;
        let bytes = snapshot_size(&*self.document).saturating_add(snapshot_size(&candidate));
        if self.limits.history_transactions > 0 && bytes > self.history_byte_limit {
            return Err(EditFailure::batch(EditError::LimitExceeded));
        }
        let before = std::mem::replace(&mut *self.document, candidate);
        let blocks = whole_story_changes(&before, self.document);
        self.notes_settled = true;
        self.record(
            Change::Whole {
                before: Box::new(before),
                after: Box::new(self.document.clone()),
            },
            bytes,
            next,
        );
        Ok(self.changes(blocks))
    }
    /// Keeps `change` as the newest undo state and moves to `next`.
    fn record(&mut self, change: Change, bytes: usize, next: u64) {
        if self.limits.history_transactions > 0 {
            self.undo.push_back(Snapshot { change, bytes });
            while self.undo.len() > self.limits.history_transactions {
                self.undo.pop_front();
            }
        }
        self.redo.clear();
        self.trim_history();
        self.revision = next;
        self.support_stale = true;
    }
    /// The estimated bytes the undo and redo history holds; see
    /// [`with_history_byte_limit`](Self::with_history_byte_limit).
    pub fn history_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(&self.redo)
            .map(|snapshot| snapshot.bytes)
            .fold(0_usize, usize::saturating_add)
    }
    /// Undo one transaction.
    pub fn undo(&mut self, revision: u64) -> Result<ChangeSet, EditError> {
        self.check(revision)?;
        if self.undo.is_empty() {
            return Err(EditError::EmptyHistory);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(EditError::LimitExceeded)?;
        let snapshot = self.undo.pop_back().ok_or(EditError::EmptyHistory)?;
        let blocks = match &snapshot.change {
            Change::Blocks(patches) => {
                restore(self.document, patches);
                backward(patches)
            }
            Change::Whole { before, after } => {
                // The state before may share notes again.
                self.document.clone_from(before);
                self.notes_settled = has_no_notes(self.document);
                whole_story_changes(after, self.document)
            }
        };
        self.redo.push_back(snapshot);
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(blocks))
    }
    /// Redo one transaction.
    pub fn redo(&mut self, revision: u64) -> Result<ChangeSet, EditError> {
        self.check(revision)?;
        if self.redo.is_empty() {
            return Err(EditError::EmptyHistory);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(EditError::LimitExceeded)?;
        let snapshot = self.redo.pop_back().ok_or(EditError::EmptyHistory)?;
        let blocks = match &snapshot.change {
            Change::Blocks(patches) => {
                reapply(self.document, patches);
                forward(patches)
            }
            Change::Whole { before, after } => {
                self.document.clone_from(after);
                self.notes_settled = true;
                whole_story_changes(before, self.document)
            }
        };
        self.undo.push_back(snapshot);
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(blocks))
    }
    fn check(&self, revision: u64) -> Result<(), EditError> {
        if self.revision == revision {
            Ok(())
        } else {
            Err(EditError::StaleRevision)
        }
    }
    fn trim_history(&mut self) {
        let mut total = self.history_bytes();
        while total > self.history_byte_limit {
            // The oldest undo state goes first, then the redo state furthest
            // from the present.
            let Some(dropped) = self.undo.pop_front().or_else(|| self.redo.pop_front()) else {
                break;
            };
            total = total.saturating_sub(dropped.bytes);
        }
    }
    fn changes(&self, blocks: Vec<BlockChange>) -> ChangeSet {
        let changed = !blocks.is_empty();
        ChangeSet {
            revision: self.revision,
            blocks,
            paragraphs: if changed {
                (0..self.document.body.blocks.len()).collect()
            } else {
                vec![]
            },
            invalidate_layout: changed,
            invalidate_support: changed,
        }
    }
}
/// The top-level block range `command` may replace in its story: the block it
/// targets, the outermost container that holds it, or - for the commands that
/// add or remove top-level blocks - the blocks around the change.
fn region(command: &Edit) -> (usize, usize) {
    let at = command.address();
    if let Some(outer) = at.containers.first() {
        let top = match outer {
            Container::Cell { table, .. } => *table,
            Container::Sdt(index) => *index,
            Container::TextBox { paragraph, .. } => *paragraph,
        };
        return (top, top.saturating_add(1));
    }
    match command {
        Edit::Insert { .. } => (at.block, at.block),
        Edit::Join { .. } => (at.block, at.block.saturating_add(2)),
        _ => (at.block, at.block.saturating_add(1)),
    }
}
/// Applies `command` to the live document and returns the range it replaced.
///
/// A refused command leaves the document as it found it.
fn apply_patched(
    document: &mut Document,
    command: &Edit,
    limits: EditLimits,
) -> Result<Patch, EditError> {
    let story = command.address().story.clone();
    let (start, end) = region(command);
    let mut patch = Patch {
        story,
        start,
        before: Vec::new(),
        after: Vec::new(),
    };
    let mut length = 0;
    mutate_story(document, &patch.story, |root| {
        length = root.len();
        let end = end.min(root.len());
        patch.start = patch.start.min(end);
        patch.before = root
            .get(patch.start..end)
            .map(<[Block]>::to_vec)
            .unwrap_or_default();
        Ok(())
    })?;
    let result = apply(document, command, limits);
    let _ = mutate_story(document, &patch.story, |root| {
        // The range grew or shrank by as much as the story did.
        let span = (patch.before.len() + root.len()).saturating_sub(length);
        let end = (patch.start + span).min(root.len());
        patch.after = root
            .get(patch.start.min(end)..end)
            .map(<[Block]>::to_vec)
            .unwrap_or_default();
        Ok(())
    });
    match result {
        Ok(()) => Ok(patch),
        Err(error) => {
            restore(document, std::slice::from_ref(&patch));
            Err(error)
        }
    }
}
/// Puts back what `patches` replaced, newest first.
fn restore(document: &mut Document, patches: &[Patch]) {
    for patch in patches.iter().rev() {
        let _ = mutate_story(document, &patch.story, |root| {
            let end = (patch.start + patch.after.len()).min(root.len());
            root.splice(patch.start.min(end)..end, patch.before.iter().cloned());
            Ok(())
        });
    }
}
/// Replaces again what `patches` replaced, oldest first.
fn reapply(document: &mut Document, patches: &[Patch]) {
    for patch in patches {
        let _ = mutate_story(document, &patch.story, |root| {
            let end = (patch.start + patch.before.len()).min(root.len());
            root.splice(patch.start.min(end)..end, patch.after.iter().cloned());
            Ok(())
        });
    }
}
/// The history estimate of `patches`, and whether any of them holds a note
/// reference (`FootnoteRef` / `EndnoteRef`, as an inline or as run content).
fn inspect(patches: &[Patch]) -> (usize, bool) {
    let mut bytes = 0_usize;
    let mut notes = false;
    for patch in patches {
        let text = format!("{:?}{:?}", patch.before, patch.after);
        bytes = bytes.saturating_add(text.len());
        notes |= text.contains("FootnoteRef(") || text.contains("EndnoteRef(");
    }
    (bytes, notes)
}
/// The block changes of `patches`, applied oldest first.
fn forward(patches: &[Patch]) -> Vec<BlockChange> {
    let mut changes = Vec::new();
    for patch in patches {
        fold(
            &mut changes,
            &patch.story,
            patch.start,
            patch.before.len(),
            patch.after.len(),
        );
    }
    finish(changes)
}
/// The block changes of undoing `patches`, newest first.
fn backward(patches: &[Patch]) -> Vec<BlockChange> {
    let mut changes = Vec::new();
    for patch in patches.iter().rev() {
        fold(
            &mut changes,
            &patch.story,
            patch.start,
            patch.after.len(),
            patch.before.len(),
        );
    }
    finish(changes)
}
/// Folds one replacement - `removed` blocks at `start` became `added` - into
/// `changes`, which are kept disjoint and in the coordinates after it.
///
/// A change that overlaps or touches the replaced range is absorbed: the
/// merged range covers both, and its `replaced` counts the original blocks
/// under it - the blocks no earlier change touched, one for one, plus what
/// each absorbed change replaced.
fn fold(changes: &mut Vec<BlockChange>, story: &Story, start: usize, removed: usize, added: usize) {
    let end = start + removed;
    let (mut low, mut high) = (start, end);
    let (mut current, mut original) = (0_usize, 0_usize);
    let mut kept = Vec::with_capacity(changes.len() + 1);
    for change in changes.drain(..) {
        if change.story != *story || change.range.end < start {
            kept.push(change);
        } else if change.range.start > end {
            let range =
                (change.range.start - removed + added)..(change.range.end - removed + added);
            kept.push(BlockChange { range, ..change });
        } else {
            low = low.min(change.range.start);
            high = high.max(change.range.end);
            current += change.range.len();
            original += change.replaced;
        }
    }
    let span = high - low;
    kept.push(BlockChange {
        story: story.clone(),
        range: low..(low + span - removed + added),
        replaced: span - current + original,
    });
    *changes = kept;
}
/// `changes`, ordered by start within each story.
fn finish(mut changes: Vec<BlockChange>) -> Vec<BlockChange> {
    changes.sort_by_key(|change| change.range.start);
    changes
}
/// Every story of `after` as one change, against its length in `before`.
fn whole_story_changes(before: &Document, after: &Document) -> Vec<BlockChange> {
    let length = |document: &Document, story: &Story| -> usize {
        match story {
            Story::Body => document.body.blocks.len(),
            Story::HeaderFooter(part) => document
                .headers_footers
                .iter()
                .find(|header| &header.part == part)
                .map_or(0, |header| header.blocks.len()),
            Story::Footnote(id) => document
                .footnotes
                .get(*id)
                .map_or(0, |note| note.blocks.len()),
            Story::Endnote(id) => document
                .endnotes
                .get(*id)
                .map_or(0, |note| note.blocks.len()),
        }
    };
    let mut stories = vec![Story::Body];
    stories.extend(
        after
            .headers_footers
            .iter()
            .map(|header| Story::HeaderFooter(header.part.clone())),
    );
    stories.extend(after.footnotes.iter().map(|note| Story::Footnote(note.id)));
    stories.extend(after.endnotes.iter().map(|note| Story::Endnote(note.id)));
    stories
        .into_iter()
        .map(|story| BlockChange {
            range: 0..length(after, &story),
            replaced: length(before, &story),
            story,
        })
        .collect()
}
/// Whether the document has no footnotes or endnotes for references to share.
fn has_no_notes(document: &Document) -> bool {
    document.footnotes.iter().next().is_none() && document.endnotes.iter().next().is_none()
}
fn snapshot_size(document: &Document) -> usize {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0 = self.0.saturating_add(s.len());
            Ok(())
        }
    }
    let mut count = Count(0);
    let _ = std::fmt::write(&mut count, format_args!("{document:?}"));
    count.0
}
fn same_content(a: &Document, b: &Document) -> bool {
    a.body == b.body
        && a.headers_footers == b.headers_footers
        && a.sections == b.sections
        && a.footnotes.iter().eq(b.footnotes.iter())
        && a.endnotes.iter().eq(b.endnotes.iter())
}

#[cfg(test)]
mod tests {
    //! The block-patch path against the whole-document path it replaced: the
    //! same commands must be accepted or refused alike and leave the same
    //! document, and undo/redo must agree too.

    use super::Editor;
    use crate::{Address, Edit, EditLimits, FormatPatch, Story};
    use proptest::prelude::*;
    use strict_ooxml_core::opc::{OpenOptions, Package};
    use strict_ooxml_testkit::DocxBuilder;
    use strict_ooxml_wml::model::{Document, TriState};
    use strict_ooxml_wml::{parse_document, ParseOptions};

    fn document() -> Document {
        // A bookmark across paragraphs, a complex field and a section break:
        // the invariants that span blocks, which a patch is checked against
        // only when it touches their marks.
        let body = concat!(
            "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"b\"/><w:r><w:t>Alpha beta</w:t></w:r></w:p>",
            "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>",
            "<w:r><w:instrText>PAGE</w:instrText></w:r>",
            "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
            "<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>first section</w:t></w:r></w:p>",
            "<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid>",
            "<w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
            "<w:sdt><w:sdtContent><w:p><w:r><w:t>sdt</w:t></w:r></w:p></w:sdtContent></w:sdt>",
            "<w:p><w:r><w:t>gamma</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p>",
            "<w:p><w:r><w:t>last</w:t></w:r></w:p>",
            "<w:sectPr/>",
        );
        let bytes = DocxBuilder::strict().body(body).build();
        let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
        parse_document(&package, &ParseOptions::default()).expect("document")
    }

    fn snapshot(document: &Document) -> String {
        format!("{document:?}")
    }

    fn edit(editor: &Editor<'_>, pick: usize, kind: u8, a: usize, b: usize) -> Edit {
        let addresses = editor.paragraphs(&Story::Body).unwrap_or_default();
        let at = if kind == 9 || addresses.is_empty() {
            Address::body(pick % 8)
        } else {
            addresses[pick % addresses.len()].clone()
        };
        match kind {
            0 | 1 => Edit::Text {
                at,
                range: a..a + b,
                text: "xy".repeat(b % 3),
            },
            2 => Edit::Format {
                at,
                range: a..a + b,
                patch: FormatPatch {
                    italic: Some(TriState::On),
                    ..FormatPatch::default()
                },
            },
            3 => Edit::Split { at, offset: a },
            4 => Edit::Join { at },
            5 => Edit::Delete { at },
            6 => Edit::Identify { at },
            _ => Edit::Text {
                at,
                range: a..a,
                text: "z".to_owned(),
            },
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

        #[test]
        fn the_patch_path_matches_the_whole_document_path(
            steps in prop::collection::vec((any::<usize>(), 0u8..10, 0usize..8, 0usize..5), 1..20),
            batch in 1usize..3,
        ) {
            let mut fast_document = document();
            let mut whole_document = document();
            let mut fast = Editor::new(&mut fast_document, EditLimits::default()).expect("fast");
            let mut whole = Editor::new(&mut whole_document, EditLimits::default()).expect("whole");
            for chunk in steps.chunks(batch) {
                let edits: Vec<Edit> = chunk
                    .iter()
                    .map(|&(pick, kind, a, b)| edit(&fast, pick, kind, a, b))
                    .collect();
                let revision = fast.revision();
                let left = fast.transact_detailed(revision, &edits);
                let right = whole.transact_whole(&edits);
                prop_assert_eq!(left.is_ok(), right.is_ok(), "{:?}", edits);
                prop_assert_eq!(snapshot(fast.document()), snapshot(whole.document()));
            }
            // A batch that changes and then restores the same blocks is one
            // history entry on the patch path and none on the whole path, so
            // the two are compared at the ends of their histories.
            while fast.undo(fast.revision()).is_ok() {}
            while whole.undo(whole.revision()).is_ok() {}
            prop_assert_eq!(snapshot(fast.document()), snapshot(&document()));
            prop_assert_eq!(snapshot(whole.document()), snapshot(&document()));
            while fast.redo(fast.revision()).is_ok() {}
            while whole.redo(whole.revision()).is_ok() {}
            prop_assert_eq!(snapshot(fast.document()), snapshot(whole.document()));
        }
    }
}
