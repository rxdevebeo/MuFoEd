//! Block-container nesting: the one place that counts it.
//!
//! Three subsystems need the same answer to "how deeply does this model nest
//! containers" - the reader, which must refuse a document that nests past the
//! budget, and the renderer and the writer, which must refuse a *model* that
//! nests past it. A model built in code never met the reader, so for those two
//! it is the only check there is.
//!
//! One implementation rather than three: a walk that counted `w:tbl` in one place
//! and `w:tbl` plus `w:sdt` in another would let a document through the first
//! and be refused by the second, which is worse than either answer alone.
//!
//! Two counters, because the containers do not cost the same. A `w:tbl` is one
//! frame of parser state; a text box is ten (a paragraph, a run, a drawing, an
//! inline, a graphic, a graphic-data, a shape, the box and its block children),
//! measured at 125 408 bytes of stack in a debug build. One bound for both would
//! either overflow the stack or refuse tables twelve deep for the sake of a text
//! box the same document would never carry.
//!
//! The bounds are [`ResourceLimits::max_block_nesting`] and
//! [`ResourceLimits::max_text_box_nesting`].

use strict_ooxml_core::limits::ResourceLimits;

use crate::model::block::Block;
use crate::model::drawing::{DrawingKind, Graphic};
use crate::model::Document;

/// Which counter ran out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestingKind {
    /// `w:tbl`, a block-level `w:sdt`, a `w:customXml` wrapper, a note body, a
    /// `w:hdr`/`w:ftr`.
    Block,
    /// A text box (`wps:txbx` / `w:txbxContent`).
    TextBox,
}

impl NestingKind {
    /// A stable, machine-readable name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block nesting",
            Self::TextBox => "text box nesting",
        }
    }
}

/// The nesting a check refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exceeded {
    /// Which budget was spent.
    pub kind: NestingKind,
    /// The budget that was spent.
    pub limit: u32,
    /// The nesting that was reached.
    pub actual: u32,
}

impl Exceeded {
    /// The `LimitKind` this maps onto in [`StrictError`](strict_ooxml_core::error::StrictError).
    #[must_use]
    pub fn limit_kind(self) -> strict_ooxml_core::error::LimitKind {
        match self.kind {
            NestingKind::Block => strict_ooxml_core::error::LimitKind::BlockNesting,
            NestingKind::TextBox => strict_ooxml_core::error::LimitKind::TextBoxNesting,
        }
    }
}

impl std::fmt::Display for Exceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} deeper than the budget of {}",
            self.kind.as_str(),
            self.actual,
            self.limit
        )
    }
}

impl std::error::Error for Exceeded {}

/// The two counters, taken apart from a [`ResourceLimits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// `w:tbl` and the other block containers.
    pub block: u32,
    /// Text boxes.
    pub text_box: u32,
}

impl Budget {
    /// The budget named by `limits`.
    #[must_use]
    pub fn of(limits: &ResourceLimits) -> Self {
        Self {
            block: limits.max_block_nesting,
            text_box: limits.max_text_box_nesting,
        }
    }
}

/// Checks `blocks` against `limits`.
///
/// # Errors
///
/// Returns the [`Exceeded`] nesting of the first container past its budget.
pub fn check_blocks(blocks: &[Block], limits: &ResourceLimits) -> Result<(), Exceeded> {
    let budget = Budget::of(limits);
    blocks_at(blocks, 0, 0, budget)
}

/// Checks every container of `document`.
///
/// The body, the headers and footers, the footnotes and the endnotes are each
/// checked from zero: they are separate parts of the package, each parsed by its
/// own parser, and a table in a footer is not "deeper" than the same table in the
/// body.
///
/// # Errors
///
/// Returns the [`Exceeded`] nesting of the first part that is over budget.
pub fn check_document(document: &Document, limits: &ResourceLimits) -> Result<(), Exceeded> {
    check_blocks(&document.body.blocks, limits)?;
    for header_footer in &document.headers_footers {
        check_blocks(&header_footer.blocks, limits)?;
    }
    for note in document.footnotes.iter().chain(document.endnotes.iter()) {
        check_blocks(&note.blocks, limits)?;
    }
    Ok(())
}

