//! Table layout: grid, spans, vertical merges, borders, shading
//! (`STAGE-4-TASK.md` §5.5, `STAGE-5-TASK.md` §5.7–§5.8).

use strict_ooxml_wml::model::drawing::AnchorDrawing;
use strict_ooxml_wml::model::props::{CellProperties, FrameProperties};
use strict_ooxml_wml::model::values::{
    Borders, HeightRule, TextDirection, Twips, VerticalJc, VerticalMerge, Width, WidthKind,
};
use strict_ooxml_wml::model::{Block, Table};

use crate::layout::border::{edge_items, resolve_border, ResolvedBorder, Side as BorderSide};
use crate::layout::paragraph::layout_paragraph;
use crate::layout::{Flow, Item, LayoutContext, LineItem, RectItem, TableRowFlow};
use crate::units::twips_to_px;

/// Default horizontal cell margin in twips (`CT_TblCellMar` start/end).
const DEFAULT_CELL_MARGIN: i32 = 108;
/// Default vertical cell margin in twips (`CT_TblCellMar` top/bottom is `0`).
const DEFAULT_CELL_MARGIN_VERTICAL: i32 = 0;

thread_local! {
    static CELL_HEIGHT_REQUIRED: std::cell::RefCell<Vec<bool>> = const { std::cell::RefCell::new(Vec::new()) };
    static CELL_ANCHORS: std::cell::RefCell<Vec<Vec<crate::layout::RowAnchor>>> = const { std::cell::RefCell::new(Vec::new()) };
}
struct CellAnchorGuard;
impl CellAnchorGuard {
    fn enter() -> Self {
        CELL_ANCHORS.with(|stack| stack.borrow_mut().push(Vec::new()));
        CELL_HEIGHT_REQUIRED.with(|stack| stack.borrow_mut().push(false));
        Self
    }
    // The receiver ties every access to a live scope: the stacks are pushed in
    // `enter` and popped in `drop`.
    #[allow(clippy::unused_self)]
    fn needs_height(&self) -> bool {
        CELL_HEIGHT_REQUIRED.with(|stack| stack.borrow().last().copied().unwrap_or(false))
    }
    #[allow(clippy::unused_self)]
    fn take(&self) -> Vec<crate::layout::RowAnchor> {
        CELL_ANCHORS.with(|stack| {
            stack
                .borrow_mut()
                .last_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        })
    }
}
impl Drop for CellAnchorGuard {
    fn drop(&mut self) {
        CELL_ANCHORS.with(|stack| {
            stack.borrow_mut().pop();
        });
        CELL_HEIGHT_REQUIRED.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}
/// Measure an automatic row before applying cell-relative floating exclusions.
pub(crate) fn cell_anchor_wrap_enabled(anchor: &AnchorDrawing) -> bool {
    CELL_ANCHORS.with(|stack| {
        stack.borrow().is_empty()
            || (anchor.layout_in_cell
                && crate::table_style::current_text_flow().is_some_and(|flow| flow.1.is_some()))
    })
}
fn capture_anchor(anchor: crate::layout::RowAnchor) -> bool {
    CELL_ANCHORS.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(anchors) = stack.last_mut() {
            anchors.push(anchor);
            true
        } else {
            false
        }
    })
}

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
    anchors: Vec<crate::layout::RowAnchor>,
    margin_top: f64,
    content_height: f64,
    margin_bottom: f64,
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
    let resolved_table = crate::table_style::resolve_table(ctx.document, table);
    let _numbering_aliases = crate::numbering::alias_table(table, &resolved_table);
    let table = &resolved_table;
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
        let mut raw = layout_row(
            ctx,
            table,
            row_index,
            &widths,
            table_x,
            depth,
            escape_frames,
            skip_cell_h_margins,
            None,
        );
        if raw.needs_cell_height {
            for _ in 0..3 {
                let previous = raw.height.max(1.0);
                let declared = row
                    .props
                    .height
                    .as_ref()
                    .and_then(|h| h.value)
                    .map_or(0.0, |h| twips_to_px(h.value(), scale));
                let cell_height = previous.max(declared);
                raw = layout_row(
                    ctx,
                    table,
                    row_index,
                    &widths,
                    table_x,
                    depth,
                    escape_frames,
                    skip_cell_h_margins,
                    Some(cell_height),
                );
                if (raw.height - previous).abs() < 0.001 {
                    break;
                }
            }
        }
        let mut height = raw.height;
        if let Some(declared) = &row.props.height {
            let value = declared
                .value
                .map_or(0.0, |value| twips_to_px(value.value(), scale));
            // `w:hRule`: `exact` fixes the row whatever its content, `auto`
            // lets the content decide, and `atLeast` — what Word reads when
            // the attribute is absent — is a minimum.
            height = match declared.rule {
                Some(HeightRule::Exact) if value > 0.0 => value,
                Some(HeightRule::Auto) => height,
                _ => height.max(value),
            };
        }
        if height <= 0.0 {
            height = 1.0;
        }
        rows.push(RawRow {
            header: row.props.header.is_on(),
            height,
            cells: raw.cells,
        });
    }

    fit_rotated_merges(&mut rows);
    if table.props.bidi_visual {
        mirror_columns(&mut rows, table_x, total_width);
    }
    row_flows(ctx, table, &rows)
}

/// Lays a `w:bidiVisual` table out right to left: the first cell of a row is
/// the rightmost, and a cell's start edge is its right one.
///
/// The cells were laid out left to right; each moves to its mirrored place
/// with its content, so the text inside a cell keeps its own direction.
fn mirror_columns(rows: &mut [RawRow], table_x: f64, total_width: f64) {
    for row in rows {
        for cell in &mut row.cells {
            let mirrored = 2.0 * table_x + total_width - cell.x - cell.width;
            let dx = mirrored - cell.x;
            cell.x = mirrored;
            for item in &mut cell.items {
                *item = offset_item(item, dx, 0.0);
            }
            let borders = &mut cell.properties.borders;
            std::mem::swap(&mut borders.start, &mut borders.end);
        }
    }
}

