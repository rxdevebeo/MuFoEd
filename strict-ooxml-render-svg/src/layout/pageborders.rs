//! Page-border rendering (`STAGE-5B-TASK.md` §5.5).

use strict_ooxml_wml::model::drawing::ShapeColor;
use strict_ooxml_wml::model::props::{
    BorderOffsetFrom, BorderZOrder, PageBorder, PageBorders, Section,
};
use strict_ooxml_wml::model::values::BorderStyle;

use strict_ooxml_core::error::Result;

use crate::layout::border::{edge_items, ResolvedBorder, Side as BorderSide};
use crate::layout::{geometry_for, Item, LayoutContext, PlacedPage};
use crate::style::resolve_shape_color;
use crate::units::pt_to_px;

/// One page-border edge.
#[derive(Clone, Copy)]
enum Side {
    Top,
    Left,
    Bottom,
    Right,
}

/// The four `w:space` offsets that place the border box inside the page (or
/// the text area).
#[derive(Clone, Copy, Default)]
struct Insets {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// Adds each page's section page borders (AUD-74).
pub(crate) fn apply(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    sections: &[Section],
) -> Result<()> {
    let scale = ctx.options.scale;
    let mirrored = ctx.document.settings.mirror_margins;
    for (index, page) in pages.iter_mut().enumerate() {
        let section = sections
            .get(page.section_index)
            .map(|section| &section.properties);
        let Some(borders) = section.and_then(|section| section.page_borders.as_ref()) else {
            continue;
        };
        if !has_visible_edge(borders) {
            continue;
        }
        let mut geometry = geometry_for(section, scale, None, ctx.document.settings.gutter_at_top);
        // An even page of a mirrored document has its margins swapped.
        if mirrored && index % 2 == 1 {
            std::mem::swap(&mut geometry.left, &mut geometry.right);
        }
        let from_text = matches!(borders.offset_from, Some(BorderOffsetFrom::Text));
        let (left, top, right, bottom) = if from_text {
            (
                geometry.left,
                geometry.top,
                page.width_px - geometry.right,
                page.height_px - geometry.bottom,
            )
        } else {
            (0.0, 0.0, page.width_px, page.height_px)
        };
        let inset = |edge: Option<&PageBorder>| {
            edge.and_then(|edge| edge.space)
                .map_or(0.0, |space| pt_to_px(f64::from(space), scale))
        };
        let pad = Insets {
            left: inset(borders.left.as_ref()),
            top: inset(borders.top.as_ref()),
            right: inset(borders.right.as_ref()),
            bottom: inset(borders.bottom.as_ref()),
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
                items.extend(edge_line(
                    ctx, edge, side, left, top, right, bottom, pad, scale,
                ));
            }
        }
        if items.is_empty() {
            continue;
        }
        ctx.charge_items(items.len())?;
        let behind = matches!(borders.z_order, Some(BorderZOrder::Back));
        if behind {
            let mut combined = items;
            combined.append(&mut page.items);
            page.items = combined;
        } else {
            page.items.extend(items);
        }
    }
    Ok(())
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

/// Builds a line for one page-border edge, clipped to the border box.
///
/// ISO/IEC 29500-1 §17.6.2: `w:pgBorders` describes *one* box drawn around the
/// page (or around the text area, for `w:offsetFrom="text"`). Each edge is
/// offset from its own page edge by `w:space` and runs between the other two
/// edges' offsets, so the four lines close into a rectangle. Drawing each edge
/// at full page length leaves the border open, with the vertical rules running
/// off past the horizontal ones.
#[allow(clippy::too_many_arguments)]
fn edge_line(
    ctx: &LayoutContext<'_>,
    edge: &PageBorder,
    side: Side,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    pad: Insets,
    scale: f64,
) -> Vec<Item> {
    let Some(style) = edge.style.filter(|_| is_visible(edge)) else {
        return Vec::new();
    };
    let color = resolve_shape_color(
        ctx.document.theme.as_ref(),
        &ShapeColor {
            value: edge.color.clone(),
            theme: edge.theme_color.as_deref().cloned(),
            markup: None,
        },
    )
    .unwrap_or_else(|| "#000000".to_owned());
    let border = ResolvedBorder {
        frame: edge.frame,
        shadow: edge.shadow,
        style,
        width_pt: edge
            .size
            .map_or(0.375, |size| f64::from(size.value()) / 8.0)
            .max(0.25),
        color,
        space_pt: 0.0,
    };
    let px_per_pt = pt_to_px(1.0, scale);
    let x0 = left + pad.left;
    let x1 = right - pad.right;
    let y0 = top + pad.top;
    let y1 = bottom - pad.bottom;
    // The edge's band is centred where a single line used to be drawn, so a
    // `double` or `thinThick` page border keeps the box it always had.
    let half = border.band_pt() * px_per_pt / 2.0;
    let (side, at, from, to) = match side {
        Side::Top => (BorderSide::Top, y0 + half, x0, x1),
        Side::Bottom => (BorderSide::Bottom, y1 - half, x0, x1),
        Side::Left => (BorderSide::Left, x0 + half, y0, y1),
        Side::Right => (BorderSide::Right, x1 - half, y0, y1),
    };
    edge_items(&border, side, at, from, to, px_per_pt)
}
