//! DrawingML shape/group/picture rendering into paint items
//! (`STAGE-5B-TASK.md` §5.4).

use std::fmt::Write as _;

use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, CustomGeometry, Graphic, GroupShape, PathCommand, Picture, Shape, ShapeFill,
    ShapeStroke, TextAnchor, TextBox,
};
use strict_ooxml_wml::model::Drawing;

use crate::layout::table::{layout_blocks_inline, offset_item};
use crate::layout::{ImageItem, Item, LayoutContext, PathItem, RectItem};
use crate::style::resolve_shape_color;
use crate::units::emu_to_px;

/// Default text-box inset in EMU (`w:bodyPr` defaults, 0.1").
const DEFAULT_INSET_EMU: i64 = 91_440;
/// Default top/bottom text-box inset in EMU (0.05").
const DEFAULT_VERTICAL_INSET_EMU: i64 = 45_720;

/// Builds the paint items for an anchored graphic.
pub(crate) fn anchor_items(
    ctx: &LayoutContext<'_>,
    anchor: &AnchorDrawing,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    graphic_items(ctx, anchor.graphic.as_ref(), x, y, w, h)
}

/// Builds items for an inline (in-flow) graphic that is not a plain picture.
pub(crate) fn inline_items(
    ctx: &LayoutContext<'_>,
    drawing: &Drawing,
) -> Option<(Vec<Item>, f64, f64)> {
    let strict_ooxml_wml::model::DrawingKind::Inline(inline) = &drawing.kind else {
        return None;
    };
    match inline.graphic.as_ref() {
        Graphic::Shape(_)
        | Graphic::Group(_)
        | Graphic::Chart
        | Graphic::Diagram
        | Graphic::Other => {}
        Graphic::None | Graphic::Picture(_) => return None,
    }
    let extent = inline.extent?;
    let w = emu_to_px(extent.cx.value(), ctx.options.scale).max(1.0);
    let h = emu_to_px(extent.cy.value(), ctx.options.scale).max(1.0);
    let items = graphic_items(ctx, inline.graphic.as_ref(), 0.0, 0.0, w, h);
    Some((items, w, h))
}

/// Builds the paint items for one graphic at the given box.
pub(crate) fn graphic_items(
    ctx: &LayoutContext<'_>,
    graphic: &Graphic,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    match graphic {
        Graphic::Picture(picture) => picture_items(ctx, picture, None, x, y, w, h),
        Graphic::Shape(shape) => shape_items(ctx, shape, x, y, w, h),
        Graphic::Group(group) => group_items(ctx, group, x, y, w, h),
        Graphic::None | Graphic::Chart | Graphic::Diagram | Graphic::Other => {
            vec![placeholder(x, y, w, h)]
        }
    }
}

/// Builds items for a picture.
pub(crate) fn picture_items(
    ctx: &LayoutContext<'_>,
    picture: &Picture,
    alt: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let part = picture
        .blip
        .as_ref()
        .and_then(|blip| blip.resolved.as_ref());
    let href = part.and_then(|part| crate::paint::image::media_href(ctx, part));
    let alt = alt.unwrap_or_else(|| {
        picture
            .descr
            .clone()
            .or_else(|| picture.name.clone())
            .map_or_else(String::new, |value| value.to_string())
    });
    vec![Item::Image(ImageItem {
        x,
        y,
        w,
        h,
        href,
        part: part.cloned(),
        alt,
        transform: xfrm_transform(picture.xfrm, x, y, w, h),
    })]
}

