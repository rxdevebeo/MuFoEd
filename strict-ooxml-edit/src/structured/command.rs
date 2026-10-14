//! The commands of a structural transaction and how a refused one is reported.
use crate::{EditError, FormatPatch};
use std::ops::Range;
use strict_ooxml_wml::model::{
    Block, CellProperties, Drawing, FrameProperties, GridCol, Inline, ParagraphProperties,
    RunProperties, SectionProperties, TableProperties, TableRow,
};

use super::address::Address;

/// A point in a paragraph's inline tree: before content `content` of the run
/// at the inline path `inline` (as in [`crate::TextSlice`]), `offset` scalars
/// into it when that content is text and `0` otherwise. `content` equal to
/// the run's content length is the run's end. Points are ordered as the
/// paragraph reads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InlinePoint {
    /// Inline path ending at a run.
    pub inline: Vec<usize>,
    /// Content index in that run.
    pub content: usize,
    /// Scalar offset in that content when it is text.
    pub offset: usize,
}
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
    pub(super) fn batch(error: EditError) -> Self {
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
    /// Split a paragraph at a point of its inline tree, whatever else it
    /// holds (K06). Hyperlinks and bidi wrappers around the point continue in
    /// the right half with the same properties; the right half takes a fresh
    /// identity, the paragraph mark and the section boundary, as with
    /// [`Edit::Split`]. A point inside a field, an inline content control or a
    /// tracked run is refused.
    SplitAt {
        /// Target paragraph.
        at: Address,
        /// Where to split.
        point: InlinePoint,
    },
    /// Format the text and other run content between two points of a
    /// paragraph's inline tree, whatever else it holds (K06): each run the
    /// range covers in part is cut where the range ends, and wrappers are not
    /// divided. Runs the patch leaves as they were are not cut. A range
    /// touching a tracked run is refused.
    FormatAt {
        /// Target paragraph.
        at: Address,
        /// Start of the range.
        from: InlinePoint,
        /// End of the range, not before `from`.
        to: InlinePoint,
        /// Properties to set or clear.
        patch: FormatPatch,
    },
    /// Replace what lies between two points of a paragraph's inline tree with
    /// plain text, whatever else the paragraph holds (K06): the text goes into
    /// the run at `from` with its properties. Tabs, breaks, drawings and other
    /// inlines between the points go; bookmark and comment range marks stay;
    /// runs and wrappers left empty go. A range over field characters, a note
    /// or comment reference, a tracked run or opaque markup is refused, and so
    /// is a point inside a field.
    ReplaceAt {
        /// Target paragraph.
        at: Address,
        /// Start of the range.
        from: InlinePoint,
        /// End of the range, not before `from`.
        to: InlinePoint,
        /// Replacement text, without control characters.
        text: String,
    },
    /// Join a paragraph with its immediate successor, whatever their inlines
    /// hold.
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
    /// Move a block to an insertion point of the same story - into or out of
    /// a cell, a block SDT or a text box - keeping its paragraph identities
    /// (K06). Both addresses are read in the story as it is before the move;
    /// a section boundary paragraph does not move.
    Move {
        /// Block to move.
        at: Address,
        /// Insertion point, as for [`Edit::Insert`].
        to: Address,
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
    /// Insert a footnote (or, with `endnote`, an endnote) holding `blocks`,
    /// under the next free positive id, and a run with its reference mark at
    /// top-level inline `index` of the paragraph at `at`, which must not be in
    /// a note. The note's first paragraph starts with its number marker.
    InsertNote {
        /// Paragraph that gets the reference.
        at: Address,
        /// Top-level inline insertion index.
        index: usize,
        /// An endnote rather than a footnote.
        endnote: bool,
        /// The note's content; a final paragraph is added if it has none.
        blocks: Vec<Block>,
    },
    /// Delete the normal footnote or endnote whose story `at` names. One that
    /// a reference still names is refused: delete the reference (with
    /// `DeleteInline`) in the same transaction. Deleting content never drops
    /// a note on its own.
    DeleteNote {
        /// Any address in the note's story.
        at: Address,
    },
    /// Set the properties of the section the top-level body paragraph at `at`
    /// belongs to (or ends): page size and orientation, margins, columns,
    /// header/footer references, page numbering. The section list and the
    /// section's boundary paragraph change together.
    SectionProperties {
        /// A top-level body paragraph of the section.
        at: Address,
        /// Complete section properties.
        properties: Box<SectionProperties>,
    },
    /// End a new section with the top-level body paragraph at `at`; the new
    /// section takes the properties of the one it was split from.
    InsertSectionBreak {
        /// The paragraph that ends the new section.
        at: Address,
    },
    /// Remove the section break the top-level body paragraph at `at` carries.
    /// The merged section keeps the properties of the section before the
    /// break, or with `keep_after` those of the section after it.
    RemoveSectionBreak {
        /// The paragraph carrying the break.
        at: Address,
        /// Keep the properties of the section after the break.
        keep_after: bool,
    },
    /// Merge the cells covering `rows` × `columns` of the table's grid into
    /// one. The top-left cell keeps its properties and receives every cell's
    /// content in reading order; a cell that sticks out of the rectangle, or a
    /// gap in it, refuses the merge.
    MergeCells {
        /// Target table.
        at: Address,
        /// Half-open row range.
        rows: Range<usize>,
        /// Half-open grid column range.
        columns: Range<usize>,
    },
    /// Split the cell covering grid position (`row`, `column`) into
    /// `columns` × `rows` cells (at most 63 each). Grid columns or rows are
    /// added when the cell has fewer than the parts; the total width stays and
    /// the top-left part keeps the content.
    SplitCell {
        /// Target table.
        at: Address,
        /// Row of a grid position inside the cell.
        row: usize,
        /// Grid column of a position inside the cell.
        column: usize,
        /// Rows to split into.
        rows: usize,
        /// Columns to split into.
        columns: usize,
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
    /// Accept the paragraph's tracked changes: inserted and moved-in runs
    /// stay, deleted and moved-out runs go, and a deleted paragraph mark joins
    /// the paragraph with the next one.
    AcceptRevisions {
        /// Target paragraph.
        at: Address,
    },
    /// Replace plain paragraph text as a tracked change, the way Word does with
    /// Track Changes on: the replaced text stays, marked deleted, and the new
    /// text follows it, marked inserted. Inside an insertion - or right at its
    /// end - the edit extends or trims that insertion instead.
    TrackedText {
        /// Target paragraph.
        at: Address,
        /// Half-open Unicode scalar range; tracked text counts.
        range: Range<usize>,
        /// Replacement text.
        text: String,
        /// Author recorded on the change (`w:author`).
        author: String,
        /// Date recorded on the change (`w:date`, ISO 8601), if any.
        date: Option<String>,
    },
    /// Split a paragraph as a tracked change, the way Word does with Track
    /// Changes on: the left half ends with a new paragraph mark marked
    /// inserted; the right half keeps the original mark.
    TrackedSplit {
        /// Target paragraph.
        at: Address,
        /// Scalar offset.
        offset: usize,
        /// Author recorded on the change (`w:author`).
        author: String,
        /// Date recorded on the change (`w:date`, ISO 8601), if any.
        date: Option<String>,
    },
    /// Join a paragraph with the next one as a tracked change: its mark is
    /// marked deleted, and accepting the change joins them. A mark that is
    /// itself a tracked insertion goes at once, joining the paragraphs, as
    /// Word takes back its own insertion; one already marked deleted stays.
    TrackedJoin {
        /// Left paragraph.
        at: Address,
        /// Author recorded on the change (`w:author`).
        author: String,
        /// Date recorded on the change (`w:date`, ISO 8601), if any.
        date: Option<String>,
    },
    /// Reject the paragraph's tracked changes: deleted and moved-out runs
    /// stay, inserted and moved-in runs go, and an inserted paragraph mark
    /// joins the paragraph with the next one.
    RejectRevisions {
        /// Target paragraph.
        at: Address,
    },
    /// Accept only the paragraph's tracked change with `w:id` `id` - its
    /// runs and, when it is the mark's change, the mark - as
    /// [`AcceptRevisions`](Self::AcceptRevisions) does for all of them.
    AcceptRevision {
        /// Target paragraph.
        at: Address,
        /// The change's `w:id`.
        id: u32,
    },
    /// Reject only the paragraph's tracked change with `w:id` `id`.
    RejectRevision {
        /// Target paragraph.
        at: Address,
        /// The change's `w:id`.
        id: u32,
    },
}
impl Edit {
    /// The block the command targets.
    pub fn address(&self) -> &Address {
        match self {
            Self::Split { at, .. }
            | Self::Join { at }
            | Self::SplitAt { at, .. }
            | Self::FormatAt { at, .. }
            | Self::ReplaceAt { at, .. }
            | Self::Insert { at, .. }
            | Self::Delete { at }
            | Self::Move { at, .. }
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
            | Self::MergeCells { at, .. }
            | Self::InsertNote { at, .. }
            | Self::SectionProperties { at, .. }
            | Self::InsertSectionBreak { at }
            | Self::RemoveSectionBreak { at, .. }
            | Self::DeleteNote { at }
            | Self::SplitCell { at, .. }
            | Self::Drawing { at, .. }
            | Self::Identify { at }
            | Self::TrackedText { at, .. }
            | Self::TrackedSplit { at, .. }
            | Self::TrackedJoin { at, .. }
            | Self::AcceptRevisions { at }
            | Self::RejectRevisions { at }
            | Self::AcceptRevision { at, .. }
            | Self::RejectRevision { at, .. } => at,
        }
    }
}
