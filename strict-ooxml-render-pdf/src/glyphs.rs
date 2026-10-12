//! Shaped glyphs drawn as outlines over an invisible text layer (WQ-044).
//!
//! Arabic shaping places a letter's dots and the vowel marks as glyphs of
//! their own, above or below the baseline. Written as text, every one of them
//! is a character to a reader: a dot without a `ToUnicode` entry comes out as
//! its raw code, and a raised mark starts a line of its own, so pdfium broke
//! every word apart. Such a run is drawn twice instead: the glyphs as outlines
//! (one form XObject per glyph, reused), and the text as one invisible `TJ` on
//! the baseline with one glyph per cluster carrying the cluster's text.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::GlyphId;
use skrifa::{FontRef, MetadataProvider};
use strict_ooxml_render_svg::font::{ShapedGlyph, ShapedText};

use crate::font::{EmbeddedFont, FaceKey};

/// Whether `shaped` has glyphs a text layer cannot carry one per character:
/// a cluster of several glyphs, or a glyph moved off the baseline.
#[must_use]
pub(crate) fn needs_outlines(shaped: &ShapedText) -> bool {
    shaped
        .clusters
        .iter()
        .any(|cluster| cluster.glyph_count > 1)
        || shaped
            .glyphs
            .iter()
            .any(|glyph| glyph.y_offset_em.abs() > 1e-9)
}

/// The glyph of a cluster that carries the cluster's text: on the baseline if
/// any is, and the widest of those (the letter's body, not its dots).
///
/// The font collector labels this glyph and the painter looks it up by the same
/// rule, so the two always agree on which `(glyph, text)` pair has a CID.
#[must_use]
pub(crate) fn holder(glyphs: &[ShapedGlyph]) -> usize {
    let mut best = 0;
    let mut best_key = (false, f64::NEG_INFINITY);
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.glyph_id == 0 {
            continue;
        }
        let key = (glyph.y_offset_em.abs() <= 1e-9, glyph.x_advance_em);
        if (key.0 && !best_key.0) || (key.0 == best_key.0 && key.1 > best_key.1) {
            best = index;
            best_key = key;
        }
    }
    best
}

/// One glyph outline, written as a form XObject in font units.
pub(crate) struct GlyphForm {
    /// Resource name the content streams use.
    pub(crate) name: String,
    /// The path, filled with the non-zero rule.
    pub(crate) content: Vec<u8>,
    /// Outline bounds in font units.
    pub(crate) bbox: [f32; 4],
    /// Font units per em, for the form matrix.
    pub(crate) units_per_em: f32,
}

/// The outlines the document draws, in first-use order.
#[derive(Default)]
pub(crate) struct GlyphForms {
    forms: Vec<GlyphForm>,
    index: BTreeMap<(FaceKey, u16), Option<usize>>,
}

impl GlyphForms {
    /// The resource name for CID `cid` of `font`; `None` for a glyph without
    /// an outline (a space), which draws nothing.
    pub(crate) fn name(&mut self, font: &EmbeddedFont, cid: u16) -> Option<&str> {
        let key = (font.key.clone(), cid);
        let slot = match self.index.get(&key) {
            Some(slot) => *slot,
            None => {
                let slot = outline(font, cid).map(|(content, bbox, units_per_em)| {
                    let index = self.forms.len();
                    self.forms.push(GlyphForm {
                        name: format!("G{}", index + 1),
                        content,
                        bbox,
                        units_per_em,
                    });
                    index
                });
                self.index.insert(key, slot);
                slot
            }
        };
        slot.and_then(|index| self.forms.get(index))
            .map(|form| form.name.as_str())
    }

    /// Every form, in the order the names were handed out.
    pub(crate) fn forms(&self) -> &[GlyphForm] {
        &self.forms
    }
}

