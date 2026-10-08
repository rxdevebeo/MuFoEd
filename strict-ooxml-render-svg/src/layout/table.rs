//! Table layout: grid, spans, vertical merges, borders, shading
//! (`STAGE-4-TASK.md` §5.5, `STAGE-5-TASK.md` §5.7–§5.8).

use strict_ooxml_wml::model::drawing::AnchorDrawing;
use strict_ooxml_wml::model::props::{CellProperties, FrameProperties};
use strict_ooxml_wml::model::values::{
    Border, BorderStyle, CellMargins, Twips, VerticalMerge, Width, WidthKind,
};
use strict_ooxml_wml::model::{Block, Table};

use crate::layout::paragraph::layout_paragraph;
use crate::layout::{Flow, Item, LayoutContext, LineItem, RectItem, TableRowFlow};
use crate::style::parse_color;
use crate::units::{eighths_point_to_px, twips_to_px};

/// Default horizontal cell margin in twips (`CT_TblCellMar` start/end).
const DEFAULT_CELL_MARGIN: i32 = 108;
/// Default vertical cell margin in twips (`CT_TblCellMar` top/bottom is `0`).
const DEFAULT_CELL_MARGIN_VERTICAL: i32 = 0;

/// Vertical merge state of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VState {
    /// No vertical merge.
    None,
    /// The cell starts a merged region (`w:vMerge w:val="restart"`).
    Restart,
    /// The cell continues the region above (`w:vMerge`/`continuous`).
    Continue,
}

impl VState {
    fn from_merge(merge: Option<VerticalMerge>) -> Self {
        match merge {
            Some(VerticalMerge::Restart) => Self::Restart,
            Some(VerticalMerge::Continue) => Self::Continue,
            None => Self::None,
        }
    }
}

/// A laid-out cell before row assembly.
struct RawCell {
    col: usize,
    span: usize,
    x: f64,
    width: f64,
    properties: CellProperties,
    items: Vec<Item>,
    /// Page-absolute frames taken out of the cell flow.
    page_frames: Vec<Item>,
    margin_top: f64,
    vmerge: VState,
}

/// A laid-out row before flow assembly.
struct RawRow {
    header: bool,
    height: f64,
    cells: Vec<RawCell>,
}

/// Lays out a table into per-row flows.
///
/// `depth` is the number of block containers already open; a cell's content is
/// one level deeper.
#[must_use]
pub(crate) fn layout_table(
    ctx: &LayoutContext<'_>,
    table: &Table,
    content_left: f64,
    content_width: f64,
    depth: u32,
    escape_frames: bool,
) -> Vec<Flow> {
    let scale = ctx.options.scale;
    let column_count = column_count(table);
    if column_count == 0 {
        return Vec::new();
    }
    if depth.saturating_add(1) > ctx.options.limits.max_block_nesting {
        return Vec::new();
    }
    let widths = column_widths(ctx, table, column_count, content_width);
    let widths = fit_to_box(widths, content_width);
    let total_width: f64 = widths.iter().sum();
    let table_x = table_x(ctx, table, content_left, content_width, total_width);
    // Page-anchored `framePr` tables are a drawing, not a Word cell box.
    // WPS paints SNP labels from the frame origin plus indent, without
    // `tblCellMar` start/end (D06: 10 twips ≈ 0.67–0.87 px).
    let skip_cell_h_margins = table_uniform_frame(table).is_some();

    let mut rows: Vec<RawRow> = Vec::with_capacity(table.rows.len());
    for (row_index, row) in table.rows.iter().enumerate() {
        let raw = layout_row(
            ctx,
            table,
            row_index,
            &widths,
            table_x,
            depth,
            escape_frames,
            skip_cell_h_margins,
        );
        let mut height = raw.height;
        if let Some(declared) = &row.props.height {
            height = height.max(
                declared
                    .value
                    .map_or(0.0, |value| twips_to_px(value.value(), scale)),
            );
        }
        if height <= 0.0 {
            height = 1.0;
        }
        rows.push(RawRow {
            header: row.props.header,
            height,
            cells: raw.cells,
        });
    }

    row_flows(ctx, table, &rows)
}

/// Paints the laid-out rows (shading, cell content, borders) into row flows.
///
/// Kept out of [`layout_table`], which recurses once per nested table: in a
/// debug build every local of a function occupies its frame, and the
/// temporaries built here (rect and row-flow values) used to be paid at every
/// nesting level of a 1 MiB stack (`hostile` `twelve_nested_tables_*`).
#[inline(never)]
fn row_flows(ctx: &LayoutContext<'_>, table: &Table, rows: &[RawRow]) -> Vec<Flow> {
    let mut flows = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let mut items = Vec::new();
        let mut page_frames = Vec::new();
        let row_end = row
            .cells
            .iter()
            .map(|cell| cell.col + cell.span)
            .max()
            .unwrap_or(0);
        for cell in &row.cells {
            let (source, paint_text, top_edge, bottom_edge) = match cell.vmerge {
                VState::Continue => {
                    let Some(source) = restart_cell(rows, row_index, cell) else {
                        continue;
                    };
                    (
                        source,
                        false,
                        false,
                        !merge_continues(rows, row_index, cell),
                    )
                }
                VState::Restart => (cell, true, true, !merge_continues(rows, row_index, cell)),
                VState::None => (cell, true, true, true),
            };
            if let Some(fill) = shading_fill(ctx, &source.properties) {
                items.push(Item::Rect(RectItem {
                    x: cell.x,
                    y: 0.0,
                    w: cell.width,
                    h: row.height,
                    fill: Some(fill),
                    stroke: None,
                    stroke_w: 0.0,
                }));
            }
            if paint_text {
                for item in &cell.items {
                    items.push(offset_item(item, 0.0, cell.margin_top));
                }
                page_frames.extend(cell.page_frames.iter().cloned());
            }
            // Edges shared with another cell take the table's insideH/insideV
            // when the cell does not set its own (TableGrid draws its grid
            // this way).
            let interior = [
                row_index > 0,
                row_index + 1 < rows.len(),
                cell.col > 0,
                cell.col + cell.span < row_end,
            ];
            push_borders(
                ctx,
                table,
                cell,
                &source.properties,
                row.height,
                (top_edge, bottom_edge),
                interior,
                &mut items,
            );
        }
        flows.push(Flow::TableRow(TableRowFlow {
            items,
            height: row.height,
            header: row.header,
            page_frames,
        }));
    }
    flows
}

