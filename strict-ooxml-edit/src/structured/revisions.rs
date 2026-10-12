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

/// Whether `paragraph` or any run in it carries a tracked change.
pub(crate) fn has_revisions(paragraph: &Paragraph) -> bool {
    paragraph.revision.is_some() || inlines_have_revisions(&paragraph.inlines)
}

fn inlines_have_revisions(items: &[Inline]) -> bool {
    items.iter().any(|item| match item {
        Inline::Run(run) => run.revision.is_some(),
        Inline::Hyperlink(v) => inlines_have_revisions(&v.inlines),
        Inline::SdtInline(v) => inlines_have_revisions(&v.inlines),
        Inline::Directional(v) => inlines_have_revisions(&v.inlines),
        Inline::Field(v) => inlines_have_revisions(&v.inlines),
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

/// Resolves every tracked change of the runs in `items`, recursively through
/// hyperlinks, inline SDTs, directional wrappers and simple fields.
fn resolve_inlines(items: &mut Vec<Inline>, accept: bool) {
    items.retain_mut(|item| match item {
        Inline::Run(run) => match run.revision.as_ref().map(|revision| revision.kind) {
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
            resolve_inlines(&mut v.inlines, accept);
            true
        }
        Inline::SdtInline(v) => {
            resolve_inlines(&mut v.inlines, accept);
            true
        }
        Inline::Directional(v) => {
            resolve_inlines(&mut v.inlines, accept);
            true
        }
        Inline::Field(v) => {
            resolve_inlines(&mut v.inlines, accept);
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
pub(super) fn resolve(
    blocks: &mut Vec<Block>,
    index: usize,
    accept: bool,
) -> Result<(), EditError> {
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(index) else {
        return Err(EditError::InvalidParagraph);
    };
    resolve_inlines(&mut paragraph.inlines, accept);
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
