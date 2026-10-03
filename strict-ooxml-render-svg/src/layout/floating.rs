//! Anchored (floating) object placement (`STAGE-5B-TASK.md` §5.2, §5.3).

use strict_ooxml_wml::model::drawing::{AnchorDrawing, Position, WrapKind};
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
        let Some(extent) = pending.anchor.extent else {
            continue;
        };
        let w = emu_to_px(extent.cx.value(), scale).max(0.0);
        let h = emu_to_px(extent.cy.value(), scale).max(0.0);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let section = pages
            .get(pending.page)
            .and_then(|page| sections.get(page.section_index))
            .map(|section| &section.properties);
        let geometry = geometry_for(section, scale, None);
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
    let base = match relative {
        Some("page") => 0.0,
        Some("rightMargin" | "outsideMargin") => geometry.width - geometry.right,
        Some("margin" | "leftMargin" | "insideMargin" | "column") => geometry.left,
        _ => host_x,
    };
    if let Some(align) = pos.and_then(|pos| pos.align.as_deref()) {
        return match align {
            "center" => (geometry.width - w) / 2.0,
            "right" | "outside" => (geometry.width - geometry.right - w).max(0.0),
            _ => base,
        };
    }
    let offset = pos
        .and_then(|pos| pos.offset)
        .map_or(0.0, |offset: Emu| emu_to_px(offset.value(), scale));
    base + offset
}

/// Resolves a vertical position to an absolute page y.
fn resolve_v(pos: Option<&Position>, geometry: &Geometry, host_y: f64, h: f64, scale: f64) -> f64 {
    let relative = pos.and_then(|pos| pos.relative_from.as_deref());
    let base = match relative {
        Some("page" | "topMargin") => 0.0,
        Some("bottomMargin") => geometry.height - geometry.bottom,
        Some("paragraph" | "line") => host_y,
        _ => geometry.top,
    };
    if let Some(align) = pos.and_then(|pos| pos.align.as_deref()) {
        return match align {
            "center" => (geometry.height - h) / 2.0,
            "bottom" | "outside" => (geometry.height - geometry.bottom - h).max(0.0),
            _ => base,
        };
    }
    let offset = pos
        .and_then(|pos| pos.offset)
        .map_or(0.0, |offset: Emu| emu_to_px(offset.value(), scale));
    base + offset
}

/// Returns `true` if the anchor reserves vertical flow space (`wrapTopAndBottom`).
#[must_use]
pub(crate) fn reserves_vertical_space(anchor: &AnchorDrawing) -> bool {
    anchor
        .wrap
        .as_ref()
        .is_some_and(|wrap| matches!(wrap.kind, WrapKind::TopAndBottom))
}
