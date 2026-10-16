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
// Never-crash (docs/WORDCRAFT_ADOPTION_2026-10-07.md §4.2): library paths
// return errors or degrade with a report; tests may still unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]
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
pub mod metafile;
pub mod style;
pub mod units;

pub mod fields;
mod math;
mod notes;
mod numbering;
mod paint;
mod table_style;

pub use layout::{
    ImageItem, Item, LineItem, PathItem, PlacedPage, RectItem, TextAdvanceKind, TextItem,
};
use strict_ooxml_core::control;
use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::Package;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;

pub use error::RenderError;
pub use font::{FontMetrics, FontProvider, FontProviderKind};
pub use math::{math_expression_to_mathml, math_paragraph_to_mathml, MathMlError};
pub use paint::image::{media_file_name, unique_media_file_name};

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
///
/// [`PageSelection::Range`] also caps layout: the paginator stops after
/// producing `end` pages (1-based, inclusive). Page-number fields such as
/// `NUMPAGES` then reflect that truncated count, not the full document.
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

    /// Inclusive layout stop page for [`Self::Range`], or `None` for [`Self::All`].
    #[must_use]
    pub fn layout_end(self) -> Option<usize> {
        match self {
            Self::All => None,
            Self::Range { end, .. } => Some(end),
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

/// Coordinate interpretation for wrapping polygons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WrapCoordinates {
    /// ISO coordinates and this library's VML-normalized model, in EMU.
    #[default]
    Emu,
    /// Office's fixed 21600 by 21600 shape space (MS-OI29500 2.1.1349).
    Office21600,
}

/// What a font the document names, and neither the document nor the caller
/// supplies, does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontPolicy {
    /// It is measured and drawn with a metric-compatible bundled face, and a
    /// `font.substitution` warning names it (the default).
    #[default]
    Substitute,
    /// The rendering is refused: output must use exactly the fonts named.
    Reject,
}

/// Options controlling rendering.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Maximum combined SVG payload. Painting stops with a warning before
    /// retaining output beyond this bound; the default is unlimited.
    pub max_svg_bytes: usize,
    /// Wrapping polygon coordinate convention; no size-based guessing is used.
    pub wrap_coordinates: WrapCoordinates,
    /// Output scale: px per inch (default `96.0` = 96 DPI).
    pub scale: f64,
    /// Media emission mode.
    pub media: MediaMode,
    /// Font provider kind.
    pub font_provider: FontProviderKind,
    /// Fonts the caller supplies ([`font::FontAssets`]): measured, drawn and
    /// embedded like a document's own embedded fonts, for the families and
    /// styles the document does not embed. Default none: the bundled faces.
    pub font_assets: Option<std::sync::Arc<font::FontAssets>>,
    /// What a font the document names and nothing supplies does. Default
    /// [`FontPolicy::Substitute`].
    pub font_policy: FontPolicy,
    /// Page selection.
    pub pages: PageSelection,
    /// Draw a white page background.
    pub background: bool,
    /// Embed each face a page paints as an `@font-face` (default `true`),
    /// so a standalone SVG draws without the machine's fonts. A consumer
    /// with the faces at hand (a rasterizer with its own font database)
    /// turns it off: the faces are most of a page's bytes.
    pub embed_fonts: bool,
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
    /// Stretch Tinos advances to the WPS-embedded Times of the Clio audit
    /// (`font::substitute_width_scale`). Default `false`.
    ///
    /// Tinos is metric-compatible with Times New Roman, so the default layout
    /// already measures Times the way Word does. The calibration was fitted to
    /// one document's WPS export (P1, pages 54/104) and applied after
    /// justification, so on every other document it moved glyphs off the
    /// measured layout — a justified line ended 4–5 px past the right margin and
    /// the `strict-text` pixel gate failed. It is kept for reproducing that
    /// audit, behind this switch.
    pub wps_times_calibration: bool,
    /// How `w:noWrap` on a table cell is honoured. Default [`CellNoWrap::Spec`].
    pub cell_no_wrap: CellNoWrap,
    /// Place caret anchors (default `false`): an empty text item of zero
    /// width where an empty paragraph's line starts, and one spanning each
    /// tab, in the run's own style. They change no other placement and are
    /// meant for interaction maps, not for painting: the SVG and PDF
    /// backends skip them ([`TextItem::is_caret_anchor`]).
    pub caret_anchors: bool,
    /// Progress and cancellation handle (default none); see
    /// [`strict_ooxml_core::control`]. Layout reports pages laid out, painting
    /// pages written, and either stops with `StrictError::Cancelled`.
    pub control: Option<strict_ooxml_core::control::OpenControl>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            max_svg_bytes: usize::MAX,
            wrap_coordinates: WrapCoordinates::Emu,
            scale: 96.0,
            media: MediaMode::EmbedDataUri,
            font_provider: FontProviderKind::Builtin,
            font_assets: None,
            font_policy: FontPolicy::Substitute,
            pages: PageSelection::All,
            background: true,
            embed_fonts: true,
            floating: true,
            math: true,
            revisions: RevisionView::Final,
            limits: strict_ooxml_core::limits::ResourceLimits::default(),
            wps_times_calibration: false,
            cell_no_wrap: CellNoWrap::Spec,
            caret_anchors: false,
            control: None,
        }
    }
}