/// Rotated content occupies the complete merged region, not its first row.
fn fit_rotated_merges(rows: &mut [RawRow]) {
    for index in 0..rows.len() {
        let constraints: Vec<(usize, usize, f64)> = rows
            .get(index)
            .into_iter()
            .flat_map(|row| &row.cells)
            .filter(|cell| {
                cell.vmerge == VState::Restart
                    && matches!(
                        cell.properties.text_direction,
                        Some(
                            TextDirection::BtLr
                                | TextDirection::TbRl
                                | TextDirection::TbRlV
                                | TextDirection::TbLrV
                        )
                    )
            })
            .map(|cell| {
                (
                    cell.col,
                    cell.span,
                    cell.content_height + cell.margin_top + cell.margin_bottom,
                )
            })
            .collect();
        for (col, span, required) in constraints {
            let mut end = index + 1;
            while rows.get(end).is_some_and(|row| {
                row.cells.iter().any(|cell| {
                    cell.col == col && cell.span == span && cell.vmerge == VState::Continue
                })
            }) {
                end += 1;
            }
            let available: f64 = rows
                .get(index..end)
                .unwrap_or_default()
                .iter()
                .map(|row| row.height)
                .sum();
            if let Some(last) = rows.get_mut(end - 1) {
                last.height += (required - available).max(0.0);
            }
        }
    }
}

/// Authored row minima give a vertical text cell its horizontal wrapping width.
fn authored_cell_height(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row: usize,
    col: usize,
    span: usize,
    merge: Option<VerticalMerge>,
) -> Option<f64> {
    let mut height = 0.0;
    for (index, next) in table.rows.iter().enumerate().skip(row) {
        if index > row {
            if merge != Some(VerticalMerge::Restart) {
                break;
            }
            let mut column = omitted_columns(next.props.grid_before);
            let continuation = next.cells.iter().any(|cell| {
                let width = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
                let matches = column == col
                    && width == span
                    && cell.props.vertical_merge == Some(VerticalMerge::Continue);
                column += width;
                matches
            });
            if !continuation {
                break;
            }
        }
        let declared = next.props.height.as_ref()?.value?.value();
        if declared <= 0 {
            return None;
        }
        height += twips_to_px(declared, ctx.options.scale);
    }
    (height > 0.0).then_some(height)
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
        let mut anchors = Vec::new();
        let mut bands = Vec::new();
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
            if let Some(fill) = shading_fill(ctx, &source.properties)
                .or_else(|| table_shading_fill(ctx, table, row_index))
            {
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
                let shift = vertical_align_shift(rows, row_index, cell);
                for item in &cell.items {
                    let item = offset_item(item, 0.0, cell.margin_top + shift);
                    bands.push(item_vertical_bounds(ctx, &item));
                    items.push(item);
                }
                page_frames.extend(cell.page_frames.iter().cloned());
                anchors.extend(cell.anchors.iter().cloned().map(|mut anchor| {
                    anchor.y += cell.margin_top + shift;
                    anchor
                }));
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
            let row_borders = table
                .rows
                .get(row_index)
                .map(|row| &row.props.exception_borders);
            push_borders(
                ctx,
                (table, row_borders),
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
            anchors,
            split_points: if row.header
                || table
                    .rows
                    .get(row_index)
                    .is_some_and(|row| row.props.cant_split)
            {
                Vec::new()
            } else {
                row_split_points(bands, row.height)
            },
        }));
    }
    flows
}

/// Occupied vertical ink interval, used to keep a page break out of content.
pub(crate) fn item_vertical_bounds(ctx: &LayoutContext<'_>, item: &Item) -> (f64, f64) {
    match item {
        Item::Group(group) => (group.bounds[1], group.bounds[1] + group.bounds[3]),
        Item::Text(text) => {
            let family = crate::style::chosen_family(&text.run, &text.text);
            let metrics = crate::font::embedded::run_metrics(&text.run, &family, ctx.font);
            let top = -metrics.ascent_em() * text.size_px;
            let bottom = metrics.descender * text.size_px;
            let (sin, cos) = text.run.rotation_deg.to_radians().sin_cos();
            let ys = [
                top * cos,
                bottom * cos,
                text.width * sin + top * cos,
                text.width * sin + bottom * cos,
            ];
            (
                text.baseline + ys.iter().copied().fold(f64::INFINITY, f64::min),
                text.baseline + ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            )
        }
        Item::Rect(rect) => (
            rect.y - rect.stroke_w / 2.0,
            rect.y + rect.h + rect.stroke_w / 2.0,
        ),
        Item::Line(line) => (
            line.y1.min(line.y2) - line.width / 2.0,
            line.y1.max(line.y2) + line.width / 2.0,
        ),
        Item::Path(path) => {
            let (sin, cos) = path.rotate_deg.to_radians().sin_cos();
            let half = (path.w * sin.abs() + path.h * cos.abs() + path.stroke_w) / 2.0;
            (path.y + path.h / 2.0 - half, path.y + path.h / 2.0 + half)
        }
        Item::Image(image) => (image.y, image.y + image.h),
    }
}

/// Union the occupied intervals across every cell before selecting boundaries.
pub(crate) fn row_split_points(mut bands: Vec<(f64, f64)>, height: f64) -> Vec<f64> {
    bands.retain(|(top, bottom)| top.is_finite() && bottom.is_finite() && bottom >= top);
    bands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut points = Vec::new();
    let mut end = 0.0_f64;
    for (top, bottom) in bands {
        if top > end + 0.01 && end > 0.0 && end < height {
            points.push(f64::midpoint(end, top));
        }
        end = end.max(bottom);
    }
    if end > 0.0 && end < height - 0.01 {
        points.push(f64::midpoint(end, height));
    }
    points
}

