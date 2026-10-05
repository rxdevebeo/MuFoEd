//! Anchored (floating) object placement (`STAGE-5B-TASK.md` §5.2, §5.3).

use strict_ooxml_wml::model::drawing::{AnchorDrawing, Position, RelativeSize, WrapKind};
use strict_ooxml_wml::model::values::Emu;

use strict_ooxml_core::error::Result;
use strict_ooxml_wml::model::props::Section;

use crate::layout::{geometry_for, Geometry, Item, LayoutContext, PlacedPage};
use crate::units::emu_to_px;

/// An anchored object recorded during pagination.
pub(crate) struct PendingAnchor {
    /// Zero-based page index the host paragraph started on.
    pub page: usize,
    /// Absolute page x of the paragraph's leading edge, in px.
    pub host_x: f64,
    /// Absolute page y of the paragraph top, in px.
    pub host_y: f64,
    /// The parsed anchor.
    pub anchor: AnchorDrawing,
}

/// Resolves pending anchors into paint items and inserts them into their pages.
pub(crate) fn resolve(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    anchors: &[PendingAnchor],
    sections: &[Section],
) -> Result<()> {
    if !ctx.options.floating {
        return Ok(());
    }
    let scale = ctx.options.scale;
    // Behind-document objects are inserted at the page front; the rest appended.
    let mut behind: Vec<(usize, i64, Vec<Item>)> = Vec::new();
    let mut front: Vec<(usize, i64, Vec<Item>)> = Vec::new();
    for pending in anchors {
        let section = pages
            .get(pending.page)
            .and_then(|page| sections.get(page.section_index))
            .map(|section| &section.properties);
        let geometry = geometry_for(section, scale, None);
        let w = resolved_axis(
            ctx,
            pending.anchor.size_rel_h.as_ref(),
            true,
            &geometry,
            pending
                .anchor
                .extent
                .map_or(0.0, |extent| emu_to_px(extent.cx.value(), scale)),
        );
        let h = resolved_axis(
            ctx,
            pending.anchor.size_rel_v.as_ref(),
            false,
            &geometry,
            pending
                .anchor
                .extent
                .map_or(0.0, |extent| emu_to_px(extent.cy.value(), scale)),
        );
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let x = resolve_h(
            pending.anchor.position_h.as_ref(),
            &geometry,
            pending.host_x,
            w,
            scale,
        );
        let y = resolve_v(
            pending.anchor.position_v.as_ref(),
            &geometry,
            pending.host_y,
            h,
            scale,
        );
        let items = crate::paint::graphics::anchor_items(ctx, &pending.anchor, x, y, w, h);
        if items.is_empty() {
            continue;
        }
        let order = pending.anchor.relative_height.map_or(0, i64::from);
        let entry = (pending.page, order, items);
        if pending.anchor.behind_doc {
            behind.push(entry);
        } else {
            front.push(entry);
        }
    }
    behind.sort_by_key(|(page, order, _)| (*page, *order));
    front.sort_by_key(|(page, order, _)| (*page, *order));
    for (page, _, mut items) in behind {
        if let Some(page) = pages.get_mut(page) {
            ctx.charge_items(items.len())?;
            let mut combined = std::mem::take(&mut items);
            combined.append(&mut page.items);
            page.items = combined;
        }
    }
    for (page, _, items) in front {
        if let Some(page) = pages.get_mut(page) {
            ctx.charge_items(items.len())?;
            page.items.extend(items);
        }
    }
    Ok(())
}

/// Resolves a horizontal position to an absolute page x.
fn resolve_h(pos: Option<&Position>, geometry: &Geometry, host_x: f64, w: f64, scale: f64) -> f64 {
    let relative = pos.and_then(|pos| pos.relative_from.as_deref());
    let (origin, span) = horizontal_box(relative, geometry, host_x);
    if let Some(align) = pos.and_then(|pos| pos.align.as_deref()) {
        return match align {
            "center" => origin + (span - w) / 2.0,
            "right" | "outside" => origin + (span - w).max(0.0),
            _ => origin,
        };
    }
    let offset = pos
        .and_then(|pos| pos.offset)
        .map_or(0.0, |offset: Emu| emu_to_px(offset.value(), scale));
    origin + offset
}

