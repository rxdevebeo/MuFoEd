//! K06 (first part): moving a block between containers of one story.
use crate::{EditError, Invariant, Unsupported};
use strict_ooxml_wml::model::Document;

use super::address::{descend_mut, mutate_story, Address, Container};
use super::validate::contains_boundary;

/// The block index a container stands at in its parent's blocks.
fn index(container: &Container) -> usize {
    match container {
        Container::Cell { table, .. } => *table,
        Container::Sdt(index) => *index,
        Container::TextBox { paragraph, .. } => *paragraph,
    }
}

fn set_index(container: &mut Container, value: usize) {
    match container {
        Container::Cell { table, .. } => *table = value,
        Container::Sdt(index) => *index = value,
        Container::TextBox { paragraph, .. } => *paragraph = value,
    }
}

/// The top-level range of `at`'s story a move from `at` to `to` changes.
pub(super) fn region(at: &Address, to: &Address) -> (usize, usize) {
    let source = at.containers.first().map_or(at.block, index);
    let (start, end) = match to.containers.first() {
        Some(outer) => (index(outer), index(outer).saturating_add(1)),
        // An insertion point changes no block of its own.
        None => (to.block, to.block),
    };
    (source.min(start), source.saturating_add(1).max(end))
}

/// Where `to`, written against the story before the move, points once the
/// block at `at` is gone; `None` when it points into that block.
fn after_removal(at: &Address, to: &Address) -> Option<Address> {
    let mut to = to.clone();
    let depth = at.containers.len();
    if to.containers.get(..depth) != Some(at.containers.as_slice()) {
        return Some(to);
    }
    match to.containers.get_mut(depth) {
        // Inside a sibling of the moved block, or inside the block itself.
        Some(container) => {
            let at_index = index(container);
            if at_index == at.block {
                return None;
            }
            if at_index > at.block {
                set_index(container, at_index - 1);
            }
        }
        // In the same container.
        None => {
            if to.block > at.block {
                to.block -= 1;
            }
        }
    }
    Some(to)
}

/// Moves the block at `at` to the insertion point `to` of the same story,
/// both addressed in the story as it is before the move. The block keeps its
/// paragraph identities, since it leaves the place it had.
pub(super) fn move_block(
    document: &mut Document,
    at: &Address,
    to: &Address,
) -> Result<(), EditError> {
    if at.story != to.story {
        return Err(EditError::UnsupportedContent(Unsupported::Target));
    }
    let to = after_removal(at, to).ok_or(EditError::UnsupportedContent(Unsupported::Target))?;
    mutate_story(document, &at.story, |root| {
        let blocks = descend_mut(root, &at.containers)?;
        let block = blocks.get(at.block).ok_or(EditError::InvalidParagraph)?;
        // A section ends where its boundary paragraph is; moving one would
        // carry the section's properties elsewhere.
        if contains_boundary(block) {
            return Err(EditError::InvalidModel(Invariant::SectionBoundary));
        }
        let block = blocks.remove(at.block);
        let target = descend_mut(root, &to.containers)?;
        if to.block > target.len() {
            return Err(EditError::InvalidParagraph);
        }
        target.insert(to.block, block);
        Ok(())
    })
}