/// Paint one vertical row slice. Decorations extend across each fragment;
/// safe boundaries ensure images, paths and glyphs are never cut in half.
pub(crate) fn row_fragment(row: &TableRowFlow, start: f64, end: f64) -> TableRowFlow {
    let mut items = Vec::new();
    for item in &row.items {
        let clipped = match item {
            Item::Rect(rect) => {
                let top = rect.y.max(start);
                let bottom = (rect.y + rect.h).min(end);
                if bottom <= top {
                    continue;
                }
                let mut rect = rect.clone();
                rect.y = top;
                rect.h = bottom - top;
                Item::Rect(rect)
            }
            Item::Line(line) => {
                let mut line = line.clone();
                if (line.y2 - line.y1).abs() < 1e-9 {
                    if line.y1.abs() < 0.01 {
                        line.y1 = start;
                        line.y2 = start;
                    } else if (line.y1 - row.height).abs() < 0.01 {
                        line.y1 = end;
                        line.y2 = end;
                    } else if line.y1 < start || line.y1 >= end {
                        continue;
                    }
                } else {
                    let low = line.y1.min(line.y2).max(start);
                    let high = line.y1.max(line.y2).min(end);
                    if high <= low {
                        continue;
                    }
                    let x_at = |y: f64| {
                        line.x1 + (line.x2 - line.x1) * (y - line.y1) / (line.y2 - line.y1)
                    };
                    let (y1, y2) = if line.y1 < line.y2 {
                        (low, high)
                    } else {
                        (high, low)
                    };
                    let (x1, x2) = (x_at(y1), x_at(y2));
                    line.x1 = x1;
                    line.x2 = x2;
                    line.y1 = y1;
                    line.y2 = y2;
                }
                Item::Line(line)
            }
            Item::Text(text) if text.baseline >= start && text.baseline < end => item.clone(),
            Item::Path(path) if path.y + path.h / 2.0 >= start && path.y + path.h / 2.0 < end => {
                item.clone()
            }
            Item::Image(image)
                if image.y + image.h / 2.0 >= start && image.y + image.h / 2.0 < end =>
            {
                item.clone()
            }
            Item::Group(group)
                if group.bounds[1] + group.bounds[3] / 2.0 >= start
                    && group.bounds[1] + group.bounds[3] / 2.0 < end =>
            {
                item.clone()
            }
            _ => continue,
        };
        items.push(offset_item(&clipped, 0.0, -start));
    }
    TableRowFlow {
        anchors: row
            .anchors
            .iter()
            .filter(|anchor| anchor.y >= start && (anchor.y < end || end >= row.height))
            .cloned()
            .map(|mut anchor| {
                anchor.y -= start;
                anchor
            })
            .collect(),
        items,
        height: end - start,
        header: false,
        page_frames: if start == 0.0 {
            row.page_frames.clone()
        } else {
            Vec::new()
        },
        split_points: Vec::new(),
    }
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
    needs_cell_height: bool,
    cells: Vec<RawCell>,
    height: f64,
}

/// Lays out one row's cells against `widths`.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn layout_row(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row_index: usize,
    widths: &[f64],
    table_x: f64,
    depth: u32,
    escape_frames: bool,
    skip_cell_h_margins: bool,
    row_height_override: Option<f64>,
) -> LaidOutRow {
    let Some(row) = table.rows.get(row_index) else {
        return LaidOutRow {
            needs_cell_height: false,
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
    let mut column = omitted_columns(row.props.grid_before);
    let mut cells = Vec::with_capacity(row.cells.len());
    let mut max_content: f64 = 0.0;
    let mut needs_cell_height = false;
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
        let properties = crate::table_style::resolve_cell_properties(
            ctx.document,
            table,
            conditions,
            &cell.props,
        );
        let cell_style = crate::table_style::enter_cell(
            table,
            conditions,
            suppresses_breaks(ctx, table, &properties) || properties.fit_text,
        );
        let direction = properties.text_direction;
        let anchor_scope = CellAnchorGuard::enter();
        let (mut items, mut content_height, page_frames) = layout_oriented_cell_content(
            ctx,
            &cell.blocks,
            x + margins.0,
            cell_content_width,
            depth + 1,
            escape_frames,
            direction,
            authored_cell_height(
                ctx,
                table,
                row_index,
                column,
                span,
                cell.props.vertical_merge,
            )
            .or(row_height_override)
            .map(|height| (height - margins.2 - margins.3).max(1.0)),
        );
        if properties.hide_mark && cell.blocks.iter().all(|block| {
            matches!(block, Block::Paragraph(paragraph) if paragraph.inlines.is_empty())
        }) {
            items.clear();
            content_height = 0.0;
        }
        if properties.fit_text && !items.is_empty() {
            let measured =
                crate::paint::graphics::transform_items(items, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            let width = match measured.first() {
                Some(Item::Group(group)) => group.bounds[2],
                _ => 0.0,
            };
            let factor = (cell_content_width / width.max(1.0)).min(1.0);
            items = crate::paint::graphics::transform_items(
                measured,
                [factor, 0.0, 0.0, 1.0, (x + margins.0) * (1.0 - factor), 0.0],
            );
        }
        drop(cell_style);
        let height = content_height + margins.2 + margins.3;
        if cell.props.vertical_merge != Some(VerticalMerge::Continue)
            && !(cell.props.vertical_merge == Some(VerticalMerge::Restart)
                && matches!(
                    direction,
                    Some(
                        TextDirection::BtLr
                            | TextDirection::TbRl
                            | TextDirection::TbRlV
                            | TextDirection::TbLrV
                    )
                ))
        {
            max_content = max_content.max(height);
        }
        needs_cell_height |= anchor_scope.needs_height();
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
                margin_bottom: margins.3,
                content_height,
                conditions,
            },
            items,
            page_frames,
            anchor_scope.take(),
        );
        column += span;
    }
    LaidOutRow {
        needs_cell_height,
        cells,
        height: max_content,
    }
}