/// Sums a slice of widths, or nothing at all when the range is out of bounds.
///
/// `<f64 as Sum>::sum` is a method call on the slice, so an absent range sums to
/// `0.0` and the arithmetic the rest of the layout does is unchanged - see the
/// comment at the call site.
fn sum_slice(range: Option<&[f64]>) -> f64 {
    range.map_or(0.0, |slice| slice.iter().sum())
}

/// One laid-out row: its cells and the height its content asked for.
struct LaidOutRow {
    cells: Vec<RawCell>,
    height: f64,
}

/// Lays out one row's cells against `widths`.
#[allow(clippy::too_many_arguments)]
fn layout_row(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row_index: usize,
    widths: &[f64],
    table_x: f64,
    depth: u32,
    escape_frames: bool,
    skip_cell_h_margins: bool,
) -> LaidOutRow {
    let Some(row) = table.rows.get(row_index) else {
        return LaidOutRow {
            cells: Vec::new(),
            height: 0.0,
        };
    };
    let row_columns: usize = row
        .cells
        .iter()
        .map(|cell| usize::from(cell.props.grid_span.unwrap_or(1)).max(1))
        .sum();
    if row_columns > table.grid.len() {
        // Malformed but renderable: the row claims more columns than the grid
        // declares. The extra columns are laid out rather than dropped, and the
        // page says so - a row silently narrower than it was authored is the
        // kind of loss a screenshot cannot show.
        ctx.warn(format!(
            "table row has {row_columns} grid columns, tblGrid declares {}",
            table.grid.len()
        ));
    }
    let mut column = 0usize;
    let mut cells = Vec::with_capacity(row.cells.len());
    let mut max_content: f64 = 0.0;
    for cell in &row.cells {
        let span = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
        let span = span.min(widths.len().saturating_sub(column).max(1));
        // `w:tblStylePr` conditions of this cell, as a mask: the resolved
        // properties are built in helpers, not in this recursive frame.
        let conditions = crate::table_style::cell_conditions(
            ctx.document,
            table,
            row_index,
            column,
            span,
            widths.len(),
        );
        let mut margins = effective_margins(ctx, table, row, cell, conditions);
        if skip_cell_h_margins {
            margins.0 = 0.0;
            margins.1 = 0.0;
        }
        // `get(..)` rather than a slice: `column` is the sum of the spans before
        // `w:gridSpan` of 65535 puts it far past the end, which is where the
        // old index range used to end the process (AUD-08).
        //
        // The slice the range denotes is summed, not the elements: `&[f64]`'s
        // `Sum` is pairwise and an iterator's is a linear fold, and the
        // converter's table detection turns on the last bit of a column
        // position. "Not a panic" is the fix; "a different number" would be a
        // second change nobody asked for.
        let spacing = cell_spacing_px(ctx, table, row);
        let x: f64 = table_x + sum_slice(widths.get(..column)) + spacing * column as f64;
        let end = column.saturating_add(span).min(widths.len());
        let width: f64 = sum_slice(widths.get(column..end)).max(f64::MIN_POSITIVE);
        let cell_content_width = (width - margins.0 - margins.1).max(1.0);
        let cell_style = crate::table_style::enter_cell(table, conditions);
        let (items, content_height, page_frames) = layout_cell_content(
            ctx,
            &cell.blocks,
            x + margins.0,
            cell_content_width,
            depth + 1,
            escape_frames,
        );
        drop(cell_style);
        let height = content_height + margins.2 + margins.3;
        max_content = max_content.max(height);
        push_raw_cell(
            ctx,
            table,
            &mut cells,
            cell,
            CellPlacement {
                col: column,
                span,
                x,
                width,
                margin_top: margins.2,
                conditions,
            },
            items,
            page_frames,
        );
        column += span;
    }
    LaidOutRow {
        cells,
        height: max_content,
    }
}

/// Where a laid-out cell sits in its row.
#[derive(Clone, Copy)]
struct CellPlacement {
    col: usize,
    span: usize,
    x: f64,
    width: f64,
    margin_top: f64,
    /// `w:tblStylePr` conditions that apply (see `table_style::cell_conditions`).
    conditions: u16,
}

