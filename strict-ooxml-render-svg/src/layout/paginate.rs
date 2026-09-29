//! Pagination: section geometry, page breaks, footnotes and overflow
//! (`STAGE-4-TASK.md` §5.6, `STAGE-5-TASK.md` §5.4).

use std::collections::HashMap;

use strict_ooxml_core::error::Result;

use crate::error::RenderError;
use crate::fields::FieldKind;
use crate::layout::floating::{reserves_vertical_space, PendingAnchor};
use crate::layout::paragraph::layout_paragraph;
use crate::layout::table::{layout_blocks_inline, layout_table, offset_item};
use crate::layout::{
    geometry_for, Flow, Geometry, Item, Layout, LayoutContext, LineItem, PlacedPage, TableRowFlow,
    TextLine,
};
use strict_ooxml_wml::model::Block;

/// Hard cap on the number of pages (output-size guard).
const MAX_PAGES: usize = 10_000;
/// Hard cap on the number of paint items (output-size guard).
const MAX_ITEMS: usize = 2_000_000;
/// Height reserved for the footnote separator, in px.
const FOOTNOTE_SEPARATOR_HEIGHT: f64 = 10.0;
/// Padding between the footnote area and the bottom content edge, in px.
const FOOTNOTE_BOTTOM_PAD: f64 = 4.0;
/// Height reserved for the endnote separator block, in px (gap + line).
const ENDNOTE_SEPARATOR_HEIGHT: f64 = 16.0;
/// Vertical gap before the endnote separator line, in px.
const ENDNOTE_SEPARATOR_GAP: f64 = 8.0;

/// Lays out the whole document into pages.
pub(crate) fn layout_document(ctx: &LayoutContext<'_>) -> Result<Layout> {
    // First pass: count pages. If no computed field is present, it is final.
    let (layout, has_fields) = layout_once(ctx, 1)?;
    if !has_fields {
        return Ok(layout);
    }
    // Second pass onward: substitute the real page total (NUMPAGES) and re-run
    // until the page count is stable (a bounded, deterministic iteration).
    let mut total = layout.pages.len().max(1);
    let mut layout = layout;
    for _ in 0..8 {
        let (next, _) = layout_once(ctx, total)?;
        if next.pages.len() == total {
            return Ok(next);
        }
        total = next.pages.len().max(1);
        layout = next;
    }
    Ok(layout)
}

/// Lays out the document once, using `total_pages` for NUMPAGES/SECTIONPAGES.
fn layout_once(ctx: &LayoutContext<'_>, total_pages: usize) -> Result<(Layout, bool)> {
    let section = ctx
        .document
        .sections
        .last()
        .map(|section| &section.properties);
    let geometry = geometry_for(section, ctx.options.scale);
    let mut paginator = Paginator::new(ctx, geometry, total_pages);
    layout_blocks(
        ctx,
        &ctx.document.body.blocks,
        geometry.content_width(),
        &mut paginator,
        0,
    )?;
    append_endnotes(ctx, &mut paginator)?;
    paginator.flush_pending()?;
    let has_fields = paginator.has_fields;
    let mut layout = paginator.finish();
    crate::layout::floating::resolve(ctx, &mut layout.pages, &layout.anchors, &geometry);
    crate::layout::pageborders::apply(ctx, &mut layout.pages, &geometry, section);
    crate::layout::headerfooter::decorate_pages(ctx, &mut layout.pages, geometry, section);
    Ok((layout, has_fields))
}

/// Walks blocks, appending their flows to the paginator.
fn layout_blocks(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    width: f64,
    paginator: &mut Paginator<'_>,
    depth: usize,
) -> Result<()> {
    if depth > 16 {
        return Ok(());
    }
    let left = paginator.geometry.left;
    let grid = paginator.geometry.grid_line_pitch;
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let flow = layout_paragraph(ctx, para, left, width, grid, None);
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
                            Flow::TableRow(row) => row.height,
                            Flow::PageBreak => 0.0,
                        })
                        .sum();
                    if !paginator.at_page_top()
                        && paginator.cursor + total > paginator.body_height()
                    {
                        paginator.page_break()?;
                    }
                }
                let page = paginator.pages.len();
                let host_x = paginator.geometry.left;
                let host_y = paginator.geometry.top + paginator.cursor;
                for item in flow.flows {
                    paginator.place(item)?;
                }
                paginator.add_vspace(flow.space_after);
                for anchor in flow.anchors {
                    if reserves_vertical_space(&anchor) {
                        if let Some(extent) = anchor.extent {
                            let height =
                                crate::units::emu_to_px(extent.cy.value(), ctx.options.scale);
                            paginator.add_vspace(height);
                        }
                    }
                    paginator.anchors.push(PendingAnchor {
                        page,
                        host_x,
                        host_y,
                        anchor,
                    });
                }
            }
            Block::Table(table) => {
                let flows = layout_table(ctx, table, left, width);
                paginator.set_table_headers(&flows);
                for flow in flows {
                    paginator.place(flow)?;
                }
                paginator.clear_table_headers();
            }
            Block::SdtBlock(sdt) => {
                layout_blocks(ctx, &sdt.blocks, width, paginator, depth + 1)?;
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
    Ok(())
}

