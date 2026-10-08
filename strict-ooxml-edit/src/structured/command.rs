//! The commands of a structural transaction and how a refused one is reported.
use crate::{EditError, FormatPatch};
use std::ops::Range;
use strict_ooxml_wml::model::{
    Block, CellProperties, Drawing, FrameProperties, GridCol, Inline, ParagraphProperties,
    RunProperties, TableProperties, TableRow,
};

use super::address::Address;

/// A rejected transaction: which command failed, where, and why.
///
/// The document and its history are unchanged, as for any [`EditError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditFailure {
    /// Index of the failing command in the batch; `None` when the batch as a
    /// whole was refused (a stale revision, the model check after the last
    /// command, the history budget).
    pub command: Option<usize>,
    /// The failing command's target, when one command failed.
    pub address: Option<Address>,
    /// What went wrong.
    pub error: EditError,
}
impl EditFailure {
    /// A failure of the batch as a whole.
    fn batch(error: EditError) -> Self {
        Self {
            command: None,
            address: None,
            error,
        }
    }
}
impl std::fmt::Display for EditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.command, &self.address) {
            (Some(index), Some(at)) => write!(
                f,
                "command {index} (story {:?}, block {}): {}",
                at.story, at.block, self.error
            ),
            (Some(index), None) => write!(f, "command {index}: {}", self.error),
            _ => write!(f, "transaction: {}", self.error),
        }
    }
}
impl std::error::Error for EditFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
impl From<EditFailure> for EditError {
    fn from(failure: EditFailure) -> Self {
        failure.error
    }
}
/// One command in a structural transaction.
#[derive(Clone, Debug)]
pub enum Edit {
    /// Split a paragraph; the right half inherits properties and the section boundary.
    Split {
        /// Target paragraph.
        at: Address,
        /// Scalar offset.
        offset: usize,
    },
    /// Join a paragraph with its immediate successor.
    Join {
        /// Left paragraph.
        at: Address,
    },
    /// Insert a block before the address (including at the sequence end).
    Insert {
        /// Insertion address.
        at: Address,
        /// New block.
        block: Box<Block>,
    },
    /// Delete a block, preserving the required final cell paragraph.
    Delete {
        /// Target block.
        at: Address,
    },
    /// Replace plain paragraph text using scalar offsets.
    Text {
        /// Target paragraph.
        at: Address,
        /// Scalar range.
        range: Range<usize>,
        /// Replacement text.
        text: String,
    },
    /// Change specified character properties on plain paragraph text.
    Format {
        /// Target paragraph.
        at: Address,
        /// Scalar range.
        range: Range<usize>,
        /// Direct property patch.
        patch: FormatPatch,
    },
    /// Replace paragraph properties; section boundaries cannot be changed here.
    ParagraphProperties {
        /// Target paragraph.
        at: Address,
        /// Complete direct properties.
        properties: Box<ParagraphProperties>,
    },
    /// Set or clear a text frame.
    Frame {
        /// Target paragraph.
        at: Address,
        /// New frame properties.
        frame: Option<FrameProperties>,
    },
    /// Replace one run's direct properties without flattening wrappers.
    RunProperties {
        /// Target paragraph.
        at: Address,
        /// Inline indices through hyperlink, SDT or directional wrappers.
        inline: Vec<usize>,
        /// Complete direct properties.
        properties: Box<RunProperties>,
    },
    /// Replace a scalar range inside one text node, preserving all other content.
    TextNode {
        /// Target paragraph.
        at: Address,
        /// Inline indices ending at the containing run.
        inline: Vec<usize>,
        /// Run content index.
        content: usize,
        /// Scalar range within the text node.
        range: Range<usize>,
        /// Replacement text.
        text: String,
    },
    /// Insert a typed inline (tabs, breaks, drawings and wrappers remain structured).
    InsertInline {
        /// Target paragraph.
        at: Address,
        /// Top-level inline insertion index.
        index: usize,
        /// New inline.
        inline: Box<Inline>,
    },
    /// Delete one inline; validation refuses dangling bookmark/field boundaries.
    DeleteInline {
        /// Target paragraph.
        at: Address,
        /// Top-level inline index.
        index: usize,
    },
    /// Replace table properties without changing its contents.
    TableProperties {
        /// Target table.
        at: Address,
        /// Complete direct properties.
        properties: Box<TableProperties>,
    },
    /// Replace cell properties; merge topology is validated atomically.
    CellProperties {
        /// Target table.
        at: Address,
        /// Row index.
        row: usize,
        /// Cell index.
        cell: usize,
        /// Complete direct properties.
        properties: Box<CellProperties>,
    },
    /// Set the table grid.
    Grid {
        /// Target table.
        at: Address,
        /// New columns.
        columns: Vec<GridCol>,
    },
    /// Insert a row with fresh paragraph identities.
    InsertRow {
        /// Target table.
        at: Address,
        /// Insertion index.
        index: usize,
        /// New row.
        row: Box<TableRow>,
    },
    /// Delete one table row; an empty table is rejected.
    DeleteRow {
        /// Target table.
        at: Address,
        /// Row index.
        index: usize,
    },
    /// Replace a drawing's modeled geometry/payload, keeping source bytes external.
    Drawing {
        /// Target paragraph.
        at: Address,
        /// Inline wrapper indices.
        inline: Vec<usize>,
        /// Run content index, or None for `Inline::Drawing`.
        content: Option<usize>,
        /// New drawing.
        drawing: Box<Drawing>,
    },
    /// Give a paragraph a stable session identity if it has none.
    Identify {
        /// Target paragraph.
        at: Address,
    },
}
impl Edit {
    /// The block the command targets.
    pub fn address(&self) -> &Address {
        match self {
            Self::Split { at, .. }
            | Self::Join { at }
            | Self::Insert { at, .. }
            | Self::Delete { at }
            | Self::Text { at, .. }
            | Self::Format { at, .. }
            | Self::ParagraphProperties { at, .. }
            | Self::Frame { at, .. }
            | Self::RunProperties { at, .. }
            | Self::TextNode { at, .. }
            | Self::InsertInline { at, .. }
            | Self::DeleteInline { at, .. }
            | Self::TableProperties { at, .. }
            | Self::CellProperties { at, .. }
            | Self::Grid { at, .. }
            | Self::InsertRow { at, .. }
            | Self::DeleteRow { at, .. }
            | Self::Drawing { at, .. }
            | Self::Identify { at } => at,
        }
    }
}
