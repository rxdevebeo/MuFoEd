//! Table layout: grid, spans, vertical merges, borders, shading
//! (`STAGE-4-TASK.md` §5.5, `STAGE-5-TASK.md` §5.7–§5.8).

use strict_ooxml_wml::model::props::CellProperties;
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
    let total_width: f64 = widths.iter().sum();
    let table_x = table_x(ctx, table, content_left, content_width, total_width);

    let mut rows: Vec<RawRow> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let raw = layout_row(ctx, table, row, &widths, table_x, depth);
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

    let mut flows = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let mut items = Vec::new();
        for cell in &row.cells {
            // A continuation cell draws nothing: the restart cell owns the
            // merged region's background, borders and content.
            if cell.vmerge == VState::Continue {
                continue;
            }
            let region_height = if cell.vmerge == VState::Restart {
                merged_height(&rows, row_index, cell)
            } else {
                row.height
            };
            if let Some(fill) = shading_fill(&cell.properties) {
                items.push(Item::Rect(RectItem {
                    x: cell.x,
                    y: 0.0,
                    w: cell.width,
                    h: region_height,
                    fill: Some(fill),
                    stroke: None,
                    stroke_w: 0.0,
                }));
            }
            for item in &cell.items {
                items.push(offset_item(item, 0.0, cell.margin_top));
            }
            push_borders(ctx, table, cell, region_height, &mut items);
        }
        flows.push(Flow::TableRow(TableRowFlow {
            items,
            height: row.height,
            header: row.header,
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
fn layout_row(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row: &strict_ooxml_wml::model::TableRow,
    widths: &[f64],
    table_x: f64,
    depth: u32,
) -> LaidOutRow {
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
        let margins = effective_margins(ctx, table, row, cell);
        // `get(..)` rather than a slice: `column` is the sum of the spans before
        // `w:gridSpan` of 65535 puts it far past the end, which is where the
        // old index range used to end the process (AUD-08).
        //
        // The slice the range denotes is summed, not the elements: `&[f64]`'s
        // `Sum` is pairwise and an iterator's is a linear fold, and the
        // converter's table detection turns on the last bit of a column
        // position. "Not a panic" is the fix; "a different number" would be a
        // second change nobody asked for.
        let x: f64 = table_x + sum_slice(widths.get(..column));
        let end = column.saturating_add(span).min(widths.len());
        let width: f64 = sum_slice(widths.get(column..end)).max(f64::MIN_POSITIVE);
        let cell_content_width = (width - margins.0 - margins.1).max(1.0);
        let (items, content_height) = layout_cell_content(
            ctx,
            &cell.blocks,
            x + margins.0,
            cell_content_width,
            depth + 1,
        );
        let height = content_height + margins.2 + margins.3;
        max_content = max_content.max(height);
        cells.push(RawCell {
            col: column,
            span,
            x,
            width,
            properties: cell.props.clone(),
            items,
            margin_top: margins.2,
            vmerge: VState::from_merge(cell.props.vertical_merge),
        });
        column += span;
    }
    LaidOutRow {
        cells,
        height: max_content,
    }
}

/// Returns the total height of the merged region starting at `row`/`cell`.
fn merged_height(rows: &[RawRow], row: usize, cell: &RawCell) -> f64 {
    let mut total = rows[row].height;
    let mut index = row + 1;
    while index < rows.len() {
        let continues = rows[index].cells.iter().any(|candidate| {
            candidate.vmerge == VState::Continue
                && candidate.col == cell.col
                && candidate.span == cell.span
        });
        if !continues {
            break;
        }
        total += rows[index].height;
        index += 1;
    }
    total
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
        Some(Width {
            kind: WidthKind::Dxa,
            value: Some(value),
        }) => twips_to_px(value, scale),
        Some(Width {
            kind: WidthKind::Pct,
            value: Some(value),
        }) => content_width * f64::from(value) / 5000.0,
        _ => {
            let grid: i32 = table
                .grid
                .iter()
                .map(|col| col.width.map_or(0, Twips::value))
                .sum();
            if grid > 0 {
                twips_to_px(grid, scale)
            } else {
                content_width
            }
        }
    };
    width.clamp(1.0, content_width)
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
    match table.props.alignment {
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

/// Effective cell margins `(left, right, top, bottom)` in px.
fn effective_margins(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row: &strict_ooxml_wml::model::TableRow,
    cell: &strict_ooxml_wml::model::TableCell,
) -> (f64, f64, f64, f64) {
    let scale = ctx.options.scale;
    let source = if cell.props.margins != CellMargins::default() {
        cell.props.margins
    } else if row.props.cell_margins != CellMargins::default() {
        row.props.cell_margins
    } else {
        table.props.cell_margins
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

/// Lays out cell block content, returning items (y relative) and height.
fn layout_cell_content(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    depth: u32,
) -> (Vec<Item>, f64) {
    let mut items = Vec::new();
    let mut y = 0.0;
    layout_blocks_inline(ctx, blocks, left, width, &mut y, &mut items, depth, None);
    (items, y)
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_blocks_inline(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    y: &mut f64,
    items: &mut Vec<Item>,
    depth: u32,
    note_marker: Option<&str>,
) {
    if depth > ctx.options.limits.max_block_nesting {
        return;
    }
    ctx.set_block_depth(depth);
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let flow = layout_paragraph(ctx, para, left, width, None, note_marker);
                *y += flow.space_before;
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
                        }
                        | Flow::TableRow(TableRowFlow {
                            items: block_items,
                            height,
                            ..
                        }) => {
                            for item in block_items {
                                items.push(offset_item(&item, 0.0, *y));
                            }
                            *y += height;
                        }
                        Flow::PageBreak => {}
                    }
                }
                *y += flow.space_after;
            }
            Block::Table(table) => {
                for flow in layout_table(ctx, table, left, width, depth) {
                    if let Flow::Block {
                        items: block_items,
                        height,
                    }
                    | Flow::TableRow(TableRowFlow {
                        items: block_items,
                        height,
                        ..
                    }) = flow
                    {
                        for item in block_items {
                            items.push(offset_item(&item, 0.0, *y));
                        }
                        *y += height;
                    }
                }
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
                );
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
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

/// Emits the four borders of a cell region.
fn push_borders(
    ctx: &LayoutContext<'_>,
    table: &Table,
    cell: &RawCell,
    height: f64,
    items: &mut Vec<Item>,
) {
    let x = cell.x;
    let y = 0.0;
    let right = x + cell.width;
    let bottom = y + height;
    let edges = [
        (0u8, x, y, right, y),
        (1, x, bottom, right, bottom),
        (2, x, y, x, bottom),
        (3, right, y, right, bottom),
    ];
    for (edge, x1, y1, x2, y2) in edges {
        if let Some(stroke) = resolve_edge(ctx, table, &cell.properties, edge) {
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
) -> Option<Stroke> {
    let cell_border = match edge {
        0 => properties.borders.top.as_ref(),
        1 => properties.borders.bottom.as_ref(),
        2 => properties.borders.start.as_ref(),
        _ => properties.borders.end.as_ref(),
    };
    let table_border = match edge {
        0 => table.props.borders.top.as_ref(),
        1 => table.props.borders.bottom.as_ref(),
        2 => table.props.borders.start.as_ref(),
        _ => table.props.borders.end.as_ref(),
    };
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

/// Returns the cell/row/table shading fill colour, if any.
fn shading_fill(cell: &CellProperties) -> Option<String> {
    cell.shading
        .as_ref()
        .and_then(|shading| shading.fill.as_ref())
        .and_then(parse_color)
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
