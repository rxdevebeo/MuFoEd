//! The editor: revisions, transactions and the undo/redo history.
use crate::{ChangeSet, EditError, EditLimits};
use std::collections::VecDeque;
use strict_ooxml_wml::model::{Document, ParaId, Paragraph};

use super::address::{addresses, read_paragraph, Address, Story};
use super::apply::apply;
use super::command::{Edit, EditFailure};
use super::notes::split_shared_notes;
use super::validate::validate;

/// Structural editor over the live WML document; `EditSession` is a body-only
/// view of it.
pub struct Editor<'a> {
    pub(crate) document: &'a mut Document,
    revision: u64,
    limits: EditLimits,
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
    history_byte_limit: usize,
    pub(crate) support_stale: bool,
    pub(crate) input_pipeline: strict_ooxml_core::pipeline::PipelineSummary,
}
struct Snapshot {
    before: Document,
    after: Document,
    bytes: usize,
}
impl<'a> Editor<'a> {
    /// Opens an exclusive session.
    pub fn new(document: &'a mut Document, limits: EditLimits) -> Result<Self, EditError> {
        validate(document)?;
        Ok(Self {
            document,
            revision: 0,
            limits,
            undo: VecDeque::new(),
            redo: vec![],
            history_byte_limit: 64 * 1024 * 1024,
            support_stale: false,
            input_pipeline: strict_ooxml_core::pipeline::PipelineSummary::new(),
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
    /// The estimate is the UTF-8 debug serialization of both model snapshots;
    /// media bytes remain external. Older undo states are evicted first.
    #[must_use]
    pub fn with_history_byte_limit(mut self, bytes: usize) -> Self {
        self.history_byte_limit = bytes;
        self.trim_history();
        self
    }
    /// Explicitly validates the current model and all editable stories.
    pub fn validate(&self) -> Result<(), EditError> {
        validate(self.document)
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
        let mut candidate = self.document.clone();
        for (index, command) in commands.iter().enumerate() {
            apply(&mut candidate, command, self.limits).map_err(|error| EditFailure {
                command: Some(index),
                address: Some(command.address().clone()),
                error,
            })?;
        }
        split_shared_notes(&mut candidate).map_err(EditFailure::batch)?;
        validate(&candidate).map_err(EditFailure::batch)?;
        if same_content(self.document, &candidate) {
            return Ok(self.changes(false));
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or_else(|| EditFailure::batch(EditError::LimitExceeded))?;
        if self.limits.history_transactions > 0 {
            let bytes = snapshot_size(self.document).saturating_add(snapshot_size(&candidate));
            if bytes > self.history_byte_limit {
                return Err(EditFailure::batch(EditError::LimitExceeded));
            }
            self.undo.push_back(Snapshot {
                before: self.document.clone(),
                after: candidate.clone(),
                bytes,
            });
            while self.undo.len() > self.limits.history_transactions {
                self.undo.pop_front();
            }
        }
        *self.document = candidate;
        self.redo.clear();
        self.trim_history();
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(true))
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
        let change = self.undo.pop_back().ok_or(EditError::EmptyHistory)?;
        *self.document = change.before.clone();
        self.redo.push(change);
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(true))
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
        let change = self.redo.pop().ok_or(EditError::EmptyHistory)?;
        *self.document = change.after.clone();
        self.undo.push_back(change);
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(true))
    }
    fn check(&self, revision: u64) -> Result<(), EditError> {
        if self.revision == revision {
            Ok(())
        } else {
            Err(EditError::StaleRevision)
        }
    }
    fn trim_history(&mut self) {
        while self
            .undo
            .iter()
            .chain(&self.redo)
            .map(|snapshot| snapshot.bytes)
            .fold(0_usize, usize::saturating_add)
            > self.history_byte_limit
        {
            if self.undo.pop_front().is_none() {
                if self.redo.is_empty() {
                    break;
                }
                self.redo.remove(0);
            }
        }
    }
    fn changes(&self, changed: bool) -> ChangeSet {
        ChangeSet {
            revision: self.revision,
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