/// Builds a [`RawCell`] in its own frame: the cell's properties are resolved
/// against the table style here, not in [`layout_row`], which recurses once per
/// nesting level (see [`row_flows`]).
#[inline(never)]
fn push_raw_cell(
    ctx: &LayoutContext<'_>,
    table: &Table,
    cells: &mut Vec<RawCell>,
    cell: &strict_ooxml_wml::model::TableCell,
    placement: CellPlacement,
    items: Vec<Item>,
    page_frames: Vec<Item>,
) {
    cells.push(RawCell {
        col: placement.col,
        span: placement.span,
        x: placement.x,
        width: placement.width,
        properties: crate::table_style::resolve_cell_properties(
            ctx.document,
            table,
            placement.conditions,
            &cell.props,
        ),
        items,
        page_frames,
        margin_top: placement.margin_top,
        vmerge: VState::from_merge(cell.props.vertical_merge),
    });
}

/// Scales `widths` down so their sum is at most `ceiling`.
///
/// `split_widths` hands every column the grid does not declare the *average* of
/// the columns it does, which is what AUD-08 prescribes and is right for the one
/// or two columns a producer misspells. It is not right for `w:gridSpan` of
/// 65535: 65 535 columns at the average of two is millions of pixels across, and
/// the acceptance criterion for the task is "no wider than the content box plus
/// one pixel". The table is the thing that has to fit, so the fitting happens
/// here, where the content box is known, and the per-column rule stays the one the
/// plan states.
fn fit_to_box(mut widths: Vec<f64>, ceiling: f64) -> Vec<f64> {
    let total: f64 = widths.iter().sum();
    if total > ceiling && total > 0.0 {
        let factor = ceiling / total;
        for width in &mut widths {
            *width *= factor;
        }
    }
    widths
}

/// The restart cell of the merge that `cell` continues, if that run is intact.
fn restart_cell<'a>(rows: &'a [RawRow], row: usize, cell: &RawCell) -> Option<&'a RawCell> {
    let mut index = row;
    while index > 0 {
        index -= 1;
        let candidate = rows
            .get(index)?
            .cells
            .iter()
            .find(|candidate| candidate.col == cell.col && candidate.span == cell.span)?;
        match candidate.vmerge {
            VState::Restart => return Some(candidate),
            VState::Continue => {}
            VState::None => return None,
        }
    }
    None
}

/// Whether the next row still belongs to this cell's vertical merge.
fn merge_continues(rows: &[RawRow], row: usize, cell: &RawCell) -> bool {
    rows.get(row + 1).is_some_and(|next| {
        next.cells.iter().any(|candidate| {
            candidate.vmerge == VState::Continue
                && candidate.col == cell.col
                && candidate.span == cell.span
        })
    })
}

/// Number of grid columns.
///
/// The explicit grid, or the widest row's spans, or whichever is larger.
///
/// Not the explicit grid alone: a row whose `w:gridSpan` values add up to more
/// columns than `w:tblGrid` declares is malformed markup, and the layout used to
/// take the grid's word for it and then index past the end of the widths it had
/// built from that same word. The row is the wider of the two claims, so the row
/// wins, and the disagreement is reported (AUD-08).
fn column_count(table: &Table) -> usize {
    let widest_row = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| usize::from(cell.props.grid_span.unwrap_or(1)))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    table.grid.len().max(widest_row)
}

/// Computes column widths in px.
///
/// `column_count` may exceed the declared grid (see [`column_count`]), and the
/// columns beyond it have no declared width to scale. They are given the width
/// the declared columns leave over, split evenly; if the declared columns already
/// fill the table, each of them takes the average of the declared ones instead,
/// so a table whose grid is narrower than its rows still ends up the width of the
/// content box rather than a row of zero-width cells hanging off the end.
fn column_widths(
    ctx: &LayoutContext<'_>,
    table: &Table,
    column_count: usize,
    content_width: f64,
) -> Vec<f64> {
    if column_count == 0 {
        return Vec::new();
    }
    let scale = ctx.options.scale;
    let total = table_total_width(ctx, table, content_width);
    let grid: Vec<f64> = table
        .grid
        .iter()
        .take(column_count)
        .map(|col| twips_to_px(col.width.map_or(0, Twips::value), scale))
        .collect();
    split_widths(total, &grid, column_count)
}

/// Divides `total` between `column_count` columns, given the widths `grid`
/// declares for the first `grid.len()` of them.
///
/// Separate from [`column_widths`] because this is arithmetic and the rest is
/// units: the arithmetic is where the AUD-08 defect lived and it is worth
/// testing without building a document, a font provider and a layout context
/// around it.
fn split_widths(total: f64, grid: &[f64], column_count: usize) -> Vec<f64> {
    if column_count == 0 {
        return Vec::new();
    }
    let grid_sum: f64 = grid.iter().sum();
    if grid.len() == column_count && grid_sum > 0.0 {
        let factor = total / grid_sum;
        return grid.iter().map(|width| width * factor).collect();
    }
    let declared = grid.len();
    if declared == column_count {
        let equal = total / column_count as f64;
        return vec![equal; column_count];
    }
    // Columns the grid does not declare. They take the width the declared
    // columns leave over; if the declared columns already fill the table, each
    // undeclared one takes their average instead, so a table whose grid is
    // narrower than its rows still ends up the width of the content box rather
    // than a row of zero-width cells hanging off the end.
    let missing = column_count.saturating_sub(declared);
    if missing == 0 {
        return vec![total / column_count as f64; column_count];
    }
    let leftover = (total - grid_sum).max(0.0);
    let fallback = if declared == 0 {
        total / column_count as f64
    } else {
        grid_sum / declared as f64
    };
    let each = if leftover > 0.0 {
        leftover / missing as f64
    } else {
        fallback
    };
    let mut widths: Vec<f64> = if grid_sum > 0.0 {
        // The declared columns keep their share of what the grid asked for; only
        // the leftover is handed out.
        let factor = (total / grid_sum).min(1.0);
        grid.iter().map(|width| width * factor).collect()
    } else {
        Vec::new()
    };
    widths.resize(column_count, each);
    widths
}

