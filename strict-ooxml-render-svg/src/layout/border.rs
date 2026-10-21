//! Border edges: resolution from `w:pBdr`/`w:tcBorders`/`w:pgBorders` values
//! and the strokes one edge is drawn with.
//!
//! An edge is a *band*: compound styles (`double`, `triple`, `thinThick*`)
//! draw several parallel lines inside it, so the band is wider than `w:sz`.
//! Word sizes the band the same way, which is why the space it reserves for
//! a `double` border is three line widths, not one.

use std::fmt::Write as _;

use strict_ooxml_wml::model::theme::Theme;
use strict_ooxml_wml::model::values::{Border, BorderStyle};

use super::{Item, LineItem, PathItem};

/// One border edge after the cascade, in points.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBorder {
    /// Draw an offset shadow behind the border.
    pub shadow: bool,
    /// Reverse compound border appearance from inside to outside.
    pub frame: bool,
    /// Line style.
    pub style: BorderStyle,
    /// Width of one line (`w:sz`), in points.
    pub width_pt: f64,
    /// Colour, `#rrggbb` (`auto` is black).
    pub color: String,
    /// Distance from the text (`w:space`), in points.
    pub space_pt: f64,
}

impl ResolvedBorder {
    /// Width of the whole band, in points.
    #[must_use]
    pub fn band_pt(&self) -> f64 {
        self.width_pt * band_factor(self.style)
    }
}

/// Resolves one `w:pBdr`-style edge. `None` for `nil`/`none` or no style.
#[must_use]
pub fn resolve_border(border: &Border, theme: Option<&Theme>) -> Option<ResolvedBorder> {
    let style = border.style?;
    if matches!(style, BorderStyle::Nil | BorderStyle::None) {
        return None;
    }
    let color = border
        .theme_color
        .as_deref()
        .and_then(|reference| crate::style::resolve_theme_color(reference, theme))
        .or_else(|| border.color.as_ref().and_then(crate::style::parse_color))
        .unwrap_or_else(|| "#000000".to_owned());
    // `w:sz` is in eighths of a point; Word draws nothing thinner than a
    // quarter point (`ST_EighthPointMeasure` minimum 2).
    let width_pt = border
        .size
        .map_or(0.5, |size| f64::from(size.value()) / 8.0)
        .max(0.25);
    Some(ResolvedBorder {
        shadow: border.shadow,
        frame: border.frame,
        style,
        width_pt,
        color,
        space_pt: f64::from(border.space.unwrap_or(0)),
    })
}

/// How many line widths the band of `style` spans.
fn band_factor(style: BorderStyle) -> f64 {
    lines_of(style)
        .last()
        .map_or(1.0, |(start, width)| start + width)
}

/// The parallel lines of a style as `(offset, width)` in line widths, from
/// the outer edge of the band inwards.
fn lines_of(style: BorderStyle) -> &'static [(f64, f64)] {
    match style {
        BorderStyle::Double | BorderStyle::DoubleWave => &[(0.0, 1.0), (2.0, 1.0)],
        BorderStyle::ThreeDEmboss | BorderStyle::ThreeDEngrave => {
            &[(0.0, 0.5), (0.5, 1.0), (1.5, 0.5)]
        }
        BorderStyle::Triple => &[(0.0, 1.0), (2.0, 1.0), (4.0, 1.0)],
        BorderStyle::ThinThickSmallGap => &[(0.0, 0.5), (1.0, 1.0)],
        BorderStyle::ThickThinSmallGap => &[(0.0, 1.0), (1.5, 0.5)],
        BorderStyle::ThinThickMediumGap => &[(0.0, 0.5), (1.5, 1.0)],
        BorderStyle::ThickThinMediumGap => &[(0.0, 1.0), (2.0, 0.5)],
        BorderStyle::ThinThickLargeGap => &[(0.0, 0.5), (2.5, 1.0)],
        BorderStyle::ThickThinLargeGap => &[(0.0, 1.0), (3.0, 0.5)],
        BorderStyle::ThinThickThinSmallGap => &[(0.0, 0.5), (1.0, 1.0), (2.5, 0.5)],
        BorderStyle::ThinThickThinMediumGap => &[(0.0, 0.5), (1.5, 1.0), (3.5, 0.5)],
        BorderStyle::ThinThickThinLargeGap => &[(0.0, 0.5), (2.5, 1.0), (6.0, 0.5)],
        _ => &[(0.0, 1.0)],
    }
}

