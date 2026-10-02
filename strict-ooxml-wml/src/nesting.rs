//! Block-container nesting: the one place that counts it.
//!
//! Three subsystems need the same answer to "how deeply does this model nest
//! block containers" - the reader, which must refuse a document that nests past
//! the budget, and the renderer and the writer, which must refuse a *model*
//! that nests past it. A model built in code never met the reader, so for those
//! two it is the only check there is.
//!
//! One implementation rather than three: a walk that counted `w:tbl` in one place
//! and `w:tbl` plus `w:sdt` in another would let a document through the first
//! and be refused by the second, which is worse than either answer alone.
//!
//! A *block container* is an element whose children are themselves blocks:
//! `w:tbl`, a block-level `w:sdt`, a `w:customXml` wrapper, a text box's
//! `w:txbxContent`, a `w:footnote`/`w:endnote` body and a `w:hdr`/`w:ftr`. The
//! budget is [`ResourceLimits::max_block_nesting`](strict_ooxml_core::limits::ResourceLimits::max_block_nesting).

use strict_ooxml_core::limits::ResourceLimits;

use crate::model::block::Block;
use crate::model::drawing::{DrawingKind, Graphic};
use crate::model::Document;

/// The nesting that [`check_blocks`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exceeded {
    /// The budget that was spent.
    pub limit: u32,
    /// The nesting that was reached.
    pub actual: u32,
}

impl std::fmt::Display for Exceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "block nesting {} deeper than the budget of {}",
            self.actual, self.limit
        )
    }
}

impl std::error::Error for Exceeded {}

/// Checks `blocks` against `limit`.
///
/// # Errors
///
/// Returns the [`Exceeded`] nesting when `blocks` nests a block container
/// deeper than `limit`.
pub fn check_blocks(blocks: &[Block], limit: u32) -> Result<(), Exceeded> {
    blocks_at(blocks, 0, limit)
}

/// Checks every block container of `document` against
/// [`ResourceLimits::max_block_nesting`].
///
/// The body, the headers and footers, the footnotes and the endnotes are each
/// checked from zero: they are separate parts of the package, each parsed by its
/// own parser, and a table in a footer is not "deeper" than the same table in the
/// body.
///
/// # Errors
///
/// Returns the [`Exceeded`] nesting of the first part that is over budget.
pub fn check_document(document: &Document, limit: u32) -> Result<(), Exceeded> {
    check_blocks(&document.body.blocks, limit)?;
    for header_footer in &document.headers_footers {
        check_blocks(&header_footer.blocks, limit)?;
    }
    for note in document.footnotes.iter().chain(document.endnotes.iter()) {
        check_blocks(&note.blocks, limit)?;
    }
    Ok(())
}

/// Checks `document` against its own configured limit.
pub fn check_document_with(document: &Document, limits: &ResourceLimits) -> Result<(), Exceeded> {
    check_document(document, limits.max_block_nesting)
}

