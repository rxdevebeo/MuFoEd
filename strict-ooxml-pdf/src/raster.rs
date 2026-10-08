//! Page and region rasterization (`STAGE-8-TASK.md` §2, §6 O3).
//!
//! Everything else in this crate answers questions about a page's *ink*; this
//! module answers the one question ink cannot: what the page looks like. Two
//! callers need different things of it.
//!
//! - **A page with no text layer** (C7) is text-less because it is a scan. A
//!   vision model can read a picture of it; it cannot read the eleven glyphs a
//!   naive extractor invents for it. So the page is rasterized and handed over
//!   as an image (`O11`, `TextRecovery`).
//! - **A graphic region** is rasterized on its own, at a scale chosen for the
//!   region rather than the page (`O1`, `FigureClassifier`).
//!
//! # The budget, and why it counts pixels
//!
//! Every other budget in [`PdfLimits`] counts something a producer wrote. This
//! one counts something a *caller* asked for: a Letter page at scale 4 is 7.7 M
//! pixels, at scale 8 it is 31 M, and 31 M pixels of RGBA is 124 MiB before a
//! single glyph has been drawn. The check happens before the rasterizer is asked
//! for anything — the same rule as everywhere else in this crate, because a
//! budget checked afterwards has already been spent.
//!
//! # The rasterizer is somebody else's
//!
//! [`hayro`] is a software rasterizer — no GPU, no native library, MIT/Apache-2.0
//! — and this module is deliberately thin: it decides the scale, checks the
//! budget, crops and encodes, and every question about *what a page looks like*
//! is answered by hayro. The feature is off by default, because the dependency is
//! 75 crates and a caller who wants glyph geometry does not need a rasterizer.
//!
//! Pictures are composited onto white, always. A page has no transparency worth
//! preserving, and anything downstream that treats an alpha channel as black —
//! half of every image pipeline — turns a transparent page into a black
//! rectangle.
//!
//! ```no_run
//! # use strict_ooxml_pdf::raster::{RasterOptions, Rasterizer};
//! # fn main() -> Result<(), strict_ooxml_pdf::PdfError> {
//! let bytes = std::fs::read("scan.pdf").expect("readable");
//! let mut rasterizer = Rasterizer::new(&bytes, strict_ooxml_pdf::PdfLimits::default())?;
//! let png = rasterizer.page_png(1, &RasterOptions::default())?;
//! std::fs::write("page1.png", png).expect("writable");
//! # Ok(())
//! # }
//! ```

// The hostile-input limits this module relies on (`tests/hostile.rs` `mod raster`)
// live in the vendored `hayro` (`vendor/README.md`), wired by `[patch.crates-io]`,
// which cargo ignores for dependents of a published crate. Referencing the patch
// marker makes such a build fail here, loudly, instead of running unguarded.
const _: u32 = hayro::PATCHED_LIMITS;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::Pixmap;
use hayro::{render, RenderCache, RenderSettings};

use crate::error::{LimitKind, PdfError, PdfLimits, Result};

/// The largest scale a caller may ask for.
///
/// At 64 a Letter page is 39 000 × 50 000 pixels, which the pixel budget
/// refuses anyway; the point of the bound is to refuse a *nonsense* scale with a
/// message about the scale rather than about pixels.
const MAX_SCALE: f64 = 64.0;

/// How a rasterized picture is produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterOptions {
    /// Pixels per point. 1.0 is 72 dpi; 2.0 is what a vision model wants to read.
    pub scale: f64,
    /// The largest picture, in pixels, one call may produce.
    ///
    /// Default 16 Mi (4096²), which is a Letter page at scale 4. It is a
    /// per-call bound on purpose: the limit belongs to the *request*, and a
    /// document-wide setting would make a single caller unable to ask for a
    /// thumbnail because another caller wanted a proof.
    pub max_pixels: u64,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            scale: 2.0,
            max_pixels: 4096 * 4096,
        }
    }
}

/// A rectangle of a page, in points from its **top-left** corner.
///
/// The same convention as every other coordinate in this crate (see
/// [`crate::content::Glyph`]), because a region that means one thing to the
/// reader and another to a caller is a bug with a long fuse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    /// Left edge, points from the left.
    pub x: f64,
    /// Top edge, points from the top.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

