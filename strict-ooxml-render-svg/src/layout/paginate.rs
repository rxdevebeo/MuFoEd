//! Pagination: section geometry, page breaks, footnotes and overflow
//! (`STAGE-4-TASK.md` §5.6, `STAGE-5-TASK.md` §5.4).

use std::collections::HashMap;

use strict_ooxml_core::error::Result;

use crate::error::RenderError;
use crate::layout::exclusions::PageExclusion;
use crate::layout::floating::{page_exclusion, PendingAnchor};
use crate::layout::paragraph::layout_paragraph;
use crate::layout::table::{
    frame_group_end, frame_placement, framed_table_items, layout_blocks_inline,
    layout_frame_contents, layout_table, offset_item, table_uniform_frame,
};
use crate::layout::{
    geometry_for, Flow, Geometry, Item, Layout, LayoutContext, LineItem, PlacedPage, TableRowFlow,
    TextLine,
};
use crate::style::compute_paragraph;
use crate::units::pt_to_px;
use strict_ooxml_wml::model::values::SectionType;
use strict_ooxml_wml::model::Block;

/// Height reserved for the footnote separator, in px.
const FOOTNOTE_SEPARATOR_HEIGHT: f64 = 10.0;
/// Padding between the footnote area and the bottom content edge, in px.
const FOOTNOTE_BOTTOM_PAD: f64 = 4.0;
/// Height reserved for the endnote separator block, in px (gap + line).
const ENDNOTE_SEPARATOR_HEIGHT: f64 = 16.0;
/// Vertical gap before the endnote separator line, in px.
const ENDNOTE_SEPARATOR_GAP: f64 = 8.0;
/// F13: PAGE/NUMPAGES/SECTIONPAGES and region geometry must stabilize in this many passes.
const MAX_LAYOUT_PASSES: usize = 8;

/// Totals assumed for FieldEnv on one layout pass.
struct AssumedTotals {
    pages: usize,
    section_pages: Vec<usize>,
}

/// Observed page count + quantized active header/footer heights.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LayoutFingerprint {
    pages: usize,
    section_pages: Vec<usize>,
    regions: Vec<(i64, i64)>,
}

impl LayoutFingerprint {
    fn new(pages: &[PlacedPage], regions: &[(f64, f64)]) -> Self {
        let count = pages
            .iter()
            .map(|page| page.section_index)
            .max()
            .map_or(1, |index| index + 1);
        let mut section_pages = vec![0usize; count];
        for page in pages {
            let index = page.section_index.min(section_pages.len() - 1);
            if let Some(count) = section_pages.get_mut(index) {
                *count += 1;
            }
        }
        Self {
            pages: pages.len(),
            section_pages,
            regions: regions
                .iter()
                .map(|(header, footer)| (quantize_px(*header), quantize_px(*footer)))
                .collect(),
        }
    }
}

fn quantize_px(value: f64) -> i64 {
    (value * 1000.0).round() as i64
}

/// Lays out the whole document into pages.
pub(crate) fn layout_document(ctx: &LayoutContext<'_>) -> Result<Layout> {
    // The model's block nesting, checked before a single item is placed (AUD-05
    // п.2). A `Document` built in code never met the reader's bound, so this is
    // the only place the renderer can refuse one; and the check has to come
    // before the layout because the layout's own depth guard answers an
    // over-deep document by laying out less of it, which is a wrong page rather
    // than a wrong answer.
    strict_ooxml_wml::nesting::check_document(ctx.document, &ctx.options.limits).map_err(
        |exceeded| {
            crate::error::RenderError::LimitExceeded {
                what: exceeded.kind.as_str(),
                limit: u64::from(exceeded.limit),
                actual: u64::from(exceeded.actual),
            }
            .into_strict()
        },
    )?;
    let mut assumed = AssumedTotals {
        pages: 1,
        section_pages: Vec::new(),
    };
    let mut previous: Option<LayoutFingerprint> = None;
    for _ in 0..MAX_LAYOUT_PASSES {
        let (layout, fingerprint, has_fields) = layout_once(ctx, &assumed)?;
        if previous.as_ref() == Some(&fingerprint) {
            let mut layout = layout;
            sanitize_finite_coords(&mut layout);
            return Ok(layout);
        }
        if !has_fields && previous.is_none() {
            let mut layout = layout;
            sanitize_finite_coords(&mut layout);
            return Ok(layout);
        }
        assumed.pages = fingerprint.pages.max(1);
        assumed.section_pages.clone_from(&fingerprint.section_pages);
        previous = Some(fingerprint);
    }
    Err(RenderError::DidNotConverge {
        passes: u64::try_from(MAX_LAYOUT_PASSES).unwrap_or(8),
    }
    .into_strict())
}

