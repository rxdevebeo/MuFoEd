//! Table layout: grid, spans, borders, shading (`STAGE-4-TASK.md` §5.5).

use strict_ooxml_wml::model::props::CellProperties;
use strict_ooxml_wml::model::values::{Border, BorderStyle, CellMargins, Twips, Width, WidthKind};
use strict_ooxml_wml::model::{Block, Table};

use crate::layout::paragraph::layout_paragraph;
use crate::layout::{Flow, Item, LayoutContext, LineItem, RectItem};
use crate::style::parse_color;
use crate::units::{eighths_point_to_px, twips_to_px};

/// Default cell margin in twips (0.075 inch).
const DEFAULT_CELL_MARGIN: i32 = 108;

/// Lays out a table into per-row atomic flows.
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

    let mut flows = Vec::new();
    let mut grid_before = 0usize;
    for row in &table.rows {
        let (items, height) = layout_row(ctx, table, row, &widths, table_x, grid_before, scale);
        flows.push(Flow::Block { items, height });
        grid_before = 0;
    }
    flows
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

/// Lays out one table row.
fn layout_row(
    ctx: &LayoutContext<'_>,
    table: &Table,
    row: &strict_ooxml_wml::model::TableRow,
    widths: &[f64],
    table_x: f64,
    grid_before: usize,
    scale: f64,
) -> (Vec<Item>, f64) {
    let mut items = Vec::new();
    let mut column = grid_before.min(widths.len());
    let mut cells: Vec<CellPlacement> = Vec::new();
    let mut max_height: f64 = 0.0;

    for cell in &row.cells {
        let span = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
        let span = span.min(widths.len().saturating_sub(column).max(1));
        let cell_x: f64 = table_x + widths[..column].iter().sum::<f64>();
        let cell_width: f64 = widths[column..(column + span).min(widths.len())]
            .iter()
            .sum();
        let margins = effective_margins(ctx, table, row, cell);
        let content_width = (cell_width - margins.0 - margins.1).max(1.0);
        let (cell_items, content_height) =
            layout_cell_content(ctx, &cell.blocks, cell_x + margins.0, content_width);
        let height = content_height + margins.2 + margins.3;
        max_height = max_height.max(height);
        cells.push(CellPlacement {
            x: cell_x,
            width: cell_width,
            properties: cell.props.clone(),
            items: cell_items,
            margin_top: margins.2,
        });
        column += span;
    }

    // Row height from `w:trHeight`.
    if let Some(height) = &row.props.height {
        let declared = height
            .value
            .map_or(0.0, |value| twips_to_px(value.value(), scale));
        max_height = max_height.max(declared);
    }
    if max_height <= 0.0 {
        max_height = 1.0;
    }

    // Draw borders/shading and place the cell content.
    for cell in &cells {
        if let Some(fill) = shading_fill(&cell.properties) {
            items.push(Item::Rect(RectItem {
                x: cell.x,
                y: 0.0,
                w: cell.width,
                h: max_height,
                fill: Some(fill),
                stroke: None,
                stroke_w: 0.0,
            }));
        }
        for item in &cell.items {
            items.push(offset_item(item, 0.0, cell.margin_top));
        }
        push_borders(ctx, table, cell, max_height, &mut items);
    }

    (items, max_height)
}

/// A placed table cell.
struct CellPlacement {
    x: f64,
    width: f64,
    properties: CellProperties,
    items: Vec<Item>,
    margin_top: f64,
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
    let value = |margin: Option<Twips>| {
        twips_to_px(margin.map_or(DEFAULT_CELL_MARGIN, Twips::value), scale)
    };
    (
        value(source.start),
        value(source.end),
        value(source.top),
        value(source.bottom),
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
    layout_blocks_inline(ctx, blocks, left, width, &mut y, &mut items, 0);
    (items, y)
}

/// Stacks blocks vertically (used inside cells; page breaks are ignored).
#[allow(clippy::too_many_arguments)]
fn layout_blocks_inline(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    y: &mut f64,
    items: &mut Vec<Item>,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    for block in blocks {
        match block {
            Block::Paragraph(para) => {
                let flow = layout_paragraph(ctx, para, left, width);
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
                        } => {
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
                    } = flow
                    {
                        for item in block_items {
                            items.push(offset_item(&item, 0.0, *y));
                        }
                        *y += height;
                    }
                }
            }
            Block::SdtBlock(sdt) => {
                layout_blocks_inline(ctx, &sdt.blocks, left, width, y, items, depth + 1);
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

/// Emits the four borders of a cell.
fn push_borders(
    ctx: &LayoutContext<'_>,
    table: &Table,
    cell: &CellPlacement,
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
        if let Some(stroke) = resolve_edge(ctx, table, cell, edge) {
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
    cell: &CellPlacement,
    edge: u8,
) -> Option<Stroke> {
    let cell_border = match edge {
        0 => cell.properties.borders.top.as_ref(),
        1 => cell.properties.borders.bottom.as_ref(),
        2 => cell.properties.borders.start.as_ref(),
        _ => cell.properties.borders.end.as_ref(),
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