/// Rotate a horizontal text layout before deriving the physical row height.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn layout_oriented_cell_content(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    x: f64,
    width: f64,
    depth: u32,
    escape_frames: bool,
    direction: Option<TextDirection>,
    along_height: Option<f64>,
) -> (Vec<Item>, f64, Vec<Item>) {
    let section = ctx
        .field_env
        .get()
        .map_or(ctx.current_section.get(), |env| {
            env.section_index.saturating_sub(1)
        });
    let parent = crate::table_style::current_text_flow()
        .map(|flow| flow.0)
        .or_else(|| {
            ctx.document
                .sections
                .get(section)
                .and_then(|s| s.properties.text_direction)
        })
        .unwrap_or(TextDirection::LrTb);
    let desired = direction.unwrap_or(parent);
    let _flow_direction = crate::table_style::enter_text_flow(desired, along_height);
    let direction_angle = |direction| match direction {
        TextDirection::BtLr => -90.0_f64,
        TextDirection::TbRl | TextDirection::TbRlV | TextDirection::TbLrV => 90.0,
        _ => 0.0,
    };
    let angle = direction_angle(desired) - direction_angle(parent);
    let rotated = |direction| {
        matches!(
            direction,
            TextDirection::LrTbV | TextDirection::TbRlV | TextDirection::TbLrV
        )
    };
    if angle == 0.0 {
        let (mut items, height, frames) =
            layout_cell_content(ctx, blocks, x, width, depth, escape_frames);
        if rotated(desired) && !rotated(parent) {
            items = rotate_east_asian_items(ctx, items);
        }
        return (items, height, frames);
    }
    let left_to_right = desired == TextDirection::TbLrV;
    let available = if crate::layout::paragraph::is_half_turn(angle) {
        width
    } else {
        along_height
            .unwrap_or_else(|| ctx.frame_anchor.get().page_height.max(width))
            .max(1.0)
    };
    let (mut items, cross_height, frames) =
        layout_cell_content(ctx, blocks, 0.0, available, depth, escape_frames);
    if rotated(desired) && !rotated(parent) {
        items = rotate_east_asian_items(ctx, items);
    }
    if crate::layout::paragraph::is_half_turn(angle) {
        return (
            crate::paint::graphics::transform_items(
                items,
                [-1.0, 0.0, 0.0, -1.0, x + width, cross_height],
            ),
            cross_height,
            frames,
        );
    }
    if items.iter().any(|item| !matches!(item, Item::Text(_))) {
        let measured =
            crate::paint::graphics::transform_items(items, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let bounds = match measured.first() {
            Some(Item::Group(group)) => group.bounds,
            _ => [0.0; 4],
        };
        let height = along_height.unwrap_or(bounds[2].max(0.0));
        let left = if along_height.is_some() {
            0.0
        } else {
            bounds[0]
        };
        let pad = ((width - cross_height) / 2.0).max(0.0);
        let matrix = if angle < 0.0 {
            [0.0, -1.0, 1.0, 0.0, x + pad, height + left]
        } else {
            [0.0, 1.0, -1.0, 0.0, x + pad + cross_height, -left]
        };
        if left_to_right {
            let mut oriented = Vec::new();
            for item in measured.into_iter().flat_map(|item| match item {
                Item::Group(group) => group.items,
                other => vec![other],
            }) {
                let center = if let Item::Text(text) = &item {
                    text.baseline
                } else {
                    let bounds = crate::paint::graphics::transform_items(
                        vec![item.clone()],
                        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    );
                    match bounds.first() {
                        Some(Item::Group(group)) => group.bounds[1] + group.bounds[3] / 2.0,
                        _ => 0.0,
                    }
                };
                oriented.extend(crate::paint::graphics::transform_items(
                    vec![item],
                    [0.0, 1.0, -1.0, 0.0, x + pad + 2.0 * center, -left],
                ));
            }
            return (oriented, height, frames);
        }
        return (
            crate::paint::graphics::transform_items(measured, matrix),
            height,
            frames,
        );
    }
    let left = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(t) => Some(t.x),
            _ => None,
        })
        .min_by(f64::total_cmp)
        .unwrap_or(0.0);
    let right = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(t) => Some(t.x + t.width),
            _ => None,
        })
        .max_by(f64::total_cmp)
        .unwrap_or(left);
    let height = along_height.map_or((right - left).max(0.0), |height| height.max(right));
    let pad = ((width - cross_height) / 2.0).max(0.0);
    for item in &mut items {
        if let Item::Text(text) = item {
            let along = text.x - if along_height.is_some() { 0.0 } else { left };
            let across = text.baseline;
            text.x = x
                + pad
                + if angle < 0.0 || left_to_right {
                    across
                } else {
                    cross_height - across
                };
            text.baseline = if angle < 0.0 { height - along } else { along };
            text.run.rotation_deg += angle;
        }
    }
    (items, height, frames)
}

/// Word's `tbV` retains horizontal flow but rotates East Asian glyphs 270°.
pub(crate) fn rotate_east_asian_items(ctx: &LayoutContext<'_>, items: Vec<Item>) -> Vec<Item> {
    rotate_upright_items(ctx, items, false)
}

/// Keeps each grapheme upright when a vertical container rotates its flow.
pub(crate) fn rotate_upright_items(
    ctx: &LayoutContext<'_>,
    items: Vec<Item>,
    all: bool,
) -> Vec<Item> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Group(mut group) => {
                group.items = rotate_upright_items(ctx, group.items, all);
                out.push(Item::Group(group));
            }
            Item::Text(text)
                if all
                    || text
                        .text
                        .chars()
                        .any(crate::font::metrics::is_wide_character) =>
            {
                let mut x = text.x;
                for cluster in
                    unicode_segmentation::UnicodeSegmentation::graphemes(text.text.as_str(), true)
                {
                    let mut glyph = text.clone();
                    cluster.clone_into(&mut glyph.text);
                    glyph.width = ctx.measure(cluster, &text.run);
                    glyph.x = x;
                    if all || cluster.chars().any(crate::font::metrics::is_wide_character) {
                        let family = crate::style::chosen_family(&text.run, cluster);
                        let ascent =
                            crate::font::embedded::run_metrics(&text.run, &family, ctx.font)
                                .ascent_em()
                                * text.size_px;
                        let cx = x + glyph.width / 2.0;
                        let cy = text.baseline - ascent + text.size_px / 2.0;
                        glyph.x = cx + text.baseline - cy;
                        glyph.baseline = cy + glyph.width / 2.0;
                        glyph.run.rotation_deg -= 90.0;
                    }
                    x += glyph.width;
                    out.push(Item::Text(glyph));
                }
            }
            item => out.push(item),
        }
    }
    out
}

/// Where a laid-out cell sits in its row.
#[derive(Clone, Copy)]
struct CellPlacement {
    col: usize,
    span: usize,
    x: f64,
    width: f64,
    margin_top: f64,
    margin_bottom: f64,
    content_height: f64,
    /// `w:tblStylePr` conditions that apply (see `table_style::cell_conditions`).
    conditions: u16,
}

/// Builds a [`RawCell`] in its own frame: the cell's properties are resolved
/// against the table style here, not in [`layout_row`], which recurses once per
/// nesting level (see [`row_flows`]).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn push_raw_cell(
    ctx: &LayoutContext<'_>,
    table: &Table,
    cells: &mut Vec<RawCell>,
    cell: &strict_ooxml_wml::model::TableCell,
    placement: CellPlacement,
    items: Vec<Item>,
    page_frames: Vec<Item>,
    anchors: Vec<crate::layout::RowAnchor>,
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
        anchors,
        margin_top: placement.margin_top,
        margin_bottom: placement.margin_bottom,
        content_height: placement.content_height,
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

