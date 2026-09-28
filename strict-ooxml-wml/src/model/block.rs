//! Block-level model: paragraphs, tables and structured document tags.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;

use super::ids::{ParaId, TextId};
use super::inline::Inline;
use super::props::{CellProperties, ParagraphProperties, RowProperties, TableProperties};
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

/// A table (`w:tbl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// Table properties.
    pub props: TableProperties,
    /// Table grid.
    pub grid: Vec<GridCol>,
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
    /// Source location.
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
    /// Block content when the tag is block-level.
    pub blocks: Vec<Block>,
    /// Inline content when the tag is inline-level.
    pub inlines: Vec<Inline>,
    /// Source location.
    pub location: SourceLocation,
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