/// SVG dash array of a dashed style, in line widths.
fn dash_of(style: BorderStyle) -> Option<&'static [f64]> {
    Some(match style {
        BorderStyle::Dotted => &[1.0, 1.0],
        BorderStyle::Dashed => &[4.0, 3.0],
        BorderStyle::DashSmallGap => &[4.0, 1.0],
        BorderStyle::DotDash => &[4.0, 2.0, 1.0, 2.0],
        BorderStyle::DotDotDash => &[4.0, 2.0, 1.0, 2.0, 1.0, 2.0],
        BorderStyle::DashDotStroked => &[6.0, 2.0, 1.0, 2.0],
        _ => return None,
    })
}

fn shade_color(color: &str, light: bool) -> String {
    let hex = color.trim_start_matches('#');
    if hex.len() != 6 {
        return color.to_owned();
    }
    let mut result = String::from("#");
    for pair in hex.as_bytes().chunks_exact(2) {
        let value = std::str::from_utf8(pair)
            .ok()
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
            .unwrap_or(0);
        let value = if light {
            u16::midpoint(u16::from(value), 255)
        } else {
            u16::from(value) / 2
        };
        let _ = write!(result, "{value:02x}");
    }
    result
}

/// Which side of the box an edge is on; the band grows away from the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Above the text: the band grows upwards from `at`.
    Top,
    /// Below the text: the band grows downwards from `at`.
    Bottom,
    /// Left of the text: the band grows leftwards from `at`.
    Left,
    /// Right of the text: the band grows rightwards from `at`.
    Right,
}

