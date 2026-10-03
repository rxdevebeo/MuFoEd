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
pub mod layout;
pub mod style;
pub mod units;

pub mod fields;
mod math;
mod notes;
mod numbering;
mod paint;

pub use layout::{ImageItem, Item, LineItem, PathItem, PlacedPage, RectItem, TextItem};
use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::Package;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;

pub use error::RenderError;
pub use font::{FontMetrics, FontProvider, FontProviderKind};
pub use math::{math_expression_to_mathml, math_paragraph_to_mathml, MathMlError};
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

/// Which tracked-change view the renderer shows (ADR-0018).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RevisionView {
    /// Final document: hide `w:del` / `w:moveFrom`, keep `w:ins` / `w:moveTo`.
    #[default]
    Final,
    /// Original document: hide `w:ins` / `w:moveTo`, keep `w:del` / `w:moveFrom`.
    Original,
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
    /// Render floating (anchored) DrawingML objects.
    pub floating: bool,
    /// Render OMML formulas (`STAGE-5C-TASK.md` §6).
    ///
    /// The default reproduces Word/WPS behaviour: formulas are laid out and
    /// drawn. Setting it to `false` skips them (the parser still models them,
    /// so the Feature Report is unchanged).
    pub math: bool,
    /// Tracked-change view (ADR-0018). Default [`RevisionView::Final`].
    pub revisions: RevisionView,
    /// Resource limits, including the block-nesting bound.
    ///
    /// The renderer takes its budget from the same `ResourceLimits` the reader
    /// used, so a document that parsed cannot fail to render on depth alone - and
    /// a `Document` built programmatically, which never met the parser's bound at
    /// all, is held to the same number instead of to whatever constant the layout
    /// happened to carry.
    pub limits: strict_ooxml_core::limits::ResourceLimits,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            scale: 96.0,
            media: MediaMode::EmbedDataUri,
            font_provider: FontProviderKind::Builtin,
            pages: PageSelection::All,
            background: true,
            floating: true,
            math: true,
            revisions: RevisionView::Final,
            limits: strict_ooxml_core::limits::ResourceLimits::default(),
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

    /// Enables or disables floating (anchored) DrawingML objects.
    #[must_use]
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }

    /// Enables or disables the layout and drawing of OMML formulas.
    #[must_use]
    pub fn math(mut self, math: bool) -> Self {
        self.math = math;
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
    /// What the page could not show as the model has it, one line each,
    /// prefixed with a stable id (`render.invalid-xml-char: ...`).
    pub warnings: Vec<String>,
}