/// Builds the items of a shape (path + optional text box).
fn shape_items(
    ctx: &LayoutContext<'_>,
    shape: &Shape,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let mut items = Vec::new();
    let path = shape_path(shape, w, h);
    let fill = shape.fill.as_ref().and_then(|fill| fill_color(ctx, fill));
    let (stroke, stroke_w, dash) = stroke_paint(ctx, shape.stroke.as_ref());
    let (rotate_deg, flip_h, flip_v) = shape.xfrm.map_or((0.0, false, false), |xfrm| {
        (
            xfrm.rot.map_or(0.0, |rot| f64::from(rot) / 60_000.0),
            xfrm.flip_h,
            xfrm.flip_v,
        )
    });
    if let Some(d) = path {
        items.push(Item::Path(PathItem {
            x,
            y,
            w,
            h,
            d,
            fill,
            stroke,
            stroke_w,
            dash,
            rotate_deg,
            flip_h,
            flip_v,
        }));
    } else {
        items.push(placeholder(x, y, w, h));
    }
    if let Some(text) = &shape.text {
        if !text.blocks.is_empty() {
            items.extend(text_box_items(ctx, text, x, y, w, h));
        }
    }
    items
}

/// Builds the items of a group by composing child coordinates.
fn group_items(
    ctx: &LayoutContext<'_>,
    group: &GroupShape,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let scale = ctx.options.scale;
    let Some(transform) = group.xfrm else {
        return group
            .children
            .iter()
            .flat_map(|child| {
                let (cw, ch) = graphic_size(ctx, child, w, h);
                graphic_items(ctx, child, x, y, cw, ch)
            })
            .collect();
    };
    let off_x = transform
        .offset
        .map_or(0.0, |(x, _)| emu_to_px(x.value(), scale));
    let off_y = transform
        .offset
        .map_or(0.0, |(_, y)| emu_to_px(y.value(), scale));
    let ext_w = transform
        .extent
        .map_or(w, |ext| emu_to_px(ext.cx.value(), scale));
    let ext_h = transform
        .extent
        .map_or(h, |ext| emu_to_px(ext.cy.value(), scale));
    let ch_off_x = transform
        .child_offset
        .map_or(0.0, |(x, _)| emu_to_px(x.value(), scale));
    let ch_off_y = transform
        .child_offset
        .map_or(0.0, |(_, y)| emu_to_px(y.value(), scale));
    let ch_ext_w = transform
        .child_extent
        .map_or(ext_w, |ext| emu_to_px(ext.cx.value(), scale));
    let ch_ext_h = transform
        .child_extent
        .map_or(ext_h, |ext| emu_to_px(ext.cy.value(), scale));
    let scale_x = if ch_ext_w.abs() > f64::EPSILON {
        ext_w / ch_ext_w
    } else {
        1.0
    };
    let scale_y = if ch_ext_h.abs() > f64::EPSILON {
        ext_h / ch_ext_h
    } else {
        1.0
    };
    let base_x = x + off_x - ch_off_x * scale_x;
    let base_y = y + off_y - ch_off_y * scale_y;
    let mut items = Vec::new();
    for child in &group.children {
        let (child_off, child_ext) = child_box(child, scale);
        let cx = base_x + child_off.0 * scale_x;
        let cy = base_y + child_off.1 * scale_y;
        let cw = child_ext.0 * scale_x;
        let ch = child_ext.1 * scale_y;
        let (cw, ch) = if cw > 0.0 && ch > 0.0 {
            (cw, ch)
        } else {
            graphic_size(ctx, child, w, h)
        };
        items.extend(graphic_items(ctx, child, cx, cy, cw, ch));
    }
    items
}