/// Drops paint items with non-finite coordinates and records a warning (AUD-71).
fn sanitize_finite_coords(layout: &mut Layout) {
    let mut removed = false;
    for page in &mut layout.pages {
        let before = page.items.len();
        page.items.retain(item_coords_finite);
        removed |= page.items.len() < before;
    }
    if removed {
        let message =
            "render.non-finite-coord: removed paint item with non-finite coordinates".to_owned();
        if !layout.warnings.contains(&message) {
            layout.warnings.push(message);
        }
    }
}

/// Whether every numeric coordinate on `item` is finite.
fn item_coords_finite(item: &Item) -> bool {
    match item {
        Item::Text(text) => {
            text.x.is_finite()
                && text.baseline.is_finite()
                && text.width.is_finite()
                && text.size_px.is_finite()
        }
        Item::Rect(rect) => {
            rect.x.is_finite()
                && rect.y.is_finite()
                && rect.w.is_finite()
                && rect.h.is_finite()
                && rect.stroke_w.is_finite()
        }
        Item::Line(line) => {
            line.x1.is_finite()
                && line.y1.is_finite()
                && line.x2.is_finite()
                && line.y2.is_finite()
                && line.width.is_finite()
        }
        Item::Path(path) => {
            path.x.is_finite()
                && path.y.is_finite()
                && path.w.is_finite()
                && path.h.is_finite()
                && path.stroke_w.is_finite()
                && path.rotate_deg.is_finite()
        }
        Item::Image(image) => {
            image.x.is_finite() && image.y.is_finite() && image.w.is_finite() && image.h.is_finite()
        }
    }
}

/// Lays out the document once, using `assumed` totals for NUMPAGES/SECTIONPAGES.
fn layout_once(
    ctx: &LayoutContext<'_>,
    assumed: &AssumedTotals,
) -> Result<(Layout, LayoutFingerprint, bool)> {
    ctx.render_items.set(0);
    ctx.reset_frame_cursors();
    let sections = &ctx.document.sections;
    let runs = section_runs(&ctx.document.body.blocks, sections.len());
    let first_props = sections.first().map(|section| &section.properties);
    let geometry = geometry_for(first_props, ctx.options.scale, Some(ctx));
    let page_start = first_props
        .and_then(|properties| properties.page_number.as_ref())
        .and_then(|page_number| page_number.start)
        .unwrap_or(1);
    let page_format = crate::notes::NumberFormat::from_strict(
        first_props
            .and_then(|properties| properties.page_number.as_ref())
            .and_then(|page_number| page_number.format.as_deref()),
        crate::notes::NumberFormat::Decimal,
    );
    let mut paginator = Paginator::new(ctx, geometry, assumed, page_start, page_format);
    paginator.section_index = 0;
    for (index, (start, end)) in runs.iter().enumerate() {
        if paginator.selection_exhausted() {
            break;
        }
        let props = sections.get(index).map(|section| &section.properties);
        if index > 0 {
            // `w:type` on the *ending* section's sectPr describes the break
            // into this section (ISO/IEC 29500-1 §17.6.22).
            let break_type = sections
                .get(index - 1)
                .and_then(|section| section.properties.section_type);
            let geometry = geometry_for(props, ctx.options.scale, Some(ctx));
            paginator.apply_section_break(break_type, geometry, index)?;
            if paginator.selection_exhausted() {
                break;
            }
            if let Some(properties) = props {
                if let Some(start) = properties
                    .page_number
                    .as_ref()
                    .and_then(|page_number| page_number.start)
                {
                    paginator.page_start = start.max(1);
                }
                paginator.page_format = crate::notes::NumberFormat::from_strict(
                    properties
                        .page_number
                        .as_ref()
                        .and_then(|page_number| page_number.format.as_deref()),
                    crate::notes::NumberFormat::Decimal,
                );
            }
        }
        let width = paginator.geometry.content_width();
        let body_blocks = &ctx.document.body.blocks;
        let run_blocks = body_blocks.get(*start..*end).unwrap_or_default();
        layout_blocks(ctx, run_blocks, width, &mut paginator, 0)?;
    }
    if !paginator.selection_exhausted() {
        append_endnotes(ctx, &mut paginator)?;
        paginator.flush_pending()?;
    }
    let mut has_fields = paginator.has_fields;
    // AUD-70: PAGE/NUMPAGES in headers/footers also force a second pass so
    // NUMPAGES sees the real page count.
    if !has_fields {
        has_fields = any_headers_footers_have_dynamic_fields(ctx, sections);
    }
    let (mut layout, regions) = paginator.finish()?;
    crate::layout::floating::resolve(ctx, &mut layout.pages, &layout.anchors, sections)?;
    crate::layout::pageborders::apply(ctx, &mut layout.pages, sections)?;
    crate::layout::headerfooter::decorate_pages(
        ctx,
        &mut layout.pages,
        sections,
        assumed.pages.max(1),
        &assumed.section_pages,
    )?;
    let fingerprint = LayoutFingerprint::new(&layout.pages, &regions);
    Ok((layout, fingerprint, has_fields))
}