/// Walks `blocks`, counting containers; `depth` is how many are already open.
fn blocks_at(blocks: &[Block], depth: u32, limit: u32) -> Result<(), Exceeded> {
    for block in blocks {
        match block {
            Block::Table(table) => {
                let inner = enter(depth, limit)?;
                for row in &table.rows {
                    for cell in &row.cells {
                        blocks_at(&cell.blocks, inner, limit)?;
                    }
                }
            }
            Block::SdtBlock(sdt) => {
                let inner = enter(depth, limit)?;
                blocks_at(&sdt.blocks, inner, limit)?;
            }
            Block::Paragraph(paragraph) => {
                for inline in &paragraph.inlines {
                    if let crate::model::inline::Inline::Drawing(drawing) = inline {
                        match &drawing.kind {
                            DrawingKind::Inline(inline) => {
                                walk_graphic(inline.graphic.as_ref(), depth, limit)?;
                            }
                            DrawingKind::Anchor(anchor) => {
                                walk_graphic(anchor.graphic.as_ref(), depth, limit)?;
                            }
                            DrawingKind::Opaque(_) => {}
                        }
                    }
                }
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
    Ok(())
}

/// Walks a graphic for text boxes, each of which is a block container.
fn walk_graphic(graphic: &Graphic, depth: u32, limit: u32) -> Result<(), Exceeded> {
    match graphic {
        Graphic::Shape(shape) => {
            if let Some(text_box) = &shape.text {
                let inner = enter(depth, limit)?;
                blocks_at(&text_box.blocks, inner, limit)?;
            }
        }
        Graphic::Group(group) => {
            for child in &group.children {
                walk_graphic(child, depth, limit)?;
            }
        }
        Graphic::None
        | Graphic::Picture(_)
        | Graphic::Chart(_)
        | Graphic::Diagram(_)
        | Graphic::Other => {}
    }
    Ok(())
}

/// Enters one block container, failing if the budget is spent.
fn enter(depth: u32, limit: u32) -> Result<u32, Exceeded> {
    let depth = depth.saturating_add(1);
    if depth > limit {
        return Err(Exceeded {
            limit,
            actual: depth,
        });
    }
    Ok(depth)
}

#[cfg(test)]
mod tests {
    use crate::model::block::{Paragraph, Table, TableCell, TableRow};
    use crate::model::props::{
        CellProperties, ParagraphProperties, RowProperties, TableProperties,
    };
    use crate::model::values::Rsids;
    use crate::model::{Body, DocumentSource};
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::part::PartId;

    use super::{check_document, Exceeded};
    use crate::model::block::Block;
    use crate::model::Document;

    fn location() -> SourceLocation {
        SourceLocation::new(PartId::new("/word/document.xml"), 1, 1, 0)
    }

    fn paragraph() -> Block {
        Block::Paragraph(Paragraph {
            props: ParagraphProperties::default(),
            inlines: Vec::new(),
            rsids: Rsids::default(),
            para_id: None,
            text_id: None,
            location: location(),
        })
    }

    /// A document whose body is `depth` nested tables.
    fn document(depth: usize) -> Document {
        let mut blocks = vec![paragraph()];
        for _ in 0..depth {
            blocks = vec![Block::Table(Table {
                props: TableProperties::default(),
                grid: Vec::new(),
                rows: vec![TableRow {
                    props: RowProperties::default(),
                    cells: vec![TableCell {
                        props: CellProperties::default(),
                        blocks,
                        location: location(),
                    }],
                    location: location(),
                }],
                location: location(),
            })];
        }
        Document {
            body: Body { blocks },
            styles: crate::model::StyleTable::default(),
            numbering: crate::model::NumberingTable::default(),
            footnotes: crate::model::NoteTable::default(),
            endnotes: crate::model::NoteTable::default(),
            settings: crate::model::Settings::default(),
            theme: None,
            font_table: None,
            sections: Vec::new(),
            headers_footers: Vec::new(),
            media: crate::model::drawing::MediaIndex::new(),
            support: crate::model::support::SupportModel::new(),
            source: DocumentSource {
                main_document: PartId::new("/word/document.xml"),
                styles: None,
                numbering: None,
                settings: None,
                font_table: None,
                footnotes: None,
                endnotes: None,
                theme: None,
            },
        }
    }

    #[test]
    fn the_budget_is_the_boundary_and_the_message_names_it() {
        assert!(check_document(&document(12), 12).is_ok());
        assert_eq!(
            check_document(&document(13), 12),
            Err(Exceeded {
                limit: 12,
                actual: 13
            })
        );
        assert!(check_document(&document(3), 12).is_ok());
    }

    #[test]
    fn a_deep_table_in_a_header_is_a_header_problem_not_a_body_one() {
        let mut deep = document(3);
        deep.headers_footers = vec![crate::model::HeaderFooter {
            part: PartId::new("/word/header1.xml"),
            is_header: true,
            blocks: match document(20).body.blocks.into_iter().next() {
                Some(Block::Table(table)) => vec![Block::Table(table)],
                _ => Vec::new(),
            },
            location: location(),
        }];
        // The body is three deep and within budget; the header is not.
        let error = check_document(&deep, 12).expect_err("the header is over budget");
        assert_eq!(error.actual, 13, "{error}");
    }
}