/// Returns a child's offset/extent in px (0 when absent).
fn child_box(graphic: &Graphic, scale: f64) -> ((f64, f64), (f64, f64)) {
    let emu = |value: i64| emu_to_px(value, scale);
    match graphic {
        Graphic::Shape(shape) => {
            let offset = shape
                .offset
                .map_or((0.0, 0.0), |(x, y)| (emu(x.value()), emu(y.value())));
            let extent = shape
                .extent
                .map_or((0.0, 0.0), |ext| (emu(ext.cx.value()), emu(ext.cy.value())));
            (offset, extent)
        }
        Graphic::Picture(picture) => {
            let offset = picture
                .xfrm
                .and_then(|xfrm| xfrm.offset)
                .map_or((0.0, 0.0), |(x, y)| (emu(x.value()), emu(y.value())));
            let extent = picture
                .extent
                .map_or((0.0, 0.0), |ext| (emu(ext.cx.value()), emu(ext.cy.value())));
            (offset, extent)
        }
        Graphic::Group(group) => group.xfrm.map_or(((0.0, 0.0), (0.0, 0.0)), |xfrm| {
            (
                xfrm.offset
                    .map_or((0.0, 0.0), |(x, y)| (emu(x.value()), emu(y.value()))),
                xfrm.extent
                    .map_or((0.0, 0.0), |ext| (emu(ext.cx.value()), emu(ext.cy.value()))),
            )
        }),
        _ => ((0.0, 0.0), (0.0, 0.0)),
    }
}

/// Returns a fallback size for a graphic.
fn graphic_size(ctx: &LayoutContext<'_>, graphic: &Graphic, w: f64, h: f64) -> (f64, f64) {
    let scale = ctx.options.scale;
    match graphic {
        Graphic::Shape(shape) => shape.extent.map_or((w, h), |ext| {
            (
                emu_to_px(ext.cx.value(), scale),
                emu_to_px(ext.cy.value(), scale),
            )
        }),
        Graphic::Picture(picture) => picture.extent.map_or((w, h), |ext| {
            (
                emu_to_px(ext.cx.value(), scale),
                emu_to_px(ext.cy.value(), scale),
            )
        }),
        Graphic::Group(group) => group
            .xfrm
            .and_then(|xfrm| xfrm.extent)
            .map_or((w, h), |ext| {
                (
                    emu_to_px(ext.cx.value(), scale),
                    emu_to_px(ext.cy.value(), scale),
                )
            }),
        _ => (w, h),
    }
}

/// Builds items for a text box's blocks inside the frame.
fn text_box_items(
    ctx: &LayoutContext<'_>,
    text: &TextBox,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let body = text.body.unwrap_or_default();
    let scale = ctx.options.scale;
    let left_inset = body
        .left_inset
        .map_or(DEFAULT_INSET_EMU, strict_ooxml_wml::model::Emu::value);
    let right_inset = body
        .right_inset
        .map_or(DEFAULT_INSET_EMU, strict_ooxml_wml::model::Emu::value);
    let top_inset = body.top_inset.map_or(
        DEFAULT_VERTICAL_INSET_EMU,
        strict_ooxml_wml::model::Emu::value,
    );
    let bottom_inset = body.bottom_inset.map_or(
        DEFAULT_VERTICAL_INSET_EMU,
        strict_ooxml_wml::model::Emu::value,
    );
    let left = x + emu_to_px(left_inset, scale);
    let top = y + emu_to_px(top_inset, scale);
    let inner_width = (w - emu_to_px(left_inset + right_inset, scale)).max(1.0);
    let mut items = Vec::new();
    let mut content_y = 0.0;
    layout_blocks_inline(
        ctx,
        &text.blocks,
        0.0,
        inner_width,
        &mut content_y,
        &mut items,
        0,
        None,
    );
    let available = (h - emu_to_px(top_inset + bottom_inset, scale)).max(0.0);
    let shift = match body.anchor.unwrap_or(TextAnchor::Top) {
        TextAnchor::Top => 0.0,
        TextAnchor::Center => ((available - content_y) / 2.0).max(0.0),
        TextAnchor::Bottom => (available - content_y).max(0.0),
    };
    items
        .iter()
        .map(|item| offset_item(item, left, top + shift))
        .collect()
}

/// Returns the SVG path for a shape in local coordinates.
fn shape_path(shape: &Shape, w: f64, h: f64) -> Option<String> {
    match &shape.geometry {
        strict_ooxml_wml::model::drawing::ShapeGeometry::None => Some(rect_path(w, h)),
        strict_ooxml_wml::model::drawing::ShapeGeometry::Preset(preset) => {
            preset_path(preset, w, h)
        }
        strict_ooxml_wml::model::drawing::ShapeGeometry::Custom(custom) => {
            Some(custom_path(custom, w, h))
        }
    }
}

