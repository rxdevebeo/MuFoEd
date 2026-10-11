//! Pagination: section geometry, page breaks, footnotes and overflow
//! (`STAGE-4-TASK.md` §5.6, `STAGE-5-TASK.md` §5.4).

use std::collections::HashMap;

use strict_ooxml_core::error::Result;

use crate::error::RenderError;
use crate::layout::exclusions::PageExclusion;
use crate::layout::floating::{page_exclusion, PendingAnchor, WrapSide};
use crate::layout::paragraph::{layout_paragraph, paragraph_neighbors, ParagraphNeighbors};
use crate::layout::table::{
    frame_placement, framed_table_items, layout_blocks_inline, layout_frame_contents, layout_table,
    offset_item, table_uniform_frame,
};
use crate::layout::{
    body_geometry_for as geometry_for, Flow, Geometry, Item, Layout, LayoutContext, LineItem,
    PlacedPage, TableRowFlow, TextLine,
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
    note_pages: Vec<(u32, usize)>,
    chapters: Vec<[Option<(u32, crate::notes::NumberFormat)>; 9]>,
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
            note_pages: Vec::new(),
            chapters: Vec::new(),
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
    let _mapping = crate::style::color_mapping(ctx.document);
    for warning in &ctx.font_programs.warnings {
        ctx.warn(warning.clone());
    }
    strict_ooxml_core::control::stage(strict_ooxml_core::control::Stage::Layout, 0);
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
        if !has_fields && !ctx.note_numbers.has_page_restart() && previous.is_none() {
            let mut layout = layout;
            sanitize_finite_coords(&mut layout);
            return Ok(layout);
        }
        assumed.pages = fingerprint.pages.max(1);
        assumed.section_pages.clone_from(&fingerprint.section_pages);
        ctx.note_numbers.update_page_numbers();
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
        Item::Group(group) => {
            group.matrix.iter().all(|value| value.is_finite())
                && group.bounds.iter().all(|value| value.is_finite())
                && group
                    .clip
                    .is_none_or(|clip| clip.iter().all(|value| value.is_finite()))
                && group.opacity.is_finite()
                && group
                    .clip_path
                    .as_ref()
                    .is_none_or(|path| item_coords_finite(&Item::Path(path.clone())))
                && group.items.iter().all(item_coords_finite)
        }
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
    ctx.note_numbers.begin_layout_pass();
    ctx.reset_frame_cursors();
    let sections = &ctx.document.sections;
    let runs = section_runs(&ctx.document.body.blocks, sections.len());
    let first_props = sections.first().map(|section| &section.properties);
    let geometry = geometry_for(
        first_props,
        ctx.options.scale,
        Some(ctx),
        ctx.document.settings.gutter_at_top,
    );
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
            let geometry = geometry_for(
                props,
                ctx.options.scale,
                Some(ctx),
                ctx.document.settings.gutter_at_top,
            );
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
        let width = paginator.column_width();
        let body_blocks = &ctx.document.body.blocks;
        let run_blocks = body_blocks.get(*start..*end).unwrap_or_default();
        paginator.body_lines = true;
        layout_blocks(ctx, run_blocks, width, &mut paginator, 0)?;
        paginator.body_lines = false;
        append_endnotes(ctx, &mut paginator, Some(index))?;
    }
    if !paginator.selection_exhausted() {
        append_endnotes(ctx, &mut paginator, None)?;
        paginator.flush_pending()?;
    }
    let mut has_fields = paginator.has_fields;
    // AUD-70: PAGE/NUMPAGES in headers/footers also force a second pass so
    // NUMPAGES sees the real page count.
    if !has_fields {
        has_fields = any_headers_footers_have_dynamic_fields(ctx, sections);
    }
    let (mut layout, regions) = paginator.finish()?;
    let mut headings = [None; 9];
    let mut chapter_numbers = Vec::with_capacity(layout.pages.len());
    for page in &layout.pages {
        for item in page.items.iter().flat_map(Item::leaves) {
            if let Item::Text(text) = item {
                if let Some((level, number, format)) = text.run.chapter_heading {
                    if let Some(heading) = headings.get_mut(usize::from(level)) {
                        *heading = Some((number, format));
                    }
                }
            }
        }
        chapter_numbers.push(headings);
    }
    ctx.chapter_numbers.replace(chapter_numbers);
    crate::layout::floating::resolve(ctx, &mut layout.pages, &layout.anchors, sections)?;
    crate::layout::sectionpage::apply(
        ctx,
        &mut layout.pages,
        &mut layout.line_marks,
        &mut layout.column_regions,
        sections,
    )?;
    crate::layout::sectionpage::orient_body(ctx, &mut layout.pages, sections);
    crate::layout::headerfooter::decorate_pages(
        ctx,
        &mut layout.pages,
        sections,
        assumed.pages.max(1),
        &assumed.section_pages,
    )?;
    // Mirrored margins move the whole page content; the page border is drawn
    // after, from the page edges.
    crate::layout::sectionpage::mirror_margins(ctx, &mut layout.pages, sections);
    crate::layout::pageborders::apply(ctx, &mut layout.pages, sections)?;
    let mut fingerprint = LayoutFingerprint::new(&layout.pages, &regions);
    fingerprint.note_pages = ctx.note_numbers.reference_pages();
    fingerprint.chapters = ctx.chapter_numbers.borrow().clone();
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
    let grid = paginator.geometry.grid_line_pitch;
    let _ = width;
    // The space between two paragraphs is the **larger** of the first's
    // `w:after` and the second's `w:before`, not their sum: `06-strict-math-display`
    // leaves 27 px of white between a body paragraph and the `Heading2` that
    // follows it, and 10.667 px of the 37 px we leave is the `after` we added on
    // top of the `before`. A page break ends the paragraph, so nothing pending
    // follows it onto the next page.
    let mut pending_after = 0.0f64;
    let mut index = 0;
    while let Some(block) = blocks.get(index) {
        ctx.current_section.set(paginator.section_index);
        if paginator.selection_exhausted() {
            return Ok(());
        }
        if let Block::Paragraph(para) = block {
            if let Some(frame) = compute_paragraph(ctx.document, para).frame {
                let mut end = index + 1;
                while let Some(Block::Paragraph(next)) = blocks.get(end) {
                    let computed = compute_paragraph(ctx.document, next);
                    if computed.page_break_before || computed.frame.as_ref() != Some(&frame) {
                        break;
                    }
                    end += 1;
                }
                let group = blocks.get(index..end).unwrap_or_default();
                place_frame_group(ctx, group, paginator)?;
                pending_after = 0.0;
                index = end;
                continue;
            }
        }
        // Each block is laid out for the column it starts in: columns may differ
        // in width (`w:equalWidth="0"`), and a continuous section break may have
        // changed the margins since the previous block.
        let left = paginator.geometry.left;
        let width = paginator.column_width();
        match block {
            Block::Paragraph(para) => {
                let neighbors = paragraph_neighbors(ctx.document, blocks, index, false);
                pending_after = place_body_paragraph(
                    ctx,
                    para,
                    left,
                    width,
                    grid,
                    paginator,
                    pending_after,
                    neighbors,
                )?;
            }
            Block::Table(table) => {
                let resolved = crate::table_style::resolve_table(ctx.document, table);
                let _numbering_aliases = crate::numbering::alias_table(table, &resolved);
                let table = &resolved;
                ctx.frame_anchor
                    .set(crate::layout::FrameAnchor::of(&paginator.host_geometry()));
                if table.props.positioning.is_some() {
                    place_floating_table(ctx, table, paginator, depth)?;
                    pending_after = 0.0;
                } else if let Some(frame) = table_uniform_frame(table) {
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
#[allow(clippy::too_many_arguments)]
fn place_body_paragraph(
    ctx: &LayoutContext<'_>,
    para: &strict_ooxml_wml::model::Paragraph,
    left: f64,
    width: f64,
    grid: Option<f64>,
    paginator: &mut Paginator<'_>,
    pending_after: f64,
    neighbors: ParagraphNeighbors,
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
    // Lines are laid out from the content left edge and painted in their
    // column: page-space wrap boxes move back by the column's offset.
    let column_dx = paginator.column_offset();
    let host_x = paginator.geometry.left + column_dx;
    let host_y_pred = paginator.geometry.top + paginator.cursor + gap;
    let local_exclusions: Vec<_> = paginator
        .wrap_exclusions
        .iter()
        .cloned()
        .map(|exclusion| exclusion.into_paragraph_local(host_y_pred, column_dx))
        .collect();
    let host_geometry = paginator.host_geometry();
    let flow = layout_paragraph(
        ctx,
        para,
        left,
        width,
        grid,
        None,
        &local_exclusions,
        Some((&host_geometry, host_x, host_y_pred)),
        neighbors,
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
                Flow::PageBreak | Flow::ColumnBreak => 0.0,
            })
            .sum();
        if !paginator.at_column_top() && paginator.cursor + total > paginator.body_height() {
            paginator.overflow_break()?;
            if paginator.selection_exhausted() {
                return Ok(0.0);
            }
        }
    }
    if paginator.selection_exhausted() {
        return Ok(0.0);
    }
    let widow_break = if flow.widow_control && !boundary_marker {
        paginator.widow_break(&flow.flows)
    } else {
        None
    };
    if widow_break == Some(0) {
        paginator.overflow_break()?;
        if paginator.selection_exhausted() {
            return Ok(0.0);
        }
    }
    // The paragraph's anchors hang off where its first flow lands, which an
    // overflow may have moved to another column or page.
    let expected = paginator.placement_here();
    paginator.last_placed = None;
    let mut host = None;
    for (index, item) in flow.flows.into_iter().enumerate() {
        if index > 0 && widow_break == Some(index) {
            paginator.overflow_break()?;
        }
        if boundary_marker {
            paginator.place_boundary_marker(item)?;
        } else {
            paginator.place(item)?;
        }
        if host.is_none() {
            host = paginator.last_placed;
        }
    }
    paginator.place_anchors(ctx, flow.anchors, host.unwrap_or(expected));
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
    let computed = compute_paragraph(ctx.document, first);
    let Some(frame) = computed.frame.as_ref() else {
        return Ok(());
    };
    let scale = ctx.options.scale;
    let (origin_x, mut origin_y, frame_width) = frame_placement(
        frame,
        &crate::layout::FrameAnchor::of(&paginator.host_geometry()),
        scale,
    );
    paginator.started = true;
    let page = paginator.pages.len();
    let resume = ctx.frame_resume(frame);
    let (mut items, mut anchors, height) =
        layout_frame_contents(ctx, blocks, origin_x, origin_y + resume, frame_width);
    let declared_height = frame
        .height
        .map_or(0.0, |value| crate::units::twips_to_px(value.value(), scale));
    let frame_height = if frame.height_rule.as_deref() == Some("exact") {
        declared_height
    } else {
        declared_height.max(resume + height)
    };
    let original_y = origin_y;
    if let Some((_, _, shift)) = paginator
        .frame_boxes
        .iter()
        .find(|(signature, _, _)| signature == frame)
    {
        origin_y += *shift;
    } else if computed.suppress_overlap {
        // Each collision moves the frame below a previously occupied box;
        // monotonic movement makes at most one pass per preceding frame.
        for _ in 0..paginator.frame_boxes.len() {
            let next = paginator
                .frame_boxes
                .iter()
                .filter(|(_, rect, _)| {
                    origin_x < rect[0] + rect[2]
                        && origin_x + frame_width > rect[0]
                        && origin_y < rect[1] + rect[3]
                        && origin_y + frame_height > rect[1]
                })
                .map(|(_, rect, _)| rect[1] + rect[3])
                .fold(origin_y, f64::max);
            if next <= origin_y {
                break;
            }
            origin_y = next;
        }
    }
    let shift = origin_y - original_y;
    if shift != 0.0 {
        for item in &mut items {
            *item = offset_item(item, 0.0, shift);
        }
        for (host_y, _) in &mut anchors {
            *host_y += shift;
        }
    }
    if let Some((_, rect, _)) = paginator
        .frame_boxes
        .iter_mut()
        .find(|(signature, _, _)| signature == frame)
    {
        rect[3] = frame_height;
    } else {
        paginator.frame_boxes.push((
            frame.clone(),
            [origin_x, origin_y, frame_width, frame_height],
            shift,
        ));
    }
    ctx.frame_advance(frame, height);
    if frame.wrap.as_deref() != Some("none") {
        let declared_height = frame
            .height
            .map_or(0.0, |value| crate::units::twips_to_px(value.value(), scale));
        let frame_height = if frame.height_rule.as_deref() == Some("exact") {
            declared_height
        } else {
            declared_height.max(resume + height)
        };
        let horizontal_gap = frame
            .h_space
            .map_or(0.0, |value| crate::units::twips_to_px(value.value(), scale));
        let vertical_gap = frame
            .v_space
            .map_or(0.0, |value| crate::units::twips_to_px(value.value(), scale));
        let (left, right) = if frame.wrap.as_deref() == Some("notBeside") {
            let (column_left, column_width) = paginator.host_geometry().active_column_box();
            (column_left, column_left + column_width)
        } else {
            (
                origin_x - horizontal_gap,
                origin_x + frame_width + horizontal_gap,
            )
        };
        paginator.wrap_exclusions.push(PageExclusion {
            left,
            right,
            top: origin_y - vertical_gap,
            bottom: origin_y + frame_height + vertical_gap,
            side: WrapSide::Both,
            contour: None,
        });
    }
    for (host_y, anchor) in anchors {
        paginator.anchors.push(PendingAnchor {
            page,
            host_x: origin_x,
            host_y,
            column: paginator.column_index,
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
        &crate::layout::FrameAnchor::of(&paginator.host_geometry()),
        scale,
    );
    paginator.started = true;
    for item in framed_table_items(ctx, table, origin_x, origin_y, frame_width, depth) {
        paginator.push_item(item)?;
    }
    Ok(())
}

/// Place a `tblpPr` table outside the body flow and register text clearance.
fn place_floating_table(
    ctx: &LayoutContext<'_>,
    table: &strict_ooxml_wml::model::Table,
    paginator: &mut Paginator<'_>,
    depth: u32,
) -> Result<()> {
    let Some(position) = &table.props.positioning else {
        return Ok(());
    };
    let geometry = paginator.host_geometry();
    let width = crate::layout::table::table_total_width(ctx, table, paginator.column_width());
    let mut floating = table.clone();
    floating.props.indent = None;
    floating.props.alignment = Some(strict_ooxml_wml::model::values::Justification::Start);
    let _numbering_aliases = crate::numbering::alias_table(table, &floating);
    let flows = layout_table(ctx, &floating, 0.0, paginator.column_width(), depth, true);
    let height: f64 = flows
        .iter()
        .map(|flow| match flow {
            Flow::TableRow(row) => row.height,
            Flow::Block { height, .. } => *height,
            Flow::Line(line) => line.height,
            _ => 0.0,
        })
        .sum();
    let (base_x, span_x) = match position.horz_anchor.as_deref() {
        Some("page") => (0.0, geometry.width),
        Some("text") => geometry.active_column_box(),
        _ => (geometry.left, geometry.content_width()),
    };
    let (base_y, span_y) = match position.vert_anchor.as_deref() {
        Some("page") => (0.0, geometry.height),
        Some("text") => (
            geometry.top + paginator.cursor,
            geometry.content_height() - paginator.cursor,
        ),
        _ => (geometry.top, geometry.content_height()),
    };
    let x = crate::layout::table::frame_origin(
        position.x,
        position.x_align.as_deref(),
        base_x,
        span_x,
        width,
        ctx.options.scale,
    );
    let mut y = crate::layout::table::frame_origin(
        position.y,
        position.y_align.as_deref(),
        base_y,
        span_y,
        height,
        ctx.options.scale,
    );
    let allow_overlap = table.props.allow_overlap.unwrap_or(true);
    for _ in 0..=paginator.floating_tables.len() {
        let next = paginator
            .floating_tables
            .iter()
            .filter(|(rect, allowed)| {
                (!allow_overlap || !allowed)
                    && x < rect[0] + rect[2]
                    && x + width > rect[0]
                    && y < rect[1] + rect[3]
                    && y + height > rect[1]
            })
            .map(|(rect, _)| rect[1] + rect[3])
            .fold(y, f64::max);
        if next <= y + 0.001 {
            break;
        }
        y = next;
    }
    paginator.started = true;
    let mut cursor = y;
    let mut fragment_top = y;
    let mut headers: Vec<TableRowFlow> = Vec::new();
    let distance = |value: Option<strict_ooxml_wml::model::values::Twips>| {
        value.map_or(0.0, |value| {
            crate::units::twips_to_px(value.value(), ctx.options.scale)
        })
    };
    let register = |paginator: &mut Paginator<'_>, top: f64, bottom: f64| {
        if bottom <= top {
            return;
        }
        paginator
            .floating_tables
            .push(([x, top, width, bottom - top], allow_overlap));
        paginator.wrap_exclusions.push(PageExclusion {
            left: x - distance(position.left_from_text),
            right: x + width + distance(position.right_from_text),
            top: top - distance(position.top_from_text),
            bottom: bottom + distance(position.bottom_from_text),
            side: WrapSide::Both,
            contour: None,
        });
    };
    for flow in flows {
        match flow {
            Flow::TableRow(row) => {
                let mut fresh_page = false;
                let mut start = 0.0;
                while start < row.height - 0.001 {
                    let available = paginator.geometry.top + paginator.body_height() - cursor;
                    let remaining = row.height - start;
                    let end = if remaining <= available + 0.001 {
                        row.height
                    } else {
                        row.split_points
                            .iter()
                            .copied()
                            .filter(|point| *point > start + 0.001 && *point <= start + available)
                            .next_back()
                            .unwrap_or(start)
                    };
                    if end <= start + 0.001
                        && cursor > paginator.geometry.top + 0.001
                        && !fresh_page
                    {
                        register(paginator, fragment_top, cursor);
                        paginator.page_break()?;
                        if paginator.selection_exhausted() {
                            return Ok(());
                        }
                        cursor = paginator.geometry.top;
                        fragment_top = cursor;
                        fresh_page = true;
                        for header in &headers {
                            if cursor + header.height
                                < paginator.geometry.top + paginator.body_height() - 1.0
                            {
                                for item in &header.items {
                                    for leaf in item.leaves() {
                                        if let Item::Text(text) = leaf {
                                            if let Some(id) = text.run.footnote_id {
                                                ctx.note_numbers.record_reference_page(
                                                    id,
                                                    paginator.pages.len(),
                                                );
                                                paginator.reserve_footnote(id);
                                            }
                                        }
                                    }
                                    paginator.push_item(offset_item(item, x, cursor))?;
                                }
                                for anchor in &header.anchors {
                                    paginator.place_anchors(
                                        ctx,
                                        vec![anchor.anchor.clone()],
                                        PlacedAt {
                                            page: paginator.pages.len(),
                                            column: paginator.column_index,
                                            x: x + anchor.x,
                                            y: cursor + anchor.y,
                                            run: None,
                                        },
                                    );
                                }
                                for frame in &header.page_frames {
                                    paginator.push_item(frame.clone())?;
                                }
                                cursor += header.height;
                            }
                        }
                        continue;
                    }
                    let end = if end > start + 0.001 {
                        end
                    } else {
                        row.split_points
                            .iter()
                            .copied()
                            .find(|point| *point > start + 0.001)
                            .unwrap_or(row.height)
                    };
                    let fragment = crate::layout::table::row_fragment(&row, start, end);
                    for item in &fragment.items {
                        for leaf in item.leaves() {
                            if let Item::Text(text) = leaf {
                                if let Some(id) = text.run.footnote_id {
                                    ctx.note_numbers
                                        .record_reference_page(id, paginator.pages.len());
                                    paginator.reserve_footnote(id);
                                }
                            }
                        }
                    }
                    let factor = ((paginator.geometry.top + paginator.body_height() - cursor)
                        .max(1.0)
                        / fragment.height.max(1.0))
                    .min(1.0);
                    if factor < 1.0 {
                        ctx.warn("render.floating-table.oversize: indivisible row fragment scaled to page".to_owned());
                    }
                    let mut items = fragment.items;
                    if factor < 1.0 {
                        items = crate::paint::graphics::transform_items(
                            items,
                            [factor, 0.0, 0.0, factor, 0.0, 0.0],
                        );
                    }
                    for item in &items {
                        for leaf in item.leaves() {
                            if let Item::Text(text) = leaf {
                                if let Some(id) = text.run.footnote_id {
                                    ctx.note_numbers
                                        .record_reference_page(id, paginator.pages.len());
                                    paginator.reserve_footnote(id);
                                }
                            }
                        }
                        paginator.push_item(offset_item(item, x, cursor))?;
                    }
                    for anchor in fragment.anchors {
                        paginator.place_anchors(
                            ctx,
                            vec![anchor.anchor],
                            PlacedAt {
                                page: paginator.pages.len(),
                                column: paginator.column_index,
                                x: x + anchor.x * factor,
                                y: cursor + anchor.y * factor,
                                run: None,
                            },
                        );
                    }
                    for frame in fragment.page_frames {
                        paginator.push_item(frame)?;
                    }
                    cursor += fragment.height * factor;
                    start = end;
                    fresh_page = false;
                }
                if row.header {
                    headers.push(row.clone());
                }
            }
            Flow::Block { items, height } => {
                for item in items {
                    paginator.push_item(offset_item(&item, x, cursor))?;
                }
                cursor += height;
            }
            _ => {}
        }
    }
    register(paginator, fragment_top, cursor);
    Ok(())
}

/// Lays out the referenced endnotes at the end of the document.
fn append_endnotes(
    ctx: &LayoutContext<'_>,
    paginator: &mut Paginator<'_>,
    section: Option<usize>,
) -> Result<()> {
    let ids = ctx.note_numbers.endnotes_at(section);
    if ids.is_empty() {
        return Ok(());
    }
    // Endnotes continue in the body's columns: the separator and the notes
    // take the width of the column they start in.
    let left = paginator.geometry.left;
    let width = paginator.column_width();
    let line = Item::Line(LineItem {
        x1: left,
        y1: ENDNOTE_SEPARATOR_GAP,
        x2: left + (width / 3.0).max(8.0),
        y2: ENDNOTE_SEPARATOR_GAP,
        color: "#000000".to_owned(),
        width: 1.0,
        dashed: false,
    });
    let mut properties = ctx.document.settings.endnote_properties.clone();
    if let Some(section) = ctx
        .document
        .sections
        .get(section.unwrap_or(paginator.section_index))
    {
        properties.merge_from(&section.properties.endnote_properties);
    }
    let separator = properties
        .separator_ids
        .iter()
        .filter_map(|id| ctx.document.endnotes.get(*id))
        .find(|note| note.kind == strict_ooxml_wml::model::NoteKind::Separator)
        .or_else(|| ctx.document.endnotes.separator());
    if let Some(separator) = separator {
        append_note_blocks(ctx, &separator.blocks, left, width, "", paginator, 1)?;
    } else {
        paginator.place(Flow::Block {
            items: vec![line],
            height: ENDNOTE_SEPARATOR_HEIGHT,
        })?;
    }
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
    for (index, block) in blocks.iter().enumerate() {
        match block {
            Block::Paragraph(para) => {
                let neighbors = paragraph_neighbors(ctx.document, blocks, index, true);
                let space_before = pt_to_px(
                    compute_paragraph(ctx.document, para).space_before_pt,
                    ctx.options.scale,
                );
                let column_dx = paginator.column_offset();
                let host_x = paginator.geometry.left + column_dx;
                let host_y_pred = paginator.geometry.top + paginator.cursor + space_before;
                let local_exclusions: Vec<_> = paginator
                    .wrap_exclusions
                    .iter()
                    .cloned()
                    .map(|exclusion| exclusion.into_paragraph_local(host_y_pred, column_dx))
                    .collect();
                let host_geometry = paginator.host_geometry();
                let flow = layout_paragraph(
                    ctx,
                    para,
                    paginator.geometry.left,
                    paginator.column_width(),
                    grid,
                    Some(marker),
                    &local_exclusions,
                    Some((&host_geometry, host_x, host_y_pred)),
                    neighbors,
                );
                paginator.add_vspace(flow.space_before);
                let expected = paginator.placement_here();
                paginator.last_placed = None;
                let mut host = None;
                for item in flow.flows {
                    paginator.place(item)?;
                    if host.is_none() {
                        host = paginator.last_placed;
                    }
                }
                paginator.place_anchors(ctx, flow.anchors, host.unwrap_or(expected));
                paginator.add_vspace(flow.space_after);
            }
            Block::Table(table) => {
                ctx.frame_anchor
                    .set(crate::layout::FrameAnchor::of(&paginator.host_geometry()));
                let (left, width) = (paginator.geometry.left, paginator.column_width());
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
    /// Active body column on this page.
    column_index: usize,
    /// First y position of columns in the active continuous section.
    column_start: f64,
    /// Furthest cursor reached in each column, used when a continuous section
    /// switches back to a wider body frame.
    column_bottoms: Vec<f64>,
    /// Keep the next flushed empty page (explicit break / section break, AUD-75).
    keep_empty_page: bool,
    /// `PageSelection::Range` already produced `end` pages; further content is dropped.
    truncated: bool,
    /// Geometry deferred by a `continuous` section break until the next page.
    pending_geometry: Option<(Geometry, usize)>,
    /// Footnote ids reserved in each body column of the current page, in
    /// order. Footnotes follow the section's columns: each column keeps its own
    /// area at the foot of the page (Word's "match section layout").
    column_notes: Vec<Vec<u32>>,
    /// Footnote ids deferred to the next page (continuation).
    pending: Vec<u32>,
    /// Sum of each column's footnote heights, excluding the separator.
    column_notes_height: Vec<f64>,
    /// Whether the first column's footnote area continues from the previous page.
    continuation: bool,
    /// Laid-out footnotes by id and laid-out width (`width_key`): items from
    /// `x = 0`, `y = 0`, and the note's height.
    note_cache: HashMap<(u32, i64), (Vec<Item>, f64)>,
    note_consumed: HashMap<u32, f64>,
    /// Keep continuation slices in one coordinate system across unequal columns.
    note_layout_widths: HashMap<u32, i64>,
    note_slices: HashMap<u32, (f64, f64)>,
    note_scales: HashMap<u32, f64>,
    /// Header rows of the table currently being placed (`w:tblHeader`).
    table_headers: Vec<TableRowFlow>,
    /// Anchored (floating) objects recorded during pagination.
    anchors: Vec<PendingAnchor>,
    /// Active Square/TopAndBottom exclusions on the current page (page coords).
    wrap_exclusions: Vec<PageExclusion>,
    floating_tables: Vec<([f64; 4], bool)>,
    frame_boxes: Vec<(
        strict_ooxml_wml::model::props::FrameProperties,
        [f64; 4],
        f64,
    )>,
    /// Active header/footer heights used for the current page.
    current_region: (f64, f64),
    /// Per-page active header/footer heights (F13 fingerprint).
    page_regions: Vec<(f64, f64)>,
    /// Body lines placed so far, for `w:lnNumType`.
    line_marks: Vec<crate::layout::LineMark>,
    /// Whether lines being placed are body text (not endnotes).
    body_lines: bool,
    /// Whether anything has been placed in the current column.
    column_used: bool,
    /// Where the last flow was placed.
    last_placed: Option<PlacedAt>,
    /// Flows placed in the current multi-column area of this page, in order:
    /// what a continuous section break balances.
    region_runs: Vec<RegionRun>,
    /// Anchors hosted by a region run: `(anchor, run, wrap exclusion)`.
    region_anchors: Vec<(usize, usize, Option<usize>)>,
    /// Multi-column areas finished so far, for `w:sep`.
    column_regions: Vec<crate::layout::ColumnRegion>,
    separator_cache: HashMap<(i32, i64), (Vec<Item>, f64)>,
}

/// Where a flow was placed.
#[derive(Clone, Copy, Debug)]
struct PlacedAt {
    /// Page index.
    page: usize,
    /// Column (reading order).
    column: usize,
    /// Page x of the column's left edge.
    x: f64,
    /// Page y of the flow's top.
    y: f64,
    /// The region run it opened, when the page is in columns.
    run: Option<usize>,
}

/// One placed flow (a line, an image, a block, a table row) of a
/// multi-column area.
#[derive(Clone, Debug)]
struct RegionRun {
    /// Column it sits in.
    column: usize,
    /// Body cursor of its top.
    top: f64,
    /// Space between it and the flow above it in its column.
    gap: f64,
    /// Its height.
    height: f64,
    /// Its paint items in `Paginator::current`.
    items: std::ops::Range<usize>,
    /// Its line-number marks.
    marks: std::ops::Range<usize>,
    /// Footnotes its line reserved in its column.
    notes: Vec<u32>,
}

/// Cache key of a laid-out width: thousandths of a pixel.
fn width_key(width: f64) -> i64 {
    (width * 1000.0).round() as i64
}

impl<'a> Paginator<'a> {
    fn new(
        ctx: &'a LayoutContext<'a>,
        geometry: Geometry,
        assumed: &AssumedTotals,
        page_start: u32,
        page_format: crate::notes::NumberFormat,
    ) -> Self {
        // A footnote is set in the width of the column that references it, so
        // it is laid out once for every column width a section defines.
        let mut widths = vec![width_key(geometry.content_width())];
        for section in &ctx.document.sections {
            let section = geometry_for(
                Some(&section.properties),
                ctx.options.scale,
                None,
                ctx.document.settings.gutter_at_top,
            );
            widths.push(width_key(section.content_width()));
            widths.extend(
                (0..section.column_count()).map(|index| width_key(section.column_width(index))),
            );
        }
        widths.sort_unstable();
        widths.dedup();
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
                for key in &widths {
                    let mut items = Vec::new();
                    let mut y = 0.0;
                    let mut page_frames = Vec::new();
                    layout_blocks_inline(
                        ctx,
                        &note.blocks,
                        0.0,
                        *key as f64 / 1000.0,
                        &mut y,
                        &mut items,
                        0,
                        Some(&marker),
                        false,
                        &mut page_frames,
                    );
                    note_cache.insert((id, *key), (items, y));
                }
            }
        }
        let initial_column_count = geometry.column_count();
        let mut separator_cache = HashMap::new();
        for note in ctx.document.footnotes.iter().filter(|note| {
            matches!(
                note.kind,
                strict_ooxml_wml::model::NoteKind::Separator
                    | strict_ooxml_wml::model::NoteKind::ContinuationSeparator
                    | strict_ooxml_wml::model::NoteKind::ContinuationNotice
            )
        }) {
            for key in &widths {
                let mut items = Vec::new();
                let mut height = 0.0;
                let mut frames = Vec::new();
                layout_blocks_inline(
                    ctx,
                    &note.blocks,
                    0.0,
                    *key as f64 / 1000.0,
                    &mut height,
                    &mut items,
                    1,
                    None,
                    false,
                    &mut frames,
                );
                separator_cache.insert((note.id, *key), (items, height));
            }
        }
        let mut paginator = Self {
            separator_cache,
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
            column_index: 0,
            column_start: 0.0,
            column_bottoms: vec![0.0; initial_column_count],
            keep_empty_page: false,
            truncated: false,
            pending_geometry: None,
            column_notes: vec![Vec::new(); initial_column_count],
            pending: Vec::new(),
            column_notes_height: vec![0.0; initial_column_count],
            continuation: false,
            note_cache,
            note_consumed: HashMap::new(),
            note_layout_widths: HashMap::new(),
            note_slices: HashMap::new(),
            note_scales: HashMap::new(),
            table_headers: Vec::new(),
            anchors: Vec::new(),
            wrap_exclusions: Vec::new(),
            floating_tables: Vec::new(),
            frame_boxes: Vec::new(),
            current_region: (0.0, 0.0),
            page_regions: Vec::new(),
            line_marks: Vec::new(),
            body_lines: false,
            column_used: false,
            last_placed: None,
            region_runs: Vec::new(),
            region_anchors: Vec::new(),
            column_regions: Vec::new(),
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
                geometry: crate::layout::geometry_for(
                    self.ctx
                        .document
                        .sections
                        .get(self.section_index)
                        .map(|s| &s.properties),
                    self.ctx.options.scale,
                    None,
                    self.ctx.document.settings.gutter_at_top,
                ),
            },
        );
        self.geometry = self.base_geometry;
        use strict_ooxml_wml::model::values::TextDirection;
        let properties = self
            .ctx
            .document
            .sections
            .get(self.section_index)
            .map(|s| &s.properties);
        match properties.and_then(|s| s.text_direction) {
            Some(TextDirection::TbRl | TextDirection::TbRlV | TextDirection::TbLrV) => {
                self.geometry.left += reserve.extra_top;
                self.geometry.right += reserve.extra_bottom;
                self.geometry.columns = crate::layout::ColumnLayout::from_columns(
                    properties.and_then(|s| s.columns.as_ref()),
                    properties.is_some_and(|s| s.bidi),
                    self.geometry.content_width(),
                    self.ctx.options.scale,
                );
            }
            Some(TextDirection::BtLr) => {
                self.geometry.right += reserve.extra_top;
                self.geometry.left += reserve.extra_bottom;
                self.geometry.columns = crate::layout::ColumnLayout::from_columns(
                    properties.and_then(|s| s.columns.as_ref()),
                    properties.is_some_and(|s| s.bidi),
                    self.geometry.content_width(),
                    self.ctx.options.scale,
                );
            }
            _ => {
                self.geometry.top =
                    (self.base_geometry.top + reserve.extra_top).min(self.geometry.height * 0.75);
                self.geometry.bottom = (self.base_geometry.bottom + reserve.extra_bottom)
                    .min(self.geometry.height - self.geometry.top);
            }
        }
        self.current_region = (reserve.header_height, reserve.footer_height);
    }

    /// Applies a section break before laying out the next section's body (AUD-74).
    fn apply_section_break(
        &mut self,
        break_type: Option<SectionType>,
        geometry: Geometry,
        section_index: usize,
    ) -> Result<()> {
        let mut break_type = break_type.unwrap_or(SectionType::NextPage);
        let direction = |index: usize| {
            self.ctx
                .document
                .sections
                .get(index)
                .and_then(|section| section.properties.text_direction)
                .unwrap_or(strict_ooxml_wml::model::values::TextDirection::LrTb)
        };
        if matches!(
            break_type,
            SectionType::Continuous | SectionType::NextColumn
        ) && direction(self.section_index) != direction(section_index)
        {
            self.ctx.warn(
                "render.section.text-direction: direction change starts a new page".to_owned(),
            );
            break_type = SectionType::NextPage;
        }
        match break_type {
            SectionType::Continuous => {
                // Same page; the page-level properties apply from the next page.
                self.pending_geometry = Some((geometry, section_index));
                self.activate_columns(geometry);
                Ok(())
            }
            SectionType::NextPage => {
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
                self.reset_columns_for_page();
                self.apply_page_regions();
                Ok(())
            }
            SectionType::NextColumn => {
                let next_column = self.column_index.saturating_add(1);
                let same_columns = geometry.column_count() == self.geometry.column_count();
                if same_columns && next_column < geometry.column_count() {
                    // ISO/IEC 29500-1 §17.18.77: the section begins in the next
                    // column of this page, at the top of the columns - not of the
                    // page, which a continuous section above may already fill.
                    self.record_column_bottom();
                    self.pending_geometry = Some((geometry, section_index));
                    self.take_column_margins(&geometry);
                    self.column_index = next_column;
                    self.cursor = self.column_start;
                    self.column_used = false;
                    Ok(())
                } else if !same_columns && self.started {
                    // A different column layout has no "next column" to begin in:
                    // it starts below the current columns, as after a continuous
                    // break.
                    self.pending_geometry = Some((geometry, section_index));
                    self.activate_columns(geometry);
                    Ok(())
                } else {
                    self.keep_empty_page = true;
                    self.page_break()?;
                    if self.selection_exhausted() {
                        return Ok(());
                    }
                    self.keep_empty_page = true;
                    self.base_geometry = geometry;
                    self.geometry = geometry;
                    self.section_index = section_index;
                    self.reset_columns_for_page();
                    self.apply_page_regions();
                    Ok(())
                }
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
                self.reset_columns_for_page();
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
                self.reset_columns_for_page();
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

    /// Height of column `column`'s footnote area (separator included).
    fn notes_area_height(&self, column: usize) -> f64 {
        if self.column_notes.get(column).is_none_or(Vec::is_empty) {
            0.0
        } else {
            self.separator_height(column)
                + self.column_notes_height.get(column).copied().unwrap_or(0.0)
                + FOOTNOTE_BOTTOM_PAD
        }
    }

    fn separator_height(&self, column: usize) -> f64 {
        let Some(id) = self.separator_id(column) else {
            return FOOTNOTE_SEPARATOR_HEIGHT;
        };
        self.separator_cache
            .get(&(id, width_key(self.geometry.column_width(column))))
            .map_or(FOOTNOTE_SEPARATOR_HEIGHT, |(_, height)| *height)
    }

    fn separator_id(&self, column: usize) -> Option<i32> {
        use strict_ooxml_wml::model::NoteKind;
        let kind = if self.continuation && column == 0 {
            NoteKind::ContinuationSeparator
        } else {
            NoteKind::Separator
        };
        self.note_definition_id(kind)
    }

    fn note_definition_id(&self, kind: strict_ooxml_wml::model::NoteKind) -> Option<i32> {
        let mut properties = self.ctx.document.settings.footnote_properties.clone();
        if let Some(section) = self.ctx.document.sections.get(self.section_index) {
            properties.merge_from(&section.properties.footnote_properties);
        }
        properties
            .separator_ids
            .iter()
            .filter_map(|id| self.ctx.document.footnotes.get(*id))
            .find(|note| note.kind == kind)
            .or_else(|| {
                self.ctx
                    .document
                    .footnotes
                    .iter()
                    .find(|note| note.kind == kind)
            })
            .map(|note| note.id)
    }

    /// Height of the current column's footnote area (separator included).
    fn reserved_height(&self) -> f64 {
        self.notes_area_height(self.column_index)
            + self.continuation_notice_height(self.column_index)
    }

    fn continuation_notice_height(&self, column: usize) -> f64 {
        if self.pending.is_empty() || self.column_notes.get(column).is_none_or(Vec::is_empty) {
            return 0.0;
        }
        self.note_definition_id(strict_ooxml_wml::model::NoteKind::ContinuationNotice)
            .and_then(|id| {
                self.separator_cache
                    .get(&(id, width_key(self.geometry.column_width(column))))
            })
            .map_or(0.0, |(_, height)| *height)
    }

    /// Body height available in the current column.
    fn body_height(&self) -> f64 {
        (self.geometry.content_height() - self.reserved_height()).max(1.0)
    }

    /// Width of the current column.
    fn column_width(&self) -> f64 {
        self.geometry.column_width(self.column_index)
    }

    /// How far right of the content left edge the current column starts.
    fn column_offset(&self) -> f64 {
        self.geometry.column_left(self.column_index) - self.geometry.left
    }

    /// The page geometry with the current column active: what positions a
    /// floating object or a frame relative to `column` / `text`.
    fn host_geometry(&self) -> Geometry {
        let mut geometry = self.geometry;
        geometry.active_column = self.column_index;
        geometry
    }

    /// Whether something already sits above the cursor in this column, so a
    /// flow that does not fit can move on rather than being forced in. The
    /// first column keeps the page test: whatever the page holds is above it.
    fn has_content_above(&self) -> bool {
        if self.column_index == 0 {
            !self.current.is_empty()
        } else {
            self.column_used
        }
    }

    /// Where the next flow would be placed.
    fn placement_here(&self) -> PlacedAt {
        PlacedAt {
            page: self.pages.len(),
            column: self.column_index,
            x: self.geometry.column_left(self.column_index),
            y: self.geometry.top + self.cursor,
            run: None,
        }
    }

    /// Records that a flow is about to be placed at the cursor. Returns the
    /// item and line-mark counts before it, for [`Self::end_flow`].
    fn begin_flow(&mut self) -> (usize, usize) {
        self.column_used = true;
        let mut here = self.placement_here();
        here.run = (self.geometry.column_count() > 1).then_some(self.region_runs.len());
        self.last_placed = Some(here);
        (self.current.len(), self.line_marks.len())
    }

    /// Records a placed flow of `height` ending at the cursor, in a
    /// multi-column area, for balancing.
    fn end_flow(&mut self, start: (usize, usize), items_end: usize, height: f64, notes: Vec<u32>) {
        if self.geometry.column_count() < 2 {
            return;
        }
        let column = self.column_index;
        let top = self.cursor - height;
        let previous_end = self
            .region_runs
            .iter()
            .rev()
            .find(|run| run.column == column)
            .map_or(self.column_start, |run| run.top + run.height);
        self.region_runs.push(RegionRun {
            column,
            top,
            gap: (top - previous_end).max(0.0),
            height,
            items: start.0..items_end,
            marks: start.1..self.line_marks.len(),
            notes,
        });
    }

    /// Registers a paragraph's anchors at `host`, where its first flow landed,
    /// with their wrap boxes when that is still the current page.
    fn place_anchors(
        &mut self,
        ctx: &LayoutContext<'_>,
        anchors: Vec<strict_ooxml_wml::model::drawing::AnchorDrawing>,
        host: PlacedAt,
    ) {
        for anchor in anchors {
            let mut geometry = self.geometry;
            geometry.active_column = host.column;
            let exclusion = if host.page == self.pages.len() {
                page_exclusion(ctx, &anchor, &geometry, host.x, host.y).map(|exclusion| {
                    self.wrap_exclusions.push(exclusion);
                    self.wrap_exclusions.len() - 1
                })
            } else {
                None
            };
            if let Some(run) = host.run.filter(|_| host.page == self.pages.len()) {
                self.region_anchors
                    .push((self.anchors.len(), run, exclusion));
            }
            self.anchors.push(PendingAnchor {
                page: host.page,
                host_x: host.x,
                host_y: host.y,
                column: host.column,
                anchor,
            });
        }
    }

    fn overflow_break(&mut self) -> Result<()> {
        if self.column_index + 1 < self.geometry.column_count() {
            // The wrap boxes are page-space and still stand beside the next
            // column; they are cleared with the page, not the column.
            self.record_column_bottom();
            self.column_index += 1;
            self.cursor = self.column_start;
            self.column_used = false;
            Ok(())
        } else {
            self.page_break()
        }
    }

    fn record_column_bottom(&mut self) {
        if let Some(bottom) = self.column_bottoms.get_mut(self.column_index) {
            *bottom = bottom.max(self.cursor);
        }
    }

    /// Takes the next section's left/right margins and columns on this page:
    /// a continuous (or next-column) break keeps the page's size, top and
    /// bottom, but the new section's text is set in its own frame.
    fn take_column_margins(&mut self, geometry: &Geometry) {
        for target in [&mut self.geometry, &mut self.base_geometry] {
            target.left = geometry.left;
            target.right = geometry.right;
            target.columns = geometry.columns;
        }
        let count = self.geometry.column_count();
        self.column_bottoms.resize(count, self.column_start);
        self.fit_note_columns();
    }

    /// Keeps one footnote list per column. Notes of columns that no longer
    /// exist join the first column.
    fn fit_note_columns(&mut self) {
        let count = self.geometry.column_count();
        if self.column_notes.len() > count {
            let extra: Vec<u32> = self.column_notes.drain(count..).flatten().collect();
            self.column_notes_height.truncate(count);
            if let Some(first) = self.column_notes.first_mut() {
                first.extend(extra);
            }
        }
        self.column_notes.resize(count, Vec::new());
        self.column_notes_height.resize(count, 0.0);
        for column in 0..count {
            let width = self.geometry.column_width(column);
            let mut height = 0.0;
            let ids = self.column_notes[column].clone();
            for id in ids {
                let available = self.geometry.content_height()
                    - self.separator_height(column)
                    - FOOTNOTE_BOTTOM_PAD
                    - 32.0
                    - height;
                if available < 1.0 && height > 0.0 {
                    if !self.pending.contains(&id) {
                        self.pending.push(id);
                    }
                    self.column_notes[column].retain(|note| *note != id);
                    self.note_slices.remove(&id);
                    self.note_scales.remove(&id);
                    continue;
                }
                let budget = available.max(1.0);
                if !self.note_slices.contains_key(&id) {
                    if let Some(slice) = self.note_slice(id, width, budget) {
                        self.note_scales
                            .insert(id, (budget / (slice.1 - slice.0).max(1.0)).min(1.0));
                        self.note_slices.insert(id, slice);
                    }
                }
                height += self.note_height_at(id, width);
                if let Some((_, end)) = self.note_slices.get(&id) {
                    if self
                        .note_layout(id, width)
                        .is_some_and(|(_, total)| *end < *total - 0.01)
                        && !self.pending.contains(&id)
                    {
                        self.pending.push(id);
                    }
                }
            }
            self.column_notes_height[column] = height;
        }
    }

    /// Starts the next section's columns below the current ones (a continuous
    /// break): the columns above are balanced first, as Word does.
    fn activate_columns(&mut self, geometry: Geometry) {
        let previous_count = self.geometry.column_count();
        if previous_count > 1 {
            self.balance_region();
        }
        self.close_region();
        let section_start = self
            .column_bottoms
            .iter()
            .copied()
            .fold(self.cursor, f64::max);
        let next_count = geometry.column_count();
        self.take_column_margins(&geometry);
        if previous_count > 1 || next_count > 1 {
            self.column_index = 0;
            self.column_start = section_start;
            self.cursor = section_start;
            self.column_bottoms = vec![section_start; next_count];
            self.column_used = false;
        }
        if previous_count != next_count {
            // Footnotes reserved under the old columns stay on the page, in the
            // first column of the new ones.
            let notes: Vec<u32> = self.column_notes.drain(..).flatten().collect();
            self.column_notes = vec![Vec::new(); next_count];
            self.column_notes[0] = notes;
            self.fit_note_columns();
        }
        self.region_runs.clear();
        self.region_anchors.clear();
    }

    /// Records the current multi-column area of this page for its `w:sep`
    /// lines.
    fn close_region(&mut self) {
        let columns = self.geometry.columns;
        if columns.count() < 2 || !columns.separator {
            return;
        }
        let bottom = self
            .region_runs
            .iter()
            .map(|run| run.top + run.height)
            .fold(f64::NEG_INFINITY, f64::max);
        if !bottom.is_finite() || bottom <= self.column_start + 1e-6 {
            return;
        }
        self.column_regions.push(crate::layout::ColumnRegion {
            page: self.pages.len(),
            left: self.geometry.left,
            top: self.geometry.top + self.column_start,
            bottom: self.geometry.top + bottom,
            columns,
        });
    }

    /// Balances the columns of the current area of this page: the lowest
    /// column height that still holds its flows, in their order and with their
    /// footnotes, then moves every flow (its items, line marks, footnotes,
    /// anchors and their wrap boxes) to its new place.
    fn balance_region(&mut self) {
        let count = self.geometry.column_count();
        if count < 2 || self.region_runs.len() < 2 {
            return;
        }
        self.fit_note_columns();
        let start = self.column_start;
        let content = self.geometry.content_height();
        let in_runs: std::collections::HashSet<u32> = self
            .region_runs
            .iter()
            .flat_map(|run| run.notes.iter().copied())
            .collect();
        let fixed: Vec<Vec<u32>> = self
            .column_notes
            .iter()
            .map(|notes| {
                notes
                    .iter()
                    .copied()
                    .filter(|id| !in_runs.contains(id))
                    .collect()
            })
            .collect();
        let widths: Vec<f64> = (0..count)
            .map(|column| self.geometry.column_width(column))
            .collect();
        let note_sum = |ids: &[u32], column: usize| -> f64 {
            ids.iter()
                .map(|id| self.note_height_at(*id, widths[column]))
                .sum()
        };
        let area = |notes: usize, height: f64| {
            if notes == 0 {
                0.0
            } else {
                FOOTNOTE_SEPARATOR_HEIGHT + height + FOOTNOTE_BOTTOM_PAD
            }
        };
        let runs = &self.region_runs;
        let fill = |target: f64| -> Option<Vec<(usize, f64)>> {
            let mut column = 0usize;
            let mut y = 0.0f64;
            let mut notes = (fixed[0].len(), note_sum(&fixed[0], 0));
            let mut placed = Vec::with_capacity(runs.len());
            for run in runs {
                let mut run_notes = note_sum(&run.notes, column);
                let need = if y > 0.0 {
                    run.gap + run.height
                } else {
                    run.height
                };
                let with = area(notes.0 + run.notes.len(), notes.1 + run_notes);
                if y > 0.0 && (y + need > target + 1e-6 || start + y + need + with > content + 1e-6)
                {
                    column += 1;
                    if column >= count {
                        return None;
                    }
                    y = 0.0;
                    notes = (fixed[column].len(), note_sum(&fixed[column], column));
                    run_notes = note_sum(&run.notes, column);
                }
                let top = if y > 0.0 { y + run.gap } else { 0.0 };
                placed.push((column, top));
                y = top + run.height;
                notes = (notes.0 + run.notes.len(), notes.1 + run_notes);
            }
            Some(placed)
        };
        let current_height = self
            .region_runs
            .iter()
            .map(|run| run.top + run.height - start)
            .fold(0.0f64, f64::max);
        let tallest = self
            .region_runs
            .iter()
            .map(|run| run.height)
            .fold(0.0f64, f64::max);
        if fill(current_height).is_none() {
            return;
        }
        let (mut low, mut high) = (tallest, current_height.max(tallest));
        for _ in 0..40 {
            let middle = (low + high) / 2.0;
            if fill(middle).is_some() {
                high = middle;
            } else {
                low = middle;
            }
            if high - low < 0.01 {
                break;
            }
        }
        let Some(placed) = fill(high) else {
            return;
        };
        self.apply_balance(&placed, fixed);
    }

    /// Moves the region's runs to `placed` (`(column, top above start)` each).
    fn apply_balance(&mut self, placed: &[(usize, f64)], fixed: Vec<Vec<u32>>) {
        let start = self.column_start;
        let mut moves = Vec::with_capacity(placed.len());
        for (run, (column, top)) in self.region_runs.iter_mut().zip(placed) {
            let dx = self.geometry.column_left(*column) - self.geometry.column_left(run.column);
            let dy = start + top - run.top;
            moves.push((dx, dy));
            for index in run.items.clone() {
                if let Some(item) = self.current.get_mut(index) {
                    *item = offset_item(item, dx, dy);
                }
            }
            for index in run.marks.clone() {
                if let Some(mark) = self.line_marks.get_mut(index) {
                    mark.column_left += dx;
                    mark.baseline += dy;
                }
            }
            run.column = *column;
            run.top = start + top;
        }
        for (anchor, run, exclusion) in &self.region_anchors {
            let Some(&(dx, dy)) = moves.get(*run) else {
                continue;
            };
            if let Some(pending) = self.anchors.get_mut(*anchor) {
                pending.host_x += dx;
                pending.host_y += dy;
                pending.column = self.region_runs[*run].column;
            }
            if let Some(slot) = exclusion.and_then(|index| self.wrap_exclusions.get_mut(index)) {
                *slot = slot.shifted(dx, dy);
            }
        }
        let count = self.geometry.column_count();
        let mut notes = fixed;
        notes.resize(count, Vec::new());
        let mut bottoms = vec![start; count];
        for run in &self.region_runs {
            notes[run.column].extend(run.notes.iter().copied());
            bottoms[run.column] = bottoms[run.column].max(run.top + run.height);
        }
        self.column_notes = notes;
        self.fit_note_columns();
        self.column_bottoms = bottoms;
        self.column_index = self.region_runs.last().map_or(0, |run| run.column);
        self.cursor = self.column_bottoms[self.column_index];
    }

    fn reset_columns_for_page(&mut self) {
        self.column_index = 0;
        self.column_start = 0.0;
        self.column_used = false;
        self.column_bottoms = vec![0.0; self.geometry.column_count()];
        self.region_runs.clear();
        self.region_anchors.clear();
        self.fit_note_columns();
    }

    /// Height of footnote `id` set in the current column.
    fn note_height(&self, id: u32) -> f64 {
        self.note_height_at(id, self.column_width())
    }

    /// Height of footnote `id` set `width` px wide.
    fn note_height_at(&self, id: u32, width: f64) -> f64 {
        if let Some((start, end)) = self.note_slices.get(&id) {
            return (end - start) * self.note_scales.get(&id).copied().unwrap_or(1.0);
        }
        self.note_slice(
            id,
            width,
            self.geometry.content_height() - FOOTNOTE_SEPARATOR_HEIGHT - FOOTNOTE_BOTTOM_PAD - 32.0,
        )
        .map_or(0.0, |(start, end)| end - start)
    }

    fn note_slice(&self, id: u32, width: f64, budget: f64) -> Option<(f64, f64)> {
        let (items, height) = self.note_layout(id, width)?;
        let start = self
            .note_consumed
            .get(&id)
            .copied()
            .unwrap_or(0.0)
            .min(*height);
        if *height <= start + budget {
            return Some((start, *height));
        }
        let points = crate::layout::table::row_split_points(
            items
                .iter()
                .filter(|item| !matches!(item, Item::Rect(_) | Item::Line(_)))
                .map(|item| crate::layout::table::item_vertical_bounds(self.ctx, item))
                .collect(),
            *height,
        );
        let end = points
            .iter()
            .copied()
            .filter(|point| *point > start && *point <= start + budget)
            .max_by(f64::total_cmp)
            .or_else(|| points.iter().copied().find(|point| *point > start))
            .unwrap_or(*height);
        Some((start, end))
    }

    /// Footnote `id` laid out `width` px wide, or at the nearest width it was.
    fn note_layout(&self, id: u32, width: f64) -> Option<&(Vec<Item>, f64)> {
        let key = self
            .note_layout_widths
            .get(&id)
            .copied()
            .unwrap_or_else(|| width_key(width));
        self.note_cache.get(&(id, key)).or_else(|| {
            self.note_cache
                .iter()
                .filter(|((note, _), _)| *note == id)
                .min_by_key(|((_, laid), _)| (laid - key).abs())
                .map(|(_, layout)| layout)
        })
    }

    /// Whether footnote `id` already has a place on this page.
    fn has_note(&self, id: u32) -> bool {
        self.column_notes.iter().any(|notes| notes.contains(&id))
    }

    /// Whether this page has any footnote area.
    fn has_page_notes(&self) -> bool {
        self.column_notes.iter().any(|notes| !notes.is_empty())
    }

    /// Where `w:widowControl` moves the first page break of a paragraph.
    ///
    /// `Some(0)` moves the whole paragraph on when only its first line would
    /// stay behind (an orphan); `Some(k)` breaks before line `k` when only its
    /// last line would go over (a widow). `None` when the break, if any, can
    /// stay where the lines put it. Only paragraphs made of text lines are
    /// considered: an image or a formula block decides its own break.
    fn widow_break(&self, flows: &[Flow]) -> Option<usize> {
        let heights: Option<Vec<f64>> = flows
            .iter()
            .map(|flow| match flow {
                Flow::Line(line) => Some(line.height),
                _ => None,
            })
            .collect();
        let heights = heights?;
        let total = heights.len();
        if total < 2 {
            return None;
        }
        let body = self.body_height();
        let mut cursor = self.cursor;
        let mut fitting = 0;
        for height in &heights {
            let empty = fitting == 0 && !self.has_content_above();
            if !empty && cursor + height > body {
                break;
            }
            cursor += height;
            fitting += 1;
        }
        if fitting == total || fitting == 0 {
            return None;
        }
        let widow = total - fitting == 1;
        if fitting >= 3 && widow {
            return Some(fitting - 1);
        }
        // An orphan, or a widow that cannot be fixed without leaving one.
        if (fitting == 1 || (fitting == 2 && widow)) && !self.at_column_top() {
            return Some(0);
        }
        None
    }

    fn at_page_top(&self) -> bool {
        !self.started || (self.current.is_empty() && self.cursor <= 0.0)
    }

    fn at_column_top(&self) -> bool {
        !self.started || self.cursor <= self.column_start + 1e-9
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
        self.record_column_bottom();
    }

    fn check_page_capacity(&self) -> Result<()> {
        // Called once per page: the place to report layout progress and to
        // look at the clock.
        strict_ooxml_core::control::checkpoint_now()?;
        strict_ooxml_core::control::advance(self.pages.len() as u64);
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
        self.ctx.charge_items(item.leaves().count())?;
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
        self.close_region();
        self.flush()?;
        if self.selection_exhausted() {
            // `PageSelection::Range` asked only for pages through `end`.
            // Do not open the next page or keep laying out overflow.
            self.truncated = true;
            self.current.clear();
            self.started = true;
            self.keep_empty_page = false;
            self.pending.clear();
            self.column_notes.iter_mut().for_each(Vec::clear);
            self.column_notes_height
                .iter_mut()
                .for_each(|height| *height = 0.0);
            self.continuation = false;
            return Ok(());
        }
        self.ctx.reset_frame_cursors();
        self.take_pending_geometry();
        self.reset_columns_for_page();
        self.cursor = 0.0;
        self.started = true;
        self.current = Vec::new();
        self.wrap_exclusions.clear();
        self.frame_boxes.clear();
        self.floating_tables.clear();
        // Notes carried over continue at the foot of the first column.
        let carried = std::mem::take(&mut self.pending);
        self.continuation = !carried.is_empty();
        let count = self.geometry.column_count();
        self.column_notes = vec![Vec::new(); count];
        self.column_notes[0] = carried;
        self.column_notes_height = vec![0.0; count];
        self.fit_note_columns();
        self.apply_page_regions();
        Ok(())
    }

    /// Flushes the current page, appending its footnote area.
    fn flush(&mut self) -> Result<()> {
        let keep_empty = self.keep_empty_page;
        self.keep_empty_page = false;
        if self.current.is_empty()
            && !self.has_page_notes()
            && !self.pages.is_empty()
            && !keep_empty
        {
            // AUD-75: skip only when the page was not opened by an explicit
            // page/section break.
            return Ok(());
        }
        if self.has_page_notes() {
            let items = self.footnote_area_items();
            self.ctx
                .charge_items(items.iter().map(|item| item.leaves().count()).sum())?;
            self.current.extend(items);
        }
        for (id, (_, end)) in self.note_slices.drain() {
            self.note_consumed.insert(id, end);
        }
        self.note_scales.clear();
        self.pages.push(PlacedPage {
            width_px: self.geometry.width,
            height_px: self.geometry.height,
            items: std::mem::take(&mut self.current),
            section_index: self.section_index,
        });
        self.page_regions.push(self.current_region);
        Ok(())
    }

    /// Builds the absolute-positioned paint items of the current footnote
    /// areas: one at the foot of each column that has notes, as wide as it.
    fn footnote_area_items(&self) -> Vec<Item> {
        let bottom = self.geometry.height - self.geometry.bottom;
        let area_bottom = bottom - FOOTNOTE_BOTTOM_PAD;
        let mut items = Vec::new();
        for (column, ids) in self.column_notes.iter().enumerate() {
            if ids.is_empty() {
                continue;
            }
            let left = self.geometry.column_left(column);
            let width = self.geometry.column_width(column);
            let total: f64 = ids
                .iter()
                .map(|id| self.note_height_at(*id, width))
                .sum::<f64>()
                + self.continuation_notice_height(column);
            let separator_height = self.separator_height(column);
            let mut properties = self.ctx.document.settings.footnote_properties.clone();
            if let Some(section) = self.ctx.document.sections.get(self.section_index) {
                properties.merge_from(&section.properties.footnote_properties);
            }
            let area_top = if properties.position.as_deref() == Some("beneathText") {
                (self.geometry.top
                    + self
                        .column_bottoms
                        .get(column)
                        .copied()
                        .unwrap_or(self.cursor))
                .min(area_bottom - separator_height - total)
            } else {
                area_bottom - separator_height - total
            };
            let separator_length = if self.continuation && column == 0 {
                width
            } else {
                (width / 3.0).max(8.0)
            };
            if let Some((custom, _)) = self.separator_cache.get(&(
                self.separator_id(column).unwrap_or(i32::MIN),
                width_key(width),
            )) {
                items.extend(custom.iter().map(|item| offset_item(item, left, area_top)));
            } else {
                items.push(Item::Line(LineItem {
                    x1: left,
                    y1: area_top,
                    x2: left + separator_length,
                    y2: area_top,
                    color: "#000000".to_owned(),
                    width: 1.0,
                    dashed: false,
                }));
            }
            let mut y = area_top + separator_height;
            for id in ids {
                if let Some((note_items, height)) = self.note_layout(*id, width) {
                    let (start, end) = self.note_slices.get(id).copied().unwrap_or((0.0, *height));
                    let row = TableRowFlow {
                        anchors: Vec::new(),
                        items: note_items.clone(),
                        height: *height,
                        header: false,
                        page_frames: Vec::new(),
                        split_points: Vec::new(),
                    };
                    let fragment = crate::layout::table::row_fragment(&row, start, end);
                    let factor = self.note_scales.get(id).copied().unwrap_or(1.0);
                    let x_factor = self.note_layout_widths.get(id).map_or(1.0, |source| {
                        (width * 1000.0 / (*source as f64).max(1.0)).min(1.0)
                    });
                    let fragment_items = if factor < 1.0 || x_factor < 1.0 {
                        crate::paint::graphics::transform_items(
                            fragment.items,
                            [factor * x_factor, 0.0, 0.0, factor, 0.0, 0.0],
                        )
                    } else {
                        fragment.items
                    };
                    for item in fragment_items {
                        items.push(offset_item(&item, left, y));
                    }
                    y += (end - start) * factor;
                }
            }
            if !self.pending.is_empty() {
                if let Some((notice, _)) = self
                    .note_definition_id(strict_ooxml_wml::model::NoteKind::ContinuationNotice)
                    .and_then(|id| self.separator_cache.get(&(id, width_key(width))))
                {
                    items.extend(notice.iter().map(|item| offset_item(item, left, y)));
                }
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
            let fits = !self.has_content_above() || self.cursor + line.height <= self.body_height();
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
            Flow::ColumnBreak => self.overflow_break(),
            Flow::Line(line) => self.place_line(line),
            Flow::Image(image) => {
                self.started = true;
                if self.has_content_above() && self.cursor + image.h > self.body_height() {
                    self.overflow_break()?;
                    if self.selection_exhausted() {
                        return Ok(());
                    }
                }
                let start = self.begin_flow();
                let mut image = image;
                let height = image.h;
                image.x += self.column_offset();
                image.y += self.geometry.top + self.cursor;
                self.cursor += height;
                self.record_column_bottom();
                self.push_item(Item::Image(image))?;
                self.end_flow(start, self.current.len(), height, Vec::new());
                Ok(())
            }
            Flow::Block { items, height } => {
                self.started = true;
                if self.has_content_above() && self.cursor + height > self.body_height() {
                    self.overflow_break()?;
                    if self.selection_exhausted() {
                        return Ok(());
                    }
                }
                let start = self.begin_flow();
                let dy = self.geometry.top + self.cursor;
                let dx = self.column_offset();
                for item in &items {
                    self.push_item(offset_item(item, dx, dy))?;
                }
                self.cursor += height;
                self.record_column_bottom();
                self.end_flow(start, self.current.len(), height, Vec::new());
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
        if !row.split_points.is_empty() {
            let mut start = 0.0;
            let mut fresh_page = false;
            while start < row.height - 1e-9 {
                let remaining = row.height - start;
                let available = self.body_height() - self.cursor;
                if remaining <= available + 1e-9 {
                    let fragment = crate::layout::table::row_fragment(row, start, row.height);
                    return self.place_row_items(&fragment);
                }
                let cut = row
                    .split_points
                    .iter()
                    .copied()
                    .filter(|point| *point > start + 1e-9 && *point <= start + available)
                    .next_back();
                if let Some(end) = cut {
                    let fragment = crate::layout::table::row_fragment(row, start, end);
                    self.place_row_items(&fragment)?;
                    start = end;
                } else if fresh_page || !self.has_content_above() {
                    // An indivisible object taller than the page must still be
                    // painted once. Continue the rest of the row on a new page.
                    let end = row
                        .split_points
                        .iter()
                        .copied()
                        .find(|point| *point > start + 1e-9)
                        .unwrap_or(row.height);
                    let fragment = crate::layout::table::row_fragment(row, start, end);
                    self.place_row_items(&fragment)?;
                    start = end;
                }
                if start >= row.height - 1e-9 {
                    return Ok(());
                }
                self.overflow_break()?;
                if self.selection_exhausted() {
                    return Ok(());
                }
                self.repeat_table_headers()?;
                fresh_page = true;
            }
            return Ok(());
        }
        if self.has_content_above() && self.cursor + row.height > self.body_height() {
            self.overflow_break()?;
            if self.selection_exhausted() {
                return Ok(());
            }
            self.repeat_table_headers()?;
        }
        self.place_row_items(row)
    }

    /// Places a row's items at the current cursor.
    fn place_row_items(&mut self, row: &TableRowFlow) -> Result<()> {
        let mut ids: Vec<u32> = row
            .items
            .iter()
            .flat_map(Item::leaves)
            .filter_map(|item| match item {
                Item::Text(text) => text.run.footnote_id,
                _ => None,
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(*id));
        let extra = self.prospective_footnote_extra_ids(&ids);
        if !ids.is_empty()
            && self.has_content_above()
            && self.cursor + row.height + extra > self.body_height()
        {
            self.overflow_break()?;
            if self.selection_exhausted() {
                return Ok(());
            }
            // Header repetition is managed by the table flow; avoid recursion
            // if a repeated header itself contains a note reference.
            if !row.header {
                self.repeat_table_headers()?;
            }
        }
        let mut notes = Vec::new();
        for id in ids {
            self.ctx
                .note_numbers
                .record_reference_page(id, self.pages.len());
            if self.reserve_footnote(id) {
                notes.push(id);
            }
        }
        let start = self.begin_flow();
        let dy = self.geometry.top + self.cursor;
        let dx = self.column_offset();
        for item in &row.items {
            self.push_item(offset_item(item, dx, dy))?;
        }
        let items_end = self.current.len();
        for anchor in &row.anchors {
            self.place_anchors(
                self.ctx,
                vec![anchor.anchor.clone()],
                PlacedAt {
                    page: self.pages.len(),
                    column: self.column_index,
                    x: anchor.x + dx,
                    y: anchor.y + dy,
                    run: None,
                },
            );
        }
        for item in &row.page_frames {
            self.push_item(item.clone())?;
        }
        self.cursor += row.height;
        self.record_column_bottom();
        self.end_flow(start, items_end, row.height, notes);
        Ok(())
    }

    /// Re-emits the header rows at the top of a continued table page.
    fn repeat_table_headers(&mut self) -> Result<()> {
        let headers = self.table_headers.clone();
        for header in headers {
            if self.has_content_above() && self.cursor + header.height > self.body_height() {
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
        if self.has_content_above() {
            let extra = self.prospective_footnote_extra(&line);
            if self.cursor + line.height + extra > self.body_height() {
                self.overflow_break()?;
                if self.selection_exhausted() {
                    return Ok(());
                }
            }
        }
        let notes = self.reserve_line_footnotes(&line);
        for id in &line.footnote_refs {
            self.ctx
                .note_numbers
                .record_reference_page(*id, self.pages.len());
        }
        self.resolve_fields(&mut line);
        let start = self.begin_flow();
        let height = line.height;
        if self.body_lines && !line.suppress_line_number {
            self.line_marks.push(crate::layout::LineMark {
                page: self.pages.len(),
                section: self.section_index,
                column_left: self.geometry.column_left(self.column_index),
                baseline: self.geometry.top + self.cursor + line.ascent,
            });
        }
        line.offset(self.column_offset(), self.geometry.top + self.cursor);
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
        self.cursor += height;
        self.record_column_bottom();
        self.end_flow(start, self.current.len(), height, notes);
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
            chapter: crate::fields::chapter_on_page(self.ctx, self.pages.len(), self.section_index),
        };
        for item in &mut line.items {
            let Some(marker) = item.field else {
                continue;
            };
            let text = env.format(marker);
            item.width = self.ctx.measure(&text, &item.run);
            item.text = text;
            self.has_fields = true;
        }
    }

    /// Reserves the footnotes referenced on `line` in the current column;
    /// returns the ones it placed there.
    fn reserve_line_footnotes(&mut self, line: &TextLine) -> Vec<u32> {
        line.footnote_refs
            .clone()
            .into_iter()
            .filter(|id| self.reserve_footnote(*id))
            .collect()
    }

    /// Reserves one footnote in the current column, or defers it to the next
    /// page when it cannot fit. Returns whether it was placed here.
    fn reserve_footnote(&mut self, id: u32) -> bool {
        if self.has_note(id) || self.pending.contains(&id) {
            return false;
        }
        let column = self.column_index;
        self.fit_note_columns();
        let budget = self.geometry.content_height()
            - self.separator_height(column)
            - FOOTNOTE_BOTTOM_PAD
            - 32.0
            - self
                .note_definition_id(strict_ooxml_wml::model::NoteKind::ContinuationNotice)
                .and_then(|id| {
                    self.separator_cache
                        .get(&(id, width_key(self.column_width())))
                })
                .map_or(0.0, |(_, height)| *height)
            - self.column_notes_height[column];
        let Some((start, end)) = self.note_slice(id, self.column_width(), budget.max(1.0)) else {
            return false;
        };
        if end - start <= budget || self.column_notes[column].is_empty() {
            self.note_slices.insert(id, (start, end));
            let requested = width_key(self.column_width());
            let source_width = self
                .note_cache
                .keys()
                .filter(|(note, _)| *note == id)
                .min_by_key(|(_, width)| (*width - requested).abs())
                .map(|(_, width)| *width);
            if let Some(width) = source_width {
                self.note_layout_widths.entry(id).or_insert(width);
            }
            let factor = (budget.max(1.0) / (end - start).max(1.0)).min(1.0);
            if factor < 1.0 {
                self.ctx.warn(format!(
                    "render.footnote.oversize: indivisible fragment of note {id} scaled to fit"
                ));
            }
            self.note_scales.insert(id, factor);
            self.column_notes[column].push(id);
            self.column_notes_height[column] += (end - start) * factor;
            if self
                .note_layout(id, self.column_width())
                .is_some_and(|(_, height)| end < *height - 0.01)
            {
                self.pending.push(id);
            }
            true
        } else {
            self.pending.push(id);
            false
        }
    }

    /// Extra footnote height `line` would reserve in the current column.
    fn prospective_footnote_extra(&self, line: &TextLine) -> f64 {
        self.prospective_footnote_extra_ids(&line.footnote_refs)
    }

    fn prospective_footnote_extra_ids(&self, ids: &[u32]) -> f64 {
        let column = self.column_index;
        let mut notes = self.column_notes_height.get(column).copied().unwrap_or(0.0);
        let mut has_notes = self
            .column_notes
            .get(column)
            .is_some_and(|ids| !ids.is_empty());
        let mut extra = 0.0;
        for id in ids {
            if self.has_note(*id) || self.pending.contains(id) {
                continue;
            }
            let height = self.note_height(*id);
            if notes + height <= self.geometry.content_height() {
                if !has_notes {
                    extra += self.separator_height(column);
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
        while !self.pending.is_empty() && !self.selection_exhausted() {
            self.page_break()?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(Layout, Vec<(f64, f64)>)> {
        if self.started && !self.truncated {
            self.close_region();
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
                line_marks: self.line_marks,
                column_regions: self.column_regions,
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
