//! Run decorations: shading, highlight, underline and strikethrough.
//!
//! The decorations are geometry, not text attributes, so the SVG and PDF
//! backends draw the same marks from the same values. SVG's `text-decoration`
//! knows one line style and takes its position from the viewer's font, and PDF
//! has no counterpart at all, which is how the PDF output lost every underline.
//!
//! Positions are fractions of the em, measured from the fragment's baseline.
//! Word reads them from the face's `post` and `OS/2` tables; the bundled faces
//! are metric twins of the faces Word uses, and their values sit within a
//! hundredth of an em of the ones below.

use std::fmt::Write as _;

use strict_ooxml_wml::model::values::Underline;

use super::{Item, LineItem, PathItem, RectItem, TextItem};
use crate::font::{BuiltinFontProvider, FontProvider};

/// Underline centre below the baseline, in em.
const UNDERLINE_OFFSET_EM: f64 = 0.11;
/// Strikethrough centre above the baseline, in em (about half the x-height).
const STRIKE_OFFSET_EM: f64 = 0.28;
/// Stroke width of a regular decoration line, in em.
const LINE_WIDTH_EM: f64 = 0.055;
/// Thinnest stroke drawn, in px, so a small run keeps a visible line.
const MIN_LINE_WIDTH_PX: f64 = 0.5;
/// Top of the highlight/shading band above the baseline, in em.
const BAND_TOP_EM: f64 = 0.9;
/// Height of the highlight/shading band, in em.
const BAND_HEIGHT_EM: f64 = 1.2;

/// The paint a text fragment needs besides its glyphs.
#[derive(Clone, Debug, Default)]
pub struct TextDecorations {
    /// Drawn before the glyphs: shading, then highlight.
    pub behind: Vec<Item>,
    /// Drawn after the glyphs: underline, then strikethrough.
    pub over: Vec<Item>,
}

/// Builds the decorations of one text fragment, in page px.
#[must_use]
pub fn text_decorations(item: &TextItem) -> TextDecorations {
    let mut decorations = TextDecorations::default();
    let run = &item.run;
    let size = item.size_px;
    if item.width <= 0.0 || size <= 0.0 {
        return decorations;
    }
    let band = |fill: &String| {
        Item::Rect(RectItem {
            x: item.x,
            y: item.baseline - size * BAND_TOP_EM,
            w: item.width,
            h: size * BAND_HEIGHT_EM,
            fill: Some(fill.clone()),
            stroke: None,
            stroke_w: 0.0,
        })
    };
    if let Some(shading) = &run.shading {
        decorations.behind.push(band(shading));
    }
    if let Some(highlight) = &run.highlight {
        decorations.behind.push(band(highlight));
    }

    let text_color = run.color.clone().unwrap_or_else(|| "#000000".to_owned());
    if let Some(border) = &run.border {
        use super::border::{edge_items, Side};
        let px_per_pt = size / run.size_pt.max(0.1);
        let space = border.space_pt * px_per_pt;
        let left = item.x - space;
        let right = item.x + item.width + space;
        let top = item.baseline - size * BAND_TOP_EM - space;
        let bottom = item.baseline + size * (BAND_HEIGHT_EM - BAND_TOP_EM) + space;
        for (side, at, from, to) in [
            (Side::Top, top, left, right),
            (Side::Bottom, bottom, left, right),
            (Side::Left, left, top, bottom),
            (Side::Right, right, top, bottom),
        ] {
            decorations.over.extend(edge_items(border, side, at, from, to, px_per_pt));
        }
    }
    let thickness = (size * LINE_WIDTH_EM).max(MIN_LINE_WIDTH_PX);
    if let Some(style) = run.underline {
        let width = if style == Underline::Words {
            item.width - trailing_space_width(item)
        } else {
            item.width
        };
        if width > 0.0 {
            let color = run
                .underline_color
                .clone()
                .unwrap_or_else(|| text_color.clone());
            let stroke = Stroke {
                x: item.x,
                width,
                color,
                thickness,
                size,
            };
            underline(
                &mut decorations.over,
                &stroke,
                item.baseline + size * UNDERLINE_OFFSET_EM,
                style,
            );
        }
    }
    let strike_y = item.baseline - size * STRIKE_OFFSET_EM;
    let strike = Stroke {
        x: item.x,
        width: item.width,
        color: text_color,
        thickness,
        size,
    };
    if run.double_strike {
        let gap = thickness * 1.2;
        strike.solid(&mut decorations.over, strike_y - gap, thickness * 0.8);
        strike.solid(&mut decorations.over, strike_y + gap, thickness * 0.8);
    } else if run.strike {
        strike.solid(&mut decorations.over, strike_y, thickness);
    }
    decorations
}