/// Resolves a fill to an SVG colour (gradients/patterns use their first colour).
fn fill_color(ctx: &LayoutContext<'_>, fill: &ShapeFill) -> Option<String> {
    let theme = ctx.document.theme.as_ref();
    match fill {
        ShapeFill::None => None,
        ShapeFill::Solid { color } => resolve_shape_color(theme, color),
        ShapeFill::Gradient { stops, .. } => stops
            .first()
            .and_then(|stop| resolve_shape_color(theme, &stop.color)),
        ShapeFill::Pattern { foreground, .. } => foreground
            .as_ref()
            .and_then(|color| resolve_shape_color(theme, color)),
    }
}

/// Resolves a stroke to `(colour, width_px, dash)`.
fn stroke_paint(
    ctx: &LayoutContext<'_>,
    stroke: Option<&ShapeStroke>,
) -> (Option<String>, f64, Option<String>) {
    let Some(stroke) = stroke else {
        return (None, 0.0, None);
    };
    if stroke.none {
        return (None, 0.0, None);
    }
    let theme = ctx.document.theme.as_ref();
    let color = stroke
        .color
        .as_ref()
        .and_then(|color| resolve_shape_color(theme, color))
        .or_else(|| Some("#000000".to_owned()));
    let width = stroke
        .width
        .map_or(1.0, |emu| emu_to_px(emu.value(), ctx.options.scale))
        .max(0.5);
    let dash = stroke.dash.as_deref().map(dash_array);
    (color, width, dash)
}

/// Maps a preset dash name to an SVG dash array.
fn dash_array(name: &str) -> String {
    match name {
        "dot" | "sysDot" => "1 3".to_owned(),
        "lgDash" => "8 3".to_owned(),
        "dashDot" | "sysDashDot" => "4 2 1 2".to_owned(),
        "lgDashDot" | "lgDashDotDot" => "8 2 2 2".to_owned(),
        _ => "4 3".to_owned(),
    }
}

