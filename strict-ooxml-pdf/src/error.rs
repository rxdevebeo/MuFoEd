//! The reader's error model and its resource budget.
//!
//! Reading a PDF is reading a program written by somebody else: a content
//! stream is a bytecode sequence, a font dictionary is a set of overlapping
//! fallbacks, and both can be made to allocate without bound. Every failure is
//! an error and never a panic, and every loop is bounded by [`PdfLimits`].

use std::fmt;

/// How a read failed.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum PdfError {
    /// The bytes are not a PDF this reader could load at all.
    Malformed(String),
    /// The file is encrypted. Decryption is out of scope (`STAGE-8-TASK.md` §9);
    /// a document behind a password is a recorded refusal, not a wrong answer.
    Encrypted,
    /// A resource limit was exceeded before the corresponding work was done.
    LimitExceeded {
        /// Which budget.
        kind: LimitKind,
        /// The configured bound.
        limit: u64,
        /// The observed value.
        actual: u64,
    },
    /// A structure this reader needs is missing (no page tree, no `MediaBox`).
    Missing(String),
    /// Something was refused, and the reason is a whole sentence.
    ///
    /// Not [`PdfError::Missing`]: the structure is *there*, and saying it is
    /// missing sends whoever reads the report to look for a file that lost a part.
    /// An image whose `/Length` promises more than the stream holds is a damaged
    /// file, and this variant does not dress that sentence in a word — the first
    /// version read «missing image dictionary is missing truncated samples».
    Refused(String),
}

impl PdfError {
    /// Wraps a `lopdf` failure.
    #[must_use]
    pub fn from_lopdf(error: &lopdf::Error) -> Self {
        Self::Malformed(error.to_string())
    }
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(f, "malformed PDF: {detail}"),
            Self::Encrypted => f.write_str("the PDF is encrypted; decryption is out of scope"),
            Self::LimitExceeded {
                kind,
                limit,
                actual,
            } => {
                write!(f, "limit exceeded ({kind}): {actual} > {limit}")
            }
            Self::Missing(what) => write!(f, "missing {what}"),
            Self::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for PdfError {}

/// Which budget a [`PdfError::LimitExceeded`] refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LimitKind {
    /// `PdfLimits::max_pages`.
    Pages,
    /// `PdfLimits::max_content_bytes`, the decompressed content of one page.
    ContentBytes,
    /// `PdfLimits::max_operations`, operators in one page.
    Operations,
    /// `PdfLimits::max_glyphs`, glyphs in one page.
    Glyphs,
    /// `PdfLimits::max_font_glyphs`, glyphs recorded for one font.
    FontGlyphs,
    /// `PdfLimits::max_image_bytes`, one decoded image.
    ImageBytes,
    /// `PdfLimits::max_path_points`, points in one flattened path.
    PathPoints,
    /// `PdfLimits::max_fonts`, fonts one page may reference.
    Fonts,
    /// `PdfLimits::max_form_depth`, how deep form XObjects may nest.
    ///
    /// A form is content that draws content, and a producer's form that draws
    /// itself is a loop rather than a document. The bound is on **nesting**, and
    /// `max_operations` covers the breadth: a form drawn a thousand times inside
    /// twelve levels is a million operations, and the page is refused for that
    /// rather than for being deep.
    FormDepth,
    /// `PdfLimits::max_raster_pixels`, pixels in one rasterized page or region.
    ///
    /// Only reachable with the `raster` feature, and it is a *pixel* budget
    /// rather than a byte one: a page at scale 8 is 4 000 × 6 000 pixels before
    /// anybody asks what that costs in memory.
    RasterPixels,
}

impl LimitKind {
    /// A stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pages => "pages",
            Self::ContentBytes => "content_bytes",
            Self::Operations => "operations",
            Self::Glyphs => "glyphs",
            Self::FontGlyphs => "font_glyphs",
            Self::ImageBytes => "image_bytes",
            Self::PathPoints => "path_points",
            Self::Fonts => "fonts",
            Self::FormDepth => "form_depth",
            Self::RasterPixels => "raster_pixels",
        }
    }
}