impl RenderOptions {
    /// Selects the coordinate convention used by anchored wrap polygons.
    #[must_use]
    pub fn wrap_coordinates(mut self, coordinates: WrapCoordinates) -> Self {
        self.wrap_coordinates = coordinates;
        self
    }

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

    /// Sets the progress and cancellation handle.
    #[must_use]
    pub fn control(mut self, control: strict_ooxml_core::control::OpenControl) -> Self {
        self.control = Some(control);
        self
    }

    /// Supplies fonts ([`Self::font_assets`]).
    #[must_use]
    pub fn font_assets(mut self, assets: std::sync::Arc<font::FontAssets>) -> Self {
        self.font_assets = Some(assets);
        self
    }

    /// Sets [`Self::font_policy`].
    #[must_use]
    pub fn font_policy(mut self, policy: FontPolicy) -> Self {
        self.font_policy = policy;
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

    /// Sets [`Self::wps_times_calibration`].
    #[must_use]
    pub fn wps_times_calibration(mut self, enabled: bool) -> Self {
        self.wps_times_calibration = enabled;
        self
    }

    /// Sets [`Self::cell_no_wrap`].
    #[must_use]
    pub fn cell_no_wrap(mut self, mode: CellNoWrap) -> Self {
        self.cell_no_wrap = mode;
        self
    }

    /// Embeds the faces in each page ([`RenderOptions::embed_fonts`]).
    #[must_use]
    pub fn embed_fonts(mut self, embed: bool) -> Self {
        self.embed_fonts = embed;
        self
    }

    /// Places caret anchors ([`RenderOptions::caret_anchors`]).
    #[must_use]
    pub fn caret_anchors(mut self, anchors: bool) -> Self {
        self.caret_anchors = anchors;
        self
    }
}

/// How `w:tcPr/w:noWrap` is honoured (ISO/IEC 29500-1 §17.4.30).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellNoWrap {
    /// As specified for the automatic table layout: a cell with a fixed
    /// (`dxa`) preferred width keeps wrapping its text and is only never made
    /// narrower than that width; a cell with an `auto`, `pct` or absent width
    /// is laid out without line breaks. In a fixed-layout table the flag keeps
    /// the text unbroken. This is what WPS draws.
    #[default]
    Spec,
    /// The flag always suppresses line breaks, whatever the cell width and
    /// table layout (the behaviour before the option existed).
    Always,
}

impl CellNoWrap {
    /// Parses a CLI value: `spec` or `always`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "spec" => Some(Self::Spec),
            "always" => Some(Self::Always),
            _ => None,
        }
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
    /// Document layout warnings are repeated on every returned page; only
    /// paint diagnostics such as invalid XML characters identify this page.
    pub warnings: Vec<String>,
}

/// Places pages of a document without painting them.
///
/// This is the entry point for a second backend (Stage 8B, PDF): the SVG
/// renderer is one *consumer* of the placement, not its owner. Sharing the
/// placement is what makes the two backends agree — a PDF cannot disagree with
/// the SVG about where a line went, because there is only one answer computed
/// once.
///
/// [`RenderOptions::pages`] is honored during layout ([`PageSelection::Range`]
/// stops the paginator after `end`) and again when returning pages: only
/// indices selected by [`PageSelection::contains`] are included.
///
/// Embedded images resolve to placeholders without a `media` source, exactly as
/// [`render`] does; use [`render_with_media`] for the SVG that inlines them.
///
/// # Errors
///
/// Returns a [`StrictError`](strict_ooxml_core::error::StrictError)-backed error if a layout limit is exceeded.
pub fn place_pages(
    document: &Document,
    options: &RenderOptions,
    media: Option<&dyn MediaSource>,
) -> Result<Vec<PlacedPage>> {
    place_document(document, options, media).map(|placement| placement.pages)
}