/// Resolves a vertical position to an absolute page y.
fn resolve_v(pos: Option<&Position>, geometry: &Geometry, host_y: f64, h: f64, scale: f64) -> f64 {
    let relative = pos.and_then(|pos| pos.relative_from.as_deref());
    let (origin, span) = vertical_box(relative, geometry, host_y);
    if let Some(align) = pos.and_then(|pos| pos.align.as_deref()) {
        return match align {
            "center" => origin + (span - h) / 2.0,
            "bottom" | "outside" => origin + (span - h).max(0.0),
            _ => origin,
        };
    }
    let offset = pos
        .and_then(|pos| pos.offset)
        .map_or(0.0, |offset: Emu| emu_to_px(offset.value(), scale));
    origin + offset
}

/// Length of one axis. A positive `wp14` percent replaces the fallback extent.
fn resolved_axis(
    ctx: &LayoutContext<'_>,
    relative: Option<&RelativeSize>,
    horizontal: bool,
    geometry: &Geometry,
    fallback: f64,
) -> f64 {
    let Some(relative) = relative else {
        return fallback.max(0.0);
    };
    if relative.percent == 0 {
        ctx.warn("render.sizeRel: a zero percent does not replace the fallback extent".to_owned());
        return fallback.max(0.0);
    }
    let span = size_span(relative.relative_from.as_deref(), horizontal, geometry);
    let Some(span) = span else {
        ctx.warn(
            "render.sizeRel: the relative reference is missing or unknown; keeping the extent"
                .to_owned(),
        );
        return fallback.max(0.0);
    };
    span * f64::from(relative.percent) / 100_000.0
}

fn size_span(relative: Option<&str>, horizontal: bool, geometry: &Geometry) -> Option<f64> {
    let span = match relative {
        Some("page") => {
            if horizontal {
                geometry.width
            } else {
                geometry.height
            }
        }
        Some("margin" | "column") => {
            if horizontal {
                geometry.content_width()
            } else {
                geometry.content_height()
            }
        }
        Some("leftMargin" | "insideMargin") if horizontal => geometry.left,
        Some("rightMargin" | "outsideMargin") if horizontal => geometry.right,
        Some("topMargin") if !horizontal => geometry.top,
        Some("bottomMargin") if !horizontal => geometry.bottom,
        _ => return None,
    };
    Some(span.max(0.0))
}

fn horizontal_box(relative: Option<&str>, geometry: &Geometry, host_x: f64) -> (f64, f64) {
    match relative {
        Some("page") => (0.0, geometry.width),
        Some("rightMargin" | "outsideMargin") => (geometry.width - geometry.right, geometry.right),
        Some("leftMargin" | "insideMargin") => (0.0, geometry.left),
        Some("margin" | "column") => (geometry.left, geometry.content_width()),
        _ => (host_x, geometry.content_width()),
    }
}

fn vertical_box(relative: Option<&str>, geometry: &Geometry, host_y: f64) -> (f64, f64) {
    match relative {
        Some("page") => (0.0, geometry.height),
        Some("topMargin") => (0.0, geometry.top),
        Some("bottomMargin") => (geometry.height - geometry.bottom, geometry.bottom),
        Some("paragraph" | "line") => (host_y, geometry.content_height()),
        // `margin` shares the content box with an omitted reference.
        _ => (geometry.top, geometry.content_height()),
    }
}

/// Returns `true` if the anchor reserves vertical flow space (`wrapTopAndBottom`).
#[must_use]
pub(crate) fn reserves_vertical_space(anchor: &AnchorDrawing) -> bool {
    anchor
        .wrap
        .as_ref()
        .is_some_and(|wrap| matches!(wrap.kind, WrapKind::TopAndBottom))
}