/// Total table width in px (clamped to the content box).
fn table_total_width(ctx: &LayoutContext<'_>, table: &Table, content_width: f64) -> f64 {
    let scale = ctx.options.scale;
    let width = match table.props.width {
        // A non-positive dxa (including the common `w:w="0"`) is not a 1 px
        // table. It means "no preferred width": the grid, then the container,
        // decide (A12).
        Some(Width {
            kind: WidthKind::Dxa,
            value: Some(value),
        }) if value > 0 => twips_to_px(value, scale),
        Some(Width {
            kind: WidthKind::Pct,
            value: Some(value),
        }) if value > 0 => content_width * f64::from(value) / 5000.0,
        _ => {
            // `i64`, not `i32`, and saturating: `w:gridCol/@w:w` is a twip count a
            // producer never checked, and a thousand of them overflow an `i32`
            // in a debug build - a panic, and a silently wrapped number in
            // release, which is worse than either (AUD-09). Saturating gives the
            // same answer in both profiles: wider than any page.
            let grid: i64 = table
                .grid
                .iter()
                .map(|col| i64::from(col.width.map_or(0, Twips::value)))
                .fold(0i64, i64::saturating_add);
            if grid > 0 {
                // The clamp to `i32` is the conversion G-2 asks for, and it is
                // lossless where it matters: `table_total_width` clamps the
                // result to the content box anyway, and a table a billion
                // twips wide is the content box wide.
                twips_to_px(i32::try_from(grid).unwrap_or(i32::MAX), scale)
            } else {
                content_width
            }
        }
    };
    width.min(content_width).max(0.0)
}

/// Horizontal offset of the table within the content box.
fn table_x(
    ctx: &LayoutContext<'_>,
    table: &Table,
    content_left: f64,
    content_width: f64,
    total_width: f64,
) -> f64 {
    let indent = table
        .props
        .indent
        .map_or(0.0, |value| twips_to_px(value.value(), ctx.options.scale));
    let mut x = content_left + indent;
    // Prefer the first row's `w:jc` when set (AUD-46), else the table's.
    let alignment = table
        .rows
        .first()
        .and_then(|row| row.props.alignment)
        .or(table.props.alignment);
    match alignment {
        Some(strict_ooxml_wml::model::values::Justification::Center) => {
            x = content_left + (content_width - total_width).max(0.0) / 2.0;
        }
        Some(strict_ooxml_wml::model::values::Justification::End) => {
            x = content_left + (content_width - total_width).max(0.0);
        }
        _ => {}
    }
    x
}

/// Cell gap from `w:tblCellSpacing` on the table or row (AUD-46), in px.
fn cell_spacing_px(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row: &strict_ooxml_wml::model::TableRow,
) -> f64 {
    let width = row
        .props
        .cell_spacing
        .as_ref()
        .or(table.props.cell_spacing.as_ref());
    let Some(width) = width else {
        return 0.0;
    };
    let Some(value) = width.value else {
        return 0.0;
    };
    match width.kind {
        strict_ooxml_wml::model::values::WidthKind::Dxa => twips_to_px(value, ctx.options.scale),
        _ => 0.0,
    }
}

/// Effective cell margins `(left, right, top, bottom)` in px.
///
/// The first source that sets any side wins: the cell, the row, the table, then
/// the table style (`tcMar` of the style and its `conditions` over the style's
/// `tblCellMar`).
#[inline(never)]
fn effective_margins(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row: &strict_ooxml_wml::model::TableRow,
    cell: &strict_ooxml_wml::model::TableCell,
    conditions: u16,
) -> (f64, f64, f64, f64) {
    let scale = ctx.options.scale;
    let source = if cell.props.margins != CellMargins::default() {
        cell.props.margins
    } else if row.props.cell_margins != CellMargins::default() {
        row.props.cell_margins
    } else if table.props.cell_margins != CellMargins::default() {
        table.props.cell_margins
    } else {
        crate::table_style::style_cell_margins(ctx.document, table, conditions)
    };
    let value = |margin: Option<Twips>, default: i32| {
        twips_to_px(margin.map_or(default, Twips::value), scale)
    };
    (
        value(source.start, DEFAULT_CELL_MARGIN),
        value(source.end, DEFAULT_CELL_MARGIN),
        value(source.top, DEFAULT_CELL_MARGIN_VERTICAL),
        value(source.bottom, DEFAULT_CELL_MARGIN_VERTICAL),
    )
}

/// Lays out cell block content, returning items (y relative), height, and any
/// page-absolute frames pulled out of the flow.
fn layout_cell_content(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    depth: u32,
    escape_frames: bool,
) -> (Vec<Item>, f64, Vec<Item>) {
    let mut items = Vec::new();
    let mut frames = Vec::new();
    let mut y = 0.0;
    layout_blocks_inline(
        ctx,
        blocks,
        left,
        width,
        &mut y,
        &mut items,
        depth,
        None,
        escape_frames,
        &mut frames,
    );
    (items, y, frames)
}