/// A laid-out document: the selected pages and what layout warned about.
#[derive(Clone, Debug, Default)]
pub struct Placement {
    /// The placed pages, in order.
    pub pages: Vec<PlacedPage>,
    /// The warnings [`render_with_media`] gives every page, each once.
    pub warnings: Vec<String>,
}

/// Lays `document` out once, for painting page by page ([`paint_page`]):
/// a viewer paints only the pages it shows, and an interaction map reuses
/// the same placement.
///
/// # Errors
///
/// As [`place_pages`].
pub fn place_document(
    document: &Document,
    options: &RenderOptions,
    media: Option<&dyn MediaSource>,
) -> Result<Placement> {
    let _scope = options.control.as_ref().map(control::OpenControl::enter);
    let provider = options.font_provider.make();
    let context = layout::LayoutContext {
        document,
        options,
        font: provider.as_ref(),
        media,
        media_mode: options.media,
        font_programs: std::sync::Arc::new(font::embedded::FontPrograms::load(
            document,
            media,
            options.font_assets.as_deref(),
        )),
        note_numbers: notes::NoteNumbering::build(document),
        numbering: numbering::NumberingMarkers::build(document),
        block_depth: std::cell::Cell::new(0),
        warnings: std::cell::RefCell::new(Vec::new()),
        field_env: std::cell::Cell::new(None),
        current_section: std::cell::Cell::new(0),
        chapter_numbers: std::cell::RefCell::new(Vec::new()),
        render_items: std::cell::Cell::new(0),
        frame_anchor: std::cell::Cell::new(layout::FrameAnchor::default()),
        frame_cursors: std::cell::RefCell::new(Vec::new()),
        page_floats_preload: std::cell::RefCell::new(Vec::new()),
        page_floats_record: std::cell::RefCell::new(Vec::new()),
        page_floats_banned: std::cell::RefCell::new(std::collections::BTreeSet::new()),
        page_floats_next: std::cell::Cell::new(0),
        page_floats_forward: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        page_floats_moves: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        frame_prior_exact: std::cell::Cell::new(None),
        frame_force_exact_grid: std::cell::Cell::new(false),
        in_frame: std::cell::Cell::new(false),
        region_heights: std::cell::RefCell::new(std::collections::HashMap::new()),
        metric_advances: std::cell::Cell::new(false),
    };
    let laid_out = layout::paginate::layout_document(&context)?;
    check_font_policy(options, &laid_out.warnings)?;
    let pages: Vec<PlacedPage> = laid_out
        .pages
        .into_iter()
        .enumerate()
        .filter(|(index, _)| options.pages.contains(index + 1))
        .map(|(_, page)| page)
        .collect();
    let mut warnings = Vec::new();
    for warning in pages
        .iter()
        .filter_map(paint::invalid_char_warning)
        .chain(laid_out.warnings)
    {
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }
    Ok(Placement { pages, warnings })
}

/// The font programs layout measures with under `options`: the document's
/// embedded fonts, then the caller's [`RenderOptions::font_assets`]. A
/// [`style::compute_run`] result carries none; give it these to ask
/// [`style::installed_paint_family`] what a run is drawn with.
#[must_use]
pub fn font_programs(
    document: &Document,
    options: &RenderOptions,
    media: Option<&dyn MediaSource>,
) -> std::sync::Arc<font::embedded::FontPrograms> {
    std::sync::Arc::new(font::embedded::FontPrograms::load(
        document,
        media,
        options.font_assets.as_deref(),
    ))
}

/// Paints one placed page to an SVG document, as [`render_with_media`]
/// paints it under the same options ([`RenderOptions::background`],
/// [`RenderOptions::embed_fonts`]).
#[must_use]
pub fn paint_page(page: &PlacedPage, options: &RenderOptions) -> String {
    paint::render_page(page, options.background, options.embed_fonts)
}

