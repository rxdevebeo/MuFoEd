//! Block-level model: paragraphs, tables and structured document tags.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;

use super::ids::{ParaId, TextId};
use super::inline::Inline;
use super::props::{
    CellProperties, ParagraphProperties, RowProperties, RunProperties, TableProperties,
};
use super::revision::Revision;
use super::values::{Rsids, Twips};

/// A paragraph (`w:p`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    /// Paragraph properties.
    pub props: ParagraphProperties,
    /// Inline content.
    pub inlines: Vec<Inline>,
    /// Revision identifiers carried by the paragraph and its runs.
    pub rsids: Rsids,
    /// Tracked change of the paragraph mark (`w:pPr/w:rPr/w:ins|w:del`, ADR-0018).
    pub revision: Option<Revision>,
    /// Unique paragraph id (`w14:paraId`).
    pub para_id: Option<ParaId>,
    /// Unique text id (`w14:textId`).
    pub text_id: Option<TextId>,
    /// Source location of the paragraph.
    pub location: SourceLocation,
}

/// A table grid column (`w:gridCol`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GridCol {
    /// Column width in twips.
    pub width: Option<Twips>,
}

/// The grid a table had before a tracked change (`w:tblGridChange`).
///
/// The live [`Table::grid`] is what layout paints. This copy is the previous
/// `w:tblGrid` Word stored inside the change markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableGridChange {
    /// Revision id (`w:id`).
    pub id: u32,
    /// Column widths before the change.
    pub grid: Vec<GridCol>,
}

/// A table (`w:tbl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// Table properties.
    pub props: TableProperties,
    /// Table grid.
    pub grid: Vec<GridCol>,
    /// Previous grid from `w:tblGrid/w:tblGridChange`, when the source had one.
    pub grid_change: Option<TableGridChange>,
    /// Table rows.
    pub rows: Vec<TableRow>,
    /// Source location.
    pub location: SourceLocation,
}

/// A table row (`w:tr`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    /// Row properties.
    pub props: RowProperties,
    /// Cells.
    pub cells: Vec<TableCell>,
    /// Properties of a row-level `w:sdt` that wrapped this row (AUD-41).
    ///
    /// The parser unwraps `sdtContent/w:tr` into ordinary rows and keeps the
    /// control's `sdtPr` here so the writer can restore the wrapper.
    pub sdt: Option<SdtProperties>,
    /// Source location.
    pub location: SourceLocation,
}

/// A table cell (`w:tc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell {
    /// Cell properties.
    pub props: CellProperties,
    /// Block content (paragraphs, nested tables).
    pub blocks: Vec<Block>,
    /// Properties of a cell-level `w:sdt` that wrapped this cell (AUD-41).
    pub sdt: Option<SdtProperties>,
    /// Source location.
    pub location: SourceLocation,
}

/// Identity fields of a structured document tag (`w:sdtPr`).
///
/// Shared by block/inline [`SdtContainer`] and by row/cell-level unwrapping
/// (AUD-41), where the control is not itself a block in the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtProperties {
    /// Tag value (`w:tag`).
    pub tag: Option<Arc<str>>,
    /// Friendly alias (`w:alias`).
    pub alias: Option<Arc<str>>,
    /// Numeric id (`w:id`).
    pub id: Option<Arc<str>>,
    /// Placeholder text (`w:placeholder`).
    pub placeholder: Option<Arc<str>>,
    /// Whether the control shows an empty placeholder.
    pub showing_placeholder: bool,
    /// Placeholder run properties (`w:sdtPr/w:rPr`), including `w:sz`.
    pub run_props: Option<RunProperties>,
    /// Run properties of the control's end marker (`w:sdtEndPr/w:rPr`).
    pub end_run_props: Option<RunProperties>,
    /// Source location of the `w:sdt` element.
    pub location: SourceLocation,
}

/// A structured document tag (`w:sdt`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdtContainer {
    /// Tag value (`w:tag`).
    pub tag: Option<Arc<str>>,
    /// Friendly alias (`w:alias`).
    pub alias: Option<Arc<str>>,
    /// Numeric id (`w:id`).
    pub id: Option<Arc<str>>,
    /// Placeholder text (`w:placeholder`).
    pub placeholder: Option<Arc<str>>,
    /// Whether the control shows an empty placeholder.
    pub showing_placeholder: bool,
    /// Placeholder run properties (`w:sdtPr/w:rPr`), including `w:sz`.
    pub run_props: Option<RunProperties>,
    /// Run properties of the control's end marker (`w:sdtEndPr/w:rPr`).
    pub end_run_props: Option<RunProperties>,
    /// Block content when the tag is block-level.
    pub blocks: Vec<Block>,
    /// Inline content when the tag is inline-level.
    pub inlines: Vec<Inline>,
    /// Source location.
    pub location: SourceLocation,
}

impl SdtContainer {
    /// Returns the identity fields of this control as [`SdtProperties`].
    #[must_use]
    pub fn properties(&self) -> SdtProperties {
        SdtProperties {
            tag: self.tag.clone(),
            alias: self.alias.clone(),
            id: self.id.clone(),
            placeholder: self.placeholder.clone(),
            showing_placeholder: self.showing_placeholder,
            run_props: self.run_props.clone(),
            end_run_props: self.end_run_props.clone(),
            location: self.location.clone(),
        }
    }
}

/// An unknown block-level element preserved for the support report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueBlock {
    /// Namespace URI (empty when none).
    pub namespace: Arc<str>,
    /// Local name.
    pub local: Arc<str>,
    /// Attributes of the element.
    pub attributes: Vec<(Arc<str>, Arc<str>)>,
    /// Source location.
    pub location: SourceLocation,
}

impl OpaqueBlock {
    /// Returns a stable feature identifier for the element.
    #[must_use]
    pub fn feature_id(&self) -> String {
        if self.namespace.is_empty() {
            self.local.to_string()
        } else {
            format!("w:{}", self.local)
        }
    }
}

/// An alternative-format chunk (`w:altChunk`), never embedded (TZ decision Г.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltChunkInfo {
    /// Relationship id of the embedded chunk (`r:id`).
    pub rel_id: Option<RelId>,
    /// Resolved content type of the chunk part, if known.
    pub content_type: Option<Arc<str>>,
    /// Source location.
    pub location: SourceLocation,
}

/// Block-level content of the document body or a table cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Paragraph.
    Paragraph(Paragraph),
    /// Table.
    Table(Table),
    /// Block-level structured document tag.
    SdtBlock(SdtContainer),
    /// Alternative-format chunk.
    AltChunk(AltChunkInfo),
    /// Unknown block-level element.
    Opaque(OpaqueBlock),
}

impl Block {
    /// Returns the paragraph, if this block is one.
    #[must_use]
    pub fn as_paragraph(&self) -> Option<&Paragraph> {
        match self {
            Self::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        }
    }

    /// Returns the table, if this block is one.
    #[must_use]
    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }
}
