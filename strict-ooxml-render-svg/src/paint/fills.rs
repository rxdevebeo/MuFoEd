//! Resolve DrawingML fills once, for both output backends.

use crate::layout::{LayoutContext, PathPaint};
use crate::style::resolve_shape_color;
use strict_ooxml_wml::model::drawing::{Shape, ShapeFill};

// Declarative gradient and pattern cases stay together as one fill resolver.
#[allow(clippy::too_many_lines)]
pub(super) fn resolve(ctx: &LayoutContext<'_>, shape: &Shape, w: f64, h: f64) -> Option<PathPaint> {
    let theme = ctx.document.theme.as_ref();
    match shape.fill.as_ref()? {
        ShapeFill::Gradient {
            stops,
            angle,
            scaled,
            rotate_with_shape,
            path_shading,
        } => {
            if path_shading.is_some() {
                ctx.warn(
                    "render.gradient: path shading is unsupported; flat first-stop fallback"
                        .to_owned(),
                );
                return None;
            }
            if stops.len() > 4096 {
                ctx.warn("render.gradient: more than 4096 stops; flat fallback".to_owned());
                return None;
            }
            let mut resolved: Vec<_> = stops
                .iter()
                .filter_map(|stop| {
                    resolve_shape_color(theme, &stop.color).map(|color| {
                        (
                            (f64::from(stop.position) / 100_000.0).clamp(0.0, 1.0),
                            color,
                        )
                    })
                })
                .collect();
            resolved.sort_by(|a, b| a.0.total_cmp(&b.0));
            if resolved.is_empty() {
                ctx.warn("render.gradient: no resolvable gradient stops".to_owned());
                return None;
            }
            if resolved
                .windows(2)
                .any(|pair| matches!(pair, [a, b] if a.0.total_cmp(&b.0).is_eq()))
            {
                ctx.warn("render.gradient: coincident stops use the last colour".to_owned());
                resolved.dedup_by(|a, b| {
                    if a.0.total_cmp(&b.0).is_eq() {
                        b.1.clone_from(&a.1);
                        true
                    } else {
                        false
                    }
                });
            }
            if let Some(first) = resolved.first().cloned() {
                if first.0 > 0.0 {
                    resolved.insert(0, (0.0, first.1));
                }
            }
            if let Some(last) = resolved.last().cloned() {
                if last.0 < 1.0 {
                    resolved.push((1.0, last.1));
                }
            }
            // A single end stop is a solid gradient extending over the box.
            if resolved.len() == 1 {
                if let Some(stop) = resolved.first().cloned() {
                    resolved = vec![(0.0, stop.1.clone()), (1.0, stop.1)];
                }
            }
            let mut degrees = f64::from(angle.unwrap_or(0)) / 60_000.0;
            if *rotate_with_shape == Some(false) {
                degrees -= shape
                    .xfrm
                    .and_then(|xfrm| xfrm.rot)
                    .map_or(0.0, |rot| f64::from(rot) / 60_000.0);
            }
            let (sin, cos) = degrees.to_radians().sin_cos();
            let (dx, dy) = if scaled.unwrap_or(false) {
                let extent = cos.abs() + sin.abs();
                (cos * extent * w / 2.0, sin * extent * h / 2.0)
            } else {
                let extent = (w * cos).abs() + (h * sin).abs();
                (cos * extent / 2.0, sin * extent / 2.0)
            };
            Some(PathPaint::LinearGradient {
                stops: resolved,
                start: (w / 2.0 - dx, h / 2.0 - dy),
                end: (w / 2.0 + dx, h / 2.0 + dy),
            })
        }
        ShapeFill::Pattern {
            preset,
            foreground,
            background,
        } => {
            let preset = preset.as_deref().unwrap_or("pct5");
            let foreground = foreground
                .as_ref()
                .and_then(|color| resolve_shape_color(theme, color))
                .unwrap_or_else(|| "#000000".to_owned());
            let background = background
                .as_ref()
                .and_then(|color| resolve_shape_color(theme, color))
                .unwrap_or_else(|| "#ffffff".to_owned());
            let mut lines = Vec::new();
            let mut rectangles = Vec::new();
            match preset {
                "horz" | "ltHorz" | "dkHorz" | "narHorz" => lines.push((0.0, 0.5, 1.0, 0.5)),
                "vert" | "ltVert" | "dkVert" | "narVert" => lines.push((0.5, 0.0, 0.5, 1.0)),
                "cross" | "smGrid" | "lgGrid" => {
                    lines.extend([(0.0, 0.5, 1.0, 0.5), (0.5, 0.0, 0.5, 1.0)]);
                }
                "dnDiag" | "ltDnDiag" | "dkDnDiag" | "wdDnDiag" => lines.extend([
                    (0.0, 0.0, 1.0, 1.0),
                    (-0.5, 0.0, 0.5, 1.0),
                    (0.5, 0.0, 1.5, 1.0),
                ]),
                "upDiag" | "ltUpDiag" | "dkUpDiag" | "wdUpDiag" => lines.extend([
                    (0.0, 1.0, 1.0, 0.0),
                    (-0.5, 1.0, 0.5, 0.0),
                    (0.5, 1.0, 1.5, 0.0),
                ]),
                "diagCross" => lines.extend([(0.0, 0.0, 1.0, 1.0), (0.0, 1.0, 1.0, 0.0)]),
                "smCheck" | "lgCheck" => {
                    rectangles.extend([(0.0, 0.0, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)]);
                }
                value if value.starts_with("pct") => {
                    let Some(percent) = value
                        .strip_prefix("pct")
                        .and_then(|value| value.parse::<u32>().ok())
                        .filter(|value| *value <= 100)
                    else {
                        ctx.warn(format!("render.pattern: unsupported preset {preset}"));
                        return None;
                    };
                    // A fixed ordered 8x8 Bayer tile: 64 cells, stable at every zoom.
                    for y in 0..8_u32 {
                        for x in 0..8_u32 {
                            let mut rank = 0;
                            for bit in 0..3 {
                                rank = rank * 4
                                    + (((x >> bit) & 1) ^ ((y >> bit) & 1)) * 2
                                    + ((y >> bit) & 1);
                            }
                            if rank * 100 < percent * 64 {
                                rectangles.push((
                                    f64::from(x) / 8.0,
                                    f64::from(y) / 8.0,
                                    0.125,
                                    0.125,
                                ));
                            }
                        }
                    }
                }
                _ => {
                    ctx.warn(format!(
                        "render.pattern: unsupported preset {preset}; flat foreground fallback"
                    ));
                    return None;
                }
            }
            // The variants carry different densities, not merely different names.
            let edge = if preset.starts_with("nar") || preset.starts_with("sm") {
                4.0
            } else if preset.starts_with("wd") || preset.starts_with("lg") {
                12.0
            } else {
                8.0
            };
            if preset.starts_with("dk") {
                // Dark hatches add an adjacent parallel line.
                lines.extend(lines.clone().into_iter().map(|(x1, y1, x2, y2)| {
                    if preset.ends_with("Horz") {
                        (x1, y1 + 0.125, x2, y2 + 0.125)
                    } else {
                        (x1 + 0.125, y1, x2 + 0.125, y2)
                    }
                }));
            }
            Some(PathPaint::Pattern {
                foreground,
                background,
                size: edge * ctx.options.scale / 96.0,
                lines,
                rectangles,
            })
        }
        _ => None,
    }
}