/// Which side of a square wrap may hold text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WrapSide {
    /// Text on both sides of the object.
    Both,
    /// Text only to the left.
    Left,
    /// Text only to the right.
    Right,
    /// Text only in the wider side.
    Largest,
}

/// A square-wrap rectangle in the same coordinates as the paragraph lines.
pub(crate) struct WrapExclusion {
    /// Left edge, already expanded by `distL`.
    pub left: f64,
    /// Right edge, already expanded by `distR`.
    pub right: f64,
    /// Top edge, paragraph-local, already expanded by `distT`.
    pub top: f64,
    /// Bottom edge, paragraph-local, already expanded by `distB`.
    pub bottom: f64,
    /// Side policy.
    pub side: WrapSide,
}

/// Square wrap box for line breaking, or `None` when the anchor does not exclude text.
///
/// Tight and through contours are not reported as a rectangle. Page-relative
/// vertical anchors are skipped here: their page y is not known while the
/// paragraph is still being broken into lines.
pub(crate) fn square_exclusion(
    ctx: &LayoutContext<'_>,
    anchor: &AnchorDrawing,
    content_left: f64,
    content_width: f64,
) -> Option<WrapExclusion> {
    let wrap = anchor.wrap.as_ref()?;
    match wrap.kind {
        WrapKind::Square => {}
        WrapKind::Tight | WrapKind::Through => {
            ctx.warn(
                "render.wrap: a tight or through contour is unsupported and does not exclude text"
                    .to_owned(),
            );
            return None;
        }
        WrapKind::None | WrapKind::TopAndBottom => return None,
    }
    let extent = anchor.extent?;
    let scale = ctx.options.scale;
    let width = emu_to_px(extent.cx.value(), scale);
    let height = emu_to_px(extent.cy.value(), scale);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let vertical = anchor
        .position_v
        .as_ref()
        .and_then(|position| position.relative_from.as_deref());
    if !matches!(vertical, None | Some("paragraph" | "line")) {
        ctx.warn(
            "render.wrap: a square wrap that is not paragraph-relative is not applied to line breaks"
                .to_owned(),
        );
        return None;
    }
    let x = horizontal_origin(
        anchor.position_h.as_ref(),
        content_left,
        content_width,
        scale,
    );
    let y = anchor
        .position_v
        .as_ref()
        .and_then(|position| position.offset)
        .map_or(0.0, |offset| emu_to_px(offset.value(), scale));
    let dist = |anchor_dist: Option<u32>, wrap_dist: Option<u32>| {
        emu_to_px(i64::from(anchor_dist.or(wrap_dist).unwrap_or(0)), scale)
    };
    let left = x - dist(anchor.dist_left, wrap.dist_left);
    let right = x + width + dist(anchor.dist_right, wrap.dist_right);
    let top = y - dist(anchor.dist_top, wrap.dist_top);
    let bottom = y + height + dist(anchor.dist_bottom, wrap.dist_bottom);
    let side = match wrap.wrap_text.as_deref() {
        Some("left") => WrapSide::Left,
        Some("right") => WrapSide::Right,
        Some("largest") => WrapSide::Largest,
        _ => WrapSide::Both,
    };
    Some(WrapExclusion {
        left,
        right,
        top,
        bottom,
        side,
    })
}

fn horizontal_origin(
    position: Option<&Position>,
    content_left: f64,
    content_width: f64,
    scale: f64,
) -> f64 {
    let offset = position
        .and_then(|position| position.offset)
        .map_or(0.0, |offset| emu_to_px(offset.value(), scale));
    match position.and_then(|position| position.relative_from.as_deref()) {
        Some("page") => offset,
        Some("rightMargin" | "outsideMargin") => content_left + content_width + offset,
        _ => content_left + offset,
    }
}