/// Top-level body ranges `[start, end)` belonging to each document section (AUD-74).
fn section_runs(blocks: &[Block], section_count: usize) -> Vec<(usize, usize)> {
    if section_count == 0 {
        return vec![(0, blocks.len())];
    }
    let mut runs = Vec::with_capacity(section_count);
    let mut start = 0usize;
    let mut section = 0usize;
    for (index, block) in blocks.iter().enumerate() {
        if block_ends_section(block) {
            runs.push((start, index + 1));
            start = index + 1;
            section += 1;
            if section + 1 >= section_count {
                break;
            }
        }
    }
    while runs.len() < section_count {
        let end = if runs.len() + 1 == section_count {
            blocks.len()
        } else {
            start
        };
        runs.push((start, end));
        start = end;
    }
    if let Some(last) = runs.last_mut() {
        last.1 = blocks.len();
    }
    runs
}

/// Whether a top-level block carries a section break that ends the current section.
fn block_ends_section(block: &Block) -> bool {
    match block {
        Block::Paragraph(para) => para.props.section.is_some(),
        Block::SdtBlock(sdt) => sdt.blocks.iter().any(block_ends_section),
        _ => false,
    }
}

/// Whether any referenced header/footer part contains a computed page field.
fn any_headers_footers_have_dynamic_fields(
    ctx: &LayoutContext<'_>,
    sections: &[strict_ooxml_wml::model::props::Section],
) -> bool {
    sections.iter().any(|section| {
        section
            .properties
            .headers
            .iter()
            .chain(section.properties.footers.iter())
            .filter_map(|reference| reference.part.as_ref())
            .any(|part| {
                ctx.document
                    .header_footer(part)
                    .is_some_and(|hf| crate::fields::blocks_have_dynamic_fields(&hf.blocks))
            })
    })
}