impl fmt::Display for LimitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The reader's budget, checked *before* the corresponding work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfLimits {
    /// Pages in one document. Default: 2048.
    pub max_pages: usize,
    /// Decompressed content-stream bytes per page. Default: 32 MiB.
    pub max_content_bytes: usize,
    /// Operators in one page. Default: 2 000 000.
    pub max_operations: usize,
    /// Glyphs in one page. Default: 500 000.
    pub max_glyphs: usize,
    /// Distinct codes recorded for one font. Default: 65 536.
    pub max_font_glyphs: usize,
    /// Decoded bytes of one image. Default: 64 MiB.
    pub max_image_bytes: usize,
    /// Points in one flattened path. Default: 200 000.
    pub max_path_points: usize,
    /// Fonts one page may reference. Default: 512.
    pub max_fonts: usize,
    /// How deep form XObjects may nest. Default: 12.
    ///
    /// Deep enough for the real cases (a page of a catalogue is usually one form
    /// deep, a drawing assembled from parts is two or three) and shallow enough
    /// that a form which draws itself is refused instead of recursing.
    pub max_form_depth: usize,
    /// Bytes of decoded pictures the document holds on to between pages. Default:
    /// 64 MiB.
    ///
    /// A document that draws one picture on every page decodes it once instead of
    /// once per draw, and this cache is what makes that true — but the cache
    /// outlives the page, so a caller that reads one page at a time and drops it
    /// would otherwise keep every picture the document has. Past this ceiling a
    /// picture is decoded per draw again, which is what always happened; nothing
    /// is evicted, because a policy that guessed wrong would cost more than it
    /// saves.
    pub max_cached_image_bytes: usize,
    /// Pixels in one rasterized page or region. Default: 16 777 216 (4096²).
    ///
    /// A Letter page at scale 4 is 2448 × 3168 pixels — 7.7 M — so the default
    /// leaves room for a page at scale 4 and refuses scale 8, which is 31 M pixels
    /// and 124 MiB of RGBA before a single glyph is drawn. Checked *before* the
    /// rasterizer is asked for anything.
    pub max_raster_pixels: u64,
}

impl Default for PdfLimits {
    fn default() -> Self {
        Self {
            max_pages: 2048,
            max_content_bytes: 32 * 1024 * 1024,
            max_operations: 2_000_000,
            max_glyphs: 500_000,
            max_font_glyphs: 65_536,
            max_image_bytes: 64 * 1024 * 1024,
            max_path_points: 200_000,
            max_fonts: 512,
            max_form_depth: 12,
            max_cached_image_bytes: 64 * 1024 * 1024,
            max_raster_pixels: 4096 * 4096,
        }
    }
}

impl PdfLimits {
    /// Returns the error for an exceeded budget.
    ///
    /// The limit is widened to `u64` here rather than in the caller, so a budget
    /// that does not fit a `usize` on this platform is still reported as the
    /// number the caller set.
    #[must_use]
    pub fn exceeded(&self, kind: LimitKind, actual: u64) -> PdfError {
        let limit = match kind {
            LimitKind::Pages => self.max_pages as u64,
            LimitKind::ContentBytes => self.max_content_bytes as u64,
            LimitKind::Operations => self.max_operations as u64,
            LimitKind::Glyphs => self.max_glyphs as u64,
            LimitKind::FontGlyphs => self.max_font_glyphs as u64,
            LimitKind::ImageBytes => self.max_image_bytes as u64,
            LimitKind::PathPoints => self.max_path_points as u64,
            LimitKind::Fonts => self.max_fonts as u64,
            LimitKind::FormDepth => self.max_form_depth as u64,
            LimitKind::RasterPixels => self.max_raster_pixels,
        };
        PdfError::LimitExceeded {
            kind,
            limit,
            actual,
        }
    }
}

/// The reader's result alias.
pub type Result<T> = std::result::Result<T, PdfError>;

#[cfg(test)]
mod tests {
    use super::{LimitKind, PdfError, PdfLimits};

    #[test]
    fn defaults_are_bounded() {
        let limits = PdfLimits::default();
        assert_eq!(limits.max_pages, 2048);
        assert_eq!(limits.max_content_bytes, 32 * 1024 * 1024);
        assert!(limits.max_glyphs > 0);
        assert!(limits.max_fonts > 0);
    }

    #[test]
    fn the_exceeded_error_names_its_budget() {
        let error = PdfLimits::default().exceeded(LimitKind::Glyphs, 10);
        let text = error.to_string();
        assert!(text.contains("glyphs"), "{text}");
        assert!(text.contains("10 > 500000"), "{text}");
    }

    #[test]
    fn encryption_is_its_own_refusal() {
        let text = PdfError::Encrypted.to_string();
        assert!(text.contains("encrypted"), "{text}");
        assert!(text.contains("out of scope"), "{text}");
    }
}