/// Stacks blocks vertically (used inside cells; page breaks are ignored).
///
/// `depth` is how many block containers are already open above this point. It
/// was a literal `0` at three of the four call sites before AUD-05, so the
/// `depth > 8` guard counted from the nearest cell rather than from the
/// document: a text box inside a table cell inside a table looked like depth 1
/// however deep the document was, and eight of them overflowed a 1 MiB stack.
///
/// Exceeding the bound is an error rather than a silent `return`: the caller
/// that has a `Result` reports `RenderError::LimitExceeded`, and the ones that
/// do not (`layout_cell_content`, `layout_table`) are themselves under a caller
/// that does.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn layout_blocks_inline(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    y: &mut f64,
    items: &mut Vec<Item>,
    depth: u32,
    note_marker: Option<&str>,
    escape_frames: bool,
    page_frames: &mut Vec<Item>,
) {
    if depth > ctx.options.limits.max_block_nesting {
        return;
    }
    ctx.set_block_depth(depth);
    let mut index = 0;
    while let Some(block) = blocks.get(index) {
        if escape_frames {
            if let Block::Paragraph(para) = block {
                if para.props.frame.is_some() {
                    let end = frame_group_end(blocks, index);
                    let group = blocks.get(index..end).unwrap_or_default();
                    page_frames.extend(cell_frame_items(ctx, group));
                    index = end;
                    continue;
                }
            }
        }
        match block {
            Block::Paragraph(para) => {
                place_cell_paragraph(ctx, para, left, width, y, items, note_marker, page_frames);
            }
            Block::Table(table) => {
                if push_uniform_framed_table(ctx, table, depth, escape_frames, page_frames) {
                    index += 1;
                    continue;
                }
                let flows = layout_table(ctx, table, left, width, depth, escape_frames);
                append_table_flows(flows, y, items, page_frames);
            }
            Block::SdtBlock(sdt) => {
                layout_blocks_inline(
                    ctx,
                    &sdt.blocks,
                    left,
                    width,
                    y,
                    items,
                    depth + 1,
                    note_marker,
                    escape_frames,
                    page_frames,
                );
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
        index += 1;
    }
}

/// Lays out one paragraph of a cell and appends its items at `*y`.
///
/// Its own frame, so the paragraph flow and its line items are not part of
/// [`layout_blocks_inline`], which recurses once per nested table.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn place_cell_paragraph(
    ctx: &LayoutContext<'_>,
    para: &strict_ooxml_wml::model::Paragraph,
    left: f64,
    width: f64,
    y: &mut f64,
    items: &mut Vec<Item>,
    note_marker: Option<&str>,
    page_frames: &mut Vec<Item>,
) {
    let flow = layout_paragraph(ctx, para, left, width, None, note_marker, &[], None);
    *y += flow.space_before + flow.border_before;
    for item in flow.flows {
        match item {
            Flow::Line(line) => {
                for mut text in line.items {
                    text.baseline += *y;
                    items.push(Item::Text(text));
                }
                *y += line.height;
            }
            Flow::Image(image) => {
                let mut image = image;
                image.y += *y;
                *y += image.h;
                items.push(Item::Image(image));
            }
            Flow::Block {
                items: block_items,
                height,
            } => {
                for item in block_items {
                    items.push(offset_item(&item, 0.0, *y));
                }
                *y += height;
            }
            Flow::TableRow(row) => {
                for item in &row.items {
                    items.push(offset_item(item, 0.0, *y));
                }
                page_frames.extend(row.page_frames);
                *y += row.height;
            }
            Flow::PageBreak => {}
        }
    }
    *y += flow.space_after + flow.border_after;
}

/// Appends a nested table's row flows at `*y` (see [`place_cell_paragraph`]).
#[inline(never)]
fn append_table_flows(
    flows: Vec<Flow>,
    y: &mut f64,
    items: &mut Vec<Item>,
    page_frames: &mut Vec<Item>,
) {
    for flow in flows {
        match flow {
            Flow::Block {
                items: block_items,
                height,
            } => {
                for item in block_items {
                    items.push(offset_item(&item, 0.0, *y));
                }
                *y += height;
            }
            Flow::TableRow(row) => {
                for item in &row.items {
                    items.push(offset_item(item, 0.0, *y));
                }
                page_frames.extend(row.page_frames);
                *y += row.height;
            }
            Flow::Line(_) | Flow::Image(_) | Flow::PageBreak => {}
        }
    }
}

/// End of a run of paragraphs that share one frame signature.
pub(crate) fn frame_group_end(blocks: &[Block], start: usize) -> usize {
    let Some(Block::Paragraph(first)) = blocks.get(start) else {
        return start;
    };
    let Some(signature) = first.props.frame.clone() else {
        return start;
    };
    let mut end = start + 1;
    while let Some(block) = blocks.get(end) {
        let Block::Paragraph(para) = block else {
            break;
        };
        if para.props.page_break_before.is_on() || para.props.frame.as_ref() != Some(&signature) {
            break;
        }
        end += 1;
    }
    end
}

/// Shared page-anchored frame on every paragraph of a table, if the cells are
/// one framed figure rather than mixed in-flow content.
pub(crate) fn table_uniform_frame(table: &Table) -> Option<&FrameProperties> {
    let mut found: Option<&FrameProperties> = None;
    for row in &table.rows {
        for cell in &row.cells {
            if !blocks_uniform_frame(&cell.blocks, &mut found) {
                return None;
            }
        }
    }
    found
}

fn blocks_uniform_frame<'a>(blocks: &'a [Block], found: &mut Option<&'a FrameProperties>) -> bool {
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let Some(frame) = para.props.frame.as_ref() else {
                    return false;
                };
                match *found {
                    None => *found = Some(frame),
                    Some(signature) if signature != frame => return false,
                    Some(_) => {}
                }
            }
            Block::SdtBlock(sdt) => {
                if !blocks_uniform_frame(&sdt.blocks, found) {
                    return false;
                }
            }
            Block::Table(_) | Block::AltChunk(_) | Block::Opaque(_) => return false,
        }
    }
    true
}