/// Lays out the referenced endnotes at the end of the document.
fn append_endnotes(ctx: &LayoutContext<'_>, paginator: &mut Paginator<'_>) -> Result<()> {
    let ids = ctx.note_numbers.endnotes_in_order();
    if ids.is_empty() {
        return Ok(());
    }
    let left = paginator.geometry.left;
    let width = paginator.geometry.content_width();
    let line = Item::Line(LineItem {
        x1: left,
        y1: ENDNOTE_SEPARATOR_GAP,
        x2: left + (width / 3.0).max(8.0),
        y2: ENDNOTE_SEPARATOR_GAP,
        color: "#000000".to_owned(),
        width: 1.0,
        dashed: false,
    });
    paginator.place(Flow::Block {
        items: vec![line],
        height: ENDNOTE_SEPARATOR_HEIGHT,
    })?;
    for id in ids {
        let Some(note) = i32::try_from(id)
            .ok()
            .and_then(|id| ctx.document.endnotes.get(id))
        else {
            continue;
        };
        let marker = ctx
            .note_numbers
            .endnote_number(id)
            .map(|(number, format)| format.format(number))
            .unwrap_or_default();
        append_note_blocks(ctx, &note.blocks, left, width, &marker, paginator)?;
    }
    Ok(())
}

/// Appends one endnote's blocks to the flow.
fn append_note_blocks(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    marker: &str,
    paginator: &mut Paginator<'_>,
) -> Result<()> {
    let grid = paginator.geometry.grid_line_pitch;
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let flow = layout_paragraph(ctx, para, left, width, grid, Some(marker));
                paginator.add_vspace(flow.space_before);
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
                append_note_blocks(ctx, &sdt.blocks, left, width, marker, paginator)?;
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
    Ok(())
}

/// The paginator accumulates items into pages and manages footnotes.
struct Paginator<'a> {
    ctx: &'a LayoutContext<'a>,
    geometry: Geometry,
    total_pages: usize,
    has_fields: bool,
    pages: Vec<PlacedPage>,
    current: Vec<Item>,
    cursor: f64,
    /// Whether the current page has been started (even if empty).
    started: bool,
    /// Footnote ids rendered on the current page, in order.
    page_footnotes: Vec<u32>,
    /// Footnote ids deferred to the next page (continuation).
    pending: Vec<u32>,
    /// Sum of the current page's footnote heights, excluding the separator.
    notes_height: f64,
    /// Whether the current page's footnote area continues from the previous one.
    continuation: bool,
    /// Laid-out footnotes (items relative to the page's content left at `y = 0`).
    note_cache: HashMap<u32, (Vec<Item>, f64)>,
    /// Header rows of the table currently being placed (`w:tblHeader`).
    table_headers: Vec<TableRowFlow>,
    /// Anchored (floating) objects recorded during pagination.
    anchors: Vec<PendingAnchor>,
}

impl<'a> Paginator<'a> {
    fn new(ctx: &'a LayoutContext<'a>, geometry: Geometry, total_pages: usize) -> Self {
        let mut note_cache = HashMap::new();
        for id in ctx.note_numbers.footnotes_in_order() {
            let marker = ctx
                .note_numbers
                .footnote_number(id)
                .map(|(number, format)| format.format(number))
                .unwrap_or_default();
            if let Some(note) = i32::try_from(id)
                .ok()
                .and_then(|id| ctx.document.footnotes.get(id))
            {
                let mut items = Vec::new();
                let mut y = 0.0;
                layout_blocks_inline(
                    ctx,
                    &note.blocks,
                    geometry.left,
                    geometry.content_width(),
                    &mut y,
                    &mut items,
                    0,
                    Some(&marker),
                );
                note_cache.insert(id, (items, y));
            }
        }
        Self {
            ctx,
            geometry,
            total_pages,
            has_fields: false,
            pages: Vec::new(),
            current: Vec::new(),
            cursor: 0.0,
            started: false,
            page_footnotes: Vec::new(),
            pending: Vec::new(),
            notes_height: 0.0,
            continuation: false,
            note_cache,
            table_headers: Vec::new(),
            anchors: Vec::new(),
        }
    }

