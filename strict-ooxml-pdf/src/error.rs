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
        }
    }
}

impl PdfLimits {
    /// Returns the error for an exceeded budget.
    #[must_use]
    pub fn exceeded(&self, kind: LimitKind, actual: u64) -> PdfError {
        let limit = match kind {
            LimitKind::Pages => self.max_pages,
            LimitKind::ContentBytes => self.max_content_bytes,
            LimitKind::Operations => self.max_operations,
            LimitKind::Glyphs => self.max_glyphs,
            LimitKind::FontGlyphs => self.max_font_glyphs,
            LimitKind::ImageBytes => self.max_image_bytes,
            LimitKind::PathPoints => self.max_path_points,
            LimitKind::Fonts => self.max_fonts,
        };
        PdfError::LimitExceeded {
            kind,
            limit: limit as u64,
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