/// Lays a uniformly framed table out as one container at the frame origin.
pub(crate) fn framed_table_items(
    ctx: &LayoutContext<'_>,
    table: &Table,
    origin_x: f64,
    origin_y: f64,
    frame_width: f64,
    depth: u32,
) -> Vec<Item> {
    let flows = layout_table(ctx, table, origin_x, frame_width.max(1.0), depth, false);
    let mut items = Vec::new();
    let mut y = origin_y;
    for flow in flows {
        match flow {
            Flow::TableRow(row) => {
                for item in &row.items {
                    items.push(offset_item(item, 0.0, y));
                }
                items.extend(row.page_frames);
                y += row.height;
            }
            Flow::Block {
                items: block_items,
                height,
            } => {
                for item in &block_items {
                    items.push(offset_item(item, 0.0, y));
                }
                y += height;
            }
            Flow::Line(_) | Flow::Image(_) | Flow::PageBreak => {}
        }
    }
    items
}

/// Page-absolute items for a uniformly framed nested table, if it is one figure.
fn push_uniform_framed_table(
    ctx: &LayoutContext<'_>,
    table: &Table,
    depth: u32,
    escape_frames: bool,
    page_frames: &mut Vec<Item>,
) -> bool {
    if !escape_frames {
        return false;
    }
    let Some(frame) = table_uniform_frame(table) else {
        return false;
    };
    let (margin_x, margin_y, content_width, page_width, page_height) = ctx.frame_anchor.get();
    let scale = ctx.options.scale;
    let (origin_x, origin_y, frame_width) = frame_placement(
        frame,
        margin_x,
        margin_y,
        content_width,
        content_width,
        page_width,
        page_height,
        scale,
    );
    page_frames.extend(framed_table_items(
        ctx,
        table,
        origin_x,
        origin_y,
        frame_width,
        depth,
    ));
    true
}

/// Resolves frame origin and width against the current page/margin box.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub(crate) fn frame_placement(
    frame: &FrameProperties,
    margin_x: f64,
    margin_y: f64,
    content_width: f64,
    content_height: f64,
    page_width: f64,
    page_height: f64,
    scale: f64,
) -> (f64, f64, f64) {
    let frame_width = frame.width.map_or(content_width.max(1.0), |width| {
        twips_to_px(width.value(), scale)
    });
    let frame_height = frame
        .height
        .map_or(0.0, |height| twips_to_px(height.value(), scale));
    let box_w = match frame.h_anchor.as_deref() {
        Some("margin" | "text") => content_width,
        _ => page_width,
    };
    let box_h = match frame.v_anchor.as_deref() {
        Some("margin" | "text") => content_height,
        _ => page_height,
    };
    let origin_x = frame_origin(
        frame.x,
        frame.x_align.as_deref(),
        frame.h_anchor.as_deref(),
        margin_x,
        box_w,
        frame_width,
        scale,
    );
    let origin_y = frame_origin(
        frame.y,
        frame.y_align.as_deref(),
        frame.v_anchor.as_deref(),
        margin_y,
        box_h,
        frame_height,
        scale,
    );
    (origin_x, origin_y, frame_width)
}

/// Page origin of a frame. A missing anchor, and `page`, are the page edge.
///
/// `w:xAlign`/`w:yAlign` replace a raw offset: center and right/bottom are
/// measured in the anchor box (the page, or the margin box).
pub(crate) fn frame_origin(
    twips: Option<i32>,
    align: Option<&str>,
    anchor: Option<&str>,
    margin: f64,
    box_extent: f64,
    frame_extent: f64,
    scale: f64,
) -> f64 {
    let offset = crate::units::twips_to_px(twips.unwrap_or(0), scale);
    let base = match anchor {
        Some("margin" | "text") => margin,
        _ => 0.0,
    };
    match align {
        Some("center") => base + (box_extent - frame_extent) * 0.5 + offset,
        Some("right" | "outside" | "bottom") => base + box_extent - frame_extent - offset,
        _ => base + offset,
    }
}

