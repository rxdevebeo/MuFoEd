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
#[must_use]
pub(crate) fn layout_table(
    ctx: &LayoutContext<'_>,
    table: &Table,
    content_left: f64,
    content_width: f64,
) -> Vec<Flow> {
    let scale = ctx.options.scale;
    let column_count = column_count(table);
    if column_count == 0 {
        return Vec::new();
    }
    let widths = column_widths(ctx, table, column_count, content_width);
    let total_width: f64 = widths.iter().sum();
    let table_x = table_x(ctx, table, content_left, content_width, total_width);

    let mut rows: Vec<RawRow> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut column = 0usize;
        let mut cells = Vec::with_capacity(row.cells.len());
        let mut max_content: f64 = 0.0;
        for cell in &row.cells {
            let span = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
            let span = span.min(widths.len().saturating_sub(column).max(1));
            let x: f64 = table_x + widths[..column].iter().sum::<f64>();
            let width: f64 = widths[column..(column + span).min(widths.len())]
                .iter()
                .sum();
            let margins = effective_margins(ctx, table, row, cell);
            let content_width = (width - margins.0 - margins.1).max(1.0);
            let (items, content_height) =
                layout_cell_content(ctx, &cell.blocks, x + margins.0, content_width);
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
        let mut height = max_content;
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
            cells,
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

/// Number of grid columns: explicit grid, else the widest row's span.
fn column_count(table: &Table) -> usize {
    if !table.grid.is_empty() {
        return table.grid.len();
    }
    table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| usize::from(cell.props.grid_span.unwrap_or(1)))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0)
}

/// Computes column widths in px.
fn column_widths(
    ctx: &LayoutContext<'_>,
    table: &Table,
    column_count: usize,
    content_width: f64,
) -> Vec<f64> {
    let scale = ctx.options.scale;
    let total = table_total_width(ctx, table, content_width);
    let grid: Vec<f64> = table
        .grid
        .iter()
        .map(|col| twips_to_px(col.width.map_or(0, Twips::value), scale))
        .collect();
    let grid_sum: f64 = grid.iter().sum();
    if grid.len() == column_count && grid_sum > 0.0 {
        let factor = total / grid_sum;
        return grid.into_iter().map(|width| width * factor).collect();
    }
    let equal = total / column_count as f64;
    vec![equal; column_count]
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
) -> (Vec<Item>, f64) {
    let mut items = Vec::new();
    let mut y = 0.0;
    layout_blocks_inline(ctx, blocks, left, width, &mut y, &mut items, 0, None);
    (items, y)
}

/// Stacks blocks vertically (used inside cells; page breaks are ignored).
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_blocks_inline(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    y: &mut f64,
    items: &mut Vec<Item>,
    depth: usize,
    note_marker: Option<&str>,
) {
    if depth > 8 {
        return;
    }
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
                for flow in layout_table(ctx, table, left, width) {
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
