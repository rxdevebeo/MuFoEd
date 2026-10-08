//! Stories, container paths and block addresses, and the walks that resolve them.
use crate::{EditError, Unsupported};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, Graphic, Inline, Paragraph, RunContent,
};

/// A separately serialized document story.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Story {
    /// Main body.
    Body,
    /// Header or footer identified by its source part.
    HeaderFooter(PartId),
    /// Footnote definition id.
    Footnote(i32),
    /// Endnote definition id.
    Endnote(i32),
}
/// A descent into a block container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Container {
    /// Table block, row and cell indices.
    Cell {
        /// Table block index.
        table: usize,
        /// Row index.
        row: usize,
        /// Cell index.
        cell: usize,
    },
    /// Block SDT index.
    Sdt(usize),
    /// Shape text box, optionally inside nested graphic groups.
    TextBox {
        /// Paragraph block containing the drawing.
        paragraph: usize,
        /// Inline wrapper indices ending at a run or drawing.
        inline: Vec<usize>,
        /// Content index for a drawing inside a run.
        content: Option<usize>,
        /// Graphic group child indices.
        graphics: Vec<usize>,
    },
}
/// Revision-scoped block address, independent of physical pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    /// Document story.
    pub story: Story,
    /// Containers from outermost to innermost.
    pub containers: Vec<Container>,
    /// Index in the final block sequence.
    pub block: usize,
}
impl Address {
    /// A direct body block.
    pub fn body(block: usize) -> Self {
        Self {
            story: Story::Body,
            containers: vec![],
            block,
        }
    }
}
pub(super) fn story_blocks<'a>(
    document: &'a Document,
    story: &Story,
) -> Result<&'a [Block], EditError> {
    Ok(match story {
        Story::Body => &document.body.blocks,
        Story::HeaderFooter(part) => {
            &document
                .header_footer(part)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Story::Footnote(id) => {
            &document
                .footnotes
                .get(*id)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Story::Endnote(id) => {
            &document
                .endnotes
                .get(*id)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
    })
}
pub(super) fn mutate_story<F>(
    document: &mut Document,
    story: &Story,
    edit: F,
) -> Result<(), EditError>
where
    F: FnOnce(&mut Vec<Block>) -> Result<(), EditError>,
{
    match story {
        Story::Body => edit(&mut document.body.blocks),
        Story::HeaderFooter(part) => edit(
            &mut document
                .headers_footers
                .iter_mut()
                .find(|h| &h.part == part)
                .ok_or(EditError::InvalidParagraph)?
                .blocks,
        ),
        Story::Footnote(id) | Story::Endnote(id) => {
            let table = if matches!(story, Story::Footnote(_)) {
                &mut document.footnotes
            } else {
                &mut document.endnotes
            };
            let mut note = table.get(*id).ok_or(EditError::InvalidParagraph)?.clone();
            edit(&mut note.blocks)?;
            table.insert(note);
            Ok(())
        }
    }
}
pub(super) fn descend_mut<'a>(
    blocks: &'a mut Vec<Block>,
    containers: &[Container],
) -> Result<&'a mut Vec<Block>, EditError> {
    let Some((first, rest)) = containers.split_first() else {
        return Ok(blocks);
    };
    let child = match first {
        Container::Cell { table, row, cell } => {
            let Some(Block::Table(t)) = blocks.get_mut(*table) else {
                return Err(EditError::InvalidParagraph);
            };
            &mut t
                .rows
                .get_mut(*row)
                .and_then(|r| r.cells.get_mut(*cell))
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Container::Sdt(index) => {
            let Some(Block::SdtBlock(s)) = blocks.get_mut(*index) else {
                return Err(EditError::InvalidParagraph);
            };
            &mut s.blocks
        }
        Container::TextBox {
            paragraph,
            inline,
            content,
            graphics,
        } => {
            let p = paragraph_mut(blocks, *paragraph)?;
            let drawing = drawing_mut(&mut p.inlines, inline, *content)?;
            let graphic = match &mut drawing.kind {
                DrawingKind::Inline(v) => v.graphic.as_mut(),
                DrawingKind::Anchor(v) => v.graphic.as_mut(),
                DrawingKind::Opaque(_) => {
                    return Err(EditError::UnsupportedContent(Unsupported::OpaqueDrawing));
                }
            };
            &mut shape_mut(graphic, graphics)?
                .text
                .as_mut()
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
    };
    descend_mut(child, rest)
}
fn shape_mut<'a>(
    graphic: &'a mut Graphic,
    path: &[usize],
) -> Result<&'a mut strict_ooxml_wml::model::Shape, EditError> {
    if let Some((index, rest)) = path.split_first() {
        let Graphic::Group(g) = graphic else {
            return Err(EditError::InvalidParagraph);
        };
        shape_mut(
            g.children
                .get_mut(*index)
                .ok_or(EditError::InvalidParagraph)?,
            rest,
        )
    } else {
        match graphic {
            Graphic::Shape(s) => Ok(s),
            _ => Err(EditError::InvalidParagraph),
        }
    }
}
pub(super) fn paragraph_mut(
    blocks: &mut [Block],
    index: usize,
) -> Result<&mut Paragraph, EditError> {
    match blocks.get_mut(index) {
        Some(Block::Paragraph(p)) => Ok(p),
        _ => Err(EditError::InvalidParagraph),
    }
}
fn inline_mut<'a>(inlines: &'a mut [Inline], path: &[usize]) -> Result<&'a mut Inline, EditError> {
    let (index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    let value = inlines.get_mut(*index).ok_or(EditError::InvalidParagraph)?;
    if rest.is_empty() {
        return Ok(value);
    }
    let child = match value {
        Inline::Hyperlink(v) => &mut v.inlines,
        Inline::SdtInline(v) => &mut v.inlines,
        Inline::Directional(v) => &mut v.inlines,
        _ => return Err(EditError::UnsupportedContent(Unsupported::Target)),
    };
    inline_mut(child, rest)
}
pub(super) fn run_mut<'a>(
    inlines: &'a mut [Inline],
    path: &[usize],
) -> Result<&'a mut strict_ooxml_wml::model::Run, EditError> {
    match inline_mut(inlines, path)? {
        Inline::Run(r) if r.revision.is_none() => Ok(r),
        Inline::Run(_) => Err(EditError::UnsupportedContent(Unsupported::TrackedChange)),
        _ => Err(EditError::UnsupportedContent(Unsupported::Target)),
    }
}
pub(super) fn drawing_mut<'a>(
    inlines: &'a mut [Inline],
    path: &[usize],
    content: Option<usize>,
) -> Result<&'a mut Drawing, EditError> {
    match (inline_mut(inlines, path)?, content) {
        (Inline::Drawing(d), None) => Ok(d),
        (Inline::Run(r), Some(i)) if r.revision.is_none() => match r.content.get_mut(i) {
            Some(RunContent::Drawing(d)) => Ok(d),
            _ => Err(EditError::InvalidParagraph),
        },
        _ => Err(EditError::InvalidParagraph),
    }
}
pub(crate) fn read_paragraph<'a>(
    document: &'a Document,
    address: &Address,
) -> Result<&'a Paragraph, EditError> {
    // A read-only traversal mirrors the mutating traversal, including text boxes.
    let mut blocks = story_blocks(document, &address.story)?;
    for container in &address.containers {
        blocks = match container {
            Container::Cell { table, row, cell } => match blocks.get(*table) {
                Some(Block::Table(t)) => {
                    &t.rows
                        .get(*row)
                        .and_then(|r| r.cells.get(*cell))
                        .ok_or(EditError::InvalidParagraph)?
                        .blocks
                }
                _ => return Err(EditError::InvalidParagraph),
            },
            Container::Sdt(i) => match blocks.get(*i) {
                Some(Block::SdtBlock(s)) => &s.blocks,
                _ => return Err(EditError::InvalidParagraph),
            },
            Container::TextBox {
                paragraph,
                inline,
                content,
                graphics,
            } => {
                let Some(Block::Paragraph(p)) = blocks.get(*paragraph) else {
                    return Err(EditError::InvalidParagraph);
                };
                let drawing = drawing_ref(&p.inlines, inline, *content)?;
                let graphic = match &drawing.kind {
                    DrawingKind::Inline(v) => v.graphic.as_ref(),
                    DrawingKind::Anchor(v) => v.graphic.as_ref(),
                    DrawingKind::Opaque(_) => {
                        return Err(EditError::UnsupportedContent(Unsupported::OpaqueDrawing));
                    }
                };
                &shape_ref(graphic, graphics)?
                    .text
                    .as_ref()
                    .ok_or(EditError::InvalidParagraph)?
                    .blocks
            }
        };
    }
    match blocks.get(address.block) {
        Some(Block::Paragraph(p)) => Ok(p),
        _ => Err(EditError::InvalidParagraph),
    }
}
fn inline_ref<'a>(items: &'a [Inline], path: &[usize]) -> Result<&'a Inline, EditError> {
    let (index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    let item = items.get(*index).ok_or(EditError::InvalidParagraph)?;
    if rest.is_empty() {
        return Ok(item);
    }
    let children = match item {
        Inline::Hyperlink(v) => &v.inlines,
        Inline::SdtInline(v) => &v.inlines,
        Inline::Directional(v) => &v.inlines,
        _ => return Err(EditError::UnsupportedContent(Unsupported::Target)),
    };
    inline_ref(children, rest)
}
fn drawing_ref<'a>(
    items: &'a [Inline],
    path: &[usize],
    content: Option<usize>,
) -> Result<&'a Drawing, EditError> {
    match (inline_ref(items, path)?, content) {
        (Inline::Drawing(v), None) => Ok(v),
        (Inline::Run(r), Some(index)) => match r.content.get(index) {
            Some(RunContent::Drawing(v)) => Ok(v),
            _ => Err(EditError::InvalidParagraph),
        },
        _ => Err(EditError::InvalidParagraph),
    }
}
fn shape_ref<'a>(
    graphic: &'a Graphic,
    path: &[usize],
) -> Result<&'a strict_ooxml_wml::model::Shape, EditError> {
    if let Some((index, rest)) = path.split_first() {
        let Graphic::Group(g) = graphic else {
            return Err(EditError::InvalidParagraph);
        };
        shape_ref(
            g.children.get(*index).ok_or(EditError::InvalidParagraph)?,
            rest,
        )
    } else {
        match graphic {
            Graphic::Shape(s) => Ok(s),
            _ => Err(EditError::InvalidParagraph),
        }
    }
}
/// Lists ordinary, table and SDT paragraphs in a story in document order.
pub(crate) fn addresses(document: &Document, story: &Story) -> Result<Vec<Address>, EditError> {
    fn walk(blocks: &[Block], story: &Story, parents: &[Container], out: &mut Vec<Address>) {
        for (index, block) in blocks.iter().enumerate() {
            match block {
                Block::Paragraph(para) => {
                    out.push(Address {
                        story: story.clone(),
                        containers: parents.to_vec(),
                        block: index,
                    });
                    visit_drawings(&para.inlines, &[], &mut |drawing, inline, content| {
                        let graphic = match &drawing.kind {
                            DrawingKind::Inline(v) => v.graphic.as_ref(),
                            DrawingKind::Anchor(v) => v.graphic.as_ref(),
                            DrawingKind::Opaque(_) => return,
                        };
                        visit_boxes(graphic, &[], &mut |blocks, graphics| {
                            let mut path = parents.to_vec();
                            path.push(Container::TextBox {
                                paragraph: index,
                                inline: inline.to_vec(),
                                content,
                                graphics: graphics.to_vec(),
                            });
                            walk(blocks, story, &path, out);
                        });
                    });
                }
                Block::Table(t) => {
                    for (row, r) in t.rows.iter().enumerate() {
                        for (cell, c) in r.cells.iter().enumerate() {
                            let mut p = parents.to_vec();
                            p.push(Container::Cell {
                                table: index,
                                row,
                                cell,
                            });
                            walk(&c.blocks, story, &p, out);
                        }
                    }
                }
                Block::SdtBlock(s) => {
                    let mut p = parents.to_vec();
                    p.push(Container::Sdt(index));
                    walk(&s.blocks, story, &p, out);
                }
                _ => {}
            }
        }
    }
    let mut out = vec![];
    walk(story_blocks(document, story)?, story, &[], &mut out);
    Ok(out)
}
pub(super) fn visit_drawings(
    items: &[Inline],
    parents: &[usize],
    f: &mut impl FnMut(&Drawing, &[usize], Option<usize>),
) {
    for (index, item) in items.iter().enumerate() {
        let mut path = parents.to_vec();
        path.push(index);
        match item {
            Inline::Drawing(v) => f(v, &path, None),
            Inline::Run(r) => {
                for (content, c) in r.content.iter().enumerate() {
                    if let RunContent::Drawing(v) = c {
                        f(v, &path, Some(content));
                    }
                }
            }
            Inline::Hyperlink(v) => visit_drawings(&v.inlines, &path, f),
            Inline::SdtInline(v) => visit_drawings(&v.inlines, &path, f),
            Inline::Directional(v) => visit_drawings(&v.inlines, &path, f),
            _ => {}
        }
    }
}
pub(super) fn visit_boxes(
    graphic: &Graphic,
    parents: &[usize],
    f: &mut impl FnMut(&[Block], &[usize]),
) {
    match graphic {
        Graphic::Shape(s) => {
            if let Some(t) = &s.text {
                f(&t.blocks, parents);
            }
        }
        Graphic::Group(g) => {
            for (index, child) in g.children.iter().enumerate() {
                let mut path = parents.to_vec();
                path.push(index);
                visit_boxes(child, &path, f);
            }
        }
        _ => {}
    }
}