/// Paints a frame group at a page origin. Items are already absolute.
pub(crate) fn layout_frame_contents(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    origin_x: f64,
    origin_y: f64,
    frame_width: f64,
) -> (Vec<Item>, Vec<(f64, AnchorDrawing)>, f64) {
    let mut items = Vec::new();
    let mut anchors = Vec::new();
    let mut local_y = 0.0_f64;
    let mut pending_after = 0.0_f64;
    let mut pending_border_after = 0.0_f64;
    let mut pending_bordered = false;
    ctx.frame_prior_exact.set(None);
    ctx.frame_force_exact_grid.set(false);
    let was_in_frame = ctx.in_frame.replace(true);
    for block in blocks {
        let Block::Paragraph(para) = block else {
            continue;
        };
        let flow = layout_paragraph(ctx, para, 0.0, frame_width, None, None, &[], None);
        local_y += pending_after.max(flow.space_before);
        let curr_bordered = flow.border_before > 0.0 || flow.border_after > 0.0;
        // Consecutive `w:pBdr` paragraphs share one edge: skip the pads between
        // them (Clio HVR-I + Positions). First / last borders still apply.
        if !(pending_bordered && curr_bordered) {
            local_y += pending_border_after + flow.border_before;
        }
        for anchor in flow.anchors {
            anchors.push((origin_y + local_y, anchor));
        }
        let mut para_exact: Option<f64> = None;
        let computed = crate::style::compute_paragraph(ctx.document, para);
        let exact_para = matches!(
            computed.line_rule,
            strict_ooxml_wml::model::values::LineSpacingRule::Exact
        );
        for item in flow.flows {
            match item {
                Flow::Line(mut line) => {
                    let height = line.height;
                    if exact_para {
                        para_exact = Some(height);
                    }
                    line.offset(origin_x, origin_y + local_y);
                    let graphics = std::mem::take(&mut line.graphics);
                    let (backs, rest): (Vec<_>, Vec<_>) = graphics
                        .into_iter()
                        .partition(|item| matches!(item, Item::Rect(_)));
                    items.extend(backs);
                    items.extend(line.items.into_iter().map(Item::Text));
                    items.extend(rest);
                    local_y += height;
                }
                Flow::Image(mut image) => {
                    image.x += origin_x;
                    image.y += origin_y + local_y;
                    let height = image.h;
                    items.push(Item::Image(image));
                    local_y += height;
                }
                Flow::Block {
                    items: block_items,
                    height,
                } => {
                    for item in &block_items {
                        items.push(offset_item(item, origin_x, origin_y + local_y));
                    }
                    local_y += height;
                }
                Flow::TableRow(row) => {
                    for item in &row.items {
                        items.push(offset_item(item, origin_x, origin_y + local_y));
                    }
                    items.extend(row.page_frames);
                    local_y += row.height;
                }
                Flow::PageBreak => break,
            }
        }
        if let Some(exact) = para_exact {
            ctx.frame_prior_exact.set(Some(exact));
        }
        pending_after = flow.space_after;
        pending_border_after = flow.border_after;
        pending_bordered = curr_bordered;
    }
    ctx.frame_prior_exact.set(None);
    ctx.frame_force_exact_grid.set(false);
    ctx.in_frame.set(was_in_frame);
    local_y += pending_border_after;
    (items, anchors, local_y)
}

/// Page-absolute items for a frame group found inside a cell.
fn cell_frame_items(ctx: &LayoutContext<'_>, blocks: &[Block]) -> Vec<Item> {
    let Some(Block::Paragraph(first)) = blocks.first() else {
        return Vec::new();
    };
    let Some(frame) = first.props.frame.as_ref() else {
        return Vec::new();
    };
    let scale = ctx.options.scale;
    let (margin_x, margin_y, content_width, page_width, page_height) = ctx.frame_anchor.get();
    let (origin_x, origin_y, frame_width) = frame_placement(
        frame,
        margin_x,
        margin_y,
        content_width,
        content_width,
        page_width,
        page_height,
        scale,
    );
    let resume = ctx.frame_resume(frame);
    let (items, _anchors, height) =
        layout_frame_contents(ctx, blocks, origin_x, origin_y + resume, frame_width);
    ctx.frame_advance(frame, height);
    items
}

/// Offsets a paint item by `(dx, dy)`.
pub(crate) fn offset_item(item: &Item, dx: f64, dy: f64) -> Item {
    match item {
        Item::Text(text) => Item::Text(crate::layout::TextItem {
            x: text.x + dx,
            baseline: text.baseline + dy,
            ..text.clone()
        }),
        Item::Rect(rect) => Item::Rect(RectItem {
            x: rect.x + dx,
            y: rect.y + dy,
            ..rect.clone()
        }),
        Item::Line(line) => Item::Line(LineItem {
            x1: line.x1 + dx,
            y1: line.y1 + dy,
            x2: line.x2 + dx,
            y2: line.y2 + dy,
            ..line.clone()
        }),
        Item::Image(image) => Item::Image(crate::layout::ImageItem {
            x: image.x + dx,
            y: image.y + dy,
            ..image.clone()
        }),
        Item::Path(path) => Item::Path(crate::layout::PathItem {
            x: path.x + dx,
            y: path.y + dy,
            ..path.clone()
        }),
    }
}

/// Emits the borders of one vertical-merge fragment.
#[allow(clippy::too_many_arguments)]
fn push_borders(
    ctx: &LayoutContext<'_>,
    table: &Table,
    cell: &RawCell,
    properties: &CellProperties,
    height: f64,
    (top_edge, bottom_edge): (bool, bool),
    interior: [bool; 4],
    items: &mut Vec<Item>,
) {
    let x = cell.x;
    let y = 0.0;
    let right = x + cell.width;
    let bottom = y + height;
    let edges = [
        (0u8, top_edge, x, y, right, y),
        (1, bottom_edge, x, bottom, right, bottom),
        (2, true, x, y, x, bottom),
        (3, true, right, y, right, bottom),
    ];
    for (edge, draw, x1, y1, x2, y2) in edges {
        if !draw {
            continue;
        }
        let inside = interior
            .get(usize::from(edge))
            .copied()
            .unwrap_or_default();
        if let Some(stroke) = resolve_edge(ctx, table, properties, edge, inside) {
            items.push(Item::Line(LineItem {
                x1,
                y1,
                x2,
                y2,
                color: stroke.0,
                width: stroke.1,
                dashed: stroke.2,
            }));
        }
    }
}

