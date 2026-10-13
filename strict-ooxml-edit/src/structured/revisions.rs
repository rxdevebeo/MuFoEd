//! Accepting and rejecting tracked changes (`w:ins`, `w:del`, `w:moveFrom`,
//! `w:moveTo` around runs, and the same on a paragraph mark).
//!
//! Accepting keeps what was inserted or moved here and drops what was deleted
//! or moved away; rejecting does the opposite. Either way the kept content
//! loses its revision. A paragraph mark that goes away joins its paragraph
//! with the next one, which keeps the next paragraph's properties - the mark
//! that survives carries them.
use crate::{EditError, Invariant};
use strict_ooxml_wml::model::{Block, Inline, Paragraph, RevisionKind};

/// Whether `paragraph` or a run in it carries the tracked change `only`, or
/// any one when `only` is `None`.
pub(crate) fn has_revision(paragraph: &Paragraph, only: Option<u32>) -> bool {
    paragraph
        .revision
        .as_ref()
        .is_some_and(|revision| selected(revision.id, only))
        || inlines_have_revisions(&paragraph.inlines, only)
}

fn selected(id: u32, only: Option<u32>) -> bool {
    only.is_none_or(|only| only == id)
}

fn inlines_have_revisions(items: &[Inline], only: Option<u32>) -> bool {
    items.iter().any(|item| match item {
        Inline::Run(run) => run
            .revision
            .as_ref()
            .is_some_and(|revision| selected(revision.id, only)),
        Inline::Hyperlink(v) => inlines_have_revisions(&v.inlines, only),
        Inline::SdtInline(v) => inlines_have_revisions(&v.inlines, only),
        Inline::Directional(v) => inlines_have_revisions(&v.inlines, only),
        Inline::Field(v) => inlines_have_revisions(&v.inlines, only),
        _ => false,
    })
}

/// Whether content tracked as `kind` stays when its changes are accepted
/// (`accept`) or rejected.
fn kept(kind: RevisionKind, accept: bool) -> bool {
    match kind {
        RevisionKind::Insert | RevisionKind::MoveTo => accept,
        RevisionKind::Delete | RevisionKind::MoveFrom => !accept,
    }
}

/// Resolves the tracked changes of the runs in `items` - every one, or only
/// `only` - recursively through hyperlinks, inline SDTs, directional wrappers
/// and simple fields.
fn resolve_inlines(items: &mut Vec<Inline>, accept: bool, only: Option<u32>) {
    items.retain_mut(|item| match item {
        Inline::Run(run) => match run
            .revision
            .as_ref()
            .filter(|revision| selected(revision.id, only))
            .map(|revision| revision.kind)
        {
            None => true,
            Some(kind) => {
                let keep = kept(kind, accept);
                if keep {
                    run.revision = None;
                }
                keep
            }
        },
        Inline::Hyperlink(v) => {
            resolve_inlines(&mut v.inlines, accept, only);
            true
        }
        Inline::SdtInline(v) => {
            resolve_inlines(&mut v.inlines, accept, only);
            true
        }
        Inline::Directional(v) => {
            resolve_inlines(&mut v.inlines, accept, only);
            true
        }
        Inline::Field(v) => {
            resolve_inlines(&mut v.inlines, accept, only);
            true
        }
        _ => true,
    });
}

/// Accepts or rejects the tracked changes of the paragraph at `index` in
/// `blocks`: its runs, and its mark.
///
/// A mark that goes away joins the paragraph with the next block when that is
/// a paragraph; the last paragraph of a story or of a cell, or one followed by
/// a table, keeps its mark - Word does not delete that mark either - and only
/// loses the revision.
///
/// With `only`, just the tracked change with that `w:id` is resolved; the
/// others stay as they are.
pub(super) fn resolve(
    blocks: &mut Vec<Block>,
    index: usize,
    accept: bool,
    only: Option<u32>,
) -> Result<(), EditError> {
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(index) else {
        return Err(EditError::InvalidParagraph);
    };
    resolve_inlines(&mut paragraph.inlines, accept, only);
    if !paragraph
        .revision
        .as_ref()
        .is_some_and(|revision| selected(revision.id, only))
    {
        return Ok(());
    }
    let Some(revision) = paragraph.revision.take() else {
        return Ok(());
    };
    if kept(revision.kind, accept) {
        return Ok(());
    }
    let section = paragraph.props.section.is_some();
    let next = index.saturating_add(1);
    if !matches!(blocks.get(next), Some(Block::Paragraph(_))) {
        return Ok(());
    }
    // A section break rides on the mark: joining past it would merge two
    // sections, which a tracked change of the mark alone cannot mean. The
    // refusal rolls the transaction back, revision included.
    if section {
        return Err(EditError::InvalidModel(Invariant::SectionBoundary));
    }
    let Block::Paragraph(mut right) = blocks.remove(next) else {
        return Err(EditError::InvalidParagraph);
    };
    let Some(Block::Paragraph(left)) = blocks.get_mut(index) else {
        return Err(EditError::InvalidParagraph);
    };
    // The surviving mark is the right paragraph's: its properties, section
    // break and own revision; the left paragraph keeps its identity.
    left.inlines.append(&mut right.inlines);
    left.props = right.props;
    left.revision = right.revision;
    Ok(())
}