/// One horizontal decoration stroke across a fragment.
struct Stroke {
    x: f64,
    width: f64,
    color: String,
    thickness: f64,
    size: f64,
}

impl Stroke {
    fn solid(&self, out: &mut Vec<Item>, y: f64, width: f64) {
        out.push(Item::Line(LineItem {
            x1: self.x,
            y1: y,
            x2: self.x + self.width,
            y2: y,
            color: self.color.clone(),
            width,
            dashed: false,
        }));
    }

    /// A straight stroke with an SVG dash array, drawn as a path.
    fn dashed(&self, out: &mut Vec<Item>, y: f64, width: f64, dash: &[f64]) {
        let half = width.max(0.1);
        out.push(Item::Path(PathItem {
            x: self.x,
            y: y - half,
            w: self.width,
            h: half * 2.0,
            d: format!(
                "M0 {} H{}",
                crate::units::fmt_num(half),
                crate::units::fmt_num(self.width)
            ),
            fill: None,
            paint: None,
            stroke: Some(self.color.clone()),
            stroke_w: width,
            dash: Some(
                dash.iter()
                    .map(|value| crate::units::fmt_num(value.max(0.1)))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            rotate_deg: 0.0,
            flip_h: false,
            flip_v: false,
        }));
    }

    /// A wavy stroke centred on `y`, built from cubic arcs both backends draw.
    fn wave(&self, out: &mut Vec<Item>, y: f64, width: f64) {
        let amplitude = (self.size * 0.04).max(width);
        let period = (self.size * 0.2).max(4.0 * width);
        let half = period / 2.0;
        let mut d = format!("M0 {}", crate::units::fmt_num(amplitude));
        let mut x = 0.0;
        let mut up = true;
        while x < self.width - 1e-6 {
            let next = (x + half).min(self.width);
            let span = next - x;
            let peak = if up { 0.0 } else { amplitude * 2.0 };
            let _ = write!(
                d,
                " C{} {} {} {} {} {}",
                crate::units::fmt_num(x + span / 3.0),
                crate::units::fmt_num(peak),
                crate::units::fmt_num(x + span * 2.0 / 3.0),
                crate::units::fmt_num(peak),
                crate::units::fmt_num(next),
                crate::units::fmt_num(amplitude),
            );
            x = next;
            up = !up;
        }
        out.push(Item::Path(PathItem {
            x: self.x,
            y: y - amplitude,
            w: self.width,
            h: amplitude * 2.0,
            d,
            fill: None,
            paint: None,
            stroke: Some(self.color.clone()),
            stroke_w: width,
            dash: None,
            rotate_deg: 0.0,
            flip_h: false,
            flip_v: false,
        }));
    }
}

/// Appends the strokes of one `w:u` style.
fn underline(out: &mut Vec<Item>, stroke: &Stroke, y: f64, style: Underline) {
    let t = stroke.thickness;
    let s = stroke.size;
    let heavy = t * 2.0;
    let dot = |width: f64| [width, width * 1.5];
    let dash = [s * 0.25, s * 0.12];
    let long = [s * 0.5, s * 0.15];
    let dot_dash = |width: f64| [s * 0.25, s * 0.1, width, s * 0.1];
    let dot_dot_dash = |width: f64| [s * 0.25, s * 0.1, width, s * 0.1, width, s * 0.1];
    match style {
        Underline::None => {}
        Underline::Single | Underline::Words => stroke.solid(out, y, t),
        Underline::Thick => stroke.solid(out, y, heavy),
        Underline::Double => {
            stroke.solid(out, y - t, t * 0.8);
            stroke.solid(out, y + t * 1.4, t * 0.8);
        }
        Underline::Dotted => stroke.dashed(out, y, t, &dot(t)),
        Underline::DottedHeavy => stroke.dashed(out, y, heavy, &dot(heavy)),
        Underline::Dash => stroke.dashed(out, y, t, &dash),
        Underline::DashedHeavy => stroke.dashed(out, y, heavy, &dash),
        Underline::DashLong => stroke.dashed(out, y, t, &long),
        Underline::DashLongHeavy => stroke.dashed(out, y, heavy, &long),
        Underline::DotDash => stroke.dashed(out, y, t, &dot_dash(t)),
        Underline::DashDotHeavy => stroke.dashed(out, y, heavy, &dot_dash(heavy)),
        Underline::DotDotDash => stroke.dashed(out, y, t, &dot_dot_dash(t)),
        Underline::DashDotDotHeavy => stroke.dashed(out, y, heavy, &dot_dot_dash(heavy)),
        Underline::Wave => stroke.wave(out, y, t),
        Underline::WavyHeavy => stroke.wave(out, y, heavy),
        Underline::WavyDouble => {
            stroke.wave(out, y - t, t * 0.8);
            stroke.wave(out, y + t * 2.0, t * 0.8);
        }
    }
}

/// Advance of the fragment's trailing whitespace, in px.
///
/// `w:u w:val="words"` leaves spaces bare, and a wrap token carries its
/// trailing space.
fn trailing_space_width(item: &TextItem) -> f64 {
    thread_local! {
        static FONTS: BuiltinFontProvider = BuiltinFontProvider::new();
    }
    let family = crate::style::chosen_family(&item.run, &item.text);
    FONTS
        .with(|fonts| {
            item.text
                .chars()
                .rev()
                .take_while(|ch| ch.is_whitespace())
                .map(|ch| {
                    fonts.advance_em(&family, ch, item.run.bold, item.run.italic) * item.size_px
                })
                .sum::<f64>()
        })
        .min(item.width)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::layout::TextAdvanceKind;
    use crate::style::ComputedRun;

    fn item(run: ComputedRun) -> TextItem {
        TextItem {
            x: 10.0,
            baseline: 50.0,
            width: 40.0,
            text: "word ".to_owned(),
            run,
            size_px: 20.0,
            advance: TextAdvanceKind::Shaped,
            field: None,
            compress_punctuation: false,
            following_non_space: None,
            plain_space_factor: 1.0,
        }
    }

    #[test]
    fn plain_text_has_no_decorations() {
        let decorations = text_decorations(&item(ComputedRun::default()));
        assert!(decorations.behind.is_empty());
        assert!(decorations.over.is_empty());
    }

    #[test]
    fn double_underline_is_two_lines_in_the_underline_colour() {
        let decorations = text_decorations(&item(ComputedRun {
            underline: Some(Underline::Double),
            underline_color: Some("#ff0000".to_owned()),
            color: Some("#0000ff".to_owned()),
            ..ComputedRun::default()
        }));
        let lines: Vec<_> = decorations
            .over
            .iter()
            .filter_map(|item| match item {
                Item::Line(line) => Some(line),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.color == "#ff0000"));
        assert!(
            lines.iter().all(|line| line.y1 > 50.0),
            "below the baseline"
        );
        assert_ne!(lines[0].y1, lines[1].y1);
    }

    #[test]
    fn words_underline_skips_the_trailing_space() {
        let decorations = text_decorations(&item(ComputedRun {
            underline: Some(Underline::Words),
            ..ComputedRun::default()
        }));
        let Some(Item::Line(line)) = decorations.over.first() else {
            panic!("expected a line");
        };
        assert!(line.x2 < 50.0, "trailing space left bare: {}", line.x2);
    }

    #[test]
    fn styled_underlines_are_dashed_or_wavy_paths() {
        for style in [Underline::Dotted, Underline::DashLong, Underline::Wave] {
            let decorations = text_decorations(&item(ComputedRun {
                underline: Some(style),
                ..ComputedRun::default()
            }));
            assert!(
                matches!(decorations.over.as_slice(), [Item::Path(_)]),
                "{style:?}"
            );
        }
    }

    #[test]
    fn double_strike_and_shading_behind_highlight() {
        let decorations = text_decorations(&item(ComputedRun {
            double_strike: true,
            shading: Some("#cccccc".to_owned()),
            highlight: Some("#ffff00".to_owned()),
            ..ComputedRun::default()
        }));
        assert_eq!(decorations.over.len(), 2);
        assert!(decorations
            .over
            .iter()
            .all(|item| matches!(item, Item::Line(line) if line.y1 < 50.0)));
        let fills: Vec<_> = decorations
            .behind
            .iter()
            .filter_map(|item| match item {
                Item::Rect(rect) => rect.fill.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(fills, ["#cccccc", "#ffff00"]);
    }
}