impl Region {
    /// A page as one region, in points.
    #[must_use]
    pub fn page(width: f64, height: f64) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }
}

/// A size in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// A page rasterizer, holding the document it was built from.
///
/// Built once and reused for every page: hayro parses the file on construction,
/// and a fifty-page scan would otherwise be parsed fifty times.
pub struct Rasterizer {
    pdf: Pdf,
    limits: PdfLimits,
}

impl Rasterizer {
    /// Builds a rasterizer for a PDF.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::Malformed`] when the rasterizer cannot load a file
    /// this reader could, and [`PdfError::Encrypted`] for one behind a password:
    /// hayro does not decrypt either, and a page of noise is not an answer.
    pub fn new(bytes: &[u8], limits: PdfLimits) -> Result<Self> {
        let pdf = Pdf::new(bytes.to_vec()).map_err(|error| {
            PdfError::Malformed(format!("the rasterizer rejected it: {error:?}"))
        })?;
        Ok(Self { pdf, limits })
    }

    /// How many pages the rasterizer sees.
    ///
    /// It can disagree with [`crate::PdfDocument::page_count`], and when it does
    /// the rasterizer is the one to believe: it is the one that has to find a
    /// page by number afterwards, and a page the caller cannot rasterize is not a
    /// page the caller can use.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// The size page `number` rasterizes to at `options.scale`, in pixels.
    ///
    /// Computed the way the renderer computes it — crop box, rotation and all —
    /// so the budget is checked against the picture that would actually be
    /// allocated rather than against a guess about the page's paper size.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::Missing`] for a page this document does not have, and
    /// [`PdfError::Malformed`] for a scale that is not one.
    pub fn page_size(&self, number: usize, options: &RasterOptions) -> Result<Size> {
        let page = self.page(number)?;
        let scale = scale_of(options)?;
        let (width, height) = page.render_dimensions();
        Ok(Size {
            width: pixels(width, scale),
            height: pixels(height, scale),
        })
    }

    /// Rasterizes one page, 1-based, and encodes it as PNG.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::Missing`] for a page this document does not have,
    /// [`PdfError::LimitExceeded`] when the picture would be over budget, and
    /// [`PdfError::Malformed`] when the encoder fails.
    pub fn page_png(&self, number: usize, options: &RasterOptions) -> Result<Vec<u8>> {
        let pixmap = self.pixmap(number, options)?;
        encode(
            pixmap.data_as_u8_slice(),
            Size {
                width: u32::from(pixmap.width()),
                height: u32::from(pixmap.height()),
            },
        )
    }