/// How far `w:vAlign` moves a cell's content down from the top margin.
///
/// A merged cell aligns within the whole merged region. Rotated text keeps
/// the top: its `w:vAlign` aligns along the text, which the rotated layout
/// already decides.
fn vertical_align_shift(rows: &[RawRow], row: usize, cell: &RawCell) -> f64 {
    let factor = match cell.properties.vertical_align {
        Some(VerticalJc::Center) => 0.5,
        Some(VerticalJc::Bottom) => 1.0,
        Some(VerticalJc::Top) | None => return 0.0,
    };
    if matches!(
        cell.properties.text_direction,
        Some(
            TextDirection::BtLr | TextDirection::TbRl | TextDirection::TbRlV | TextDirection::TbLrV
        )
    ) {
        return 0.0;
    }
    let mut region = rows.get(row).map_or(0.0, |row| row.height);
    let mut index = row;
    if cell.vmerge == VState::Restart {
        while rows.get(index).is_some() && merge_continues(rows, index, cell) {
            index += 1;
            region += rows.get(index).map_or(0.0, |row| row.height);
        }
    }
    let free = region - cell.margin_top - cell.margin_bottom - cell.content_height;
    free.max(0.0) * factor
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
            let cells = row
                .cells
                .iter()
                .map(|cell| usize::from(cell.props.grid_span.unwrap_or(1)))
                .sum::<usize>();
            cells
                .saturating_add(omitted_columns(row.props.grid_before))
                .saturating_add(omitted_columns(row.props.grid_after))
        })
        .max()
        .unwrap_or(0);
    table.grid.len().max(widest_row)
}

/// Omitted columns use the same bounded grid range as `w:gridSpan`.
fn omitted_columns(value: Option<i32>) -> usize {
    value.unwrap_or(0).clamp(0, i32::from(u16::MAX)) as usize
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
    // No usable `w:tblGrid`: the cells' own `w:tcW` are the next best word on
    // the column widths, before an equal split.
    let preferred = cell_width_grid(ctx, table, column_count, total, &grid);
    let mut widths = split_widths(total, &preferred, column_count);
    if table.props.layout != Some(strict_ooxml_wml::model::values::TableLayout::Fixed) {
        let mut minimum = vec![1.0_f64; column_count];
        for (row_index, row) in table.rows.iter().enumerate() {
            let mut column = omitted_columns(row.props.grid_before);
            for cell in &row.cells {
                let span = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
                let end = column.saturating_add(span).min(column_count);
                if column >= end {
                    break;
                }
                let conditions = crate::table_style::cell_conditions(
                    ctx.document,
                    table,
                    row_index,
                    column,
                    span,
                    column_count,
                );
                let properties = crate::table_style::resolve_cell_properties(
                    ctx.document,
                    table,
                    conditions,
                    &cell.props,
                );
                let no_breaks = suppresses_breaks(ctx, table, &properties);
                let _cell = crate::table_style::enter_cell(
                    table,
                    conditions,
                    no_breaks || properties.fit_text,
                );
                let margins = effective_margins(ctx, table, row, cell, conditions);
                let (word, line) = intrinsic_cell_width(ctx, &cell.blocks, 0);
                let demand = if properties.fit_text {
                    0.0
                } else if no_breaks {
                    line
                } else if properties.no_wrap {
                    // A fixed-width `noWrap` cell is never narrower than its
                    // preferred width; its text still wraps (§17.4.30).
                    let preferred = properties
                        .width
                        .filter(|width| width.kind == WidthKind::Dxa)
                        .and_then(|width| width.value)
                        .map_or(0.0, |value| {
                            twips_to_px(value.max(0), ctx.options.scale) - margins.0 - margins.1
                        });
                    word.max(preferred)
                } else {
                    word
                };
                let share = (demand + margins.0 + margins.1) / (end - column) as f64;
                for value in minimum.iter_mut().take(end).skip(column) {
                    *value = value.max(share);
                }
                column = end;
            }
        }
        let minimum_sum: f64 = minimum.iter().sum();
        if minimum_sum >= total {
            widths = split_widths(total, &minimum, column_count);
        } else {
            let weights: Vec<_> = widths
                .iter()
                .zip(&minimum)
                .map(|(width, min)| (width - min).max(0.0))
                .collect();
            let weight_sum: f64 = weights.iter().sum();
            widths = minimum
                .iter()
                .zip(weights)
                .map(|(min, weight)| {
                    min + (total - minimum_sum)
                        * if weight_sum > 0.0 {
                            weight / weight_sum
                        } else {
                            1.0 / column_count as f64
                        }
                })
                .collect();
        }
    }
    widths
}

/// Minimum unbreakable text width and maximum unwrapped paragraph width.
fn intrinsic_cell_width(ctx: &LayoutContext<'_>, blocks: &[Block], depth: u32) -> (f64, f64) {
    if depth > ctx.options.limits.max_block_nesting {
        return (0.0, 0.0);
    }
    let mut word = 0.0_f64;
    let mut longest = 0.0_f64;
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let (minimum, line) = super::paragraph::intrinsic_widths(ctx, paragraph);
                word = word.max(minimum);
                longest = longest.max(line);
            }
            Block::Table(table) => {
                for row in &table.rows {
                    let mut row_minimum = 0.0;
                    let mut row_maximum = 0.0;
                    for cell in &row.cells {
                        let (minimum, maximum) = intrinsic_cell_width(ctx, &cell.blocks, depth + 1);
                        let padding = twips_to_px(DEFAULT_CELL_MARGIN * 2, ctx.options.scale);
                        row_minimum += minimum + padding;
                        row_maximum += maximum + padding;
                    }
                    word = word.max(row_minimum);
                    longest = longest.max(row_maximum);
                }
            }
            Block::SdtBlock(sdt) => {
                let (nested_word, nested_line) = intrinsic_cell_width(ctx, &sdt.blocks, depth + 1);
                word = word.max(nested_word);
                longest = longest.max(nested_line);
            }
            _ => {}
        }
    }
    (word, longest)
}

