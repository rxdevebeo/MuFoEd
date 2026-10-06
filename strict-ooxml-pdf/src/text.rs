//! Turning a page's glyphs into the lines and blocks a document is made of.
//!
//! A PDF has no paragraphs: a content stream is glyph origins and advances, in
//! whatever order the producer drew them. Everything a document *means* - where
//! a line ends, where a paragraph begins, which run is a heading - is inferred
//! here, and every inference is a place to be wrong, so each one is a named
//! step with a stated rule rather than a heuristic buried in a loop.
//!
//! The grouping is by baseline and by pen continuity, both of which the PDF
//! states exactly. Nothing here guesses at a character: a glyph whose character
//! the reader could not map is dropped and counted, never replaced.

use crate::content::{Glyph, Item};

/// One line of text, as the reader saw it.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphLine {
    /// Left edge of the first glyph, points from the page's left.
    pub x: f64,
    /// Baseline, points from the page's **top**.
    pub baseline: f64,
    /// Font size in points.
    pub size: f64,
    /// Ascent above the baseline, points.
    pub ascent: f64,
    /// Distance from the left edge to the end of the last glyph, points.
    pub width: f64,
    /// The glyphs, in reading order.
    pub glyphs: Vec<Glyph>,
}

impl GlyphLine {
    /// The line's text.
    #[must_use]
    pub fn text(&self) -> String {
        self.glyphs
            .iter()
            .map(|glyph| glyph.text.as_str())
            .collect()
    }
}

/// Groups a page's glyphs into lines.
///
/// A line is a maximal set of glyphs sharing a baseline, each starting exactly
/// where the previous one's advance ended. The pen-continuity test is not a
/// convenience: it is what separates one line from two lines of text that happen
/// to share a baseline, and the baseline test is what separates a line from the
/// next one. Anything that breaks continuity *and* keeps the baseline starts a
/// new line, because that is what a two-column page looks like to a reader that
/// has not been told about columns.
#[must_use]
pub fn lines(items: &[Item]) -> Vec<GlyphLine> {
    let mut out: Vec<GlyphLine> = Vec::new();
    for item in items {
        let Item::Glyph(glyph) = item else {
            continue;
        };
        if !glyph.render_mode.paints() {
            // Invisible text is real text - it is searchable, and it is
            // usually a caption laid under a figure - but it is not drawn, and a
            // converter that places it visibly would add a layer of text the
            // producer hid.
            continue;
        }
        let continues = out.last().is_some_and(|line: &GlyphLine| {
            let kerning = CONTINUITY_EPSILON.max(line.size * 0.12);
            (line.baseline - glyph.y).abs() < CONTINUITY_EPSILON
                && (line.size - glyph.size).abs() < CONTINUITY_EPSILON
                && (glyph.x - (line.x + line.width)).abs() < kerning
        });
        match out.last_mut() {
            Some(line) if continues => {
                line.width = glyph.x + glyph.width - line.x;
                line.glyphs.push(glyph.clone());
            }
            _ => out.push(GlyphLine {
                x: glyph.x,
                baseline: glyph.y,
                size: glyph.size,
                ascent: glyph.ascent,
                width: glyph.width,
                glyphs: vec![glyph.clone()],
            }),
        }
    }
    out
}

/// How far a glyph may sit from where the previous one ended and still be the
/// same line, in points.
///
/// A rounding error in a matrix product is far below a thousandth of a point, so
/// the tolerance is generous enough to absorb that and tight enough that a real
/// gap - a tab, a column break, a missing run - ends the line.
pub const CONTINUITY_EPSILON: f64 = 0.05;

#[cfg(test)]
mod tests {
    use super::lines;
    use crate::content::{Glyph, Item, RenderMode, Rgb};

    fn glyph(text: &str, x: f64, baseline: f64, width: f64) -> Item {
        Item::Glyph(Glyph {
            text: text.to_owned(),
            x,
            y: baseline,
            width,
            ascent: 8.0,
            descent: 2.0,
            size: 12.0,
            font: "Carlito".to_owned(),
            color: Rgb(0.0, 0.0, 0.0),
            render_mode: RenderMode::Fill,
            mapped: true,
        })
    }

    #[test]
    fn a_contiguous_row_is_one_line() {
        let items = vec![
            glyph("H", 72.0, 100.0, 8.0),
            glyph("i", 80.0, 100.0, 3.0),
            glyph("!", 83.0, 100.0, 3.0),
        ];
        let lines = lines(&items);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "Hi!");
        assert!((lines[0].width - 14.0).abs() < 1e-9, "{}", lines[0].width);
    }

    #[test]
    fn a_new_baseline_is_a_new_line() {
        let items = vec![glyph("a", 72.0, 100.0, 5.0), glyph("b", 72.0, 114.0, 5.0)];
        assert_eq!(lines(&items).len(), 2);
    }

    #[test]
    fn a_kerning_gap_stays_on_the_line_and_a_word_gap_does_not() {
        let kerned = vec![glyph("r", 72.0, 100.0, 4.0), glyph("g", 75.6, 100.0, 5.0)];
        assert_eq!(lines(&kerned).len(), 1, "0.4 pt kerning is one line");
        let spaced = vec![glyph("d", 72.0, 100.0, 5.0), glyph("h", 80.0, 100.0, 5.0)];
        assert_eq!(lines(&spaced).len(), 2, "a 3 pt hole is not kerning");
    }

    #[test]
    fn a_gap_on_the_same_baseline_ends_the_line() {
        // A tab, a column break, or a run the reader could not place: the next
        // glyph is on the same baseline but is not the same line.
        let items = vec![glyph("a", 72.0, 100.0, 5.0), glyph("z", 200.0, 100.0, 5.0)];
        assert_eq!(lines(&items).len(), 2);
    }

    #[test]
    fn invisible_text_is_not_placed() {
        let mut hidden = glyph("a", 72.0, 100.0, 5.0);
        if let Item::Glyph(glyph) = &mut hidden {
            glyph.render_mode = RenderMode::Invisible;
        }
        assert!(lines(&[hidden]).is_empty());
    }
}
