//! Anchored (floating) object placement (`STAGE-5B-TASK.md` §5.2, §5.3).

use strict_ooxml_wml::model::drawing::{AnchorDrawing, Position, RelativeSize, WrapKind};
use strict_ooxml_wml::model::values::Emu;

use strict_ooxml_core::error::Result;
use strict_ooxml_wml::model::props::Section;

use crate::layout::exclusions::{Contour, PageExclusion};
#[cfg(test)]
use crate::layout::geometry_for;
use crate::layout::{Geometry, Item, LayoutContext, OffPageReason, PlacedPage, OFF_PAGE_TOLERANCE};
use crate::units::emu_to_px;
use std::sync::Arc;

/// An anchored object recorded during pagination.
pub(crate) struct PendingAnchor {
    /// Zero-based page index the host paragraph started on.
    pub page: usize,
    /// Absolute page x of the paragraph's leading edge, in px.
    pub host_x: f64,
    /// Absolute page y of the paragraph top, in px.
    pub host_y: f64,
    /// Body column (reading order) the host paragraph started in: the
    /// `column` reference box of the anchor.
    pub column: usize,
    /// The parsed anchor.
    pub anchor: AnchorDrawing,
}

/// Resolves pending anchors into paint items and inserts them into their pages.
#[allow(clippy::too_many_lines)]
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
    let mut occupied: Vec<(usize, [f64; 4], bool)> = Vec::new();
    for pending in anchors {
        let section = pages
            .get(pending.page)
            .and_then(|page| sections.get(page.section_index))
            .map(|section| &section.properties);
        let mut geometry = crate::layout::body_geometry_for(
            section,
            scale,
            None,
            ctx.document.settings.gutter_at_top,
            ctx.options.default_paper,
        );
        geometry.active_column = pending.column;
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
        let (x, mut y) = anchor_position(
            &pending.anchor,
            &geometry,
            pending.host_x,
            pending.host_y,
            w,
            h,
            scale,
        );
        for _ in 0..=occupied.len() {
            let bottom = occupied
                .iter()
                .filter(|(page, rect, allowed)| {
                    *page == pending.page
                        && (!pending.anchor.allow_overlap || !allowed)
                        && x < rect[0] + rect[2]
                        && x + w > rect[0]
                        && y < rect[1] + rect[3]
                        && y + h > rect[1]
                })
                .map(|(_, rect, _)| rect[1] + rect[3])
                .fold(y, f64::max);
            if bottom <= y + 0.001 {
                break;
            }
            y = bottom;
        }
        // A frame whose box leaves the sheet (cluster 3: a duplicate text box
        // anchored far left, margin notes). The mark is diagnostic;
        // `OffPageInk::Clamp` pulls the frame back onto the page horizontally.
        let (x, off_page) = clamp_frame(ctx, &geometry, x, y, w, h);
        let mut items = crate::paint::graphics::anchor_items(ctx, &pending.anchor, x, y, w, h);
        if items.is_empty() {
            continue;
        }
        if off_page {
            mark_floating_items(&mut items);
        }
        occupied.push((pending.page, [x, y, w, h], pending.anchor.allow_overlap));
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
    // Each behind-text item goes in front of what the page holds so far, so
    // they are taken highest first: in ascending order the lowest was put in
    // front last and a page-sized background covered the picture set on it.
    for (page, _, mut items) in behind.into_iter().rev() {
        if let Some(page) = pages.get_mut(page) {
            ctx.charge_items(items.iter().map(|item| item.leaves().count()).sum())?;
            let mut combined = std::mem::take(&mut items);
            combined.append(&mut page.items);
            page.items = combined;
        }
    }
    for (page, _, items) in front {
        if let Some(page) = pages.get_mut(page) {
            ctx.charge_items(items.iter().map(|item| item.leaves().count()).sum())?;
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
    let offset = pos.and_then(|pos| pos.percent_offset).map_or_else(
        || {
            pos.and_then(|pos| pos.offset)
                .map_or(0.0, |offset: Emu| emu_to_px(offset.value(), scale))
        },
        |percent| span * f64::from(percent) / 100_000.0,
    );
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
    let offset = pos.and_then(|pos| pos.percent_offset).map_or_else(
        || {
            pos.and_then(|pos| pos.offset)
                .map_or(0.0, |offset: Emu| emu_to_px(offset.value(), scale))
        },
        |percent| span * f64::from(percent) / 100_000.0,
    );
    origin + offset
}

/// Shared origin for both painting and text exclusions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn anchor_position(
    anchor: &AnchorDrawing,
    geometry: &Geometry,
    host_x: f64,
    host_y: f64,
    w: f64,
    h: f64,
    scale: f64,
) -> (f64, f64) {
    if anchor.simple_pos {
        let (x, y) = anchor.simple_pos_point.unwrap_or((0, 0));
        return (emu_to_px(x, scale), emu_to_px(y, scale));
    }
    (
        resolve_h(anchor.position_h.as_ref(), geometry, host_x, w, scale),
        resolve_v(anchor.position_v.as_ref(), geometry, host_y, h, scale),
    )
}

/// Length of one axis. A positive `wp14` percent replaces the fallback extent.
pub(crate) fn resolved_axis(
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
        Some("margin") => {
            if horizontal {
                geometry.content_width()
            } else {
                geometry.content_height()
            }
        }
        // ISO/IEC 29500-1 §20.4.3.4: the column the anchor sits in.
        Some("column") => {
            if horizontal {
                geometry.active_column_box().1
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
        Some("margin") => (geometry.left, geometry.content_width()),
        Some("column") => geometry.active_column_box(),
        _ => (host_x, geometry.active_column_box().1),
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

/// A wrap rectangle in the same coordinates as the paragraph lines.
#[derive(Clone, Debug)]
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
    /// Shared page-space contour, when the source supplies one.
    pub contour: Option<Arc<Contour>>,
    /// Paragraph origin for sampling the page-space contour.
    pub page_y: f64,
    /// Page x of the paragraph's local x = 0 minus the paragraph `left`: how
    /// far the column the paragraph is laid out for sits from the first one.
    /// Page-space contour intervals are moved back by it.
    pub page_dx: f64,
}

/// Resolved width/height for an anchor, matching paint placement (`sizeRel` wins).
#[must_use]
pub(crate) fn resolved_extent(
    ctx: &LayoutContext<'_>,
    anchor: &AnchorDrawing,
    geometry: &Geometry,
) -> Option<(f64, f64)> {
    let scale = ctx.options.scale;
    let fallback_w = anchor
        .extent
        .map_or(0.0, |extent| emu_to_px(extent.cx.value(), scale));
    let fallback_h = anchor
        .extent
        .map_or(0.0, |extent| emu_to_px(extent.cy.value(), scale));
    let width = resolved_axis(ctx, anchor.size_rel_h.as_ref(), true, geometry, fallback_w);
    let height = resolved_axis(ctx, anchor.size_rel_v.as_ref(), false, geometry, fallback_h);
    if width <= 0.0 || height <= 0.0 {
        None
    } else {
        Some((width, height))
    }
}

/// Clamps a floating frame's resolved origin onto the page horizontally when
/// [`crate::OffPageInk::Clamp`] asks for it, and reports whether its box left
/// the page (so its text can carry [`OffPageReason::Floating`]). The two
/// anchor placements — the body pass ([`resolve`]) and the cell pass in
/// `table.rs` — share this so the mark and the correction cannot disagree.
pub(crate) fn clamp_frame(
    ctx: &LayoutContext<'_>,
    geometry: &Geometry,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> (f64, bool) {
    let off = x < -OFF_PAGE_TOLERANCE
        || x + w > geometry.width + OFF_PAGE_TOLERANCE
        || y < -OFF_PAGE_TOLERANCE
        || y + h > geometry.height + OFF_PAGE_TOLERANCE;
    if off && ctx.options.off_page_ink == crate::OffPageInk::Clamp {
        (x.clamp(0.0, (geometry.width - w).max(0.0)), off)
    } else {
        (x, off)
    }
}

/// Tags every text leaf of a frame's items with the [`OffPageReason::Floating`]
/// origin reason; the layout's off-page pass keeps it only if the box really
/// leaves the page.
pub(crate) fn mark_floating_items(items: &mut [Item]) {
    for item in items {
        for leaf in item.leaves_mut() {
            if let Item::Text(text) = leaf {
                text.off_page = Some(OffPageReason::Floating);
            }
        }
    }
}

/// Wrap exclusion in absolute page coordinates, or `None` when text is not excluded.
/// Tight/Through use the source polygon rather than its bounding rectangle.
pub(crate) fn page_exclusion(
    ctx: &LayoutContext<'_>,
    anchor: &AnchorDrawing,
    geometry: &Geometry,
    host_x: f64,
    host_y: f64,
) -> Option<PageExclusion> {
    let wrap = anchor.wrap.as_ref()?;
    let top_and_bottom = match wrap.kind {
        WrapKind::TopAndBottom => true,
        WrapKind::Tight | WrapKind::Through
            if wrap.polygon.len() < 3 || wrap.polygon.len() > 2048 =>
        {
            ctx.warn(
                "render.wrap: tight/through needs 3..2048 contour points; no contour exclusion applied"
                    .to_owned(),
            );
            return None;
        }
        WrapKind::Square | WrapKind::Tight | WrapKind::Through => false,
        WrapKind::None => return None,
    };
    let (width, height) = resolved_extent(ctx, anchor, geometry)?;
    let scale = ctx.options.scale;
    let (x, y) = anchor_position(anchor, geometry, host_x, host_y, width, height, scale);
    let dist = |anchor_dist: Option<u32>, wrap_dist: Option<u32>| {
        emu_to_px(i64::from(anchor_dist.or(wrap_dist).unwrap_or(0)), scale)
    };
    let effects = anchor.effect_extent.unwrap_or_default();
    let effect = |value: strict_ooxml_wml::model::Emu| {
        if matches!(wrap.kind, WrapKind::Square | WrapKind::TopAndBottom) {
            emu_to_px(value.value().max(0), scale)
        } else {
            0.0
        }
    };
    let (left, right, side) = if top_and_bottom {
        // Text stays off the object's lines across the column it sits in.
        let (column_left, column_width) = geometry.active_column_box();
        (column_left, column_left + column_width, WrapSide::Both)
    } else {
        let side = match wrap.wrap_text.as_deref() {
            Some("left") => WrapSide::Left,
            Some("right") => WrapSide::Right,
            Some("largest") => WrapSide::Largest,
            _ => WrapSide::Both,
        };
        (
            x - dist(anchor.dist_left, wrap.dist_left) - effect(effects.left),
            x + width + dist(anchor.dist_right, wrap.dist_right) + effect(effects.right),
            side,
        )
    };
    let distances = [
        dist(anchor.dist_left, wrap.dist_left) + effect(effects.left),
        dist(anchor.dist_right, wrap.dist_right) + effect(effects.right),
        dist(anchor.dist_top, wrap.dist_top) + effect(effects.top),
        dist(anchor.dist_bottom, wrap.dist_bottom) + effect(effects.bottom),
    ];
    let contour = if matches!(wrap.kind, WrapKind::Tight | WrapKind::Through) {
        let source = anchor.extent?;
        let (sx, sy) = match ctx.options.wrap_coordinates {
            crate::WrapCoordinates::Emu => (
                width / source.cx.value().max(1) as f64,
                height / source.cy.value().max(1) as f64,
            ),
            crate::WrapCoordinates::Office21600 => (width / 21_600.0, height / 21_600.0),
        };
        Some(Arc::new(Contour {
            points: wrap
                .polygon
                .iter()
                .map(|(px, py)| (x + *px as f64 * sx, y + *py as f64 * sy))
                .collect(),
            through: wrap.kind == WrapKind::Through,
            distance: distances,
        }))
    } else {
        None
    };
    let top = contour.as_ref().map_or(y, |contour| {
        contour
            .points
            .iter()
            .map(|point| point.1)
            .fold(f64::INFINITY, f64::min)
    }) - distances[2];
    let bottom = contour.as_ref().map_or(y + height, |contour| {
        contour
            .points
            .iter()
            .map(|point| point.1)
            .fold(f64::NEG_INFINITY, f64::max)
    }) + distances[3];
    Some(PageExclusion {
        left,
        right,
        top,
        bottom,
        side,
        contour,
    })
}

/// Paragraph-local wrap box for line breaking (Square / TopAndBottom).
///
/// `column_dx` is how far right of the paragraph's layout origin its column
/// is painted; the box is moved back by it.
pub(crate) fn wrap_exclusion(
    ctx: &LayoutContext<'_>,
    anchor: &AnchorDrawing,
    geometry: &Geometry,
    host_x: f64,
    host_y: f64,
    column_dx: f64,
) -> Option<WrapExclusion> {
    page_exclusion(ctx, anchor, geometry, host_x, host_y)
        .map(|exclusion| exclusion.into_paragraph_local(host_y, column_dx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_offsets_use_the_reference_span_including_negative_offsets() {
        let geometry = geometry_for(None, 1.0, None, false, crate::DefaultPaper::Letter);
        let position = Position {
            relative_from: Some("margin".into()),
            percent_offset: Some(25_000),
            offset: Some(Emu(914_400)),
            ..Position::default()
        };
        let x = resolve_h(Some(&position), &geometry, 0.0, 10.0, 1.0);
        assert!((x - geometry.left - geometry.content_width() / 4.0).abs() < 1e-9);
        let position = Position {
            percent_offset: Some(-50_000),
            ..position
        };
        let y = resolve_v(Some(&position), &geometry, 0.0, 10.0, 1.0);
        assert!((y - geometry.top + geometry.content_height() / 2.0).abs() < 1e-9);
    }
}