/// Preferred cell widths constrain even a partial/stale grid. Earlier rows
/// establish columns; spanning preferences retain the grid's proportions.
fn cell_width_grid(
    ctx: &LayoutContext<'_>,
    table: &Table,
    column_count: usize,
    total: f64,
    grid: &[f64],
) -> Vec<f64> {
    let mut widths = split_widths(total, grid, column_count);
    let mut assigned = vec![false; column_count];
    let preferred = |width: Option<Width>| {
        let width = width?;
        let value = width.value?.max(0);
        match width.kind {
            WidthKind::Dxa => Some(twips_to_px(value, ctx.options.scale)),
            WidthKind::Pct => Some(total * f64::from(value) / 5000.0),
            WidthKind::Auto | WidthKind::Nil => None,
        }
        .filter(|width| *width > 0.0)
    };
    for row in &table.rows {
        let mut column = omitted_columns(row.props.grid_before).min(column_count);
        let mut constraints = Vec::with_capacity(row.cells.len() + 2);
        if column > 0 {
            constraints.push((0, column, preferred(row.props.width_before)));
        }
        for cell in &row.cells {
            let end = column
                .saturating_add(usize::from(cell.props.grid_span.unwrap_or(1)).max(1))
                .min(column_count);
            if column >= end {
                break;
            }
            constraints.push((column, end, preferred(cell.props.width)));
            column = end;
        }
        let end = column
            .saturating_add(omitted_columns(row.props.grid_after))
            .min(column_count);
        if end > column {
            constraints.push((column, end, preferred(row.props.width_after)));
        }
        for (start, end, preference) in constraints {
            let Some(preference) = preference else {
                continue;
            };
            let span = || widths.iter().zip(&assigned).take(end).skip(start);
            let fixed: f64 = span().filter(|(_, &set)| set).map(|(width, _)| width).sum();
            let free_count = span().filter(|(_, &set)| !set).count();
            let weight: f64 = span()
                .filter(|(_, &set)| !set)
                .map(|(width, _)| width)
                .sum();
            let remaining = (preference - fixed).max(0.0);
            for (width, set) in widths
                .iter_mut()
                .zip(assigned.iter_mut())
                .take(end)
                .skip(start)
                .filter(|(_, set)| !**set)
            {
                *width = if weight > 0.0 {
                    remaining * *width / weight
                } else {
                    remaining / free_count as f64
                };
                *set = true;
            }
        }
    }
    widths
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
pub(crate) fn table_total_width(ctx: &LayoutContext<'_>, table: &Table, content_width: f64) -> f64 {
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
    let indent = match table.props.indent_kind {
        None | Some(WidthKind::Dxa) => table
            .props
            .indent
            .map_or(0.0, |value| twips_to_px(value.value(), ctx.options.scale)),
        // `tblInd` has no percentage or automatic indentation semantics.
        Some(WidthKind::Auto | WidthKind::Nil | WidthKind::Pct) => 0.0,
    };
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
    // `w:trPr` first, then the row's `w:tblPrEx`, then the table.
    let width = row
        .props
        .cell_spacing
        .as_ref()
        .or(row
            .props
            .exception
            .as_ref()
            .and_then(|exception| exception.cell_spacing.as_ref()))
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
    let mut source = crate::table_style::style_cell_margins(ctx.document, table, conditions);
    for layer in [
        table.props.cell_margins,
        row.props.cell_margins,
        cell.props.margins,
    ] {
        if layer.start.is_some() {
            source.start = layer.start;
        }
        if layer.end.is_some() {
            source.end = layer.end;
        }
        if layer.top.is_some() {
            source.top = layer.top;
        }
        if layer.bottom.is_some() {
            source.bottom = layer.bottom;
        }
    }
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
    let mut exclusions = Vec::new();
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
                let neighbors = crate::layout::paragraph::paragraph_neighbors(
                    ctx.document,
                    blocks,
                    index,
                    true,
                );
                place_cell_paragraph(
                    ctx,
                    para,
                    left,
                    width,
                    y,
                    items,
                    (note_marker, neighbors),
                    page_frames,
                    &mut exclusions,
                );
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
#[allow(clippy::too_many_lines)]
fn place_cell_paragraph(
    ctx: &LayoutContext<'_>,
    para: &strict_ooxml_wml::model::Paragraph,
    left: f64,
    width: f64,
    y: &mut f64,
    items: &mut Vec<Item>,
    (note_marker, neighbors): (Option<&str>, crate::layout::paragraph::ParagraphNeighbors),
    page_frames: &mut Vec<Item>,
    exclusions: &mut Vec<crate::layout::exclusions::PageExclusion>,
) {
    let grid = if ctx
        .document
        .settings
        .compat_flags
        .adjust_line_height_in_table
    {
        let index = ctx
            .field_env
            .get()
            .map_or(ctx.current_section.get(), |env| {
                env.section_index.saturating_sub(1)
            });
        crate::layout::geometry_for(
            ctx.document
                .sections
                .get(index)
                .map(|section| &section.properties),
            ctx.options.scale,
            None,
            ctx.document.settings.gutter_at_top,
        )
        .grid_line_pitch
    } else {
        None
    };
    let geometry = crate::layout::Geometry {
        width: left + width,
        height: crate::table_style::current_text_flow()
            .and_then(|flow| flow.1)
            .unwrap_or_else(|| ctx.frame_anchor.get().content_height.max(width)),
        left,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        grid_line_pitch: grid,
        columns: crate::layout::ColumnLayout::single(width),
        active_column: 0,
    };
    let host_y = *y;
    let local_exclusions: Vec<_> = exclusions
        .iter()
        .cloned()
        .map(|exclusion| exclusion.into_paragraph_local(host_y, 0.0))
        .collect();
    let flow = layout_paragraph(
        ctx,
        para,
        left,
        width,
        grid,
        note_marker,
        &local_exclusions,
        Some((&geometry, left, host_y)),
        neighbors,
    );
    let mut front_anchors = Vec::new();
    if ctx.options.floating {
        for anchor in &flow.anchors {
            let geometry = if anchor.layout_in_cell {
                geometry
            } else {
                let index = ctx.current_section.get();
                crate::layout::geometry_for(
                    ctx.document
                        .sections
                        .get(index)
                        .map(|section| &section.properties),
                    ctx.options.scale,
                    None,
                    ctx.document.settings.gutter_at_top,
                )
            };
            let Some((w, h)) = crate::layout::floating::resolved_extent(ctx, anchor, &geometry)
            else {
                continue;
            };
            let (ax, ay) = crate::layout::floating::anchor_position(
                anchor,
                &geometry,
                left,
                host_y,
                w,
                h,
                ctx.options.scale,
            );
            let anchor_items = crate::paint::graphics::anchor_items(ctx, anchor, ax, ay, w, h);
            if anchor.layout_in_cell {
                CELL_HEIGHT_REQUIRED.with(|stack| {
                    if let Some(required) = stack.borrow_mut().last_mut() {
                        *required = true;
                    }
                });
                if anchor.behind_doc {
                    items.splice(0..0, anchor_items);
                } else {
                    front_anchors.extend(anchor_items);
                }
                if cell_anchor_wrap_enabled(anchor) {
                    if let Some(exclusion) = crate::layout::floating::page_exclusion(
                        ctx, anchor, &geometry, left, host_y,
                    ) {
                        exclusions.push(exclusion);
                    }
                }
            } else if !capture_anchor(crate::layout::RowAnchor {
                anchor: anchor.clone(),
                x: left,
                y: host_y,
            }) {
                page_frames.extend(anchor_items);
            }
        }
    }
    *y += flow.space_before + flow.border_before;
    for item in flow.flows {
        match item {
            Flow::Line(line) => {
                // The line's own graphics (paragraph shading and borders, bar
                // tabs, formula rules, inline shapes) come with it: shading
                // under the text, the rest over it, as in a frame.
                let (backs, rest): (Vec<_>, Vec<_>) = line
                    .graphics
                    .into_iter()
                    .partition(|item| matches!(item, Item::Rect(_)));
                items.extend(backs.iter().map(|item| offset_item(item, 0.0, *y)));
                for mut text in line.items {
                    text.baseline += *y;
                    items.push(Item::Text(text));
                }
                items.extend(rest.iter().map(|item| offset_item(item, 0.0, *y)));
                *y += line.height;
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
                for mut anchor in row.anchors {
                    anchor.y += *y;
                    capture_anchor(anchor);
                }
                page_frames.extend(row.page_frames);
                *y += row.height;
            }
            Flow::PageBreak | Flow::ColumnBreak => {}
        }
    }
    items.extend(front_anchors);
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
                for mut anchor in row.anchors {
                    anchor.y += *y;
                    capture_anchor(anchor);
                }
                page_frames.extend(row.page_frames);
                *y += row.height;
            }
            Flow::Line(_) | Flow::PageBreak | Flow::ColumnBreak => {}
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

/// Carry page anchors through inline/frame containers without losing their hosts.
fn append_row_anchors(
    ctx: &LayoutContext<'_>,
    anchors: Vec<crate::layout::RowAnchor>,
    dx: f64,
    dy: f64,
    items: &mut Vec<Item>,
) {
    for mut anchor in anchors {
        anchor.x += dx;
        anchor.y += dy;
        if capture_anchor(anchor.clone()) {
            continue;
        }
        let section = ctx
            .document
            .sections
            .get(ctx.current_section.get())
            .map(|s| &s.properties);
        let geometry = crate::layout::body_geometry_for(
            section,
            ctx.options.scale,
            None,
            ctx.document.settings.gutter_at_top,
        );
        let Some((w, h)) = crate::layout::floating::resolved_extent(ctx, &anchor.anchor, &geometry)
        else {
            continue;
        };
        let (x, y) = crate::layout::floating::anchor_position(
            &anchor.anchor,
            &geometry,
            anchor.x,
            anchor.y,
            w,
            h,
            ctx.options.scale,
        );
        items.extend(crate::paint::graphics::anchor_items(
            ctx,
            &anchor.anchor,
            x,
            y,
            w,
            h,
        ));
    }
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
    // A table whose paragraphs share one `w:framePr` is a frame: its Exact
    // lines follow the WPS frame rule, as in `layout_frame_contents` (Clio
    // p. 56, the bold position labels of the framed SNP table).
    let was_in_frame = ctx.in_frame.replace(true);
    let flows = layout_table(ctx, table, origin_x, frame_width.max(1.0), depth, false);
    ctx.in_frame.set(was_in_frame);
    let mut items = Vec::new();
    let mut y = origin_y;
    for flow in flows {
        match flow {
            Flow::TableRow(row) => {
                for item in &row.items {
                    items.push(offset_item(item, 0.0, y));
                }
                append_row_anchors(ctx, row.anchors, 0.0, y, &mut items);
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
            Flow::Line(_) | Flow::PageBreak | Flow::ColumnBreak => {}
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
    let scale = ctx.options.scale;
    let (origin_x, origin_y, frame_width) = frame_placement(frame, &ctx.frame_anchor.get(), scale);
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
pub(crate) fn frame_placement(
    frame: &FrameProperties,
    anchor: &crate::layout::FrameAnchor,
    scale: f64,
) -> (f64, f64, f64) {
    // `w:hAnchor="text"` is the column the frame's paragraph sits in; an
    // auto-width frame takes that column's width (§17.3.1.11).
    let frame_width = frame.width.map_or(anchor.column_width.max(1.0), |width| {
        twips_to_px(width.value(), scale)
    });
    let frame_height = frame
        .height
        .map_or(0.0, |height| twips_to_px(height.value(), scale));
    let (base_x, box_w) = match frame.h_anchor.as_deref() {
        Some("text") => (anchor.column_x, anchor.column_width),
        Some("margin") => (anchor.margin_x, anchor.content_width),
        _ => (0.0, anchor.page_width),
    };
    let (base_y, box_h) = match frame.v_anchor.as_deref() {
        Some("margin" | "text") => (anchor.margin_y, anchor.content_height),
        _ => (0.0, anchor.page_height),
    };
    let origin_x = frame_origin(
        frame.x,
        frame.x_align.as_deref(),
        base_x,
        box_w,
        frame_width,
        scale,
    );
    let origin_y = frame_origin(
        frame.y,
        frame.y_align.as_deref(),
        base_y,
        box_h,
        frame_height,
        scale,
    );
    (origin_x, origin_y, frame_width)
}

/// Page origin of a frame along one axis, from its anchor box: `base` is the
/// box's page coordinate (0 for the page), `box_extent` its size.
///
/// `w:xAlign`/`w:yAlign` replace a raw offset: center and right/bottom are
/// measured in the anchor box.
pub(crate) fn frame_origin(
    twips: Option<i32>,
    align: Option<&str>,
    base: f64,
    box_extent: f64,
    frame_extent: f64,
    scale: f64,
) -> f64 {
    let offset = crate::units::twips_to_px(twips.unwrap_or(0), scale);
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
    for (index, block) in blocks.iter().enumerate() {
        let Block::Paragraph(para) = block else {
            continue;
        };
        let neighbors =
            crate::layout::paragraph::paragraph_neighbors(ctx.document, blocks, index, false);
        let flow = layout_paragraph(
            ctx,
            para,
            0.0,
            frame_width,
            None,
            None,
            &[],
            None,
            neighbors,
        );
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
                    append_row_anchors(ctx, row.anchors, origin_x, origin_y + local_y, &mut items);
                    items.extend(row.page_frames);
                    local_y += row.height;
                }
                // ISO/IEC 29500-1 §17.18.4: page breaks in a frame are ignored
                // (unless `w:showBreaksInFrames`, which is not modelled), and a
                // frame has no columns for a column break to move to.
                Flow::PageBreak | Flow::ColumnBreak => {}
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
    let (origin_x, origin_y, frame_width) = frame_placement(frame, &ctx.frame_anchor.get(), scale);
    let resume = ctx.frame_resume(frame);
    let (items, _anchors, height) =
        layout_frame_contents(ctx, blocks, origin_x, origin_y + resume, frame_width);
    ctx.frame_advance(frame, height);
    items
}

/// Offsets a paint item by `(dx, dy)`.
pub(crate) fn offset_item(item: &Item, dx: f64, dy: f64) -> Item {
    match item {
        Item::Group(group) => {
            let mut group = group.clone();
            group.offset(dx, dy);
            Item::Group(group)
        }
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
    (table, row_borders): (&Table, Option<&Borders>),
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
    let px_per_pt = crate::units::pt_to_px(1.0, ctx.options.scale);
    for (edge, draw, x1, y1, x2, y2) in edges {
        if !draw {
            continue;
        }
        let inside = interior.get(usize::from(edge)).copied().unwrap_or_default();
        let Some(border) = resolve_edge(ctx, (table, row_borders), properties, edge, inside) else {
            continue;
        };
        // A cell edge is centred on the grid line, so a compound band
        // (`double`, `thinThick*`) straddles it as a single line does.
        let half = border.band_pt() * px_per_pt / 2.0;
        let (side, at, from, to) = match edge {
            0 => (BorderSide::Top, y1 + half, x1, x2),
            1 => (BorderSide::Bottom, y1 - half, x1, x2),
            2 => (BorderSide::Left, x1 + half, y1, y2),
            _ => (BorderSide::Right, x1 - half, y1, y2),
        };
        items.extend(edge_items(&border, side, at, from, to, px_per_pt));
    }
    // `w:tl2br` and `w:tr2bl` cross the cell fragment corner to corner.
    let theme = ctx.document.theme.as_ref();
    let diagonals = [
        (properties.diagonal_down.as_ref(), (x, y, right, bottom)),
        (properties.diagonal_up.as_ref(), (x, bottom, right, y)),
    ];
    for (border, (x1, y1, x2, y2)) in diagonals {
        if let Some(border) = border.and_then(|border| resolve_border(border, theme)) {
            items.push(Item::Line(LineItem {
                x1,
                y1,
                x2,
                y2,
                color: border.color,
                width: (border.width_pt * px_per_pt).max(0.5),
                dashed: false,
            }));
        }
    }
}

/// Resolves the stroke for one cell edge (`0` top, `1` bottom, `2` left, `3` right).
fn resolve_edge(
    ctx: &LayoutContext<'_>,
    (table, row_borders): (&Table, Option<&Borders>),
    properties: &CellProperties,
    edge: u8,
    inside: bool,
) -> Option<ResolvedBorder> {
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
    // A row's `w:tblPrEx/w:tblBorders` replaces the table's for that row.
    let row_border = row_borders.and_then(|borders| match table_edge {
        0 => borders.top.as_ref(),
        1 => borders.bottom.as_ref(),
        2 => borders.start.as_ref(),
        3 => borders.end.as_ref(),
        4 => borders.inside_horizontal.as_ref(),
        _ => borders.inside_vertical.as_ref(),
    });
    let table_border = crate::table_style::table_border(ctx.document, table, table_edge);
    cell_border
        .or(row_border)
        .or(table_border)
        .and_then(|border| resolve_border(border, ctx.document.theme.as_ref()))
}

/// The table's own shading (`w:tblPr/w:shd`, or the row's `w:tblPrEx`), which
/// shows behind every cell that has none of its own.
fn table_shading_fill(ctx: &LayoutContext<'_>, table: &Table, row: usize) -> Option<String> {
    let exception = table
        .rows
        .get(row)
        .and_then(|row| row.props.exception.as_ref())
        .and_then(|exception| exception.shading.as_ref());
    exception
        .or(table.props.shading.as_ref())
        .and_then(|shading| crate::style::shading_fill_color(shading, ctx.document.theme.as_ref()))
}

/// Returns the cell shading fill colour, if any (`w:themeFill` resolved through
/// the document theme, as paragraph shading is).
fn shading_fill(ctx: &LayoutContext<'_>, cell: &CellProperties) -> Option<String> {
    cell.shading
        .as_ref()
        .and_then(|shading| crate::style::shading_fill_color(shading, ctx.document.theme.as_ref()))
}

/// Whether a cell's `w:noWrap` lays its text out without line breaks.
///
/// Under [`crate::CellNoWrap::Spec`] that holds only for the automatic table
/// layout and a cell whose preferred width is not fixed (ISO/IEC 29500-1
/// §17.4.30): a fixed `dxa` width keeps the text wrapping.
fn suppresses_breaks(
    ctx: &LayoutContext<'_>,
    table: &Table,
    properties: &strict_ooxml_wml::model::props::CellProperties,
) -> bool {
    if !properties.no_wrap {
        return false;
    }
    match ctx.options.cell_no_wrap {
        crate::CellNoWrap::Always => true,
        crate::CellNoWrap::Spec => {
            table.props.layout != Some(strict_ooxml_wml::model::values::TableLayout::Fixed)
                && !properties
                    .width
                    .is_some_and(|width| width.kind == WidthKind::Dxa && width.value.is_some())
        }
    }
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
