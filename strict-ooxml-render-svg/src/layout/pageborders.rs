//! Page-border rendering (`STAGE-5B-TASK.md` §5.5).

use strict_ooxml_wml::model::drawing::ShapeColor;
use strict_ooxml_wml::model::props::{
    BorderOffsetFrom, BorderZOrder, PageBorder, PageBorders, SectionProperties,
};
use strict_ooxml_wml::model::values::BorderStyle;

use crate::layout::{Geometry, Item, LayoutContext, LineItem, PlacedPage};
use crate::style::resolve_shape_color;
use crate::units::{eighths_point_to_px, pt_to_px};

/// One page-border edge.
#[derive(Clone, Copy)]
enum Side {
    Top,
    Left,
    Bottom,
    Right,
}

/// Adds the section's page borders to every page.
pub(crate) fn apply(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    geometry: &Geometry,
    section: Option<&SectionProperties>,
) {
    let Some(borders) = section.and_then(|section| section.page_borders.as_ref()) else {
        return;
    };
    if !has_visible_edge(borders) {
        return;
    }
    let scale = ctx.options.scale;
    let from_text = matches!(borders.offset_from, Some(BorderOffsetFrom::Text));
    let (left, top, right, bottom) = if from_text {
        (
            geometry.left,
            geometry.top,
            geometry.width - geometry.right,
            geometry.height - geometry.bottom,
        )
    } else {
        (0.0, 0.0, geometry.width, geometry.height)
    };
    let edges = [
        (Side::Top, &borders.top),
        (Side::Left, &borders.left),
        (Side::Bottom, &borders.bottom),
        (Side::Right, &borders.right),
    ];
    let mut items = Vec::new();
    for (side, edge) in edges {
        if let Some(edge) = edge {
            if let Some(line) = edge_line(ctx, edge, side, left, top, right, bottom, scale) {
                items.push(line);
            }
        }
    }
    if items.is_empty() {
        return;
    }
    let behind = matches!(borders.z_order, Some(BorderZOrder::Back));
    for page in pages.iter_mut() {
        if behind {
            let mut combined = items.clone();
            combined.append(&mut page.items);
            page.items = combined;
        } else {
            page.items.extend(items.clone());
        }
    }
}

/// Returns `true` when at least one edge is visible.
fn has_visible_edge(borders: &PageBorders) -> bool {
    [&borders.top, &borders.left, &borders.bottom, &borders.right]
        .into_iter()
        .any(|edge| edge.as_ref().is_some_and(is_visible))
}

/// Returns `true` when an edge draws a line.
fn is_visible(edge: &PageBorder) -> bool {
    edge.style
        .is_some_and(|style| !matches!(style, BorderStyle::Nil | BorderStyle::None))
}

/// Builds a line for one page-border edge, offset outward by its `space`.
#[allow(clippy::too_many_arguments)]
fn edge_line(
    ctx: &LayoutContext<'_>,
    edge: &PageBorder,
    side: Side,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    scale: f64,
) -> Option<Item> {
    if !is_visible(edge) {
        return None;
    }
    let color = resolve_shape_color(
        ctx.document.theme.as_ref(),
        &ShapeColor {
            value: edge.color.clone(),
            theme: edge.theme_color.clone(),
        },
    )
    .unwrap_or_else(|| "#000000".to_owned());
    let width = edge
        .size
        .map_or(0.5, |size| eighths_point_to_px(size.value(), scale))
        .max(0.25);
    let space = pt_to_px(f64::from(edge.space.unwrap_or(0)), scale);
    let (x1, y1, x2, y2) = match side {
        Side::Top => (left, top + space, right, top + space),
        Side::Bottom => (left, bottom - space, right, bottom - space),
        Side::Left => (left + space, top, left + space, bottom),
        Side::Right => (right - space, top, right - space, bottom),
    };
    let dashed = matches!(
        edge.style,
        Some(
            BorderStyle::Dashed
                | BorderStyle::Dotted
                | BorderStyle::DashSmallGap
                | BorderStyle::DotDash
                | BorderStyle::DotDotDash
        )
    );
    Some(Item::Line(LineItem {
        x1,
        y1,
        x2,
        y2,
        color,
        width,
        dashed,
    }))
}