    /// Height of the current page's footnote area (separator included).
    fn reserved_height(&self) -> f64 {
        if self.page_footnotes.is_empty() {
            0.0
        } else {
            FOOTNOTE_SEPARATOR_HEIGHT + self.notes_height + FOOTNOTE_BOTTOM_PAD
        }
    }

    /// Body height available on the current page.
    fn body_height(&self) -> f64 {
        (self.geometry.content_height() - self.reserved_height()).max(1.0)
    }

    /// Height of one cached footnote.
    fn note_height(&self, id: u32) -> f64 {
        self.note_cache.get(&id).map_or(0.0, |(_, height)| *height)
    }

    fn at_page_top(&self) -> bool {
        !self.started || (self.current.is_empty() && self.cursor <= 0.0)
    }

    /// Adds vertical space before the next flow.
    ///
    /// Word/WPS apply `w:spacing/@w:before` at the top of a page by default
    /// (the `suppressTopSpacing` compatibility flag is off), so it is not
    /// suppressed here.
    fn add_vspace(&mut self, space: f64) {
        if space <= 0.0 {
            return;
        }
        self.cursor = (self.cursor + space).min(self.body_height());
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

    /// Starts a new page, carrying deferred footnotes over as a continuation.
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
        let carried = std::mem::take(&mut self.pending);
        self.continuation = !carried.is_empty();
        self.page_footnotes = carried;
        self.notes_height = 0.0;
        let ids = self.page_footnotes.clone();
        for id in ids {
            let height = self.note_height(id);
            self.notes_height += height;
        }
        Ok(())
    }

    /// Flushes the current page, appending its footnote area.
    fn flush(&mut self) {
        if self.current.is_empty() && self.page_footnotes.is_empty() && !self.pages.is_empty() {
            return;
        }
        if !self.page_footnotes.is_empty() {
            let items = self.footnote_area_items();
            self.current.extend(items);
        }
        self.pages.push(PlacedPage {
            width_px: self.geometry.width,
            height_px: self.geometry.height,
            items: std::mem::take(&mut self.current),
        });
    }

    /// Builds the absolute-positioned paint items of the current footnote area.
    fn footnote_area_items(&self) -> Vec<Item> {
        let left = self.geometry.left;
        let width = self.geometry.content_width();
        let bottom = self.geometry.height - self.geometry.bottom;
        let total: f64 = self
            .page_footnotes
            .iter()
            .map(|id| self.note_height(*id))
            .sum();
        let area_bottom = bottom - FOOTNOTE_BOTTOM_PAD;
        let area_top = area_bottom - FOOTNOTE_SEPARATOR_HEIGHT - total;
        let separator_length = if self.continuation {
            width
        } else {
            (width / 3.0).max(8.0)
        };
        let mut items = vec![Item::Line(LineItem {
            x1: left,
            y1: area_top,
            x2: left + separator_length,
            y2: area_top,
            color: "#000000".to_owned(),
            width: 1.0,
            dashed: false,
        })];
        let mut y = area_top + FOOTNOTE_SEPARATOR_HEIGHT;
        for id in &self.page_footnotes {
            if let Some((note_items, height)) = self.note_cache.get(id) {
                for item in note_items {
                    items.push(offset_item(item, 0.0, y));
                }
                y += *height;
            }
        }
        items
    }