/// Strokes of one edge.
///
/// `at` is the band's inner edge (the side nearer the text), `from`..`to` its
/// extent along the edge, `px_per_pt` the page scale.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn edge_items(
    border: &ResolvedBorder,
    side: Side,
    at: f64,
    from: f64,
    to: f64,
    px_per_pt: f64,
) -> Vec<Item> {
    let unit = (border.width_pt * px_per_pt).max(0.5);
    let band = border.band_pt() * px_per_pt;
    let mut out = Vec::new();
    if to <= from {
        return out;
    }
    let dash = dash_of(border.style);
    for (offset, width) in lines_of(border.style) {
        let stroke = (width * unit).max(0.5);
        // Offsets are from the outer edge of the band.
        let from_outer = if border.frame {
            band - offset * unit - stroke / 2.0
        } else {
            offset * unit + stroke / 2.0
        };
        let position = match side {
            Side::Top | Side::Left => at - band + from_outer,
            Side::Bottom | Side::Right => at + band - from_outer,
        };
        let horizontal = matches!(side, Side::Top | Side::Bottom);
        let color = if matches!(
            border.style,
            BorderStyle::ThreeDEmboss | BorderStyle::ThreeDEngrave
        ) {
            let light = matches!(side, Side::Top | Side::Left)
                ^ (border.style == BorderStyle::ThreeDEngrave)
                ^ border.frame;
            let light = if *offset < 0.5 {
                light
            } else if *offset >= 1.5 {
                !light
            } else {
                false
            };
            if (*offset - 0.5).abs() < 1e-9 {
                border.color.clone()
            } else {
                shade_color(&border.color, light)
            }
        } else {
            border.color.clone()
        };
        if matches!(border.style, BorderStyle::Wave | BorderStyle::DoubleWave) {
            let length = to - from;
            let amplitude = unit * 0.5;
            let steps = ((length / unit.max(0.5)) * 4.0).ceil().clamp(2.0, 16384.0) as usize;
            let mut d = String::new();
            for step in 0..=steps {
                let along = length * step as f64 / steps as f64;
                let across = amplitude
                    + (along / (unit * 4.0).max(1.0) * std::f64::consts::TAU).sin() * amplitude;
                let (x, y) = if horizontal {
                    (along, across)
                } else {
                    (across, along)
                };
                let _ = write!(
                    d,
                    "{}{} {} ",
                    if step == 0 { "M" } else { "L" },
                    crate::units::fmt_num(x),
                    crate::units::fmt_num(y)
                );
            }
            out.push(Item::Path(PathItem {
                x: if horizontal {
                    from
                } else {
                    position - amplitude
                },
                y: if horizontal {
                    position - amplitude
                } else {
                    from
                },
                w: if horizontal { length } else { amplitude * 2.0 },
                h: if horizontal { amplitude * 2.0 } else { length },
                d,
                fill: None,
                paint: None,
                stroke: Some(color),
                stroke_w: stroke,
                dash: None,
                rotate_deg: 0.0,
                flip_h: false,
                flip_v: false,
            }));
            continue;
        }
        match dash {
            None => out.push(Item::Line(if horizontal {
                LineItem {
                    x1: from,
                    y1: position,
                    x2: to,
                    y2: position,
                    color: color.clone(),
                    width: stroke,
                    dashed: false,
                }
            } else {
                LineItem {
                    x1: position,
                    y1: from,
                    x2: position,
                    y2: to,
                    color: color.clone(),
                    width: stroke,
                    dashed: false,
                }
            })),
            Some(pattern) => {
                let length = to - from;
                let (left, top, width, height, outline) = if horizontal {
                    (
                        from,
                        position - stroke / 2.0,
                        length,
                        stroke,
                        format!(
                            "M0 {} H{}",
                            crate::units::fmt_num(stroke / 2.0),
                            crate::units::fmt_num(length)
                        ),
                    )
                } else {
                    (
                        position - stroke / 2.0,
                        from,
                        stroke,
                        length,
                        format!(
                            "M{} 0 V{}",
                            crate::units::fmt_num(stroke / 2.0),
                            crate::units::fmt_num(length)
                        ),
                    )
                };
                out.push(Item::Path(PathItem {
                    x: left,
                    y: top,
                    w: width,
                    h: height,
                    d: outline,
                    fill: None,
                    paint: None,
                    stroke: Some(color.clone()),
                    stroke_w: stroke,
                    dash: Some(
                        pattern
                            .iter()
                            .map(|value| crate::units::fmt_num(value * stroke))
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    rotate_deg: 0.0,
                    flip_h: false,
                    flip_v: false,
                }));
            }
        }
    }
    if border.shadow {
        let mut shadows: Vec<_> = out
            .iter()
            .map(|item| super::table::offset_item(item, unit * 2.0, unit * 2.0))
            .collect();
        for item in &mut shadows {
            match item {
                Item::Line(line) => "#808080".clone_into(&mut line.color),
                Item::Path(path) => path.stroke = Some("#808080".to_owned()),
                _ => {}
            }
        }
        shadows.extend(out);
        shadows
    } else {
        out
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use strict_ooxml_wml::model::values::{Color, EighthsPoint};

    fn border(style: BorderStyle, eighths: u16) -> Border {
        Border {
            style: Some(style),
            size: Some(EighthsPoint(eighths)),
            color: Some(Color::new("FF0000")),
            space: Some(4),
            ..Border::default()
        }
    }

    #[test]
    fn nil_and_none_draw_nothing() {
        assert!(resolve_border(&border(BorderStyle::Nil, 8), None).is_none());
        assert!(resolve_border(&border(BorderStyle::None, 8), None).is_none());
    }

    #[test]
    fn a_double_border_is_two_lines_in_a_band_three_widths_wide() {
        let resolved = resolve_border(&border(BorderStyle::Double, 8), None).expect("double");
        assert_eq!(resolved.width_pt, 1.0);
        assert_eq!(resolved.band_pt(), 3.0);
        assert_eq!(resolved.color, "#ff0000");
        let items = edge_items(&resolved, Side::Top, 30.0, 0.0, 100.0, 1.0);
        let ys: Vec<f64> = items
            .iter()
            .map(|item| match item {
                Item::Line(line) => line.y1,
                _ => panic!("solid lines"),
            })
            .collect();
        assert_eq!(ys, [27.5, 29.5], "both inside the band above `at`");
    }

    #[test]
    fn every_style_fills_its_band_on_every_side() {
        use BorderStyle as S;
        for style in [
            S::Single,
            S::Thick,
            S::Double,
            S::DoubleWave,
            S::Wave,
            S::ThreeDEmboss,
            S::ThreeDEngrave,
            S::Triple,
            S::ThinThickSmallGap,
            S::ThickThinSmallGap,
            S::ThinThickMediumGap,
            S::ThickThinMediumGap,
            S::ThinThickLargeGap,
            S::ThickThinLargeGap,
            S::ThinThickThinSmallGap,
            S::ThinThickThinMediumGap,
            S::ThinThickThinLargeGap,
            S::Dotted,
            S::Dashed,
            S::DashSmallGap,
            S::DotDash,
            S::DotDotDash,
            S::DashDotStroked,
        ] {
            let mut resolved = resolve_border(&border(style, 16), None).expect("style");
            for frame in [false, true] {
                resolved.frame = frame;
                for side in [Side::Top, Side::Bottom, Side::Left, Side::Right] {
                    let items = edge_items(&resolved, side, 50.0, 10.0, 90.0, 1.0);
                    assert_eq!(
                        items.len(),
                        lines_of(style).len(),
                        "{style:?} {side:?} frame {frame}"
                    );
                    let band = resolved.band_pt();
                    for item in &items {
                        let across = match (item, side) {
                            (Item::Line(line), Side::Top | Side::Bottom) => line.y1,
                            (Item::Line(line), _) => line.x1,
                            (Item::Path(path), Side::Top | Side::Bottom) => path.y + path.h / 2.0,
                            (Item::Path(path), _) => path.x + path.w / 2.0,
                            other => panic!("{other:?}"),
                        };
                        let inside = match side {
                            Side::Top | Side::Left => (50.0 - band..=50.0).contains(&across),
                            Side::Bottom | Side::Right => (50.0..=50.0 + band).contains(&across),
                        };
                        assert!(inside, "{style:?} {side:?}: {across} outside the band");
                    }
                }
            }
        }
    }

    #[test]
    fn three_d_borders_shade_their_outer_lines_and_shadows_are_grey() {
        let mut resolved = resolve_border(&border(BorderStyle::ThreeDEmboss, 8), None).expect("3d");
        let colors = |items: &[Item]| -> Vec<String> {
            items
                .iter()
                .map(|item| match item {
                    Item::Line(line) => line.color.clone(),
                    other => panic!("{other:?}"),
                })
                .collect()
        };
        let top = colors(&edge_items(&resolved, Side::Top, 20.0, 0.0, 10.0, 1.0));
        assert_eq!(top, ["#ff7f7f", "#ff0000", "#7f0000"]);
        let bottom = colors(&edge_items(&resolved, Side::Bottom, 20.0, 0.0, 10.0, 1.0));
        assert_eq!(bottom, ["#7f0000", "#ff0000", "#ff7f7f"]);
        resolved.style = BorderStyle::ThreeDEngrave;
        let engraved = colors(&edge_items(&resolved, Side::Top, 20.0, 0.0, 10.0, 1.0));
        assert_eq!(engraved, ["#7f0000", "#ff0000", "#ff7f7f"]);
        assert_eq!(shade_color("red", true), "red");

        resolved.style = BorderStyle::Single;
        resolved.shadow = true;
        let shadowed = edge_items(&resolved, Side::Top, 20.0, 0.0, 10.0, 1.0);
        assert_eq!(colors(&shadowed), ["#808080", "#ff0000"]);
        resolved.style = BorderStyle::Dashed;
        let dashed = edge_items(&resolved, Side::Top, 20.0, 0.0, 10.0, 1.0);
        assert!(matches!(
            dashed.first(),
            Some(Item::Path(path)) if path.stroke.as_deref() == Some("#808080")
        ));
        // Nothing to draw along an empty extent.
        assert!(edge_items(&resolved, Side::Top, 20.0, 10.0, 10.0, 1.0).is_empty());
    }

    #[test]
    fn a_border_has_a_quarter_point_floor_and_defaults_to_black() {
        let hairline = Border {
            style: Some(BorderStyle::Single),
            size: Some(EighthsPoint(1)),
            ..Border::default()
        };
        let resolved = resolve_border(&hairline, None).expect("single");
        assert_eq!(resolved.width_pt, 0.25);
        assert_eq!(resolved.color, "#000000");
        assert_eq!(resolved.space_pt, 0.0);
        let no_size = Border {
            style: Some(BorderStyle::Single),
            ..Border::default()
        };
        assert_eq!(
            resolve_border(&no_size, None).expect("single").width_pt,
            0.5
        );
        assert!(resolve_border(&Border::default(), None).is_none());
    }

    #[test]
    fn a_dotted_border_is_a_dashed_path() {
        let resolved = resolve_border(&border(BorderStyle::Dotted, 8), None).expect("dotted");
        let items = edge_items(&resolved, Side::Left, 10.0, 0.0, 50.0, 1.0);
        assert!(matches!(items.as_slice(), [Item::Path(path)] if path.dash.is_some()));
    }
}
