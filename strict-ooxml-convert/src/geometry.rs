//! Page geometry, the part of the conversion the PDF states rather than infers.
//!
//! A PDF's page box is an exact number, so a converted page needs no guessing:
//! what *is* inferred is the margin, because a PDF states where the text is but
//! not how far from the edge the author would have put it. The margin is derived
//! from the ink bounds with a floor, and the rule is here so it can be reviewed.

use strict_ooxml_pdf::PdfPage;
use strict_ooxml_wml::model::props::{PageMargins, PageSize, SectionProperties};
use strict_ooxml_wml::model::values::Twips;

use crate::to_twips;

/// The smallest margin a converted page gets, in points.
///
/// A document whose text starts 2 pt from the edge is a document the producer
/// drew full-bleed; reproducing that as a margin would make the converted page
/// unprintable, so the margin has a floor.
pub(crate) const MIN_MARGIN_PT: f64 = 18.0;

/// The margin used when a page carries no text or no images at all.
pub(crate) const DEFAULT_MARGIN_PT: f64 = 72.0;

/// The ink bounds of a page, in points from the top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InkBounds {
    /// Left edge, points from the left.
    pub left: f64,
    /// Top edge, points from the top.
    pub top: f64,
    /// Right edge.
    pub right: f64,
    /// Bottom edge.
    pub bottom: f64,
}

impl InkBounds {
    /// Bounds with nothing in them.
    pub(crate) const EMPTY: Self = Self {
        left: f64::INFINITY,
        top: f64::INFINITY,
        right: f64::NEG_INFINITY,
        bottom: f64::NEG_INFINITY,
    };

    /// Returns the bounds of everything the page placed.
    #[must_use]
    pub(crate) fn of(page: &PdfPage) -> Self {
        let mut bounds = Self::EMPTY;
        for item in page.items() {
            match item {
                strict_ooxml_pdf::Item::Glyph(glyph) => {
                    bounds.include(glyph.x, glyph.y - glyph.ascent);
                    bounds.include(glyph.x + glyph.width, glyph.y);
                }
                strict_ooxml_pdf::Item::Image(image) => {
                    bounds.include(image.x, image.y);
                    bounds.include(image.x + image.width, image.y + image.height);
                }
                strict_ooxml_pdf::Item::Vector(vector) => {
                    for subpath in &vector.subpaths {
                        for (x, y) in &subpath.points {
                            let (x, y) = vector.ctm.apply(*x, *y);
                            bounds.include(x, y);
                        }
                    }
                }
            }
        }
        bounds
    }

    /// Grows the bounds to include a point.
    pub(crate) fn include(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        self.left = self.left.min(x);
        self.top = self.top.min(y);
        self.right = self.right.max(x);
        self.bottom = self.bottom.max(y);
    }

    /// Whether anything was included.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    /// The margins that would frame this ink, as a section's page margins.
    #[must_use]
    pub(crate) fn margins(&self, page: &PdfPage) -> PageMargins {
        if self.is_empty() {
            let default = to_twips(DEFAULT_MARGIN_PT);
            return PageMargins {
                top: Some(Twips(default)),
                right: Some(Twips(default)),
                bottom: Some(Twips(default)),
                left: Some(Twips(default)),
                header: None,
                footer: None,
                gutter: None,
            };
        }
        // The top margin is measured to the top of the *ascender*, not to the
        // baseline, and the bottom to the bottom of the last line, so a
        // converted page does not start its text in the margin.
        PageMargins {
            top: Some(Twips(to_twips(self.top.floor().max(MIN_MARGIN_PT)))),
            left: Some(Twips(to_twips(self.left.floor().max(MIN_MARGIN_PT)))),
            right: Some(Twips(to_twips(
                (page.geometry.width - self.right).ceil().max(MIN_MARGIN_PT),
            ))),
            bottom: Some(Twips(to_twips(
                (page.geometry.height - self.bottom)
                    .ceil()
                    .max(MIN_MARGIN_PT),
            ))),
            header: None,
            footer: None,
            gutter: None,
        }
    }
}

