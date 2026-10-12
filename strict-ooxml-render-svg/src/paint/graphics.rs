//! DrawingML shape/group/picture rendering into paint items
//! (`STAGE-5B-TASK.md` §5.4).

use std::fmt::Write as _;

use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, CustomGeometry, DiagramDrawing, Graphic, GroupShape, PathCommand, Picture,
    Shape, ShapeFill, ShapeStroke, TextAnchor, TextBox,
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
/// Minimum group `ext`/`chExt` scale factor (AUD-71).
const MIN_GROUP_SCALE: f64 = 1e-6;
/// Maximum group `ext`/`chExt` scale factor (AUD-71).
const MAX_GROUP_SCALE: f64 = 1e6;

/// Clamps a group scale into `[1e-6, 1e6]`; non-finite becomes `1` (AUD-71).
fn clamp_group_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.clamp(MIN_GROUP_SCALE, MAX_GROUP_SCALE)
    } else {
        1.0
    }
}

/// Builds the paint items for an anchored graphic.
pub(crate) fn anchor_items(
    ctx: &LayoutContext<'_>,
    anchor: &AnchorDrawing,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    if anchor.hidden == Some(true)
        || anchor
            .doc_pr
            .as_ref()
            .is_some_and(|properties| properties.hidden == Some(true))
    {
        return Vec::new();
    }
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
        | Graphic::Chart(_)
        | Graphic::Diagram(_)
        | Graphic::LockedCanvas(_)
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
        // A chart is drawn from the cache the parser read out of its part; one
        // whose part could not be read keeps the placeholder.
        Graphic::Chart(refs) => match refs.chart.as_deref() {
            Some(chart) => crate::paint::chart::chart_items(ctx, chart, x, y, w, h),
            None => vec![placeholder(x, y, w, h)],
        },
        // A diagram is drawn from the drawing Word cached beside it; one
        // without a readable drawing keeps the placeholder.
        Graphic::Diagram(refs) => match refs.diagram.as_deref() {
            Some(diagram) => diagram_items(ctx, diagram, x, y, w, h),
            None => vec![placeholder(x, y, w, h)],
        },
        Graphic::LockedCanvas(canvas) => canvas.graphics.as_deref().map_or_else(
            || vec![placeholder(x, y, w, h)],
            |group| group_items(ctx, group, x, y, w, h),
        ),
        Graphic::None | Graphic::Other => {
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
    let href = part.and_then(|part| crate::paint::image::media_href(ctx, part, w, h));
    let alt = alt.unwrap_or_else(|| {
        picture
            .descr
            .clone()
            .or_else(|| picture.name.clone())
            .map_or_else(String::new, |value| value.to_string())
    });
    let image = Item::Image(ImageItem {
        x,
        y,
        w,
        h,
        href,
        part: part.cloned(),
        alt,
        transform: xfrm_transform(picture.xfrm, x, y, w, h),
        crop: crate::paint::image::src_rect_crop(picture.src_rect),
    });
    match picture
        .markup
        .blip_children
        .as_deref()
        .and_then(blip_washout)
    {
        Some(opacity) => vec![Item::Group(crate::layout::GroupItem {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            items: vec![image],
            bounds: [x, y, w, h],
            clip: None,
            clip_path: None,
            clip_inverse: false,
            opacity,
        })],
        None => vec![image],
    }
}

/// The opacity that stands in for a picture's colour effects: a washed-out
/// picture (`a:lum` with negative contrast or positive brightness, Word's
/// watermark) or `a:alphaModFix`. Blending with the white page under it gives
/// the light picture WPS draws (WQ-028 / doc 028's header logo).
fn blip_washout(children: &str) -> Option<f64> {
    let attribute = |element: &str, name: &str| -> Option<f64> {
        let rest = children.get(children.find(&format!("<a:{element} "))?..)?;
        let tag = rest.get(..rest.find('>')?)?;
        let key = format!(" {name}=\"");
        let value = tag.split_once(&key)?.1.split('"').next()?;
        // Strict writes a percentage (`70%`), Transitional thousandths of one.
        match value.strip_suffix('%') {
            Some(percent) => percent.parse::<f64>().ok().map(|value| value / 100.0),
            None => value.parse::<f64>().ok().map(|value| value / 100_000.0),
        }
    };
    let mut opacity: f64 = 1.0;
    let contrast = attribute("lum", "contrast").unwrap_or(0.0);
    let bright = attribute("lum", "bright").unwrap_or(0.0);
    if contrast < 0.0 {
        opacity = opacity.min(1.0 + contrast);
    } else if bright > 0.0 {
        opacity = opacity.min(1.0 - bright);
    }
    if let Some(alpha) = attribute("alphaModFix", "amt") {
        opacity *= alpha;
    }
    (opacity < 0.999).then_some(opacity.clamp(0.05, 1.0))
}

/// Builds the items of a shape (path + optional text box).
#[allow(clippy::too_many_lines)]
fn shape_items(
    ctx: &LayoutContext<'_>,
    shape: &Shape,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let mut items = Vec::new();
    if let Some(ShapeFill::Blip { blip, .. }) = &shape.fill {
        let part = blip.resolved.as_ref();
        let href = part.and_then(|part| crate::paint::image::media_href(ctx, part, w, h));
        let alt = shape
            .descr
            .clone()
            .or_else(|| shape.name.clone())
            .map_or_else(String::new, |value| value.to_string());
        items.push(Item::Image(ImageItem {
            x,
            y,
            w,
            h,
            href,
            part: part.cloned(),
            alt,
            transform: xfrm_transform(shape.xfrm, x, y, w, h),
            crop: None,
        }));
    }
    let paths = shape_paths(shape, w, h);
    let style = ShapeStyleColors::resolve(ctx, shape.style.as_ref());
    // `a:spPr` decides; with no fill or outline of its own the shape takes the
    // `wps:style` references (`a:fillRef`/`a:lnRef`), as Word draws it.
    let fill = match &shape.fill {
        Some(fill) => fill_color(ctx, fill),
        None => style.fill.clone(),
    };
    let paint = super::fills::resolve(ctx, shape, w, h);
    let (stroke, stroke_w, dash) = stroke_paint(ctx, shape.stroke.as_ref(), &style);
    let (rotate_deg, flip_h, flip_v) = shape.xfrm.map_or((0.0, false, false), |xfrm| {
        (
            xfrm.rot.map_or(0.0, |rot| f64::from(rot) / 60_000.0),
            xfrm.flip_h,
            xfrm.flip_v,
        )
    });
    let path_item = |d: String, fill: Option<String>, stroke: Option<String>| {
        Item::Path(PathItem {
            x,
            y,
            w,
            h,
            d,
            fill,
            paint: None,
            stroke,
            stroke_w,
            dash: None,
            rotate_deg,
            flip_h,
            flip_v,
        })
    };
    // AUD-49: one SVG `<path>` per `a:path`, each scaled by its own w/h.
    if paths.is_empty() {
        items.push(placeholder(x, y, w, h));
    } else {
        let mut last_stroked = None;
        for path in paths {
            let stroked = path.stroke.then(|| stroke.clone()).flatten();
            if stroked.is_some() {
                last_stroked = Some(path.d.clone());
            }
            items.push(Item::Path(PathItem {
                x,
                y,
                w,
                h,
                d: path.d,
                fill: if path.fill { fill.clone() } else { None },
                paint: if path.fill { paint.clone() } else { None },
                stroke: stroked,
                stroke_w,
                dash: dash.clone(),
                rotate_deg,
                flip_h,
                flip_v,
            }));
        }
        // Line ends (`a:headEnd`/`a:tailEnd`) sit on the first and last point
        // of the stroked outline, in the same local space and transform.
        if let (Some(d), Some(color), Some(line)) = (last_stroked, &stroke, shape.stroke.as_ref()) {
            for (kind, at_start) in [(&line.head_end, true), (&line.tail_end, false)] {
                if let Some(kind) = kind.as_deref() {
                    if let Some(head) = arrow_head(&d, at_start, kind, stroke_w) {
                        let (fill, outline) = if kind == "arrow" {
                            (None, Some(color.clone()))
                        } else {
                            (Some(color.clone()), None)
                        };
                        items.push(path_item(head, fill, outline));
                    }
                }
            }
        }
    }
    if let Some(text) = &shape.text {
        if !text.blocks.is_empty() {
            let (tx, ty, tw, th) = custom_text_box(ctx, shape, x, y, w, h).unwrap_or((x, y, w, h));
            let mut text_items = text_box_items(ctx, text, tx, ty, tw, th);
            // `a:fontRef` colours the text the runs leave on `auto`.
            if let Some(color) = &style.font {
                color_auto_text(&mut text_items, color);
            }
            let body = text.body.as_ref();
            let angle = body
                .and_then(|body| body.rot)
                .map_or(0.0, |rot| f64::from(rot) / 60_000.0)
                + if body.is_some_and(|body| body.upright == Some(true)) {
                    0.0
                } else {
                    shape
                        .xfrm
                        .and_then(|xfrm| xfrm.rot)
                        .map_or(0.0, |rot| f64::from(rot) / 60_000.0)
                };
            if angle != 0.0 {
                let (sin, cos) = angle.to_radians().sin_cos();
                let (cx, cy) = (x + w / 2.0, y + h / 2.0);
                text_items = transform_items(
                    text_items,
                    [
                        cos,
                        sin,
                        -sin,
                        cos,
                        cx - cos * cx + sin * cy,
                        cy - sin * cx - cos * cy,
                    ],
                );
            }
            items.extend(text_items);
        }
    }
    super::effects::apply(ctx, shape.effects.as_deref(), items, [x, y, w, h])
}

fn color_auto_text(items: &mut [Item], color: &str) {
    for item in items {
        match item {
            Item::Text(text) if text.run.color.is_none() => text.run.color = Some(color.to_owned()),
            Item::Group(group) => color_auto_text(&mut group.items, color),
            _ => {}
        }
    }
}

/// Resolve the text rectangle in the custom geometry's coordinate space.
fn custom_text_box(
    ctx: &LayoutContext<'_>,
    shape: &Shape,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Option<(f64, f64, f64, f64)> {
    let strict_ooxml_wml::model::drawing::ShapeGeometry::Custom(geometry) = &shape.geometry else {
        return None;
    };
    let rect = geometry.text_rect.as_ref()?;
    let path = geometry.paths.first();
    let gw = path
        .filter(|path| path.width > 0)
        .map_or(w, |path| path.width as f64);
    let gh = path
        .filter(|path| path.height > 0)
        .map_or(h, |path| path.height as f64);
    let mut guides = std::collections::HashMap::new();
    for (name, formula) in geometry.adjustments.iter().chain(&geometry.guides) {
        if let Some(value) = geometry_formula(formula, gw, gh, &guides) {
            guides.insert(name.to_string(), value);
        }
    }
    let resolve = |value: &str| -> Option<f64> {
        if let Some(value) = guides.get(value) {
            return Some(*value);
        }
        if let Some(value) = geometry_value(value, gw, gh, &guides) {
            return Some(value);
        }
        match value {
            "l" | "t" => Some(0.0),
            "r" | "w" => Some(gw),
            "b" | "h" => Some(gh),
            "hc" => Some(gw / 2.0),
            "vc" => Some(gh / 2.0),
            _ => {
                if let Some(divisor) = value
                    .strip_prefix("wd")
                    .and_then(|part| part.parse::<f64>().ok())
                    .filter(|n| *n > 0.0)
                {
                    Some(gw / divisor)
                } else if let Some(divisor) = value
                    .strip_prefix("hd")
                    .and_then(|part| part.parse::<f64>().ok())
                    .filter(|n| *n > 0.0)
                {
                    Some(gh / divisor)
                } else {
                    value.parse::<f64>().ok().filter(|n| n.is_finite())
                }
            }
        }
    };
    let values: Option<Vec<_>> = rect.iter().map(|value| resolve(value)).collect();
    let Some(&[left, top, right, bottom]) = values.as_deref() else {
        ctx.warn("render.custom-text-rect: unresolved geometry guide in text rectangle".to_owned());
        return None;
    };
    Some((
        x + left * w / gw,
        y + top * h / gh,
        ((right - left) * w / gw).max(1.0),
        ((bottom - top) * h / gh).max(1.0),
    ))
}

fn geometry_value(
    value: &str,
    w: f64,
    h: f64,
    guides: &std::collections::HashMap<String, f64>,
) -> Option<f64> {
    if let Some(value) = guides.get(value) {
        return Some(*value);
    }
    let result = match value {
        "l" | "t" => 0.0,
        "w" | "r" => w,
        "h" | "b" => h,
        "hc" => w / 2.0,
        "vc" => h / 2.0,
        "ss" => w.min(h),
        "ls" => w.max(h),
        "cd2" => 10_800_000.0,
        "cd4" => 5_400_000.0,
        "cd8" => 2_700_000.0,
        "3cd4" => 16_200_000.0,
        "3cd8" => 8_100_000.0,
        "5cd8" => 13_500_000.0,
        "7cd8" => 18_900_000.0,
        _ => {
            if let Some((base, divisor)) = ["wd", "hd", "ssd"].into_iter().find_map(|prefix| {
                value
                    .strip_prefix(prefix)
                    .and_then(|suffix| suffix.parse::<f64>().ok())
                    .filter(|divisor| *divisor > 0.0)
                    .map(|divisor| {
                        (
                            match prefix {
                                "wd" => w,
                                "hd" => h,
                                _ => w.min(h),
                            },
                            divisor,
                        )
                    })
            }) {
                base / divisor
            } else {
                value.parse::<f64>().ok()?
            }
        }
    };
    result.is_finite().then_some(result)
}

/// DrawingML guide operators; angular values are in 1/60000 degree.
#[allow(clippy::many_single_char_names)] // DrawingML w/h/x/y/matrix names
fn geometry_formula(
    formula: &str,
    w: f64,
    h: f64,
    guides: &std::collections::HashMap<String, f64>,
) -> Option<f64> {
    let mut parts = formula.split_whitespace();
    let operator = parts.next()?;
    let values: Option<Vec<_>> = parts
        .map(|value| geometry_value(value, w, h, guides))
        .collect();
    let values = values?;
    let radians = |angle: f64| (angle / 60_000.0).to_radians();
    let value = match (operator, values.as_slice()) {
        ("val", [x]) => *x,
        ("*/", [x, y, z]) if *z != 0.0 => x * y / z,
        ("+-", [x, y, z]) => x + y - z,
        ("+/", [x, y, z]) if *z != 0.0 => (x + y) / z,
        ("?:", [x, y, z]) => {
            if *x > 0.0 {
                *y
            } else {
                *z
            }
        }
        ("abs", [x]) => x.abs(),
        ("at2", [x, y]) => y.atan2(*x).to_degrees() * 60_000.0,
        ("cat2", [x, y, z]) => x * z.atan2(*y).cos(),
        ("sat2", [x, y, z]) => x * z.atan2(*y).sin(),
        ("cos", [x, angle]) => x * radians(*angle).cos(),
        ("sin", [x, angle]) => x * radians(*angle).sin(),
        ("tan", [x, angle]) => x * radians(*angle).tan(),
        ("max", [x, y]) => x.max(*y),
        ("min", [x, y]) => x.min(*y),
        ("mod", [x, y, z]) => x.hypot(*y).hypot(*z),
        ("pin", [minimum, value, maximum]) => value.max(*minimum).min(*maximum),
        ("sqrt", [x]) if *x >= 0.0 => x.sqrt(),
        _ => return None,
    };
    value.is_finite().then_some(value)
}

/// The colours a `wps:style` block gives a shape, resolved through the theme.
#[derive(Default)]
struct ShapeStyleColors {
    /// `a:fillRef` with an index of 1 or more.
    fill: Option<String>,
    /// `a:lnRef` colour and the width of its theme line style, in EMU.
    line: Option<(String, i64)>,
    /// `a:fontRef` colour.
    font: Option<String>,
}

impl ShapeStyleColors {
    fn resolve(
        ctx: &LayoutContext<'_>,
        style: Option<&strict_ooxml_wml::model::drawing::ShapeStyle>,
    ) -> Self {
        let Some(style) = style else {
            return Self::default();
        };
        let theme = ctx.document.theme.as_ref();
        let scheme = |name: Option<&str>| {
            let hex = theme?.color(name?)?;
            let digits = hex.strip_prefix('#').unwrap_or(hex);
            (digits.len() == 6).then(|| format!("#{}", digits.to_ascii_lowercase()))
        };
        // Index 0 is "no style"; 1–3 pick the subtle, moderate and intense
        // entries of the theme's lists, and 1001+ the background fills.
        let index = |value: Option<&str>| {
            value
                .and_then(|value| value.trim().parse::<u32>().ok())
                .unwrap_or(0)
        };
        let fill = (index(style.fill_ref.as_deref()) > 0)
            .then(|| scheme(style.fill_ref_color.as_deref()))
            .flatten();
        // The Office theme's line styles are 0.5, 1 and 1.5 pt wide.
        let line_index = index(style.line_ref.as_deref());
        let line = (line_index > 0)
            .then(|| scheme(style.line_ref_color.as_deref()))
            .flatten()
            .map(|color| (color, 6_350 * i64::from(line_index.min(3))));
        Self {
            fill,
            line,
            font: scheme(style.font_ref_color.as_deref()),
        }
    }
}

/// The outline of a line end at the start or end of `d`, in the same local
/// coordinates, or `None` for `none` or a path too short to have a direction.
///
/// The sizes are Word's medium: three line widths, never under 6 px.
fn arrow_head(d: &str, at_start: bool, kind: &str, stroke_w: f64) -> Option<String> {
    if kind == "none" {
        return None;
    }
    let points = path_points(d);
    let (tip, from) = if at_start {
        (*points.first()?, *points.get(1)?)
    } else {
        let count = points.len();
        (
            *points.get(count.checked_sub(1)?)?,
            *points.get(count.checked_sub(2)?)?,
        )
    };
    let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
    let length = dx.hypot(dy);
    if length < 1e-9 {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    let (nx, ny) = (-uy, ux);
    let size = (stroke_w * 3.0).max(6.0);
    let half = size / 2.0;
    let f = crate::units::fmt_num;
    let point = |along: f64, across: f64| {
        format!(
            "{} {}",
            f(tip.0 - ux * along + nx * across),
            f(tip.1 - uy * along + ny * across)
        )
    };
    Some(match kind {
        "diamond" => format!(
            "M{} L{} L{} L{} Z",
            point(-half, 0.0),
            point(0.0, half),
            point(half, 0.0),
            point(0.0, -half)
        ),
        "oval" => format!(
            "M{} A{h} {h} 0 1 0 {} A{h} {h} 0 1 0 {} Z",
            point(0.0, -half),
            point(0.0, half),
            point(0.0, -half),
            h = f(half)
        ),
        "stealth" => format!(
            "M{} L{} L{} L{} Z",
            point(0.0, 0.0),
            point(size, half),
            point(size * 0.7, 0.0),
            point(size, -half)
        ),
        "arrow" => format!(
            "M{} L{} L{}",
            point(size, half),
            point(0.0, 0.0),
            point(size, -half)
        ),
        _ => format!(
            "M{} L{} L{} Z",
            point(0.0, 0.0),
            point(size, half),
            point(size, -half)
        ),
    })
}

/// The points an SVG path visits, in order: end points and the control
/// points of curves, which give the tangent at either end.
fn path_points(d: &str) -> Vec<(f64, f64)> {
    let mut tokens = Vec::new();
    let mut number = String::new();
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() || ch == ' ' || ch == ',' {
            if !number.is_empty() {
                tokens.push(std::mem::take(&mut number));
            }
            if ch.is_ascii_alphabetic() {
                tokens.push(ch.to_string());
            }
        } else if ch == '-' && !number.is_empty() && !number.ends_with('e') {
            tokens.push(std::mem::take(&mut number));
            number.push(ch);
        } else {
            number.push(ch);
        }
    }
    if !number.is_empty() {
        tokens.push(number);
    }
    let mut points = Vec::new();
    let mut current = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    let mut command = 'M';
    let mut values: Vec<f64> = Vec::new();
    let mut flush =
        |command: char, values: &mut Vec<f64>, current: &mut (f64, f64), start: &mut (f64, f64)| {
            let take = |values: &[f64], index: usize| values.get(index).copied().unwrap_or(0.0);
            match command {
                'M' | 'L' | 'C' => {
                    for pair in values.chunks(2) {
                        if let [x, y] = pair {
                            *current = (*x, *y);
                            points.push(*current);
                        }
                    }
                    if command == 'M' {
                        *start = *current;
                    }
                }
                'H' => {
                    for value in values.iter() {
                        current.0 = *value;
                        points.push(*current);
                    }
                }
                'V' => {
                    for value in values.iter() {
                        current.1 = *value;
                        points.push(*current);
                    }
                }
                'A' => {
                    for arc in values.chunks(7) {
                        if arc.len() == 7 {
                            *current = (take(arc, 5), take(arc, 6));
                            points.push(*current);
                        }
                    }
                }
                'Z' => {
                    *current = *start;
                    points.push(*current);
                }
                _ => {}
            }
            values.clear();
        };
    for token in tokens {
        if let Some(letter) = token.chars().next().filter(char::is_ascii_alphabetic) {
            flush(command, &mut values, &mut current, &mut start);
            command = letter.to_ascii_uppercase();
            if command == 'Z' {
                flush(command, &mut values, &mut current, &mut start);
            }
        } else if let Ok(value) = token.parse::<f64>() {
            values.push(value);
        }
    }
    flush(command, &mut values, &mut current, &mut start);
    points
}

/// Builds the items of a group by composing child coordinates.
#[allow(clippy::many_single_char_names)] // DrawingML w/h/x/y/matrix names
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
    // AUD-71: a 1-EMU `chExt` under a normal `ext` yields ~1e19; clamp so
    // nested groups cannot inject NaN/inf into placement.
    let scale_x = clamp_group_scale(if ch_ext_w.abs() > f64::EPSILON {
        ext_w / ch_ext_w
    } else {
        1.0
    });
    let scale_y = clamp_group_scale(if ch_ext_h.abs() > f64::EPSILON {
        ext_h / ch_ext_h
    } else {
        1.0
    });
    let base_x = x + off_x - ch_off_x * scale_x;
    let base_y = y + off_y - ch_off_y * scale_y;
    let placement = ChildPlacement {
        base: (base_x, base_y),
        scale: (scale_x, scale_y),
        fallback: (w, h),
    };
    let items = place_children(ctx, &group.children, &placement);
    let angle = f64::from(transform.rot.unwrap_or(0)) / 60_000.0;
    if angle == 0.0 && !transform.flip_h && !transform.flip_v {
        return items;
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let sx = if transform.flip_h { -1.0 } else { 1.0 };
    let sy = if transform.flip_v { -1.0 } else { 1.0 };
    let (cx, cy) = (x + off_x + ext_w / 2.0, y + off_y + ext_h / 2.0);
    let (a, b, c, d) = (cos * sx, sin * sx, -sin * sy, cos * sy);
    transform_items(
        items,
        [a, b, c, d, cx - a * cx - c * cy, cy - b * cx - d * cy],
    )
}

/// Retains an affine transform until painting, so glyphs and image crops are
/// transformed together with their geometry in both backends.
#[allow(clippy::many_single_char_names)] // DrawingML w/h/x/y/matrix names
pub(crate) fn transform_items(items: Vec<Item>, matrix: [f64; 6]) -> Vec<Item> {
    if items.is_empty() {
        return items;
    }
    let [a, b, c, d, e, f] = matrix;
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for item in &items {
        let (x, y, w, h) = match item {
            Item::Text(text) => (
                text.x,
                text.baseline - text.size_px,
                text.width,
                1.3 * text.size_px,
            ),
            Item::Rect(rect) => (rect.x, rect.y, rect.w, rect.h),
            Item::Line(line) => (
                line.x1.min(line.x2),
                line.y1.min(line.y2),
                (line.x1 - line.x2).abs(),
                (line.y1 - line.y2).abs(),
            ),
            Item::Path(path) => (path.x, path.y, path.w, path.h),
            Item::Image(image) => (image.x, image.y, image.w, image.h),
            Item::Group(group) => (
                group.bounds[0],
                group.bounds[1],
                group.bounds[2],
                group.bounds[3],
            ),
        };
        let (angle, origin_x, origin_y) = match item {
            Item::Path(path) => (
                path.rotate_deg.to_radians(),
                path.x + path.w / 2.0,
                path.y + path.h / 2.0,
            ),
            Item::Text(text) => (text.run.rotation_deg.to_radians(), text.x, text.baseline),
            _ => (0.0, 0.0, 0.0),
        };
        for (x, y) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
            let (x, y) = if angle == 0.0 {
                (x, y)
            } else {
                let (sin, cos) = angle.sin_cos();
                let (dx, dy) = (x - origin_x, y - origin_y);
                (
                    origin_x + cos * dx - sin * dy,
                    origin_y + sin * dx + cos * dy,
                )
            };
            let (px, py) = (a * x + c * y + e, b * x + d * y + f);
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }
    }
    vec![Item::Group(crate::layout::GroupItem {
        matrix,
        items,
        bounds: [min_x, min_y, max_x - min_x, max_y - min_y],
        clip: None,
        clip_path: None,
        clip_inverse: false,
        opacity: 1.0,
    })]
}

/// Where a list of child graphics lands: child coordinates (px at the
/// document scale) map to `base + child * scale`.
struct ChildPlacement {
    /// Page position of the child-space origin.
    base: (f64, f64),
    /// Child-space to page scale, per axis.
    scale: (f64, f64),
    /// Size used for a child that carries no extent of its own.
    fallback: (f64, f64),
}

/// Builds the items of child graphics placed in their parent's space.
fn place_children(
    ctx: &LayoutContext<'_>,
    children: &[Graphic],
    placement: &ChildPlacement,
) -> Vec<Item> {
    let scale = ctx.options.scale;
    let (base_x, base_y) = placement.base;
    let (scale_x, scale_y) = placement.scale;
    let mut items = Vec::new();
    for child in children {
        let (child_off, child_ext) = child_box(child, scale);
        let cx = base_x + child_off.0 * scale_x;
        let cy = base_y + child_off.1 * scale_y;
        let cw = child_ext.0 * scale_x;
        let ch = child_ext.1 * scale_y;
        let (cw, ch) = if cw > 0.0 && ch > 0.0 {
            (cw, ch)
        } else {
            graphic_size(ctx, child, placement.fallback.0, placement.fallback.1)
        };
        items.extend(graphic_items(ctx, child, cx, cy, cw, ch));
    }
    items
}

/// Builds the items of a diagram from Word's cached drawing.
///
/// The drawing's space has its origin at the graphic frame's top-left corner
/// and is measured in EMU, so a drawing that fits its frame is placed one to
/// one. The space drawn is the frame grown to cover every shape (or the shape
/// tree's own `chOff`/`chExt`, when it has one), scaled into the box, so a
/// shape the producer placed outside the frame is pulled in rather than
/// painted over the text around it.
fn diagram_items(
    ctx: &LayoutContext<'_>,
    diagram: &DiagramDrawing,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let scale = ctx.options.scale;
    let tree_space = diagram.xfrm.and_then(|xfrm| {
        let extent = xfrm.child_extent?;
        let (off_x, off_y) = xfrm.child_offset.map_or((0.0, 0.0), |(cx, cy)| {
            (emu_to_px(cx.value(), scale), emu_to_px(cy.value(), scale))
        });
        Some((
            (off_x, off_y),
            (
                off_x + emu_to_px(extent.cx.value(), scale),
                off_y + emu_to_px(extent.cy.value(), scale),
            ),
        ))
    });
    let ((min_x, min_y), (max_x, max_y)) = tree_space.unwrap_or_else(|| {
        diagram.shapes.iter().fold(
            ((0.0_f64, 0.0_f64), (w, h)),
            |((min_x, min_y), (max_x, max_y)), child| {
                let ((off_x, off_y), (ext_w, ext_h)) = child_box(child, scale);
                (
                    (min_x.min(off_x), min_y.min(off_y)),
                    (max_x.max(off_x + ext_w), max_y.max(off_y + ext_h)),
                )
            },
        )
    });
    let span_w = max_x - min_x;
    let span_h = max_y - min_y;
    let scale_x = clamp_group_scale(if span_w > f64::EPSILON {
        w / span_w
    } else {
        1.0
    });
    let scale_y = clamp_group_scale(if span_h > f64::EPSILON {
        h / span_h
    } else {
        1.0
    });
    let placement = ChildPlacement {
        base: (x - min_x * scale_x, y - min_y * scale_y),
        scale: (scale_x, scale_y),
        fallback: (w, h),
    };
    place_children(ctx, &diagram.shapes, &placement)
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
#[allow(clippy::too_many_lines)]
fn text_box_items(
    ctx: &LayoutContext<'_>,
    text: &TextBox,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let body = text.body.clone().unwrap_or_default();
    let _text_flow = crate::table_style::enter_text_flow(
        strict_ooxml_wml::model::values::TextDirection::LrTb,
        Some(h),
    );
    let _wrap = crate::table_style::wrap_override(body.wrap.as_deref() == Some("none"));
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
    // The insets are EMU from the input and `a:bodyPr/@lIns` is a signed 64-bit
    // attribute nobody range-checks, so the sum of two of them overflows in a
    // debug build and wraps in release. Saturating, and the inner width is
    // clamped to at least one pixel: a text box whose insets exceed its own
    // extent has no interior, and "no interior" has to be drawn as something
    // rather than as a negative width (AUD-09).
    let inner_width = (w - emu_to_px(left_inset.saturating_add(right_inset), scale)).max(1.0);
    let available = (h - emu_to_px(top_inset.saturating_add(bottom_inset), scale)).max(1.0);
    let angle = match body.vert.as_deref() {
        Some("vert270") => -90.0_f64,
        Some("vert" | "eaVert" | "mongolianVert" | "wordArtVert" | "wordArtVertRtl") => 90.0,
        _ => 0.0,
    };
    let (layout_width, layout_height) = if angle == 0.0 {
        (inner_width, available)
    } else {
        (available, inner_width)
    };
    let columns = body.num_col.unwrap_or(1).clamp(1, 45) as usize;
    let gap = body
        .spc_col
        .map_or(0.0, |value| emu_to_px(i64::from(value.max(0)), scale));
    let column_width = ((layout_width - gap * (columns - 1) as f64) / columns as f64).max(1.0);
    let mut items = Vec::new();
    let mut content_y = 0.0;
    // A text box is one container, so its content is one level deeper than
    // whatever was being laid out here. The depth is read from the context rather
    // than reset to 0: this function is reached from the paint stage, so the
    // literal 0 that stood here was the reason a text box inside a table cell
    // looked like the top of the document, and a chain of them overflowed the
    // stack. The over-deep case is already refused by the model walk in
    // `layout_document`; this is the layout's own accounting.
    let mut page_frames = Vec::new();
    layout_blocks_inline(
        ctx,
        &text.blocks,
        0.0,
        column_width,
        &mut content_y,
        &mut items,
        ctx.block_depth() + 1,
        None,
        false,
        &mut page_frames,
    );
    if columns > 1 {
        let bands = items
            .iter()
            .map(|item| crate::layout::table::item_vertical_bounds(ctx, item))
            .collect();
        let splits = crate::layout::table::row_split_points(bands, content_y);
        let row = crate::layout::TableRowFlow {
            anchors: Vec::new(),
            items,
            height: content_y,
            header: false,
            page_frames: Vec::new(),
            split_points: splits,
        };
        items = Vec::new();
        let mut start = 0.0;
        let mut height = 0.0_f64;
        for index in 0..columns {
            if start >= content_y {
                break;
            }
            let end = if index + 1 == columns || content_y - start <= layout_height {
                content_y
            } else {
                row.split_points
                    .iter()
                    .copied()
                    .rfind(|point| *point > start && *point <= start + layout_height)
                    .or_else(|| {
                        row.split_points
                            .iter()
                            .copied()
                            .find(|point| *point > start)
                    })
                    .unwrap_or(content_y)
            };
            let fragment = crate::layout::table::row_fragment(&row, start, end);
            height = height.max(fragment.height);
            let column = if body.rtl_col == Some(true) {
                columns - 1 - index
            } else {
                index
            };
            items.extend(
                fragment
                    .items
                    .iter()
                    .map(|item| offset_item(item, column as f64 * (column_width + gap), 0.0)),
            );
            start = end;
        }
        content_y = height;
    }
    let shift = match body.anchor.unwrap_or(TextAnchor::Top) {
        TextAnchor::Top => 0.0,
        TextAnchor::Center => ((layout_height - content_y) / 2.0).max(0.0),
        TextAnchor::Bottom => (layout_height - content_y).max(0.0),
    };
    let horizontal_shift = if body.anchor_centered == Some(true) {
        let bounds = transform_items(items.clone(), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        match bounds.first() {
            Some(Item::Group(group)) => (layout_width - group.bounds[2]) / 2.0 - group.bounds[0],
            _ => 0.0,
        }
    } else {
        0.0
    };
    let mut items: Vec<_> = items
        .iter()
        .map(|item| offset_item(item, horizontal_shift, shift))
        .collect();
    let ellipsis = body.vert_overflow.as_deref() == Some("ellipsis");
    if ellipsis && content_y + shift > layout_height + 0.5 {
        items = ellipsize(ctx, items, layout_width, layout_height);
    }
    if body.horz_overflow.as_deref() == Some("clip")
        || body.vert_overflow.as_deref() == Some("clip")
        || ellipsis
    {
        let horizontal_clip = body.horz_overflow.as_deref() == Some("clip");
        let vertical_clip = matches!(body.vert_overflow.as_deref(), Some("clip" | "ellipsis"));
        items = transform_items(items, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        if let Some(Item::Group(group)) = items.first_mut() {
            let [bx, by, bw, bh] = group.bounds;
            let cx = if horizontal_clip { 0.0 } else { bx.min(0.0) };
            let cy = if vertical_clip { 0.0 } else { by.min(0.0) };
            let cw = if horizontal_clip {
                layout_width
            } else {
                (bx + bw).max(layout_width) - cx
            };
            let ch = if vertical_clip {
                layout_height
            } else {
                (by + bh).max(layout_height) - cy
            };
            group.clip = Some([cx, cy, cw, ch]);
            let x1 = bx.max(cx);
            let y1 = by.max(cy);
            group.bounds = [
                x1,
                y1,
                (bx + bw).min(cx + cw).max(x1) - x1,
                (by + bh).min(cy + ch).max(y1) - y1,
            ];
        }
    }
    if matches!(
        body.vert.as_deref(),
        Some("eaVert" | "mongolianVert" | "wordArtVert" | "wordArtVertRtl")
    ) {
        let all = matches!(body.vert.as_deref(), Some("wordArtVert" | "wordArtVertRtl"));
        items = crate::layout::table::rotate_upright_items(ctx, items, all);
    }
    if angle == 0.0 {
        return items
            .iter()
            .map(|item| offset_item(item, left, top))
            .collect();
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    transform_items(
        items,
        [
            cos,
            sin,
            -sin,
            cos,
            left + inner_width / 2.0 - cos * layout_width / 2.0 + sin * layout_height / 2.0,
            top + available / 2.0 - sin * layout_width / 2.0 - cos * layout_height / 2.0,
        ],
    )
}

/// One outline of a shape and whether it is filled and stroked.
struct ShapePath {
    d: String,
    fill: bool,
    stroke: bool,
}

/// Presets whose outline is open: a line or a connector has nothing to fill.
const OPEN_PRESETS: &[&str] = &[
    "line",
    "straightConnector1",
    "bentConnector2",
    "bentConnector3",
    "bentConnector4",
    "bentConnector5",
    "curvedConnector2",
    "curvedConnector3",
    "curvedConnector4",
    "curvedConnector5",
    "arc",
];

/// Returns the outlines of a shape in local coordinates (AUD-49).
fn shape_paths(shape: &Shape, w: f64, h: f64) -> Vec<ShapePath> {
    let closed = |d: String| ShapePath {
        d,
        fill: true,
        stroke: true,
    };
    match &shape.geometry {
        strict_ooxml_wml::model::drawing::ShapeGeometry::None => vec![closed(rect_path(w, h))],
        strict_ooxml_wml::model::drawing::ShapeGeometry::Preset(preset) => {
            let open = OPEN_PRESETS.contains(&preset.as_ref());
            preset_path(preset, w, h)
                .into_iter()
                .map(|d| ShapePath {
                    d,
                    fill: !open,
                    stroke: true,
                })
                .collect()
        }
        strict_ooxml_wml::model::drawing::ShapeGeometry::Custom(custom) => {
            custom_paths(custom, w, h)
        }
    }
}

/// Resolves a fill to an SVG colour (gradients/patterns use their first colour).
fn fill_color(ctx: &LayoutContext<'_>, fill: &ShapeFill) -> Option<String> {
    let theme = ctx.document.theme.as_ref();
    match fill {
        // A picture fill paints the picture, not a colour.
        ShapeFill::None | ShapeFill::Blip { .. } => None,
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
///
/// An `a:ln` that names no colour, and a shape with no `a:ln` at all, take
/// the `a:lnRef` of the shape's `wps:style`.
fn stroke_paint(
    ctx: &LayoutContext<'_>,
    stroke: Option<&ShapeStroke>,
    style: &ShapeStyleColors,
) -> (Option<String>, f64, Option<String>) {
    let scale = ctx.options.scale;
    let Some(stroke) = stroke else {
        return match &style.line {
            Some((color, width)) => (Some(color.clone()), emu_to_px(*width, scale).max(0.5), None),
            None => (None, 0.0, None),
        };
    };
    if stroke.none {
        return (None, 0.0, None);
    }
    let theme = ctx.document.theme.as_ref();
    let color = stroke
        .color
        .as_ref()
        .and_then(|color| resolve_shape_color(theme, color))
        .or_else(|| style.line.as_ref().map(|(color, _)| color.clone()))
        .or_else(|| Some("#000000".to_owned()));
    let width = stroke
        .width
        .map(strict_ooxml_wml::model::Emu::value)
        .or_else(|| style.line.as_ref().map(|(_, width)| *width))
        .map_or(1.0, |emu| emu_to_px(emu, scale))
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

/// `bodyPr/@vertOverflow="ellipsis"`: text below the last line that fits the
/// box is dropped and that line ends in "…" (ISO/IEC 29500-1 §21.1.2.1.1,
/// ST_TextVertOverflowType). Text items are positioned in the box's own
/// coordinates, `0..height`.
fn ellipsize(ctx: &LayoutContext<'_>, items: Vec<Item>, width: f64, height: f64) -> Vec<Item> {
    const DESCENT_EM: f64 = 0.25;
    let fits =
        |text: &crate::layout::TextItem| text.baseline + text.size_px * DESCENT_EM <= height + 0.5;
    let last_baseline = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) if fits(text) => Some(text.baseline),
            _ => None,
        })
        .fold(f64::NEG_INFINITY, f64::max);
    if !last_baseline.is_finite() {
        return Vec::new();
    }
    let mut kept: Vec<Item> = items
        .into_iter()
        .filter(|item| match item {
            Item::Text(text) => text.baseline <= last_baseline + 0.5,
            other => crate::layout::table::item_vertical_bounds(ctx, other).0 < height,
        })
        .collect();
    let last = kept
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Text(text) if (text.baseline - last_baseline).abs() <= 0.5 => {
                Some((index, text.x + text.width))
            }
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index);
    if let Some(Item::Text(text)) = last.and_then(|index| kept.get_mut(index)) {
        let mark = "\u{2026}";
        let mark_width = ctx.measure(mark, &text.run);
        let mut body = text.text.trim_end().to_owned();
        while !body.is_empty() && text.x + ctx.measure(&body, &text.run) + mark_width > width {
            body.pop();
        }
        body.push_str(mark);
        text.width = ctx.measure(&body, &text.run);
        text.text = body;
        text.advance = crate::layout::TextAdvanceKind::Shaped;
    }
    kept
}

/// A generic placeholder rectangle: an outline only, so an object the
/// renderer cannot draw (ink, an unknown graphic) never hides the text under
/// or beside it (WQ-023).
fn placeholder(x: f64, y: f64, w: f64, h: f64) -> Item {
    Item::Rect(RectItem {
        x,
        y,
        w: w.max(0.0),
        h: h.max(0.0),
        fill: None,
        stroke: Some("#bfbfbf".to_owned()),
        stroke_w: 0.75,
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
        "line" | "straightConnector1" => format!("M0 0 L{} {}", f(w), f(h)),
        // Connectors at their default adjustments: an elbow at the far end,
        // a dog-leg through the middle, an S-curve through the middle.
        "bentConnector2" => format!("M0 0 H{} V{}", f(w), f(h)),
        "bentConnector3" | "bentConnector4" | "bentConnector5" => {
            format!("M0 0 H{} V{} H{}", f(w / 2.0), f(h), f(w))
        }
        "curvedConnector2" => format!("M0 0 C{} 0 {} {} {} {}", f(w), f(w), f(h), f(w), f(h)),
        "curvedConnector3" | "curvedConnector4" | "curvedConnector5" => format!(
            "M0 0 C{} 0 {} {} {} {}",
            f(w / 2.0),
            f(w / 2.0),
            f(h),
            f(w),
            f(h)
        ),
        // `arc` at its default 270° → 0°: the quarter of the ellipse from the
        // top centre to the right centre.
        "arc" => format!(
            "M{} 0 A{} {} 0 0 1 {} {}",
            f(w / 2.0),
            f(w / 2.0),
            f(h / 2.0),
            f(w),
            f(h / 2.0)
        ),
        "mathMinus" => format!(
            "M{} {} H{} V{} H{} Z",
            f(w * 0.0735),
            f(h * 0.3824),
            f(w * 0.9265),
            f(h * 0.6176),
            f(w * 0.0735)
        ),
        "trapezoid" => format!(
            "M0 {} L{} 0 H{} L{} {} Z",
            f(h),
            f(w * 0.25),
            f(w * 0.75),
            f(w),
            f(h)
        ),
        "homePlate" => format!(
            "M0 0 H{} L{} {} L{} {} H0 Z",
            f(w * 0.75),
            f(w),
            f(h / 2.0),
            f(w * 0.75),
            f(h)
        ),
        "cloud" | "flowChartConnector" => preset_path("ellipse", w, h)?,
        "star24" => star_path(24, w, h),
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
        "flowChartTerminator" | "flowChartAlternateProcess" | "round2SameRect" => {
            preset_path("roundRect", w, h)?
        }
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

/// Returns one SVG `d` string per custom-geometry path (AUD-49).
fn custom_paths(custom: &CustomGeometry, w: f64, h: f64) -> Vec<ShapePath> {
    custom
        .paths
        .iter()
        .map(|path| ShapePath {
            d: geometry_path_d(custom, path, w, h),
            // `a:path/@fill="none"` outlines without filling; `@stroke="0"`
            // fills without outlining.
            fill: path.fill.as_deref() != Some("none"),
            stroke: path.stroke != Some(false),
        })
        .filter(|path| !path.d.is_empty())
        .collect()
}

#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn geometry_path_d(
    custom: &CustomGeometry,
    path: &strict_ooxml_wml::model::drawing::GeometryPath,
    w: f64,
    h: f64,
) -> String {
    let gw = if path.width > 0 { path.width as f64 } else { w };
    let gh = if path.height > 0 {
        path.height as f64
    } else {
        h
    };
    let (sx, sy) = (w / gw.max(1.0), h / gh.max(1.0));
    let mut guides = std::collections::HashMap::new();
    for (name, formula) in custom.adjustments.iter().chain(&custom.guides) {
        if let Some(value) = geometry_formula(formula, gw, gh, &guides) {
            guides.insert(name.to_string(), value);
        }
    }
    let f = crate::units::fmt_num;
    let mut out = String::new();
    let mut current = (0.0, 0.0);
    let mut origin = current;
    for command in &path.commands {
        let (kind, values): (&str, Vec<f64>) = match command {
            PathCommand::MoveTo { x, y } => ("moveTo", vec![*x as f64, *y as f64]),
            PathCommand::LineTo { x, y } => ("lnTo", vec![*x as f64, *y as f64]),
            PathCommand::CubicBezTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => (
                "cubicBezTo",
                vec![
                    *x1 as f64, *y1 as f64, *x2 as f64, *y2 as f64, *x as f64, *y as f64,
                ],
            ),
            PathCommand::Parametric { kind, values } => {
                let Some(values) = values
                    .iter()
                    .map(|v| geometry_value(v, gw, gh, &guides))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                (kind.as_ref(), values)
            }
            PathCommand::Close => {
                out.push_str("Z ");
                current = origin;
                continue;
            }
        };
        match (kind, values.as_slice()) {
            ("moveTo" | "lnTo", [x, y]) => {
                let _ = write!(
                    out,
                    "{}{} {} ",
                    if kind == "moveTo" { "M" } else { "L" },
                    f(x * sx),
                    f(y * sy)
                );
                current = (*x, *y);
                if kind == "moveTo" {
                    origin = current;
                }
            }
            ("cubicBezTo", [x1, y1, x2, y2, x, y]) => {
                let _ = write!(
                    out,
                    "C{} {} {} {} {} {} ",
                    f(x1 * sx),
                    f(y1 * sy),
                    f(x2 * sx),
                    f(y2 * sy),
                    f(x * sx),
                    f(y * sy)
                );
                current = (*x, *y);
            }
            ("quadBezTo", [x1, y1, x, y]) => {
                let (cx1, cy1) = (
                    current.0 + (x1 - current.0) * 2.0 / 3.0,
                    current.1 + (y1 - current.1) * 2.0 / 3.0,
                );
                let (cx2, cy2) = (x + (x1 - x) * 2.0 / 3.0, y + (y1 - y) * 2.0 / 3.0);
                let _ = write!(
                    out,
                    "C{} {} {} {} {} {} ",
                    f(cx1 * sx),
                    f(cy1 * sy),
                    f(cx2 * sx),
                    f(cy2 * sy),
                    f(x * sx),
                    f(y * sy)
                );
                current = (*x, *y);
            }
            ("arcTo", [rx, ry, start, sweep]) if *rx > 0.0 && *ry > 0.0 => {
                // DrawingML angles are polar rays, not ellipse parameters.
                let parameter = |angle: f64| (rx * angle.sin()).atan2(ry * angle.cos());
                let polar_start = (start / 60000.0).to_radians();
                let polar_sweep = (sweep / 60000.0)
                    .to_radians()
                    .clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
                let a = parameter(polar_start);
                let mut delta = parameter(polar_start + polar_sweep) - a;
                if polar_sweep > 0.0 && delta <= 0.0 {
                    delta += std::f64::consts::TAU;
                }
                if polar_sweep < 0.0 && delta >= 0.0 {
                    delta -= std::f64::consts::TAU;
                }
                if polar_sweep == 0.0 {
                    continue;
                }
                let (cx, cy) = (current.0 - rx * a.cos(), current.1 - ry * a.sin());
                let steps = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
                for index in 0..steps {
                    let a1 = a + delta * index as f64 / steps as f64;
                    let a2 = a + delta * (index + 1) as f64 / steps as f64;
                    let k = 4.0 / 3.0 * ((a2 - a1) / 4.0).tan();
                    let (x1, y1) = (
                        cx + rx * (a1.cos() - k * a1.sin()),
                        cy + ry * (a1.sin() + k * a1.cos()),
                    );
                    let (x2, y2) = (
                        cx + rx * (a2.cos() + k * a2.sin()),
                        cy + ry * (a2.sin() - k * a2.cos()),
                    );
                    current = (cx + rx * a2.cos(), cy + ry * a2.sin());
                    let _ = write!(
                        out,
                        "C{} {} {} {} {} {} ",
                        f(x1 * sx),
                        f(y1 * sy),
                        f(x2 * sx),
                        f(y2 * sy),
                        f(current.0 * sx),
                        f(current.1 * sy)
                    );
                }
            }
            _ => {}
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
