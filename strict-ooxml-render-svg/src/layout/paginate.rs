//! Pagination: section geometry, page breaks and overflow
//! (`STAGE-4-TASK.md` §5.6, §7).

use strict_ooxml_core::error::Result;

use crate::error::RenderError;
use crate::layout::paragraph::layout_paragraph;
use crate::layout::table::{layout_table, offset_item};
use crate::layout::{
    geometry_for, Flow, Geometry, Item, Layout, LayoutContext, PlacedPage, TextLine,
};
use strict_ooxml_wml::model::Block;

/// Hard cap on the number of pages (output-size guard).
const MAX_PAGES: usize = 10_000;
/// Hard cap on the number of paint items (output-size guard).
const MAX_ITEMS: usize = 2_000_000;

/// Lays out the whole document into pages.
pub(crate) fn layout_document(ctx: &LayoutContext<'_>) -> Result<Layout> {
    let section = ctx
        .document
        .sections
        .last()
        .map(|section| &section.properties);
    let geometry = geometry_for(section, ctx.options.scale);
    let mut paginator = Paginator::new(geometry);
    layout_blocks(
        ctx,
        &ctx.document.body.blocks,
        geometry.content_width(),
        &mut paginator,
        0,
    )?;
    Ok(paginator.finish())
}

/// Walks blocks, appending their flows to the paginator.
fn layout_blocks(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    width: f64,
    paginator: &mut Paginator,
    depth: usize,
) -> Result<()> {
    if depth > 16 {
        return Ok(());
    }
    let left = paginator.geometry.left;
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let flow = layout_paragraph(ctx, para, left, width);
                if para.props.page_break_before && !paginator.at_page_top() {
                    paginator.page_break()?;
                }
                paginator.add_vspace(flow.space_before);
                if flow.keep_lines {
                    let total: f64 = flow
                        .flows
                        .iter()
                        .map(|item| match item {
                            Flow::Line(line) => line.height,
                            Flow::Image(image) => image.h,
                            Flow::Block { height, .. } => *height,
                            Flow::PageBreak => 0.0,
                        })
                        .sum();
                    if !paginator.at_page_top()
                        && paginator.cursor + total > paginator.content_height()
                    {
                        paginator.page_break()?;
                    }
                }
                for item in flow.flows {
                    paginator.place(item)?;
                }
                paginator.add_vspace(flow.space_after);
            }
            Block::Table(table) => {
                for flow in layout_table(ctx, table, left, width) {
                    paginator.place(flow)?;
                }
            }
            Block::SdtBlock(sdt) => {
                layout_blocks(ctx, &sdt.blocks, width, paginator, depth + 1)?;
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
    Ok(())
}

/// The paginator accumulates items into pages.
struct Paginator {
    geometry: Geometry,
    pages: Vec<PlacedPage>,
    current: Vec<Item>,
    cursor: f64,
    /// Whether the current page has been started (even if empty).
    started: bool,
}

impl Paginator {
    fn new(geometry: Geometry) -> Self {
        Self {
            geometry,
            pages: Vec::new(),
            current: Vec::new(),
            cursor: 0.0,
            started: false,
        }
    }

    fn content_height(&self) -> f64 {
        self.geometry.content_height()
    }

    fn at_page_top(&self) -> bool {
        !self.started || (self.current.is_empty() && self.cursor <= 0.0)
    }

    /// Adds vertical space before the next flow (suppressed at a page top).
    fn add_vspace(&mut self, space: f64) {
        if self.at_page_top() || space <= 0.0 {
            return;
        }
        self.cursor = (self.cursor + space).min(self.content_height());
    }

    fn check_capacity(&self) -> Result<()> {
        if self.pages.len() >= MAX_PAGES {
            return Err(RenderError::LimitExceeded {
                what: "pages",
                limit: MAX_PAGES as u64,
                actual: (self.pages.len() + 1) as u64,
            }
            .into_strict());
        }
        if self.current.len() >= MAX_ITEMS {
            return Err(RenderError::LimitExceeded {
                what: "items",
                limit: MAX_ITEMS as u64,
                actual: self.current.len() as u64,
            }
            .into_strict());
        }
        Ok(())
    }

    /// Starts a new page.
    fn page_break(&mut self) -> Result<()> {
        self.check_capacity()?;
        if self.current.is_empty() && !self.started {
            // A break before any content is ignored (no leading blank page).
            self.started = true;
            return Ok(());
        }
        self.flush();
        self.cursor = 0.0;
        self.started = true;
        self.current = Vec::new();
        Ok(())
    }

    fn flush(&mut self) {
        if self.current.is_empty() && !self.pages.is_empty() {
            return;
        }
        self.pages.push(PlacedPage {
            width_px: self.geometry.width,
            height_px: self.geometry.height,
            items: std::mem::take(&mut self.current),
        });
    }

    /// Places one flow item, breaking the page if it does not fit.
    fn place(&mut self, flow: Flow) -> Result<()> {
        match flow {
            Flow::PageBreak => self.page_break(),
            Flow::Line(line) => self.place_line(line),
            Flow::Image(image) => {
                self.started = true;
                if !self.current.is_empty() && self.cursor + image.h > self.content_height() {
                    self.page_break()?;
                }
                let mut image = image;
                image.y += self.geometry.top + self.cursor;
                self.cursor += image.h;
                self.current.push(Item::Image(image));
                Ok(())
            }
            Flow::Block { items, height } => {
                self.started = true;
                if !self.current.is_empty() && self.cursor + height > self.content_height() {
                    self.page_break()?;
                }
                let dy = self.geometry.top + self.cursor;
                for item in &items {
                    self.current.push(offset_item(item, 0.0, dy));
                }
                self.cursor += height;
                Ok(())
            }
        }
    }

    fn place_line(&mut self, mut line: TextLine) -> Result<()> {
        // An empty paragraph at the very top must still produce a page.
        self.started = true;
        if !self.current.is_empty() && self.cursor + line.height > self.content_height() {
            self.page_break()?;
        }
        line.offset(0.0, self.geometry.top + self.cursor);
        for item in line.items {
            self.current.push(Item::Text(item));
        }
        self.cursor += line.height;
        self.check_capacity()
    }

    fn finish(mut self) -> Layout {
        if self.started {
            self.flush();
        }
        if self.pages.is_empty() {
            self.pages.push(PlacedPage {
                width_px: self.geometry.width,
                height_px: self.geometry.height,
                items: Vec::new(),
            });
        }
        Layout { pages: self.pages }
    }
}