/// Walks `blocks`; `blocks` and `boxes` are how many of each are already open.
fn blocks_at(
    blocks: &[Block],
    blocks_open: u32,
    boxes_open: u32,
    budget: Budget,
) -> Result<(), Exceeded> {
    for block in blocks {
        match block {
            Block::Table(table) => {
                let inner = enter(NestingKind::Block, blocks_open, budget.block)?;
                for row in &table.rows {
                    for cell in &row.cells {
                        blocks_at(&cell.blocks, inner, boxes_open, budget)?;
                    }
                }
            }
            Block::SdtBlock(sdt) => {
                let inner = enter(NestingKind::Block, blocks_open, budget.block)?;
                blocks_at(&sdt.blocks, inner, boxes_open, budget)?;
            }
            Block::Paragraph(paragraph) => {
                for inline in &paragraph.inlines {
                    if let crate::model::inline::Inline::Drawing(drawing) = inline {
                        match &drawing.kind {
                            DrawingKind::Inline(inline) => {
                                walk_graphic(
                                    inline.graphic.as_ref(),
                                    blocks_open,
                                    boxes_open,
                                    budget,
                                )?;
                            }
                            DrawingKind::Anchor(anchor) => {
                                walk_graphic(
                                    anchor.graphic.as_ref(),
                                    blocks_open,
                                    boxes_open,
                                    budget,
                                )?;
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

/// Walks a graphic for text boxes.
fn walk_graphic(
    graphic: &Graphic,
    blocks_open: u32,
    boxes_open: u32,
    budget: Budget,
) -> Result<(), Exceeded> {
    match graphic {
        Graphic::Shape(shape) => {
            if let Some(text_box) = &shape.text {
                let inner = enter(NestingKind::TextBox, boxes_open, budget.text_box)?;
                blocks_at(&text_box.blocks, blocks_open, inner, budget)?;
            }
        }
        Graphic::Group(group) => {
            for child in &group.children {
                walk_graphic(child, blocks_open, boxes_open, budget)?;
            }
        }
        Graphic::None
        | Graphic::Picture(_)
        | Graphic::Chart(_)
        | Graphic::Diagram(_)
        | Graphic::LockedCanvas(_)
        | Graphic::Other => {}
    }
    Ok(())
}

/// Enters one container, failing if its budget is spent.
fn enter(kind: NestingKind, open: u32, limit: u32) -> Result<u32, Exceeded> {
    let open = open.saturating_add(1);
    if open > limit {
        return Err(Exceeded {
            kind,
            limit,
            actual: open,
        });
    }
    Ok(open)
}

#[cfg(test)]
mod tests {
    use crate::model::block::{Block, Paragraph, Table, TableCell, TableRow};
    use crate::model::drawing::{
        Drawing, DrawingKind, Graphic, InlineDrawing, MediaIndex, Shape, ShapeGeometry, TextBox,
    };
    use crate::model::props::{
        CellProperties, ParagraphProperties, RowProperties, TableProperties,
    };
    use crate::model::support::SupportModel;
    use crate::model::values::Rsids;
    use crate::model::{Body, Document, DocumentSource};
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::part::PartId;

    use super::{check_document, Exceeded, NestingKind};

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
            revision: None,
            location: location(),
        })
    }

    fn table(blocks: Vec<Block>) -> Block {
        Block::Table(Table {
            props: TableProperties::default(),
            grid: Vec::new(),
            rows: vec![TableRow {
                props: RowProperties::default(),
                cells: vec![TableCell {
                    props: CellProperties::default(),
                    blocks,
                    sdt: None,
                    location: location(),
                }],
                sdt: None,
                location: location(),
            }],
            location: location(),
        })
    }

    /// A document whose body is `depth` nested tables.
    fn tables(depth: usize) -> Document {
        let mut blocks = vec![paragraph()];
        for _ in 0..depth {
            blocks = vec![table(blocks)];
        }
        shell(blocks)
    }

    /// A document whose body is `depth` nested tables, wrapped in a drawing.
    fn drawing(blocks: Vec<Block>) -> Block {
        Block::Paragraph(Paragraph {
            props: ParagraphProperties::default(),
            inlines: vec![crate::model::inline::Inline::Drawing(Drawing {
                kind: DrawingKind::Inline(InlineDrawing {
                    extent: None,
                    effect_extent: None,
                    doc_pr: None,
                    dist_top: None,
                    dist_bottom: None,
                    dist_left: None,
                    dist_right: None,
                    graphic_uri: None,
                    graphic: Box::new(Graphic::Shape(Shape {
                        name: None,
                        descr: None,
                        tx_box: None,
                        geometry: ShapeGeometry::None,
                        xfrm: None,
                        offset: None,
                        extent: None,
                        fill: None,
                        stroke: None,
                        text: Some(TextBox {
                            body: None,
                            blocks,
                            location: location(),
                        }),
                        style: None,
                        location: location(),
                    })),
                    location: location(),
                }),
                location: location(),
            })],
            rsids: Rsids::default(),
            para_id: None,
            text_id: None,
            revision: None,
            location: location(),
        })
    }

    fn shell(blocks: Vec<Block>) -> Document {
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
            media: MediaIndex::new(),
            support: SupportModel::new(),
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
    fn the_block_budget_is_the_boundary_and_the_message_names_it() {
        let limits = ResourceLimits::default();
        assert!(check_document(&tables(12), &limits).is_ok());
        assert_eq!(
            check_document(&tables(13), &limits),
            Err(Exceeded {
                kind: NestingKind::Block,
                limit: 12,
                actual: 13
            })
        );
        assert!(check_document(&tables(3), &limits).is_ok());
    }

    #[test]
    fn a_text_box_has_its_own_budget_and_does_not_spend_the_block_one() {
        let limits = ResourceLimits::default();
        // Five text boxes: inside the text-box budget of five, and the twelve-level
        // block budget is barely touched, because a text box is not a table.
        let mut blocks = vec![paragraph()];
        for _ in 0..5 {
            blocks = vec![drawing(blocks)];
        }
        assert!(check_document(&shell(blocks), &limits).is_ok());

        let mut blocks = vec![paragraph()];
        for _ in 0..6 {
            blocks = vec![drawing(blocks)];
        }
        assert_eq!(
            check_document(&shell(blocks), &limits),
            Err(Exceeded {
                kind: NestingKind::TextBox,
                limit: 5,
                actual: 6
            })
        );
    }

    #[test]
    fn the_two_budgets_are_independent() {
        // Eleven tables inside five text boxes: inside both budgets, and neither
        // counter has to make room for the other. A single shared number would
        // have to be five (and refuse the tables the plan requires) or twelve
        // (and let the stack overflow on the boxes).
        let limits = ResourceLimits::default();
        let mut blocks = vec![paragraph()];
        for _ in 0..11 {
            blocks = vec![table(blocks)];
        }
        for _ in 0..5 {
            blocks = vec![drawing(blocks)];
        }
        assert_eq!(check_document(&shell(blocks), &limits), Ok(()));

        // One more text box: still eleven tables, one box over.
        let mut blocks = vec![paragraph()];
        for _ in 0..11 {
            blocks = vec![table(blocks)];
        }
        for _ in 0..6 {
            blocks = vec![drawing(blocks)];
        }
        assert_eq!(
            check_document(&shell(blocks), &limits),
            Err(Exceeded {
                kind: NestingKind::TextBox,
                limit: 5,
                actual: 6
            })
        );
    }

    #[test]
    fn a_deep_table_in_a_header_is_a_header_problem_not_a_body_one() {
        let limits = ResourceLimits::default();
        let mut deep = tables(3);
        deep.headers_footers = vec![crate::model::HeaderFooter {
            part: PartId::new("/word/header1.xml"),
            is_header: true,
            blocks: match tables(20).body.blocks.into_iter().next() {
                Some(Block::Table(table)) => vec![Block::Table(table)],
                _ => Vec::new(),
            },
            location: location(),
        }];
        // The body is three deep and within budget; the header is not.
        let error = check_document(&deep, &limits).expect_err("the header is over budget");
        assert_eq!(error.actual, 13, "{error}");
    }

    use strict_ooxml_core::limits::ResourceLimits;
}