type Stroke = (String, f64, bool);

/// Resolves the stroke for one cell edge (`0` top, `1` bottom, `2` left, `3` right).
fn resolve_edge(
    ctx: &LayoutContext<'_>,
    table: &Table,
    properties: &CellProperties,
    edge: u8,
    inside: bool,
) -> Option<Stroke> {
    let cell_border = match edge {
        0 => properties.borders.top.as_ref(),
        1 => properties.borders.bottom.as_ref(),
        2 => properties.borders.start.as_ref(),
        _ => properties.borders.end.as_ref(),
    };
    // Direct `w:tblBorders`, else the table style chain's; an edge shared with
    // another cell reads `insideH` (top/bottom) or `insideV` (start/end).
    let table_edge = match (inside, edge) {
        (false, _) => edge,
        (true, 0 | 1) => 4,
        (true, _) => 5,
    };
    let table_border = crate::table_style::table_border(ctx.document, table, table_edge);
    cell_border
        .or(table_border)
        .and_then(|border| stroke(ctx, border))
}

/// Converts a `w:border` into a stroke (colour, width px, dashed).
fn stroke(ctx: &LayoutContext<'_>, border: &Border) -> Option<Stroke> {
    if matches!(
        border.style,
        Some(BorderStyle::None | BorderStyle::Nil) | None
    ) {
        return None;
    }
    let color = border
        .color
        .as_ref()
        .and_then(parse_color)
        .unwrap_or_else(|| "#000000".to_owned());
    let width = eighths_point_to_px(
        border
            .size
            .map_or(4, strict_ooxml_wml::model::values::EighthsPoint::value),
        ctx.options.scale,
    )
    .max(0.25);
    let dashed = matches!(
        border.style,
        Some(
            BorderStyle::Dashed
                | BorderStyle::Dotted
                | BorderStyle::DotDash
                | BorderStyle::DotDotDash
        )
    );
    Some((color, width, dashed))
}

/// Returns the cell shading fill colour, if any (`w:themeFill` resolved through
/// the document theme, as paragraph shading is).
fn shading_fill(ctx: &LayoutContext<'_>, cell: &CellProperties) -> Option<String> {
    cell.shading
        .as_ref()
        .and_then(|shading| crate::style::shading_fill_color(shading, ctx.document.theme.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::split_widths;

    /// The sum of `widths`, for "did it fill the box" assertions.
    fn total(widths: &[f64]) -> f64 {
        widths.iter().sum()
    }

    #[test]
    fn a_grid_that_fills_the_table_is_scaled_to_it() {
        let widths = split_widths(300.0, &[100.0, 200.0], 2);
        assert_eq!(widths, vec![100.0, 200.0]);
    }

    #[test]
    fn a_narrower_grid_is_scaled_up() {
        let widths = split_widths(300.0, &[100.0, 100.0], 2);
        assert!((total(&widths) - 300.0).abs() < 1e-9, "{widths:?}");
    }

    #[test]
    fn columns_the_grid_does_not_declare_take_the_leftover() {
        // One declared column of 100, two undeclared, out of 300.
        let widths = split_widths(300.0, &[100.0], 3);
        assert_eq!(widths.len(), 3, "{widths:?}");
        assert!((total(&widths) - 300.0).abs() < 1e-9, "{widths:?}");
        assert!(
            (widths.first().copied().unwrap_or_default() - 100.0).abs() < 1e-9,
            "{widths:?}"
        );
        assert!(
            (widths.get(1).copied().unwrap_or_default() - 100.0).abs() < 1e-9,
            "{widths:?}"
        );
    }

    #[test]
    fn a_grid_that_already_fills_the_table_gives_the_extra_columns_the_average() {
        // Declared: 100 + 200 = 300, the whole table, so there is no leftover to
        // hand out and each undeclared column takes the declared average. The
        // row is then wider than the box - which is the rule AUD-08 states, and
        // the alternative (zero-width cells) would draw a row the author can see
        // in Word and nobody can see here.
        let widths = split_widths(300.0, &[100.0, 200.0], 5);
        assert_eq!(widths, vec![100.0, 200.0, 150.0, 150.0, 150.0]);
        assert!(total(&widths) > 300.0, "{widths:?}");
    }

    #[test]
    fn no_grid_at_all_splits_the_table_evenly() {
        let widths = split_widths(300.0, &[], 4);
        assert_eq!(widths, vec![75.0; 4]);
    }

    #[test]
    fn an_absurd_column_count_still_returns_that_many_columns() {
        // The AUD-08 input: a one-column grid and a `w:gridSpan` of 65535. The
        // arithmetic must not allocate what the span says and then fail.
        let widths = split_widths(300.0, &[100.0], 65_535);
        assert_eq!(widths.len(), 65_535);
        assert!(widths
            .iter()
            .all(|width| width.is_finite() && *width >= 0.0));
    }

    #[test]
    fn zero_columns_is_no_columns() {
        assert!(split_widths(300.0, &[100.0], 0).is_empty());
    }
}