/// The section properties of one converted page.
///
/// The page size is the PDF's own box, in twips, so a converted page is the
/// size it was. The margins are inferred from the ink and are therefore a
/// judgement, which is why the caller is told.
#[must_use]
pub(crate) fn section_for(page: &PdfPage, visual: bool) -> SectionProperties {
    let bounds = InkBounds::of(page);
    let width = to_twips(page.geometry.width.round());
    let height = to_twips(page.geometry.height.round());
    let margins = if visual {
        // The visual mode reproduces positions, so the page keeps the ink where
        // it was and the margins are the *author's* frame, not the ink's.
        let default = to_twips(DEFAULT_MARGIN_PT);
        PageMargins {
            top: Some(Twips(default)),
            right: Some(Twips(default)),
            bottom: Some(Twips(default)),
            left: Some(Twips(default)),
            header: None,
            footer: None,
            gutter: None,
        }
    } else {
        bounds.margins(page)
    };
    SectionProperties {
        page_size: Some(PageSize {
            width: Some(Twips(width)),
            height: Some(Twips(height)),
            orientation: None,
            code: None,
        }),
        page_margins: Some(margins),
        ..SectionProperties::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{InkBounds, DEFAULT_MARGIN_PT, MIN_MARGIN_PT};
    use crate::to_twips;
    use strict_ooxml_pdf::{content::Item, Glyph, PdfPage, RenderMode, Rgb};

    fn page_with(glyphs: Vec<Item>, width: f64, height: f64) -> PdfPage {
        PdfPage {
            number: 1,
            geometry: strict_ooxml_pdf::PageGeometry {
                width,
                height,
                rotation: 0,
            },
            content: strict_ooxml_pdf::Content {
                items: glyphs,
                ..Default::default()
            },
            report: strict_ooxml_pdf::ReadReport::default(),
        }
    }

    fn glyph(x: f64, baseline: f64, width: f64) -> Item {
        Item::Glyph(Glyph {
            text: "x".to_owned(),
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
    fn the_margins_frame_the_ink() {
        let page = page_with(
            vec![glyph(72.0, 100.0, 40.0), glyph(72.0, 700.0, 40.0)],
            595.0,
            842.0,
        );
        let margins = InkBounds::of(&page).margins(&page);
        // The left margin is the ink's left edge, the top is the ascender.
        assert_eq!(margins.left.map(|m| m.0), Some(1440), "72 pt is 1440 twips");
        assert_eq!(margins.top.map(|m| m.0), Some(1840), "100 - 8 = 92 pt");
    }

    #[test]
    fn an_empty_page_gets_the_default_margins() {
        let page = page_with(Vec::new(), 595.0, 842.0);
        let margins = InkBounds::of(&page).margins(&page);
        assert_eq!(margins.left.map(|m| m.0), Some(1440));
        assert_eq!(margins.left.unwrap().0, to_twips(DEFAULT_MARGIN_PT));
    }

    #[test]
    fn ink_over_the_edge_gets_the_margin_floor() {
        // The ink starts 1 pt from the left and rises above the top of the
        // page, so the left and top margins are clamped to the floor.
        let page = page_with(vec![glyph(1.0, 3.0, 590.0)], 595.0, 842.0);
        let margins = InkBounds::of(&page).margins(&page);
        let floor = to_twips(MIN_MARGIN_PT);
        assert_eq!(margins.left.map(|m| m.0), Some(floor));
        assert_eq!(margins.top.map(|m| m.0), Some(floor));
        assert_eq!(margins.right.map(|m| m.0), Some(floor));
        // The bottom margin is not clamped: the ink really does stop 3 pt from
        // the top of an 842 pt page, so the margin that frames it is the rest of
        // the page. Clamping *that* to 18 pt would be wrong in the other
        // direction - it would claim the text continues below the page.
        assert_eq!(
            margins.bottom.map(|m| m.0),
            Some(to_twips(842.0 - 3.0)),
            "the bottom margin is the space the ink left, not the floor"
        );
    }

    #[test]
    fn a_non_finite_point_does_not_poison_the_bounds() {
        let mut bounds = InkBounds::EMPTY;
        bounds.include(f64::NAN, 10.0);
        bounds.include(10.0, f64::INFINITY);
        assert!(bounds.is_empty());
    }
}
