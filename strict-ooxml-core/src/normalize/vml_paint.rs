//! The fill and the outline of a converted VML shape (T7).
//!
//! A `v:rect` converted to `wps:wsp` with nothing but a `prstGeom` is an
//! invisible box: DrawingML draws no fill and no line unless they are written,
//! while VML draws both unless told not to. `filled` and `stroked` default to
//! true, `fillcolor` to white, `strokecolor` to black and `strokeweight` to
//! 0.75pt (VML §14.1.2.19), so a shape that names none of them is a white box
//! with a thin black border, and that is what is written.
//!
//! What this cannot read it leaves at the default and says nothing about: a
//! colour expression such as `fill darken(118)` is a colour, only not one this
//! module evaluates, and the default is a colour too.

use std::collections::BTreeMap;

use quick_xml::events::{BytesEnd, BytesStart, Event};

use super::tables::drawingml_thousandths_percent;

/// What a VML shape paints, with VML's defaults applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlPaint {
    /// The fill, or `None` for `filled="f"`.
    pub fill: Option<VmlFill>,
    /// The outline, or `None` for `stroked="f"`.
    pub stroke: Option<VmlStroke>,
}

impl Default for VmlPaint {
    fn default() -> Self {
        Self {
            fill: Some(VmlFill {
                rgb: "FFFFFF".to_owned(),
                alpha: None,
            }),
            stroke: Some(VmlStroke {
                rgb: "000000".to_owned(),
                width_emu: DEFAULT_WEIGHT_EMU,
                dash: None,
            }),
        }
    }
}

/// A solid fill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlFill {
    /// Six hex digits.
    pub rgb: String,
    /// Opacity in thousandths of a percent, when it is not opaque.
    pub alpha: Option<i64>,
}

/// A solid outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlStroke {
    /// Six hex digits.
    pub rgb: String,
    /// Width in EMU.
    pub width_emu: i64,
    /// `a:prstDash` preset, when the line is not solid.
    pub dash: Option<&'static str>,
}

/// 0.75pt.
const DEFAULT_WEIGHT_EMU: i64 = 9525;

/// The paint of a shape from its attributes and its `v:fill` / `v:stroke`
/// children, all keyed by local name.
#[must_use]
pub fn paint(
    shape: &BTreeMap<String, String>,
    fill: Option<&BTreeMap<String, String>>,
    stroke: Option<&BTreeMap<String, String>>,
) -> VmlPaint {
    let mut out = VmlPaint::default();
    let filled = flag(shape.get("filled"))
        .or_else(|| flag(fill.and_then(|fill| fill.get("on"))))
        .unwrap_or(true);
    out.fill = filled.then(|| {
        let color = fill
            .and_then(|fill| fill.get("color"))
            .or_else(|| shape.get("fillcolor"))
            .and_then(|value| rgb(value))
            .unwrap_or_else(|| "FFFFFF".to_owned());
        VmlFill {
            rgb: color,
            alpha: fill
                .and_then(|fill| fill.get("opacity"))
                .and_then(|value| opacity(value)),
        }
    });
    let stroked = flag(shape.get("stroked"))
        .or_else(|| flag(stroke.and_then(|stroke| stroke.get("on"))))
        .unwrap_or(true);
    out.stroke = stroked.then(|| VmlStroke {
        rgb: stroke
            .and_then(|stroke| stroke.get("color"))
            .or_else(|| shape.get("strokecolor"))
            .and_then(|value| rgb(value))
            .unwrap_or_else(|| "000000".to_owned()),
        width_emu: stroke
            .and_then(|stroke| stroke.get("weight"))
            .or_else(|| shape.get("strokeweight"))
            .and_then(|value| weight_emu(value))
            .unwrap_or(DEFAULT_WEIGHT_EMU),
        dash: stroke
            .and_then(|stroke| stroke.get("dashstyle"))
            .and_then(|value| dash(value)),
    });
    out
}

/// `spPr` children after the geometry: the fill (none for a line, which has no
/// inside) and `a:ln`.
#[must_use]
pub fn events(paint: &VmlPaint, line: bool) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    if !line {
        match &paint.fill {
            Some(fill) => {
                out.push(Event::Start(BytesStart::new("a:solidFill")));
                color_events(&mut out, &fill.rgb, fill.alpha);
                out.push(Event::End(BytesEnd::new("a:solidFill")));
            }
            None => out.push(Event::Empty(BytesStart::new("a:noFill"))),
        }
    }
    let width = paint
        .stroke
        .as_ref()
        .map_or(DEFAULT_WEIGHT_EMU, |stroke| stroke.width_emu)
        .to_string();
    let mut ln = BytesStart::new("a:ln");
    ln.push_attribute(crate::xml::escape::attribute("w", &width));
    out.push(Event::Start(ln));
    match &paint.stroke {
        Some(stroke) => {
            out.push(Event::Start(BytesStart::new("a:solidFill")));
            color_events(&mut out, &stroke.rgb, None);
            out.push(Event::End(BytesEnd::new("a:solidFill")));
            if let Some(dash) = stroke.dash {
                let mut preset = BytesStart::new("a:prstDash");
                preset.push_attribute(crate::xml::escape::attribute("val", dash));
                out.push(Event::Empty(preset));
            }
        }
        None => out.push(Event::Empty(BytesStart::new("a:noFill"))),
    }
    out.push(Event::End(BytesEnd::new("a:ln")));
    out
}