/// Builds an SVG rotate/scale transform for a picture.
fn xfrm_transform(
    xfrm: Option<strict_ooxml_wml::model::drawing::Xfrm>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Option<String> {
    let xfrm = xfrm?;
    let mut parts: Vec<String> = Vec::new();
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    if let Some(rot) = xfrm.rot {
        let deg = f64::from(rot) / 60_000.0;
        if deg.abs() > f64::EPSILON {
            parts.push(format!(
                "rotate({} {} {})",
                crate::units::fmt_num(deg),
                crate::units::fmt_num(cx),
                crate::units::fmt_num(cy)
            ));
        }
    }
    if xfrm.flip_h || xfrm.flip_v {
        let sx = if xfrm.flip_h { -1.0 } else { 1.0 };
        let sy = if xfrm.flip_v { -1.0 } else { 1.0 };
        parts.push(format!(
            "translate({} {}) scale({} {}) translate({} {})",
            crate::units::fmt_num(2.0 * cx),
            crate::units::fmt_num(2.0 * cy),
            crate::units::fmt_num(sx),
            crate::units::fmt_num(sy),
            crate::units::fmt_num(-cx),
            crate::units::fmt_num(-cy)
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// A generic placeholder rectangle.
fn placeholder(x: f64, y: f64, w: f64, h: f64) -> Item {
    Item::Rect(RectItem {
        x,
        y,
        w: w.max(0.0),
        h: h.max(0.0),
        fill: Some("#f2f2f2".to_owned()),
        stroke: Some("#999999".to_owned()),
        stroke_w: 1.0,
    })
}

fn rect_path(w: f64, h: f64) -> String {
    let (w, h) = (crate::units::fmt_num(w), crate::units::fmt_num(h));
    format!("M0 0 H{w} V{h} H0 Z")
}

/// Returns an SVG path for a preset geometry, or `None` if unsupported.
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn preset_path(preset: &str, w: f64, h: f64) -> Option<String> {
    let f = crate::units::fmt_num;
    let path = match preset {
        "rect" | "flowChartProcess" => rect_path(w, h),
        "roundRect" => {
            let radius = (w.min(h) * 0.166_666).max(0.0).min(w / 2.0).min(h / 2.0);
            let rv = f(radius);
            let wv = f(w);
            let hv = f(h);
            let wmr = f(w - radius);
            let hmr = f(h - radius);
            format!(
                "M{rv} 0 H{wmr} A{rv} {rv} 0 0 1 {wv} {rv} V{hmr} A{rv} {rv} 0 0 1 {wmr} {hv} H{rv} A{rv} {rv} 0 0 1 0 {hmr} V{rv} A{rv} {rv} 0 0 1 {rv} 0 Z"
            )
        }
        "ellipse" | "oval" => {
            let (rx, ry, cy) = (w / 2.0, h / 2.0, h / 2.0);
            format!(
                "M0 {} A{} {} 0 1 0 {} {} A{} {} 0 1 0 0 {} Z",
                f(cy),
                f(rx),
                f(ry),
                f(w),
                f(cy),
                f(rx),
                f(ry),
                f(cy)
            )
        }
        "line" => format!("M0 0 L{} {}", f(w), f(h)),
        "triangle" | "isosTriangle" => {
            format!("M{} 0 L{} {} L0 {} Z", f(w / 2.0), f(w), f(h), f(h))
        }
        "rtTriangle" => format!("M0 0 L{} {} L0 {} Z", f(w), f(h), f(h)),
        "diamond" => format!(
            "M{} 0 L{} {} L{} {} L0 {} Z",
            f(w / 2.0),
            f(w),
            f(h / 2.0),
            f(w / 2.0),
            f(h),
            f(h / 2.0)
        ),
        "hexagon" => format!(
            "M{} 0 H{} L{} {} L{} {} H{} L0 {} Z",
            f(w * 0.25),
            f(w * 0.75),
            f(w),
            f(h / 2.0),
            f(w * 0.75),
            f(h),
            f(w * 0.25),
            f(h / 2.0)
        ),
        "pentagon" => regular_polygon(5, w, h),
        "chevron" => format!(
            "M0 0 H{} L{} {} L{} {} H0 L{} {} Z",
            f(w * 0.75),
            f(w),
            f(h / 2.0),
            f(w * 0.75),
            f(h),
            f(w * 0.25),
            f(h / 2.0)
        ),
        "rightArrow" => {
            let (wv, hv) = (f(w), f(h));
            format!(
                "M0 {} H{} V0 L{wv} {} L{} {hv} V{} H0 Z",
                f(h * 0.25),
                f(w * 0.6),
                f(h / 2.0),
                f(w * 0.6),
                f(h * 0.75)
            )
        }
        "leftArrow" => {
            let (hv, wv) = (f(h), f(w));
            format!(
                "M0 {} L{} 0 V{} H{wv} V{} H{} V{hv} Z",
                f(h / 2.0),
                f(w * 0.4),
                f(h * 0.25),
                f(h * 0.75),
                f(w * 0.4)
            )
        }
        "upArrow" => {
            let (wv, hv) = (f(w), f(h));
            format!(
                "M{} 0 L{wv} {} H{} V{hv} H{} V{} H0 Z",
                f(w / 2.0),
                f(h * 0.4),
                f(w * 0.75),
                f(w * 0.25),
                f(h * 0.4)
            )
        }
        "downArrow" => {
            let (wv, hv) = (f(w), f(h));
            format!(
                "M0 0 H{wv} V{} H{} L{} {hv} L{} {} H{} V0 Z",
                f(h * 0.6),
                f(w * 0.75),
                f(w / 2.0),
                f(w * 0.25),
                f(h * 0.6),
                f(w * 0.25)
            )
        }
        "star5" => star_path(5, w, h),
        "plus" | "mathPlus" => {
            let (a, b) = (f(w * 0.35), f(w * 0.65));
            let (c, d) = (f(h * 0.35), f(h * 0.65));
            let (wv, hv) = (f(w), f(h));
            format!("M{a} 0 H{b} V{c} H{wv} V{d} H{b} V{hv} H{a} V{d} H0 V{c} H{a} Z")
        }
        "flowChartDecision" => preset_path("diamond", w, h)?,
        "flowChartTerminator" => preset_path("roundRect", w, h)?,
        "flowChartDocument" => format!(
            "M0 0 H{} V{} C{} {} {} {} 0 {} Z",
            f(w),
            f(h * 0.85),
            f(w * 0.75),
            f(h),
            f(w * 0.25),
            f(h * 0.7),
            f(h * 0.85)
        ),
        _ => return None,
    };
    Some(path)
}

/// Returns an SVG path for a custom geometry (scaled to the box).
fn custom_path(custom: &CustomGeometry, w: f64, h: f64) -> String {
    let scale_x = if custom.width > 0 {
        w / custom.width as f64
    } else {
        1.0
    };
    let scale_y = if custom.height > 0 {
        h / custom.height as f64
    } else {
        1.0
    };
    let f = crate::units::fmt_num;
    let mut out = String::new();
    for command in &custom.commands {
        match command {
            PathCommand::MoveTo { x, y } => {
                let _ = write!(
                    out,
                    "M{} {} ",
                    f(*x as f64 * scale_x),
                    f(*y as f64 * scale_y)
                );
            }
            PathCommand::LineTo { x, y } => {
                let _ = write!(
                    out,
                    "L{} {} ",
                    f(*x as f64 * scale_x),
                    f(*y as f64 * scale_y)
                );
            }
            PathCommand::CubicBezTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let _ = write!(
                    out,
                    "C{} {} {} {} {} {} ",
                    f(*x1 as f64 * scale_x),
                    f(*y1 as f64 * scale_y),
                    f(*x2 as f64 * scale_x),
                    f(*y2 as f64 * scale_y),
                    f(*x as f64 * scale_x),
                    f(*y as f64 * scale_y)
                );
            }
            PathCommand::Close => out.push_str("Z "),
        }
    }
    out.trim_end().to_owned()
}

/// Returns a regular polygon path.
#[allow(clippy::many_single_char_names)]
fn regular_polygon(sides: usize, w: f64, h: f64) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let rx = w / 2.0;
    let ry = h / 2.0;
    let f = crate::units::fmt_num;
    let mut out = String::new();
    for index in 0..sides {
        let angle =
            -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * index as f64 / sides as f64;
        let x = cx + rx * angle.cos();
        let y = cy + ry * angle.sin();
        if index == 0 {
            let _ = write!(out, "M{} {} ", f(x), f(y));
        } else {
            let _ = write!(out, "L{} {} ", f(x), f(y));
        }
    }
    out.push('Z');
    out
}

/// Returns a star polygon path.
#[allow(clippy::many_single_char_names)]
fn star_path(points: usize, w: f64, h: f64) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let rx = w / 2.0;
    let ry = h / 2.0;
    let f = crate::units::fmt_num;
    let mut out = String::new();
    let total = points * 2;
    for index in 0..total {
        let angle =
            -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * index as f64 / total as f64;
        let factor = if index % 2 == 0 { 1.0 } else { 0.382 };
        let x = cx + rx * factor * angle.cos();
        let y = cy + ry * factor * angle.sin();
        if index == 0 {
            let _ = write!(out, "M{} {} ", f(x), f(y));
        } else {
            let _ = write!(out, "L{} {} ", f(x), f(y));
        }
    }
    out.push('Z');
    out
}
