//! Portable vector approximation of DrawingML shadow and glow effects.
use super::graphics::transform_items;
use crate::layout::{Item, LayoutContext};

#[derive(Default)]
struct Effect {
    kind: String,
    color: String,
    alpha: f64,
    radius: f64,
    distance: f64,
    angle: f64,
    sx: f64,
    sy: f64,
}
#[allow(clippy::too_many_lines)]
fn read(ctx: &LayoutContext<'_>, markup: &str) -> Vec<Effect> {
    use strict_ooxml_core::{
        part::PartId,
        xml::{XmlEvent, XmlReader},
    };
    let xml =
        format!("<root xmlns:a=\"http://purl.oclc.org/ooxml/drawingml/main\">{markup}</root>");
    let Ok(mut reader) = XmlReader::new(
        xml.as_bytes(),
        PartId::new("/word/effects.xml"),
        &ctx.options.limits,
    ) else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    let mut active: Option<Effect> = None;
    loop {
        match reader.next_event() {
            Ok(XmlEvent::StartElement { name, attrs }) => {
                let value = |key: &str| {
                    attrs
                        .iter()
                        .find(|attr| attr.name.local == key)
                        .map(|attr| attr.value.as_str())
                };
                let number = |key: &str, fallback: f64| {
                    value(key)
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(fallback)
                };
                match name.local.as_str() {
                    "outerShdw" | "innerShdw" | "glow" => {
                        active = Some(Effect {
                            kind: name.local.clone(),
                            color: "#000000".to_owned(),
                            alpha: 1.0,
                            radius: crate::units::emu_to_px(
                                number(
                                    if name.local == "glow" {
                                        "rad"
                                    } else {
                                        "blurRad"
                                    },
                                    0.0,
                                ) as i64,
                                ctx.options.scale,
                            )
                            .clamp(0.0, 256.0),
                            distance: crate::units::emu_to_px(
                                number("dist", 0.0) as i64,
                                ctx.options.scale,
                            ),
                            angle: number("dir", 0.0) / 60000.0,
                            sx: number("sx", 100_000.0) / 100_000.0,
                            sy: number("sy", 100_000.0) / 100_000.0,
                        });
                    }
                    "blur" | "reflection" | "softEdge" | "fillOverlay" | "duotone" | "prstShdw" => {
                        ctx.warn(format!(
                            "render.effect.unsupported: {} retained but not rendered",
                            name.local
                        ));
                    }
                    "srgbClr" => {
                        if let (Some(effect), Some(color)) = (&mut active, value("val")) {
                            if color.len() == 6 && color.chars().all(|ch| ch.is_ascii_hexdigit()) {
                                effect.color = format!("#{color}");
                            }
                        }
                    }
                    "schemeClr" => {
                        if let (Some(effect), Some(color)) = (&mut active, value("val")) {
                            if let Some(color) = ctx
                                .document
                                .theme
                                .as_ref()
                                .and_then(|theme| theme.color(color))
                            {
                                effect.color = format!("#{color}");
                            }
                        }
                    }
                    "alpha" => {
                        if let Some(effect) = &mut active {
                            effect.alpha = (number("val", 100_000.0) / 100_000.0).clamp(0.0, 1.0);
                        }
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::EndElement { name }) => {
                if active
                    .as_ref()
                    .is_some_and(|effect| effect.kind == name.local)
                {
                    if let Some(effect) = active.take() {
                        effects.push(effect);
                    }
                }
            }
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
    }
    effects
}
fn recolor(item: &mut Item, color: &str, glow: Option<f64>) {
    match item {
        Item::Group(group) => {
            for child in &mut group.items {
                recolor(child, color, glow);
            }
        }
        Item::Path(path) => {
            if let Some(radius) = glow {
                path.fill = None;
                path.stroke = Some(color.to_owned());
                path.stroke_w += radius * 2.0;
                path.dash = None;
            } else {
                if path.fill.is_some() || path.paint.is_some() {
                    path.fill = Some(color.to_owned());
                }
                if path.stroke.is_some() {
                    path.stroke = Some(color.to_owned());
                }
            }
            path.paint = None;
        }
        Item::Rect(rect) => {
            rect.fill = Some(color.to_owned());
            rect.stroke = None;
        }
        Item::Line(line) => {
            color.clone_into(&mut line.color);
            if let Some(radius) = glow {
                line.width += radius * 2.0;
            }
        }
        Item::Text(text) => {
            text.run.color = Some(color.to_owned());
            text.run.shading = None;
            text.run.highlight = None;
        }
        Item::Image(_) => {}
    }
}
/// Places effects beneath the original shape. Blur uses bounded vector samples
/// so SVG and PDF share the same approximation without a backend-specific filter.
pub(crate) fn apply(
    ctx: &LayoutContext<'_>,
    markup: Option<&str>,
    items: Vec<Item>,
    bounds: [f64; 4],
) -> Vec<Item> {
    let Some(markup) = markup else {
        return items;
    };
    let effects = read(ctx, markup);
    let mut output = Vec::new();
    let mut foreground = Vec::new();
    let silhouette = items.iter().find_map(|item| match item {
        Item::Path(path) if path.fill.is_some() || path.paint.is_some() => Some(path.clone()),
        _ => None,
    });
    for effect in effects {
        let (sin, cos) = effect.angle.to_radians().sin_cos();
        let dx = cos * effect.distance;
        let dy = sin * effect.distance;
        let (cx, cy) = (bounds[0] + bounds[2] / 2.0, bounds[1] + bounds[3] / 2.0);
        let samples: usize = if effect.radius > 0.0 { 9 } else { 1 };
        for sample in 0..samples {
            if effect.kind == "innerShdw" {
                let Some(silhouette) = silhouette.as_ref() else {
                    continue;
                };
                let angle = sample.saturating_sub(1) as f64 * std::f64::consts::TAU / 8.0;
                let blur = if sample > 0 { effect.radius / 2.0 } else { 0.0 };
                let mut shifted = silhouette.clone();
                shifted.x += dx + angle.cos() * blur;
                shifted.y += dy + angle.sin() * blur;
                let fill = Item::Rect(crate::layout::RectItem {
                    x: bounds[0],
                    y: bounds[1],
                    w: bounds[2],
                    h: bounds[3],
                    fill: Some(effect.color.clone()),
                    stroke: None,
                    stroke_w: 0.0,
                });
                let mut inner = transform_items(vec![fill], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                if let Some(Item::Group(group)) = inner.first_mut() {
                    group.clip = Some(bounds);
                    group.clip_path = Some(shifted);
                    group.clip_inverse = true;
                }
                let mut outer = transform_items(inner, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                if let Some(Item::Group(group)) = outer.first_mut() {
                    group.clip_path = Some(silhouette.clone());
                    group.opacity = effect.alpha / samples as f64;
                }
                foreground.extend(outer);
                continue;
            }
            let mut copy: Vec<_> = items
                .iter()
                .filter(|item| !matches!(item, Item::Image(_)))
                .cloned()
                .collect();
            let glow = (effect.kind == "glow")
                .then_some(effect.radius * (sample + 1) as f64 / samples as f64);
            for item in &mut copy {
                recolor(item, &effect.color, glow);
            }
            let (bx, by) = if effect.kind != "glow" && sample > 0 {
                let angle = (sample - 1) as f64 * std::f64::consts::TAU / 8.0;
                (
                    angle.cos() * effect.radius / 2.0,
                    angle.sin() * effect.radius / 2.0,
                )
            } else {
                (0.0, 0.0)
            };
            let mut groups = transform_items(
                copy,
                [
                    effect.sx,
                    0.0,
                    0.0,
                    effect.sy,
                    dx + bx + cx * (1.0 - effect.sx),
                    dy + by + cy * (1.0 - effect.sy),
                ],
            );
            for group in &mut groups {
                if let Item::Group(group) = group {
                    group.opacity = effect.alpha / samples as f64;
                    if effect.kind == "innerShdw" {
                        group.clip = Some(bounds);
                    }
                }
            }
            output.extend(groups);
        }
    }
    output.extend(items);
    output.extend(foreground);
    output
}