/// Walks blocks, appending their flows to the paginator.
fn layout_blocks(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    width: f64,
    paginator: &mut Paginator<'_>,
    depth: u32,
) -> Result<()> {
    // The page body is depth 0; a content control inside it is 1, and a table
    // cell inside that is 2. The bound is the reader's, checked up front in
    // `layout_document`; this arm is the layout's own guard, and it stays
    // because a future container the model walk does not know about would
    // otherwise have nothing between it and the stack.
    if depth > ctx.options.limits.max_block_nesting {
        return Err(crate::error::RenderError::LimitExceeded {
            what: "block nesting",
            limit: u64::from(ctx.options.limits.max_block_nesting),
            actual: u64::from(depth),
        }
        .into_strict());
    }
    ctx.set_block_depth(depth);
    let left = paginator.geometry.left;
    let grid = paginator.geometry.grid_line_pitch;
    // The space between two paragraphs is the **larger** of the first's
    // `w:after` and the second's `w:before`, not their sum: `06-strict-math-display`
    // leaves 27 px of white between a body paragraph and the `Heading2` that
    // follows it, and 10.667 px of the 37 px we leave is the `after` we added on
    // top of the `before`. A page break ends the paragraph, so nothing pending
    // follows it onto the next page.
    let mut pending_after = 0.0f64;
    let mut index = 0;
    while let Some(block) = blocks.get(index) {
        if paginator.selection_exhausted() {
            return Ok(());
        }
        if let Block::Paragraph(para) = block {
            if para.props.frame.is_some() {
                let end = frame_group_end(blocks, index);
                let group = blocks.get(index..end).unwrap_or_default();
                place_frame_group(ctx, group, paginator)?;
                pending_after = 0.0;
                index = end;
                continue;
            }
        }
        match block {
            Block::Paragraph(para) => {
                pending_after =
                    place_body_paragraph(ctx, para, left, width, grid, paginator, pending_after)?;
            }
            Block::Table(table) => {
                ctx.frame_anchor.set((
                    paginator.geometry.left,
                    paginator.geometry.top,
                    paginator.geometry.content_width(),
                    paginator.geometry.width,
                    paginator.geometry.height,
                ));
                if let Some(frame) = table_uniform_frame(table) {
                    place_framed_table(ctx, table, frame, paginator, depth)?;
                    pending_after = 0.0;
                } else {
                    let flows = layout_table(ctx, table, left, width, depth, true);
                    paginator.set_table_headers(&flows);
                    for flow in flows {
                        paginator.place(flow)?;
                    }
                    paginator.clear_table_headers();
                }
            }
            Block::SdtBlock(sdt) => {
                layout_blocks(ctx, &sdt.blocks, width, paginator, depth + 1)?;
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
        index += 1;
    }
    Ok(())
}

/// Lays out one body paragraph, registers wrap exclusions, returns `space_after`.
fn place_body_paragraph(
    ctx: &LayoutContext<'_>,
    para: &strict_ooxml_wml::model::Paragraph,
    left: f64,
    width: f64,
    grid: Option<f64>,
    paginator: &mut Paginator<'_>,
    pending_after: f64,
) -> Result<f64> {
    // An empty paragraph that only carries `w:sectPr` is a boundary, not a line
    // of text. Letting it overflow would insert a blank page before the section
    // break that already starts the next page.
    if paginator.selection_exhausted() {
        return Ok(0.0);
    }
    let boundary_marker = para.props.section.is_some() && para.inlines.is_empty();
    let mut pending_after = pending_after;
    if para.props.page_break_before.is_on() && !paginator.at_page_top() {
        paginator.keep_empty_page = true;
        paginator.page_break()?;
        if paginator.selection_exhausted() {
            return Ok(0.0);
        }
        pending_after = 0.0;
    }
    let space_before = pt_to_px(
        compute_paragraph(ctx.document, para).space_before_pt,
        ctx.options.scale,
    );
    let gap = pending_after.max(space_before);
    let host_x = paginator.geometry.left;
    let host_y_pred = paginator.geometry.top + paginator.cursor + gap;
    let local_exclusions: Vec<_> = paginator
        .wrap_exclusions
        .iter()
        .copied()
        .map(|exclusion| exclusion.to_paragraph_local(host_y_pred))
        .collect();
    let flow = layout_paragraph(
        ctx,
        para,
        left,
        width,
        grid,
        None,
        &local_exclusions,
        Some((&paginator.geometry, host_x, host_y_pred)),
    );
    paginator.add_vspace(pending_after.max(flow.space_before));
    paginator.add_vspace(flow.border_before);
    if flow.keep_lines && !boundary_marker {
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
        if !paginator.at_page_top() && paginator.cursor + total > paginator.body_height() {
            paginator.page_break()?;
            if paginator.selection_exhausted() {
                return Ok(0.0);
            }
        }
    }
    if paginator.selection_exhausted() {
        return Ok(0.0);
    }
    let page = paginator.pages.len();
    let host_y = paginator.geometry.top + paginator.cursor;
    for item in flow.flows {
        if boundary_marker {
            paginator.place_boundary_marker(item)?;
        } else {
            paginator.place(item)?;
        }
    }
    for anchor in flow.anchors {
        if let Some(exclusion) = page_exclusion(ctx, &anchor, &paginator.geometry, host_x, host_y) {
            paginator.wrap_exclusions.push(exclusion);
        }
        paginator.anchors.push(PendingAnchor {
            page,
            host_x,
            host_y,
            anchor,
        });
    }
    Ok(flow.space_after + flow.border_after)
}

/// Lays a frame group out once and paints every child from the frame origin.
fn place_frame_group(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    paginator: &mut Paginator<'_>,
) -> Result<()> {
    let Some(Block::Paragraph(first)) = blocks.first() else {
        return Ok(());
    };
    let Some(frame) = first.props.frame.as_ref() else {
        return Ok(());
    };
    let scale = ctx.options.scale;
    let (origin_x, origin_y, frame_width) = frame_placement(
        frame,
        paginator.geometry.left,
        paginator.geometry.top,
        paginator.geometry.content_width(),
        paginator.geometry.content_height(),
        paginator.geometry.width,
        paginator.geometry.height,
        scale,
    );
    paginator.started = true;
    let page = paginator.pages.len();
    let resume = ctx.frame_resume(frame);
    let (items, anchors, height) =
        layout_frame_contents(ctx, blocks, origin_x, origin_y + resume, frame_width);
    ctx.frame_advance(frame, height);
    for (host_y, anchor) in anchors {
        paginator.anchors.push(PendingAnchor {
            page,
            host_x: origin_x,
            host_y,
            anchor,
        });
    }
    for item in items {
        paginator.push_item(item)?;
    }
    Ok(())
}

/// Places a table whose cells all share one page frame, keeping column topology.
fn place_framed_table(
    ctx: &LayoutContext<'_>,
    table: &strict_ooxml_wml::model::Table,
    frame: &strict_ooxml_wml::model::props::FrameProperties,
    paginator: &mut Paginator<'_>,
    depth: u32,
) -> Result<()> {
    let scale = ctx.options.scale;
    let (origin_x, origin_y, frame_width) = frame_placement(
        frame,
        paginator.geometry.left,
        paginator.geometry.top,
        paginator.geometry.content_width(),
        paginator.geometry.content_height(),
        paginator.geometry.width,
        paginator.geometry.height,
        scale,
    );
    paginator.started = true;
    for item in framed_table_items(ctx, table, origin_x, origin_y, frame_width, depth) {
        paginator.push_item(item)?;
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
        append_note_blocks(ctx, &note.blocks, left, width, &marker, paginator, 1)?;
    }
    Ok(())
}

/// Appends one endnote's blocks to the flow.
///
/// `depth` starts at 1: an endnote body is a block container of its own, and a
/// table inside it nests from there rather than from the page.
#[allow(clippy::too_many_arguments)]
fn append_note_blocks(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    marker: &str,
    paginator: &mut Paginator<'_>,
    depth: u32,
) -> Result<()> {
    if depth > ctx.options.limits.max_block_nesting {
        return Err(crate::error::RenderError::LimitExceeded {
            what: "block nesting",
            limit: u64::from(ctx.options.limits.max_block_nesting),
            actual: u64::from(depth),
        }
        .into_strict());
    }
    ctx.set_block_depth(depth);
    let grid = paginator.geometry.grid_line_pitch;
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let space_before = pt_to_px(
                    compute_paragraph(ctx.document, para).space_before_pt,
                    ctx.options.scale,
                );
                let host_x = paginator.geometry.left;
                let host_y_pred = paginator.geometry.top + paginator.cursor + space_before;
                let local_exclusions: Vec<_> = paginator
                    .wrap_exclusions
                    .iter()
                    .copied()
                    .map(|exclusion| exclusion.to_paragraph_local(host_y_pred))
                    .collect();
                let flow = layout_paragraph(
                    ctx,
                    para,
                    left,
                    width,
                    grid,
                    Some(marker),
                    &local_exclusions,
                    Some((&paginator.geometry, host_x, host_y_pred)),
                );
                paginator.add_vspace(flow.space_before);
                let host_y = paginator.geometry.top + paginator.cursor;
                for item in flow.flows {
                    paginator.place(item)?;
                }
                for anchor in flow.anchors {
                    if let Some(exclusion) =
                        page_exclusion(ctx, &anchor, &paginator.geometry, host_x, host_y)
                    {
                        paginator.wrap_exclusions.push(exclusion);
                    }
                    paginator.anchors.push(PendingAnchor {
                        page: paginator.pages.len(),
                        host_x,
                        host_y,
                        anchor,
                    });
                }
                paginator.add_vspace(flow.space_after);
            }
            Block::Table(table) => {
                ctx.frame_anchor.set((
                    paginator.geometry.left,
                    paginator.geometry.top,
                    paginator.geometry.content_width(),
                    paginator.geometry.width,
                    paginator.geometry.height,
                ));
                for flow in layout_table(ctx, table, left, width, depth, true) {
                    paginator.place(flow)?;
                }
            }
            Block::SdtBlock(sdt) => {
                append_note_blocks(ctx, &sdt.blocks, left, width, marker, paginator, depth + 1)?;
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
    /// Section margins before the active header/footer extra.
    base_geometry: Geometry,
    total_pages: usize,
    assumed_section_pages: Vec<usize>,
    /// First page number from `w:pgNumType/@w:start` (AUD-46).
    page_start: u32,
    /// Number format from `w:pgNumType/@w:fmt` (AUD-70).
    page_format: crate::notes::NumberFormat,
    has_fields: bool,
    pages: Vec<PlacedPage>,
    current: Vec<Item>,
    cursor: f64,
    /// Whether the current page has been started (even if empty).
    started: bool,
    /// Section index for content currently being placed (AUD-74).
    section_index: usize,
    /// Keep the next flushed empty page (explicit break / section break, AUD-75).
    keep_empty_page: bool,
    /// `PageSelection::Range` already produced `end` pages; further content is dropped.
    truncated: bool,
    /// Geometry deferred by a `continuous` section break until the next page.
    pending_geometry: Option<(Geometry, usize)>,
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
    /// Active Square/TopAndBottom exclusions on the current page (page coords).
    wrap_exclusions: Vec<PageExclusion>,
    /// Active header/footer heights used for the current page.
    current_region: (f64, f64),
    /// Per-page active header/footer heights (F13 fingerprint).
    page_regions: Vec<(f64, f64)>,
}

impl<'a> Paginator<'a> {
    fn new(
        ctx: &'a LayoutContext<'a>,
        geometry: Geometry,
        assumed: &AssumedTotals,
        page_start: u32,
        page_format: crate::notes::NumberFormat,
    ) -> Self {
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
                let mut page_frames = Vec::new();
                layout_blocks_inline(
                    ctx,
                    &note.blocks,
                    geometry.left,
                    geometry.content_width(),
                    &mut y,
                    &mut items,
                    0,
                    Some(&marker),
                    false,
                    &mut page_frames,
                );
                note_cache.insert(id, (items, y));
            }
        }
        let mut paginator = Self {
            ctx,
            geometry,
            base_geometry: geometry,
            total_pages: assumed.pages.max(1),
            assumed_section_pages: assumed.section_pages.clone(),
            page_start: page_start.max(1),
            page_format,
            has_fields: false,
            pages: Vec::new(),
            current: Vec::new(),
            cursor: 0.0,
            started: false,
            section_index: 0,
            keep_empty_page: false,
            truncated: false,
            pending_geometry: None,
            page_footnotes: Vec::new(),
            pending: Vec::new(),
            notes_height: 0.0,
            continuation: false,
            note_cache,
            table_headers: Vec::new(),
            anchors: Vec::new(),
            wrap_exclusions: Vec::new(),
            current_region: (0.0, 0.0),
            page_regions: Vec::new(),
        };
        paginator.apply_page_regions();
        paginator
    }

    /// Measures the active first/even/default header/footer and grows body insets.
    fn apply_page_regions(&mut self) {
        let page_ordinal = self.pages.len() + 1;
        let section_page_ordinal = self
            .pages
            .iter()
            .filter(|page| page.section_index == self.section_index)
            .count()
            + 1;
        let section_pages = self
            .assumed_section_pages
            .get(self.section_index)
            .copied()
            .unwrap_or(self.total_pages)
            .max(1);
        let reserve = crate::layout::headerfooter::page_body_reserve(
            &crate::layout::headerfooter::PageRegionQuery {
                ctx: self.ctx,
                sections: &self.ctx.document.sections,
                section_index: self.section_index,
                page_ordinal,
                section_page_ordinal,
                total_pages: self.total_pages.max(1),
                section_pages,
                page_start: self.page_start,
                page_format: self.page_format,
                geometry: self.base_geometry,
            },
        );
        self.geometry = self.base_geometry;
        self.geometry.top =
            (self.base_geometry.top + reserve.extra_top).min(self.geometry.height * 0.75);
        self.geometry.bottom = (self.base_geometry.bottom + reserve.extra_bottom)
            .min(self.geometry.height - self.geometry.top);
        self.current_region = (reserve.header_height, reserve.footer_height);
    }

    /// Applies a section break before laying out the next section's body (AUD-74).
    fn apply_section_break(
        &mut self,
        break_type: Option<SectionType>,
        geometry: Geometry,
        section_index: usize,
    ) -> Result<()> {
        let break_type = break_type.unwrap_or(SectionType::NextPage);
        match break_type {
            SectionType::Continuous => {
                // Same page; new geometry applies from the next page onward.
                self.pending_geometry = Some((geometry, section_index));
                Ok(())
            }
            SectionType::NextPage | SectionType::NextColumn => {
                self.keep_empty_page = true;
                self.page_break()?;
                if self.selection_exhausted() {
                    return Ok(());
                }
                // The break opened this page. An empty section (a blank PDF page)
                // has nothing to paint, and `flush` would otherwise drop it.
                self.keep_empty_page = true;
                self.base_geometry = geometry;
                self.geometry = geometry;
                self.section_index = section_index;
                self.apply_page_regions();
                Ok(())
            }
            SectionType::OddPage => {
                self.keep_empty_page = true;
                self.page_break()?;
                while !self.selection_exhausted() && (self.pages.len() + 1).is_multiple_of(2) {
                    self.keep_empty_page = true;
                    self.page_break()?;
                }
                if self.selection_exhausted() {
                    return Ok(());
                }
                self.keep_empty_page = true;
                self.base_geometry = geometry;
                self.geometry = geometry;
                self.section_index = section_index;
                self.apply_page_regions();
                Ok(())
            }
            SectionType::EvenPage => {
                self.keep_empty_page = true;
                self.page_break()?;
                while !self.selection_exhausted() && !(self.pages.len() + 1).is_multiple_of(2) {
                    self.keep_empty_page = true;
                    self.page_break()?;
                }
                if self.selection_exhausted() {
                    return Ok(());
                }
                self.keep_empty_page = true;
                self.base_geometry = geometry;
                self.geometry = geometry;
                self.section_index = section_index;
                self.apply_page_regions();
                Ok(())
            }
        }
    }

    /// Applies a deferred continuous-section geometry when a new page starts.
    fn take_pending_geometry(&mut self) {
        if let Some((geometry, section_index)) = self.pending_geometry.take() {
            self.base_geometry = geometry;
            self.geometry = geometry;
            self.section_index = section_index;
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

    /// Whether [`PageSelection::Range`] already has its last requested page.
    fn selection_exhausted(&self) -> bool {
        self.truncated
            || self
                .ctx
                .options
                .pages
                .layout_end()
                .is_some_and(|end| self.pages.len() >= end)
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

    fn check_page_capacity(&self) -> Result<()> {
        let limit = usize::try_from(self.ctx.options.limits.max_pages).unwrap_or(usize::MAX);
        if self.pages.len() >= limit {
            return Err(RenderError::LimitExceeded {
                what: "pages",
                limit: u64::from(self.ctx.options.limits.max_pages),
                actual: u64::try_from(self.pages.len().saturating_add(1)).unwrap_or(u64::MAX),
            }
            .into_strict());
        }
        Ok(())
    }

    /// Pushes one paint item onto the current page, charging the document budget.
    fn push_item(&mut self, item: Item) -> Result<()> {
        self.ctx.charge_items(1)?;
        self.current.push(item);
        Ok(())
    }

    /// Starts a new page, carrying deferred footnotes over as a continuation.
    fn page_break(&mut self) -> Result<()> {
        if self.selection_exhausted() {
            self.truncated = true;
            return Ok(());
        }
        self.check_page_capacity()?;
        if self.current.is_empty() && !self.started && !self.keep_empty_page {
            // A break before any content is ignored (no leading blank page),
            // unless an explicit/section break asked to keep the empty page.
            self.started = true;
            return Ok(());
        }
        self.flush()?;
        if self.selection_exhausted() {
            // `PageSelection::Range` asked only for pages through `end`.
            // Do not open the next page or keep laying out overflow.
            self.truncated = true;
            self.current.clear();
            self.started = true;
            self.keep_empty_page = false;
            self.pending.clear();
            self.page_footnotes.clear();
            self.notes_height = 0.0;
            self.continuation = false;
            return Ok(());
        }
        self.ctx.reset_frame_cursors();
        self.take_pending_geometry();
        self.cursor = 0.0;
        self.started = true;
        self.current = Vec::new();
        self.wrap_exclusions.clear();
        let carried = std::mem::take(&mut self.pending);
        self.continuation = !carried.is_empty();
        self.page_footnotes = carried;
        self.notes_height = 0.0;
        let ids = self.page_footnotes.clone();
        for id in ids {
            let height = self.note_height(id);
            self.notes_height += height;
        }
        self.apply_page_regions();
        Ok(())
    }

    /// Flushes the current page, appending its footnote area.
    fn flush(&mut self) -> Result<()> {
        let keep_empty = self.keep_empty_page;
        self.keep_empty_page = false;
        if self.current.is_empty()
            && self.page_footnotes.is_empty()
            && !self.pages.is_empty()
            && !keep_empty
        {
            // AUD-75: skip only when the page was not opened by an explicit
            // page/section break.
            return Ok(());
        }
        if !self.page_footnotes.is_empty() {
            let items = self.footnote_area_items();
            self.ctx.charge_items(items.len())?;
            self.current.extend(items);
        }
        self.pages.push(PlacedPage {
            width_px: self.geometry.width,
            height_px: self.geometry.height,
            items: std::mem::take(&mut self.current),
            section_index: self.section_index,
        });
        self.page_regions.push(self.current_region);
        Ok(())
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

    /// Places an inkless section-break paragraph without opening a spare page.
    ///
    /// The paragraph is the last block of the section it ends. When the line
    /// does not fit, a page break here would be a blank page in front of the
    /// section break that already starts the next page.
    fn place_boundary_marker(&mut self, flow: Flow) -> Result<()> {
        if let Flow::Line(line) = &flow {
            let inkless = line.items.is_empty() && line.graphics.is_empty();
            let fits = self.current.is_empty() || self.cursor + line.height <= self.body_height();
            if inkless && !fits {
                return Ok(());
            }
        }
        self.place(flow)
    }

    /// Places one flow item, breaking the page if it does not fit.
    fn place(&mut self, flow: Flow) -> Result<()> {
        if self.selection_exhausted() {
            return Ok(());
        }
        match flow {
            Flow::PageBreak => {
                self.keep_empty_page = true;
                self.page_break()
            }
            Flow::Line(line) => self.place_line(line),
            Flow::Image(image) => {
                self.started = true;
                if !self.current.is_empty() && self.cursor + image.h > self.body_height() {
                    self.page_break()?;
                    if self.selection_exhausted() {
                        return Ok(());
                    }
                }
                let mut image = image;
                image.y += self.geometry.top + self.cursor;
                self.cursor += image.h;
                self.push_item(Item::Image(image))?;
                Ok(())
            }
            Flow::Block { items, height } => {
                self.started = true;
                if !self.current.is_empty() && self.cursor + height > self.body_height() {
                    self.page_break()?;
                    if self.selection_exhausted() {
                        return Ok(());
                    }
                }
                let dy = self.geometry.top + self.cursor;
                for item in &items {
                    self.push_item(offset_item(item, 0.0, dy))?;
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
                Flow::TableRow(row) if row.header => {
                    let mut row = row.clone();
                    // The first placement of the row already painted the frames.
                    row.page_frames.clear();
                    Some(row)
                }
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
        if self.selection_exhausted() {
            return Ok(());
        }
        self.started = true;
        if !self.current.is_empty() && self.cursor + row.height > self.body_height() {
            self.page_break()?;
            if self.selection_exhausted() {
                return Ok(());
            }
            self.repeat_table_headers()?;
        }
        self.place_row_items(row)
    }

    /// Places a row's items at the current cursor.
    fn place_row_items(&mut self, row: &TableRowFlow) -> Result<()> {
        let dy = self.geometry.top + self.cursor;
        for item in &row.items {
            self.push_item(offset_item(item, 0.0, dy))?;
        }
        for item in &row.page_frames {
            self.push_item(item.clone())?;
        }
        self.cursor += row.height;
        Ok(())
    }

    /// Re-emits the header rows at the top of a continued table page.
    fn repeat_table_headers(&mut self) -> Result<()> {
        let headers = self.table_headers.clone();
        for header in headers {
            if !self.current.is_empty() && self.cursor + header.height > self.body_height() {
                break;
            }
            self.place_row_items(&header)?;
        }
        Ok(())
    }

    fn place_line(&mut self, mut line: TextLine) -> Result<()> {
        if self.selection_exhausted() {
            return Ok(());
        }
        // An empty paragraph at the very top must still produce a page.
        self.started = true;
        if self.current.is_empty() {
            self.reserve_line_footnotes(&line);
        } else {
            let extra = self.prospective_footnote_extra(&line);
            if self.cursor + line.height + extra > self.body_height() {
                self.page_break()?;
                if self.selection_exhausted() {
                    return Ok(());
                }
            }
            self.reserve_line_footnotes(&line);
        }
        self.resolve_fields(&mut line);
        line.offset(0.0, self.geometry.top + self.cursor);
        let graphics = std::mem::take(&mut line.graphics);
        let (backs, rest): (Vec<_>, Vec<_>) = graphics
            .into_iter()
            .partition(|item| matches!(item, Item::Rect(_)));
        for item in backs {
            self.push_item(item)?;
        }
        for item in line.items {
            self.push_item(Item::Text(item))?;
        }
        for item in rest {
            self.push_item(item)?;
        }
        self.cursor += line.height;
        Ok(())
    }

    /// Substitutes computed-field placeholders with their page-dependent value.
    fn resolve_fields(&mut self, line: &mut TextLine) {
        if !line.items.iter().any(|item| item.field.is_some()) {
            return;
        }
        let page_ordinal = self.pages.len() + 1;
        let env = crate::fields::FieldEnv {
            page_number: usize::try_from(self.page_start.saturating_sub(1))
                .unwrap_or(0)
                .saturating_add(page_ordinal),
            page_count: self.total_pages.max(1),
            section_index: self.section_index + 1,
            section_pages: self
                .assumed_section_pages
                .get(self.section_index)
                .copied()
                .unwrap_or(self.total_pages)
                .max(1),
            page_format: self.page_format,
        };
        for item in &mut line.items {
            let Some(marker) = item.field else {
                continue;
            };
            let (value, format) = env.resolve(marker);
            let text = format.format(value);
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
        if self.selection_exhausted() {
            return Ok(());
        }
        if !self.pending.is_empty() {
            self.page_break()?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(Layout, Vec<(f64, f64)>)> {
        if self.started && !self.truncated {
            self.flush()?;
        }
        if self.pages.is_empty() && !self.selection_exhausted() {
            self.pages.push(PlacedPage {
                width_px: self.geometry.width,
                height_px: self.geometry.height,
                items: Vec::new(),
                section_index: self.section_index,
            });
            self.page_regions.push(self.current_region);
        }
        Ok((
            Layout {
                pages: self.pages,
                anchors: self.anchors,
                warnings: self.ctx.take_warnings(),
            },
            self.page_regions,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{quantize_px, LayoutFingerprint, MAX_LAYOUT_PASSES};
    use crate::layout::PlacedPage;

    #[test]
    fn f13_convergence_bound_is_eight_passes() {
        assert_eq!(MAX_LAYOUT_PASSES, 8);
    }

    #[test]
    fn f13_fingerprint_includes_region_size_not_only_page_count() {
        let page = PlacedPage {
            width_px: 100.0,
            height_px: 100.0,
            items: Vec::new(),
            section_index: 0,
        };
        let pages = [page.clone(), page];
        let a = LayoutFingerprint::new(&pages, &[(10.0, 4.0), (10.0, 4.0)]);
        let b = LayoutFingerprint::new(&pages, &[(10.0, 4.0), (10.0, 20.0)]);
        assert_eq!(a.pages, b.pages);
        assert_ne!(
            a, b,
            "inverse: matching page count with a taller footer must not look converged"
        );
        assert_eq!(quantize_px(1.2344), 1234);
    }
}