    /// Rasterizes a region of one page, 1-based, and encodes it as PNG.
    ///
    /// The page is rendered whole and the region is cut out of it. A rasterizer
    /// that cannot be told «start here» gives no better answer than this one does,
    /// and a crop of a correctly rendered page is exactly the picture a
    /// classifier wants — at the region's own scale, not the page's.
    ///
    /// # Errors
    ///
    /// As [`Rasterizer::page_png`], plus [`PdfError::Malformed`] for a region with
    /// no area, which is what a zero-sized box on a scanned page produces.
    pub fn region_png(
        &self,
        number: usize,
        region: Region,
        options: &RasterOptions,
    ) -> Result<Vec<u8>> {
        if !region.width.is_finite()
            || region.width <= 0.0
            || !region.height.is_finite()
            || region.height <= 0.0
        {
            return Err(PdfError::Malformed(format!(
                "a region of {}x{} points has no area",
                region.width, region.height
            )));
        }
        let scale = scale_of(options)?;
        let pixmap = self.pixmap(number, options)?;
        let (page_width, page_height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        let left = clamp_pixel(region.x * scale, page_width);
        let top = clamp_pixel(region.y * scale, page_height);
        let right = clamp_pixel((region.x + region.width) * scale, page_width);
        let bottom = clamp_pixel((region.y + region.height) * scale, page_height);
        let width = right.saturating_sub(left).max(1).min(page_width);
        let height = bottom.saturating_sub(top).max(1).min(page_height);
        let source = pixmap.data_as_u8_slice();
        let stride = page_width as usize * 4;
        let mut out = Vec::with_capacity(width as usize * height as usize * 4);
        for row in top..top + height {
            let start = row as usize * stride + left as usize * 4;
            let end = (start + width as usize * 4).min(source.len());
            let start = start.min(source.len());
            out.extend_from_slice(&source[start..end]);
        }
        encode(&out, Size { width, height })
    }

    /// Renders a page, refusing before the work if the picture is over budget.
    fn pixmap(&self, number: usize, options: &RasterOptions) -> Result<Pixmap> {
        let page = self.page(number)?;
        let scale = scale_of(options)?;
        let (width, height) = page.render_dimensions();
        // `checked_mul` before any buffer is asked for (AUD-13): a nonsense
        // scale must not wrap the pixel count and allocate under the ceiling.
        let requested =
            u64::from(pixels(width, scale)).saturating_mul(u64::from(pixels(height, scale)));
        let ceiling = options.max_pixels.min(self.limits.max_raster_pixels);
        if requested > ceiling {
            return Err(self.limits.exceeded(LimitKind::RasterPixels, requested));
        }
        let cache = RenderCache::new();
        let scale = scale as f32;
        Ok(render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
                ..RenderSettings::default()
            },
        ))
    }

    fn page(&self, number: usize) -> Result<&hayro::hayro_syntax::page::Page<'_>> {
        if number == 0 {
            return Err(PdfError::Missing("page 0".to_owned()));
        }
        self.pdf
            .pages()
            .get(number - 1)
            .ok_or_else(|| PdfError::Missing(format!("page {number}")))
    }
}

/// The scale as a `f64`, refusing the values that are not a scale.
fn scale_of(options: &RasterOptions) -> Result<f64> {
    let scale = options.scale;
    if !(scale.is_finite() && scale > 0.0 && scale <= MAX_SCALE) {
        return Err(PdfError::Malformed(format!(
            "a scale of {scale} is not one: it must be finite, positive and at most {MAX_SCALE}"
        )));
    }
    Ok(scale)
}

/// Points at a scale, in whole pixels, rounded the way the renderer rounds.
fn pixels(points: f32, scale: f64) -> u32 {
    let value = f64::from(points) * scale;
    if !value.is_finite() || value <= 0.0 {
        return 1;
    }
    value.floor().clamp(1.0, u32::MAX as f64) as u32
}

fn clamp_pixel(value: f64, limit: u32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.round().clamp(0.0, f64::from(limit)) as u32
}

/// Encodes RGBA as a PNG.
fn encode(rgba: &[u8], size: Size) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size.width, size.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| PdfError::Malformed(error.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| PdfError::Malformed(error.to_string()))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{clamp_pixel, pixels, scale_of, RasterOptions};
    use crate::error::PdfError;

    #[test]
    fn a_scale_is_a_scale() {
        assert!(scale_of(&RasterOptions {
            scale: 1.0,
            ..RasterOptions::default()
        })
        .is_ok());
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, 65.0] {
            assert!(
                matches!(
                    scale_of(&RasterOptions {
                        scale,
                        ..RasterOptions::default()
                    }),
                    Err(PdfError::Malformed(_))
                ),
                "{scale} must be refused"
            );
        }
    }

    #[test]
    fn a_picture_is_never_zero_pixels() {
        // A zero-area page still has to produce something, and `hayro` falls back
        // to A4 for those; the budget must not be computed as 0 x 0 and let an
        // enormous scale through.
        assert_eq!(pixels(0.0, 4.0), 1);
        assert_eq!(pixels(-10.0, 4.0), 1);
        assert_eq!(pixels(f32::NAN, 4.0), 1);
        assert_eq!(pixels(612.0, 2.0), 1224);
    }

    #[test]
    fn a_crop_is_inside_the_picture() {
        assert_eq!(clamp_pixel(-5.0, 100), 0);
        assert_eq!(clamp_pixel(f64::NAN, 100), 0);
        assert_eq!(clamp_pixel(20.4, 100), 20);
        assert_eq!(clamp_pixel(500.0, 100), 100);
    }
}