/// The outline of CID `cid` in the subset program, as PDF path operators.
fn outline(font: &EmbeddedFont, cid: u16) -> Option<(Vec<u8>, [f32; 4], f32)> {
    let gid = *font.cid_to_gid.get(usize::from(cid))?;
    let face = FontRef::new(&font.data).ok()?;
    let units_per_em = f32::from(
        face.metrics(Size::unscaled(), LocationRef::default())
            .units_per_em,
    );
    if units_per_em <= 0.0 {
        return None;
    }
    let glyph = face.outline_glyphs().get(GlyphId::new(u32::from(gid)))?;
    let mut pen = FormPen::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        )
        .ok()?;
    if pen.out.is_empty() {
        return None;
    }
    pen.out.push_str("f\n");
    Some((pen.out.into_bytes(), pen.bounds(), units_per_em))
}

/// Collects an outline as `m`/`l`/`c`/`h` operators and tracks its bounds.
#[derive(Default)]
struct FormPen {
    out: String,
    current: (f32, f32),
    bounds: Option<[f32; 4]>,
}

impl FormPen {
    fn touch(&mut self, x: f32, y: f32) {
        self.bounds = Some(match self.bounds {
            None => [x, y, x, y],
            Some([min_x, min_y, max_x, max_y]) => {
                [min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y)]
            }
        });
    }

    fn bounds(&self) -> [f32; 4] {
        self.bounds.unwrap_or_default()
    }
}

// The pen's parameter names are the trait's own.
#[allow(clippy::similar_names)]
impl OutlinePen for FormPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.touch(x, y);
        let _ = writeln!(self.out, "{x} {y} m");
        self.current = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.touch(x, y);
        let _ = writeln!(self.out, "{x} {y} l");
        self.current = (x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        // A quadratic segment as the cubic with the same curve.
        let (x0, y0) = self.current;
        let c1 = (x0 + 2.0 / 3.0 * (cx0 - x0), y0 + 2.0 / 3.0 * (cy0 - y0));
        let c2 = (x + 2.0 / 3.0 * (cx0 - x), y + 2.0 / 3.0 * (cy0 - y));
        self.curve_to(c1.0, c1.1, c2.0, c2.1, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.touch(cx0, cy0);
        self.touch(cx1, cy1);
        self.touch(x, y);
        let _ = writeln!(self.out, "{cx0} {cy0} {cx1} {cy1} {x} {y} c");
        self.current = (x, y);
    }

    fn close(&mut self) {
        self.out.push_str("h\n");
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{holder, FormPen};
    use skrifa::outline::OutlinePen;
    use strict_ooxml_render_svg::font::ShapedGlyph;

    fn glyph(glyph_id: u16, advance: f64, rise: f64) -> ShapedGlyph {
        ShapedGlyph {
            glyph_id,
            byte_start: 0,
            x_advance_em: advance,
            x_offset_em: 0.0,
            y_offset_em: rise,
            x_em: 0.0,
        }
    }

    /// The dots come first in visual order; the body on the baseline holds the text.
    #[test]
    fn the_baseline_body_holds_the_cluster() {
        assert_eq!(holder(&[glyph(7, 0.0, 0.3), glyph(9, 0.42, 0.0)]), 1);
        assert_eq!(holder(&[glyph(9, 0.42, 0.0), glyph(7, 0.0, -0.2)]), 0);
        // A lone raised mark still holds its own cluster.
        assert_eq!(holder(&[glyph(5, 0.0, 0.4)]), 0);
        // A missing glyph never holds the text.
        assert_eq!(holder(&[glyph(0, 0.5, 0.0), glyph(4, 0.2, 0.0)]), 1);
    }

    #[test]
    fn a_quadratic_becomes_the_matching_cubic() {
        let mut pen = FormPen::default();
        pen.move_to(0.0, 0.0);
        pen.quad_to(3.0, 3.0, 6.0, 0.0);
        pen.close();
        assert_eq!(pen.out, "0 0 m\n2 2 4 2 6 0 c\nh\n");
        assert_eq!(pen.bounds(), [0.0, 0.0, 6.0, 2.0]);
    }
}