/// Refuses the layout under [`FontPolicy::Reject`] when a font was
/// substituted or a document font could not be used.
fn check_font_policy(options: &RenderOptions, warnings: &[String]) -> Result<()> {
    if options.font_policy != FontPolicy::Reject {
        return Ok(());
    }
    match warnings.iter().find(|warning| {
        warning.starts_with("font.substitution")
            || warning.starts_with("font.embedded.")
            || warning.starts_with("font.embedding.")
    }) {
        Some(warning) => Err(RenderError::FontUnavailable(warning.clone()).into_strict()),
        None => Ok(()),
    }
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
    let _scope = options.control.as_ref().map(control::OpenControl::enter);
    let provider = options.font_provider.make();
    let context = layout::LayoutContext {
        document,
        options,
        font: provider.as_ref(),
        media,
        media_mode: options.media,
        font_programs: std::sync::Arc::new(font::embedded::FontPrograms::load(
            document,
            media,
            options.font_assets.as_deref(),
        )),
        note_numbers: notes::NoteNumbering::build(document),
        numbering: numbering::NumberingMarkers::build(document),
        block_depth: std::cell::Cell::new(0),
        warnings: std::cell::RefCell::new(Vec::new()),
        field_env: std::cell::Cell::new(None),
        current_section: std::cell::Cell::new(0),
        chapter_numbers: std::cell::RefCell::new(Vec::new()),
        render_items: std::cell::Cell::new(0),
        frame_anchor: std::cell::Cell::new(layout::FrameAnchor::default()),
        frame_cursors: std::cell::RefCell::new(Vec::new()),
        page_floats_preload: std::cell::RefCell::new(Vec::new()),
        page_floats_record: std::cell::RefCell::new(Vec::new()),
        page_floats_banned: std::cell::RefCell::new(std::collections::BTreeSet::new()),
        page_floats_next: std::cell::Cell::new(0),
        page_floats_forward: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        page_floats_moves: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        frame_prior_exact: std::cell::Cell::new(None),
        frame_force_exact_grid: std::cell::Cell::new(false),
        in_frame: std::cell::Cell::new(false),
        region_heights: std::cell::RefCell::new(std::collections::HashMap::new()),
        metric_advances: std::cell::Cell::new(false),
    };
    let laid_out = layout::paginate::layout_document(&context)?;
    check_font_policy(options, &laid_out.warnings)?;
    let layout_warnings = laid_out.warnings.clone();

    let mut pages: Vec<Page> = Vec::new();
    let mut svg_bytes = 0usize;
    control::stage(control::Stage::Painting, laid_out.pages.len() as u64);
    for (index, page) in laid_out.pages.iter().enumerate() {
        control::checkpoint_now()?;
        control::advance(index as u64);
        if !options.pages.contains(index + 1) {
            continue;
        }
        let svg = paint::render_page(page, options.background, options.embed_fonts);
        if svg_bytes.saturating_add(svg.len()) > options.max_svg_bytes {
            let warning = format!(
                "render.output-limit: SVG payload exceeds {} bytes; remaining pages omitted",
                options.max_svg_bytes
            );
            if pages.is_empty() {
                return Err(RenderError::LimitExceeded {
                    what: "SVG bytes",
                    limit: options.max_svg_bytes as u64,
                    actual: svg.len() as u64,
                }
                .into_strict());
            }
            for previous in &mut pages {
                previous.warnings.push(warning.clone());
            }
            break;
        }
        svg_bytes += svg.len();
        pages.push(Page {
            index,
            width_px: page.width_px,
            height_px: page.height_px,
            svg,
            warnings: paint::invalid_char_warning(page)
                .into_iter()
                .chain(layout_warnings.iter().cloned())
                .collect(),
        });
    }
    control::advance(laid_out.pages.len() as u64);
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::{render, PageSelection, RenderOptions};

    #[test]
    fn page_selection_contains() {
        assert!(PageSelection::All.contains(1));
        assert!(PageSelection::All.contains(999));
        assert_eq!(PageSelection::All.layout_end(), None);
        let range = PageSelection::Range { start: 2, end: 3 };
        assert!(!range.contains(1));
        assert!(range.contains(2));
        assert!(range.contains(3));
        assert!(!range.contains(4));
        assert_eq!(range.layout_end(), Some(3));
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
                grid_change: None,
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
            comments: Vec::new(),
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
                comments: None,
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