fn color_events(out: &mut Vec<Event<'static>>, rgb: &str, alpha: Option<i64>) {
    let mut color = BytesStart::new("a:srgbClr");
    color.push_attribute(crate::xml::escape::attribute("val", rgb));
    match alpha.and_then(|alpha| drawingml_thousandths_percent(&alpha.to_string())) {
        Some(alpha) => {
            out.push(Event::Start(color));
            let mut value = BytesStart::new("a:alpha");
            value.push_attribute(crate::xml::escape::attribute("val", &alpha));
            out.push(Event::Empty(value));
            out.push(Event::End(BytesEnd::new("a:srgbClr")));
        }
        None => out.push(Event::Empty(color)),
    }
}

/// `t`/`true`/`on` or `f`/`false`/`off`; `None` for anything else or absent.
fn flag(value: Option<&String>) -> Option<bool> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "t" | "true" | "on" | "1" => Some(true),
        "f" | "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Six hex digits for a VML colour: `#rgb`, `#rrggbb`, a CSS colour name, or a
/// Word system colour. Word appends the palette index (`#e36c0a [2409]`), which
/// is dropped.
#[must_use]
pub fn rgb(value: &str) -> Option<String> {
    let value = value.split_whitespace().next()?.to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        if !hex.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return None;
        }
        return match hex.len() {
            6 => Some(hex.to_ascii_uppercase()),
            3 => Some(
                hex.chars()
                    .flat_map(|digit| [digit, digit])
                    .collect::<String>()
                    .to_ascii_uppercase(),
            ),
            _ => None,
        };
    }
    let named = match value.as_str() {
        "black" | "windowtext" | "windowframe" | "buttontext" => "000000",
        "white" | "window" | "buttonhighlight" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "lime" => "00FF00",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "aqua" | "cyan" => "00FFFF",
        "fuchsia" | "magenta" => "FF00FF",
        "gray" | "grey" | "graytext" | "buttonshadow" => "808080",
        "silver" | "buttonface" => "C0C0C0",
        "maroon" => "800000",
        "olive" => "808000",
        "navy" => "000080",
        "purple" => "800080",
        "teal" => "008080",
        _ => return None,
    };
    Some(named.to_owned())
}

/// `v:fill/@opacity` as thousandths of a percent: `.5`, `50%` or the 16.16
/// fixed form `32768f`. Fully opaque is `None`.
fn opacity(value: &str) -> Option<i64> {
    let value = value.trim();
    let fraction = if let Some(fixed) = value.strip_suffix('f') {
        fixed.trim().parse::<f64>().ok()? / 65536.0
    } else if let Some(percent) = value.strip_suffix('%') {
        percent.trim().parse::<f64>().ok()? / 100.0
    } else {
        value.parse::<f64>().ok()?
    };
    if !(0.0..1.0).contains(&fraction) {
        return None;
    }
    // In range by the check above, so the conversion is exact enough and bounded.
    #[allow(clippy::cast_possible_truncation)]
    let thousandths = (fraction * 100_000.0).round() as i64;
    Some(thousandths)
}

/// `strokeweight` in EMU: a length with a unit, a bare number being points.
fn weight_emu(value: &str) -> Option<i64> {
    let value = value.trim();
    let split = value
        .find(|character: char| !character.is_ascii_digit() && !matches!(character, '.' | '-'))
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let points = match unit.trim() {
        "pt" | "" => number,
        "px" => number * 0.75,
        "in" => number * 72.0,
        "cm" => number * 72.0 / 2.54,
        "mm" => number * 7.2 / 2.54,
        _ => return None,
    };
    if !(0.0..=1000.0).contains(&points) {
        return None;
    }
    // Bounded by the check above: at most 12 700 000 EMU.
    #[allow(clippy::cast_possible_truncation)]
    let emu = (points * 12_700.0).round() as i64;
    Some(emu)
}

/// `v:stroke/@dashstyle` as an `a:prstDash` preset; `None` for a solid line.
fn dash(value: &str) -> Option<&'static str> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "shortdash" => "sysDash",
        "shortdot" => "sysDot",
        "shortdashdot" => "sysDashDot",
        "shortdashdotdot" => "sysDashDotDot",
        "dot" => "dot",
        "dash" => "dash",
        "longdash" => "lgDash",
        "dashdot" => "dashDot",
        "longdashdot" => "lgDashDot",
        "longdashdotdot" => "lgDashDotDot",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{paint, rgb, VmlPaint};

    fn attributes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn colours_read_every_spelling_word_writes() {
        assert_eq!(rgb("#e36c0a [2409]").as_deref(), Some("E36C0A"));
        assert_eq!(rgb("#fc0").as_deref(), Some("FFCC00"));
        assert_eq!(rgb("windowText").as_deref(), Some("000000"));
        assert_eq!(rgb("fill darken(118)"), None);
    }

    #[test]
    fn a_shape_that_names_nothing_is_a_white_box_with_a_black_line() {
        assert_eq!(paint(&BTreeMap::new(), None, None), VmlPaint::default());
    }

    #[test]
    fn filled_and_stroked_off_and_the_children_win() {
        let shape = attributes(&[("filled", "f"), ("strokecolor", "red")]);
        let stroke = attributes(&[("weight", "2pt"), ("dashstyle", "dash")]);
        let painted = paint(&shape, None, Some(&stroke));
        assert_eq!(painted.fill, None);
        let line = painted.stroke.expect("stroked");
        assert_eq!(line.rgb, "FF0000");
        assert_eq!(line.width_emu, 25_400);
        assert_eq!(line.dash, Some("dash"));
        let fill = attributes(&[("opacity", ".5")]);
        let shape = attributes(&[("fillcolor", "#d99594 [1941]"), ("stroked", "f")]);
        let painted = paint(&shape, Some(&fill), None);
        let inside = painted.fill.expect("filled");
        assert_eq!(inside.rgb, "D99594");
        assert_eq!(inside.alpha, Some(50_000));
        assert_eq!(painted.stroke, None);
    }
}