/// Places every page of a document without painting it.
///
/// This is the entry point for a second backend (Stage 8B, PDF): the SVG
/// renderer is one *consumer* of the placement, not its owner. Sharing the
/// placement is what makes the two backends agree — a PDF cannot disagree with
/// the SVG about where a line went, because there is only one answer computed
/// once.
///
/// Embedded images resolve to placeholders without a `media` source, exactly as
/// [`render`] does; use [`render_with_media`] for the SVG that inlines them.
///
/// # Errors
///
/// Returns a [`StrictError`]-backed error if a layout limit is exceeded.
pub fn place_pages(
    document: &Document,
    options: &RenderOptions,
    media: Option<&dyn MediaSource>,
) -> Result<Vec<PlacedPage>> {
    let provider = options.font_provider.make();
    let context = layout::LayoutContext {
        document,
        options,
        font: provider.as_ref(),
        media,
        media_mode: options.media,
        note_numbers: notes::NoteNumbering::build(document),
        numbering: numbering::NumberingMarkers::build(document),
        block_depth: std::cell::Cell::new(0),
        warnings: std::cell::RefCell::new(Vec::new()),
    };
    Ok(layout::paginate::layout_document(&context)?.pages)
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
        note_numbers: notes::NoteNumbering::build(document),
        numbering: numbering::NumberingMarkers::build(document),
        block_depth: std::cell::Cell::new(0),
        warnings: std::cell::RefCell::new(Vec::new()),
    };
    let laid_out = layout::paginate::layout_document(&context)?;
    let layout_warnings = laid_out.warnings.clone();

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
            warnings: paint::invalid_char_warning(page)
                .into_iter()
                .chain(layout_warnings.iter().cloned())
                .collect(),
        });
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::{render, PageSelection, RenderOptions};

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

    /// Builds a document whose body is `depth` nested tables.
    ///
    /// Assembled field by field because none of the block types derive
    /// `Default`: a `Document` is what a parser produces, and the point of this
    /// fixture is the case the parser cannot produce - a model that reached the
    /// renderer without ever meeting the reader's bound.
    fn nested_tables(depth: usize) -> strict_ooxml_wml::model::Document {
        use strict_ooxml_core::error::SourceLocation;
        use strict_ooxml_core::part::PartId;
        use strict_ooxml_wml::model::block::{Block, Paragraph, Table, TableCell, TableRow};
        use strict_ooxml_wml::model::props::{
            CellProperties, ParagraphProperties, RowProperties, TableProperties,
        };
        use strict_ooxml_wml::model::support::SupportModel;
        use strict_ooxml_wml::model::values::Rsids;
        use strict_ooxml_wml::model::{
            Body, Document, DocumentSource, FontTable, MediaIndex, NoteTable, NumberingTable,
            Settings, StyleTable,
        };

        let location = || SourceLocation::new(PartId::new("/word/document.xml"), 1, 1, 0);
        let mut blocks = vec![Block::Paragraph(Paragraph {
            props: ParagraphProperties::default(),
            inlines: Vec::new(),
            rsids: Rsids::default(),
            para_id: None,
            text_id: None,
            revision: None,
            location: location(),
        })];
        for _ in 0..depth {
            blocks = vec![Block::Table(Table {
                props: TableProperties::default(),
                grid: Vec::new(),
                rows: vec![TableRow {
                    props: RowProperties::default(),
                    cells: vec![TableCell {
                        props: CellProperties::default(),
                        blocks,
                        sdt: None,
                        location: location(),
                    }],
                    sdt: None,
                    location: location(),
                }],
                location: location(),
            })];
        }
        Document {
            body: Body { blocks },
            styles: StyleTable::default(),
            numbering: NumberingTable::default(),
            footnotes: NoteTable::default(),
            endnotes: NoteTable::default(),
            settings: Settings::default(),
            theme: None,
            font_table: Option::<FontTable>::None,
            sections: Vec::new(),
            headers_footers: Vec::new(),
            media: MediaIndex::new(),
            support: SupportModel::new(),
            source: DocumentSource {
                main_document: PartId::new("/word/document.xml"),
                styles: None,
                numbering: None,
                settings: None,
                font_table: None,
                footnotes: None,
                endnotes: None,
                theme: None,
            },
        }
    }

    #[test]
    fn a_programmatically_built_document_is_held_to_the_same_block_budget() {
        // A `Document` built in code never met the parser's bound, so the layout
        // has to enforce the reader's number itself. Without this the two disagree
        // and a model the writer would refuse still reaches the stack.
        let options = RenderOptions::default();
        assert!(render(&nested_tables(12), &options).is_ok());

        let deep = render(&nested_tables(13), &options);
        let message = deep
            .expect_err("thirteen levels must be refused")
            .to_string();
        assert!(message.contains("block nesting"), "{message}");
    }

    #[test]
    fn the_render_budget_is_the_callers() {
        let document = nested_tables(3);
        let options = RenderOptions {
            limits: strict_ooxml_core::limits::ResourceLimits {
                max_block_nesting: 2,
                ..strict_ooxml_core::limits::ResourceLimits::default()
            },
            ..RenderOptions::default()
        };
        assert!(render(&document, &RenderOptions::default()).is_ok());
        let message = render(&document, &options)
            .expect_err("three levels against a budget of two")
            .to_string();
        assert!(message.contains("block nesting"), "{message}");
    }
}