    /// Places one flow item, breaking the page if it does not fit.
    fn place(&mut self, flow: Flow) -> Result<()> {
        match flow {
            Flow::PageBreak => self.page_break(),
            Flow::Line(line) => self.place_line(line),
            Flow::Image(image) => {
                self.started = true;
                if !self.current.is_empty() && self.cursor + image.h > self.body_height() {
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
                if !self.current.is_empty() && self.cursor + height > self.body_height() {
                    self.page_break()?;
                }
                let dy = self.geometry.top + self.cursor;
                for item in &items {
                    self.current.push(offset_item(item, 0.0, dy));
                }
                self.cursor += height;
                Ok(())
            }
            Flow::TableRow(row) => self.place_table_row(&row),
        }
    }

    /// Records the header rows of the table about to be placed.
    fn set_table_headers(&mut self, flows: &[Flow]) {
        self.table_headers = flows
            .iter()
            .filter_map(|flow| match flow {
                Flow::TableRow(row) if row.header => Some(row.clone()),
                _ => None,
            })
            .collect();
    }

    /// Clears the current table's header rows.
    fn clear_table_headers(&mut self) {
        self.table_headers.clear();
    }

    /// Places one table row, breaking the page and repeating header rows as needed.
    fn place_table_row(&mut self, row: &TableRowFlow) -> Result<()> {
        self.started = true;
        if !self.current.is_empty() && self.cursor + row.height > self.body_height() {
            self.page_break()?;
            self.repeat_table_headers();
        }
        self.place_row_items(row);
        Ok(())
    }

    /// Places a row's items at the current cursor.
    fn place_row_items(&mut self, row: &TableRowFlow) {
        let dy = self.geometry.top + self.cursor;
        for item in &row.items {
            self.current.push(offset_item(item, 0.0, dy));
        }
        self.cursor += row.height;
    }

    /// Re-emits the header rows at the top of a continued table page.
    fn repeat_table_headers(&mut self) {
        let headers = self.table_headers.clone();
        for header in headers {
            if !self.current.is_empty() && self.cursor + header.height > self.body_height() {
                break;
            }
            self.place_row_items(&header);
        }
    }

    fn place_line(&mut self, mut line: TextLine) -> Result<()> {
        // An empty paragraph at the very top must still produce a page.
        self.started = true;
        if self.current.is_empty() {
            self.reserve_line_footnotes(&line);
        } else {
            let extra = self.prospective_footnote_extra(&line);
            if self.cursor + line.height + extra > self.body_height() {
                self.page_break()?;
            }
            self.reserve_line_footnotes(&line);
        }
        self.resolve_fields(&mut line);
        line.offset(0.0, self.geometry.top + self.cursor);
        for item in line.items {
            self.current.push(Item::Text(item));
        }
        self.cursor += line.height;
        self.check_capacity()
    }

    /// Substitutes computed-field placeholders with their page-dependent value.
    fn resolve_fields(&mut self, line: &mut TextLine) {
        if !line.items.iter().any(|item| item.field.is_some()) {
            return;
        }
        let page_number = self.pages.len() + 1;
        for item in &mut line.items {
            let Some(marker) = item.field else {
                continue;
            };
            let value = match marker.kind {
                FieldKind::Page => page_number,
                FieldKind::NumPages | FieldKind::SectionPages => self.total_pages,
            };
            let text = marker.format.format(value.min(u32::MAX as usize) as u32);
            item.width = self.ctx.measure(&text, &item.run);
            item.text = text;
            self.has_fields = true;
        }
    }

    /// Reserves the footnotes referenced on `line` on the current page.
    fn reserve_line_footnotes(&mut self, line: &TextLine) {
        let ids = line.footnote_refs.clone();
        for id in ids {
            self.reserve_footnote(id);
        }
    }

    /// Reserves one footnote, or defers it to the next page when it cannot fit.
    fn reserve_footnote(&mut self, id: u32) {
        if self.page_footnotes.contains(&id) || self.pending.contains(&id) {
            return;
        }
        let height = self.note_height(id);
        if self.notes_height + height <= self.geometry.content_height() {
            self.page_footnotes.push(id);
            self.notes_height += height;
        } else {
            self.pending.push(id);
        }
    }

    /// Extra footnote height `line` would reserve on the current page.
    fn prospective_footnote_extra(&self, line: &TextLine) -> f64 {
        let mut notes = self.notes_height;
        let mut has_notes = !self.page_footnotes.is_empty();
        let mut extra = 0.0;
        for id in &line.footnote_refs {
            if self.page_footnotes.contains(id) || self.pending.contains(id) {
                continue;
            }
            let height = self.note_height(*id);
            if notes + height <= self.geometry.content_height() {
                if !has_notes {
                    extra += FOOTNOTE_SEPARATOR_HEIGHT;
                    has_notes = true;
                }
                notes += height;
                extra += height;
            }
        }
        extra
    }

    /// Forces a page break when footnotes were deferred past the last page.
    fn flush_pending(&mut self) -> Result<()> {
        if !self.pending.is_empty() {
            self.page_break()?;
        }
        Ok(())
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
        Layout {
            pages: self.pages,
            anchors: self.anchors,
        }
    }
}
