//! Deterministic SVG rendering for WordprocessingML Strict documents
//! (`TZ` §15 Stage 4, `STAGE-4-TASK.md`).
//!
//! The crate turns a parsed [`strict_ooxml_wml::model::Document`] into
//! one SVG document per page:
//!
//! 1. [`style`] computes effective paragraph/run properties from the style
//!    cascade;
//! 2. [`font`] supplies deterministic glyph metrics (no system fonts);
//! 3. `layout` flows paragraphs/tables and paginates by section geometry;
//! 4. `paint` writes the SVG with stable number formatting.
//!
//! Rendering never touches system fonts or the locale, so the same input yields
//! the same bytes (ADR-0006).
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml_core::opc::{OpenOptions, Package};
//! use strict_ooxml_wml::{parse_document, ParseOptions};
//! use strict_ooxml_render_svg::{render_with_media, RenderOptions};
//!
//! let package = Package::open_path("document.docx", &OpenOptions::default())?;
//! let document = parse_document(&package, &ParseOptions::default())?;
//! let pages = render_with_media(&document, &RenderOptions::default(), Some(&package))?;
//! println!("{} page(s)", pages.len());
//! # Ok::<(), strict_ooxml_core::error::StrictError>(())
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
// The computed-run model is intentionally flag-heavy; layout does pixel math
// from integer twips/EMU (the precision loss is inherent and bounded).
#![allow(
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

pub mod error;
pub mod font;
pub mod style;
pub mod units;

mod layout;
mod paint;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::Package;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;

pub use error::RenderError;
pub use font::{FontMetrics, FontProvider, FontProviderKind};
pub use paint::image::media_file_name;

/// Supplies image bytes for referenced media parts.
pub trait MediaSource {
    /// Reads a media part fully.
    ///
    /// # Errors
    ///
    /// Returns a [`strict_ooxml_core::error::StrictError`] if the part is missing
    /// or cannot be decoded.
    fn read_media(&self, part: &PartId) -> Result<Vec<u8>>;
}

impl MediaSource for Package {
    fn read_media(&self, part: &PartId) -> Result<Vec<u8>> {
        self.read_part(part)
    }
}

/// How referenced images are emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MediaMode {
    /// Inline the bytes as a `data:` URI (self-contained SVG; default).
    #[default]
    EmbedDataUri,
    /// Reference each image by its file name (the caller writes the files).
    ExternalFiles,
    /// Do not emit images; draw a placeholder rectangle.
    None,
}

/// Which pages to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PageSelection {
    /// All pages.
    #[default]
    All,
    /// A 1-based inclusive range.
    Range {
        /// First page (1-based).
        start: usize,
        /// Last page (1-based, inclusive).
        end: usize,
    },
}

impl PageSelection {
    /// Returns `true` if the 1-based `page` is selected.
    #[must_use]
    pub fn contains(self, page: usize) -> bool {
        match self {
            Self::All => true,
            Self::Range { start, end } => page >= start && page <= end,
        }
    }
}

/// Options controlling rendering.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Output scale: px per inch (default `96.0` = 96 DPI).
    pub scale: f64,
    /// Media emission mode.
    pub media: MediaMode,
    /// Font provider kind.
    pub font_provider: FontProviderKind,
    /// Page selection.
    pub pages: PageSelection,
    /// Draw a white page background.
    pub background: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            scale: 96.0,
            media: MediaMode::EmbedDataUri,
            font_provider: FontProviderKind::Builtin,
            pages: PageSelection::All,
            background: true,
        }
    }
}

impl RenderOptions {
    /// Sets the scale (px per inch).
    #[must_use]
    pub fn scale(mut self, scale: f64) -> Self {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
        self
    }

    /// Sets the media mode.
    #[must_use]
    pub fn media(mut self, media: MediaMode) -> Self {
        self.media = media;
        self
    }

    /// Sets the page selection.
    #[must_use]
    pub fn pages(mut self, pages: PageSelection) -> Self {
        self.pages = pages;
        self
    }
}

/// One rendered page.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// Zero-based index of the page in the document.
    pub index: usize,
    /// Page width in px.
    pub width_px: f64,
    /// Page height in px.
    pub height_px: f64,
    /// The SVG document.
    pub svg: String,
}

/// Renders a document without a media source.
///
/// Embedded images resolve to placeholders because the bytes are not available;
/// use [`render_with_media`] to inline them.
///
/// # Errors
///
/// Returns a [`RenderError`]-backed `StrictError` if output limits are exceeded.
pub fn render(document: &Document, options: &RenderOptions) -> Result<Vec<Page>> {
    render_with_media(document, options, None)
}

/// Renders a document, optionally resolving media through `media`.
///
/// # Errors
///
/// Returns a [`RenderError`]-backed `StrictError` if output limits are exceeded.
pub fn render_with_media(
    document: &Document,
    options: &RenderOptions,
    media: Option<&dyn MediaSource>,
) -> Result<Vec<Page>> {
    let provider = options.font_provider.make();
    let context = layout::LayoutContext {
        document,
        options,
        font: provider.as_ref(),
        media,
        media_mode: options.media,
    };
    let laid_out = layout::paginate::layout_document(&context)?;

    let mut pages = Vec::new();
    for (index, page) in laid_out.pages.iter().enumerate() {
        if !options.pages.contains(index + 1) {
            continue;
        }
        pages.push(Page {
            index,
            width_px: page.width_px,
            height_px: page.height_px,
            svg: paint::render_page(page, options.background),
        });
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::{PageSelection, RenderOptions};

    #[test]
    fn page_selection_contains() {
        assert!(PageSelection::All.contains(1));
        assert!(PageSelection::All.contains(999));
        let range = PageSelection::Range { start: 2, end: 3 };
        assert!(!range.contains(1));
        assert!(range.contains(2));
        assert!(range.contains(3));
        assert!(!range.contains(4));
    }

    #[test]
    fn options_builders_and_defaults() {
        let options = RenderOptions::default()
            .scale(72.0)
            .pages(PageSelection::Range { start: 1, end: 1 });
        assert!((options.scale - 72.0).abs() < 1e-9);
        assert_eq!(options.pages, PageSelection::Range { start: 1, end: 1 });
        // Invalid scales are ignored.
        assert!((RenderOptions::default().scale(f64::NAN).scale - 96.0).abs() < 1e-9);
        let _ = format!("{:?}", options.media);
    }
}
