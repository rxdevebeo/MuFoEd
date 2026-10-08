//! PDF to WordprocessingML Strict, in two modes (`STAGE-8-TASK.md` §5.1).
//!
//! One parsed PDF produces two different documents, and the difference is worth
//! stating plainly because it decides which one a caller should ask for.
//!
//! - **`Semantic`** reconstructs what the document *is*: paragraphs, runs,
//!   headings and tables, with the geometry left to the layout engine.
//!   The result is editable and re-flows; the page break positions are a
//!   reconstruction, not the original.
//! - **`Visual`** reconstructs where everything *was*: one section per page
//!   with the original page size, one paragraph per line at the original
//!   baseline and indent. The page looks the same; the structure is a
//!   convention, and editing it reflows the page.
//!
//! Both produce the same model type and are written out by the same writer, so
//! a caller can convert a document and then use every stage-4 to stage-7 tool
//! on the result.
//!
//! # What is inferred, and what is given
//!
//! The PDF states glyph origins, advances, font sizes, colours, paths and
//! image boxes exactly. It states nothing about paragraphs, headings or
//! tables. Every one of those is inferred here, by a named rule with a stated
//! threshold ([`ParagraphRules`], [`TableRules`]), and every inference is
//! recorded in [`ConversionReport`] rather than hidden. A conversion that
//! quietly drops a diagram is worse than one that says it could not place it.
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml_convert::{convert, Mode, PdfOptions};
//! use strict_ooxml_pdf::{PdfDocument, PdfLimits};
//!
//! let bytes = std::fs::read("document.pdf").expect("readable");
//! let mut pdf = PdfDocument::open(&bytes, PdfLimits::default())?;
//! let converted = convert(&mut pdf, &PdfOptions::default().mode(Mode::Semantic))?;
//! for loss in &converted.report.losses {
//!     eprintln!("{loss}");
//! }
//! println!("{} block(s)", converted.document.body.blocks.len());
//! # Ok::<(), strict_ooxml_convert::ConvertError>(())
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
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

mod columns;
mod geometry;
mod lists;
mod media;
mod recover;
mod report;
mod sections;
mod semantic;
mod tables;
mod vectors;
mod visual;

use strict_ooxml_pdf::PdfDocument;
use strict_ooxml_wml::model::drawing::MediaIndex;
use strict_ooxml_wml::model::notes::NoteTable;
use strict_ooxml_wml::model::numbering::NumberingTable;
use strict_ooxml_wml::model::settings::Settings;
use strict_ooxml_wml::model::styles::StyleTable;
use strict_ooxml_wml::model::support::SupportModel;
use strict_ooxml_wml::model::Document;

pub use lists::ListRules;
pub use report::{ConversionLoss, ConversionReport, Severity};
pub use semantic::ParagraphRules;
pub use tables::TableRules;

use crate::semantic::ParagraphRules as Rules;

/// One converted page's section properties.
pub struct Section {
    /// The 1-based page it came from.
    pub number: usize,
    /// The properties written into `w:sectPr`.
    pub properties: strict_ooxml_wml::model::props::SectionProperties,
}

/// An empty document with every part the writer needs.
#[must_use]
pub(crate) fn empty_document() -> Document {
    Document {
        body: strict_ooxml_wml::model::document::Body { blocks: Vec::new() },
        styles: StyleTable::default(),
        numbering: NumberingTable::default(),
        footnotes: NoteTable::default(),
        endnotes: NoteTable::default(),
        settings: Settings::default(),
        font_table: None,
        theme: None,
        sections: Vec::new(),
        headers_footers: Vec::new(),
        media: MediaIndex::default(),
        support: SupportModel::default(),
        source: strict_ooxml_wml::model::document::DocumentSource {
            main_document: strict_ooxml_core::part::PartId::new("/word/document.xml"),
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

/// A page size in twentieths of a point, the unit the model speaks.
pub type Twips = i32;

/// Points per inch, named because page geometry is the only place it appears.
pub const POINTS_PER_INCH: f64 = 72.0;

/// Which document to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Reconstruct the structure: paragraphs, runs, headings, tables.
    #[default]
    Semantic,
    /// Reconstruct the layout: one section per page, one paragraph per line.
    Visual,
}

impl Mode {
    /// The mode's name, for a report line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Visual => "visual",
        }
    }
}

/// Everything the conversion needs beyond the PDF.
#[derive(Clone)]
pub struct PdfOptions {
    /// Which document to build.
    pub mode: Mode,
    /// The rules that turn lines into paragraphs, in semantic mode.
    pub paragraphs: ParagraphRules,
    /// The rules that turn ruling lines into tables, in semantic mode.
    pub tables: TableRules,
    /// The rules that turn a drawn marker into a list, in semantic mode (P-8).
    ///
    /// Only bullets: a marker that is a literal character moves into the numbering
    /// definition and Word draws the same character. A *numbered* list would need
    /// Word to generate the sequence, so those are reported and left as text.
    pub lists: ListRules,
    /// Pages to convert; `None` means all of them.
    pub pages: Option<(usize, usize)>,
    /// Ruling lines on one page past which tables are not sought. Default: 4000.
    ///
    /// Past this ceiling the page's text stays as paragraphs and the report
    /// records `convert.table.budget` (AUD-15). The match that builds figures
    /// is O(n log n), but a page of twenty thousand rules is still not a table.
    pub max_table_lines: usize,
    /// Cells a single inferred grid may hold. Default: 10 000.
    ///
    /// A grid past this is left as paragraphs with the same report id: the
    /// text is kept, the table is not built.
    pub max_table_cells: usize,
    /// Whether to embed the images the PDF carries.
    ///
    /// Off by default because an image is the largest thing a document can
    /// contain and a caller converting text usually does not want the pages
    /// re-embedded as pictures.
    pub embed_images: bool,
    /// A model that says what each graphic region is.
    ///
    /// `None` - the default - means no model is consulted at all, and a page or
    /// region that needed one says so in the report. That is what makes a build
    /// with the `ocr-ollama` feature off produce the same document as one with it
    /// on when nothing asks for recognition (O4, O6).
    pub figure_classifier: Option<std::sync::Arc<dyn strict_ooxml_ocr::FigureClassifier>>,
    /// A model that reads the text of a page that has none.
    ///
    /// A scanned page has no glyphs, so the only way to get words out of it is to
    /// show it to something that reads pictures. `None` — the default, and the
    /// only option without the `raster` feature — means a page this converter
    /// cannot read is **reported as a loss** rather than guessed at.
    ///
    /// Recovered text is marked twice: in the document, by a paragraph style
    /// whose id is `Recovered` (see [`RECOVERED_STYLE_NAME`]), and in the report,
    /// by a [`Severity::Recovered`] entry naming the model.
    #[cfg(feature = "raster")]
    pub text_recovery: Option<std::sync::Arc<dyn strict_ooxml_ocr::TextRecovery>>,
    /// Pixels per point of the page image a [`PdfOptions::text_recovery`] model is
    /// shown. Default 2.0, which is 144 dpi — enough for a model to read body
    /// text, and cheap enough that a fifty-page scan is not a timeout. A *region*
    /// gets more than this when the region is small, down to
    /// [`PdfOptions::recovery_min_region_px`].
    #[cfg(feature = "raster")]
    pub recovery_scale: f64,
    /// Pixels on the long side that a region is given at least.
    ///
    /// Default 1200: what a vision model needs to read a line of type, and the
    /// reason a 40 pt column of text is rasterized at a scale the page would
    /// never use. It is a **wish**, not an order — see
    /// [`PdfOptions::recovery_max_pixels`], which is what actually gets spent,
    /// because the rasterizer draws the page and crops the region out of it.
    #[cfg(feature = "raster")]
    pub recovery_min_region_px: f64,
    /// Pixels one recovery call may allocate. Default 16 Mi (4096²).
    #[cfg(feature = "raster")]
    pub recovery_max_pixels: u64,
    /// The smallest picture, in points on its long side, that is worth sending to
    /// a model. Default 24 pt.
    ///
    /// Below that a picture is a bullet, a logo or a rule, and thirty calls to a
    /// model to be told «no text» is a page of text turned into an afternoon.
    #[cfg(feature = "raster")]
    pub recovery_min_region_pt: f64,
    /// How many pictures of one page are sent to a model, largest first.
    ///
    /// Default 4. A scan is one picture and costs one call; a page of a magazine
    /// is several, and the cap is where the cost stops being a decision and starts
    /// being a bill.
    #[cfg(feature = "raster")]
    pub recovery_max_regions: usize,
    /// The share of the page a picture must cover before the text *inside* it is
    /// worth a call, on a page whose own text this converter already has.
    ///
    /// Default 0.25 — a quarter of the page. A **share** and not points, because
    /// the question is whether a picture is a picture *of something* rather than a
    /// mark on the page, and that scales with the page.
    ///
    /// This is the second trigger (`Q-28`), and it is much stricter than the first
    /// on purpose. A page with no text at all is all candidate; a page whose text is
    /// complete is candidate only where a raster picture is this big, because
    /// vector art has its labels as real glyphs and a small picture is a logo, a
    /// bullet or a rule. Over the foreign corpus: of 106 526 raster pictures on
    /// 5 142 readable pages, 105 581 are under a tenth of the page and 418 reach a
    /// quarter — so this number is the whole population the rule can ever offer.
    ///
    /// Set [`PdfOptions::figure_classifier`] as well and the classifier gets the
    /// last word: a picture it calls a diagram, a table, a formula or a logo is a
    /// figure, and a figure gets a `descr`, not paragraphs guessed from its pixels.
    #[cfg(feature = "raster")]
    pub recovery_mixed_min_area_ratio: f64,
}

/// The name of the paragraph style a recovered paragraph carries, as a reader
/// of the converted document sees it in Word's styles pane.
#[cfg(feature = "raster")]
pub const RECOVERY_STYLE_NAME: &str = "Recovered from a page image";

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            paragraphs: Rules::default(),
            tables: TableRules::default(),
            lists: ListRules::default(),
            pages: None,
            max_table_lines: 4000,
            max_table_cells: 10_000,
            embed_images: true,
            figure_classifier: None,
            #[cfg(feature = "raster")]
            text_recovery: None,
            #[cfg(feature = "raster")]
            recovery_scale: 2.0,
            #[cfg(feature = "raster")]
            recovery_min_region_px: 1200.0,
            #[cfg(feature = "raster")]
            recovery_max_pixels: 4096 * 4096,
            #[cfg(feature = "raster")]
            recovery_min_region_pt: 24.0,
            #[cfg(feature = "raster")]
            recovery_max_regions: 4,
            #[cfg(feature = "raster")]
            recovery_mixed_min_area_ratio: 0.25,
        }
    }
}

impl PdfOptions {
    /// Sets the mode.
    #[must_use]
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the paragraph rules.
    #[must_use]
    pub fn paragraphs(mut self, rules: ParagraphRules) -> Self {
        self.paragraphs = rules;
        self
    }

    /// Sets the table rules.
    #[must_use]
    pub fn tables(mut self, rules: TableRules) -> Self {
        self.tables = rules;
        self
    }

    /// Sets the list rules.
    #[must_use]
    pub fn lists(mut self, rules: ListRules) -> Self {
        self.lists = rules;
        self
    }

    /// Sets whether images are embedded.
    #[must_use]
    pub fn embed_images(mut self, embed: bool) -> Self {
        self.embed_images = embed;
        self
    }

    /// Sets the model that describes graphic regions.
    #[must_use]
    pub fn figure_classifier(
        mut self,
        classifier: Option<std::sync::Arc<dyn strict_ooxml_ocr::FigureClassifier>>,
    ) -> Self {
        self.figure_classifier = classifier;
        self
    }

    /// Sets the model that reads the text of a page that has none.
    #[cfg(feature = "raster")]
    #[must_use]
    pub fn text_recovery(
        mut self,
        recovery: Option<std::sync::Arc<dyn strict_ooxml_ocr::TextRecovery>>,
    ) -> Self {
        self.text_recovery = recovery;
        self
    }

    /// Sets the scale of the page image a recovery model is shown.
    #[cfg(feature = "raster")]
    #[must_use]
    pub fn recovery_scale(mut self, scale: f64) -> Self {
        self.recovery_scale = scale;
        self
    }

    /// Sets how many pictures of one page a recovery model is shown, and how big
    /// a picture has to be to be worth showing.
    #[cfg(feature = "raster")]
    #[must_use]
    pub fn recovery_regions(mut self, max_regions: usize, min_region_pt: f64) -> Self {
        self.recovery_max_regions = max_regions;
        self.recovery_min_region_pt = min_region_pt;
        self
    }

    /// Sets the share of the page a picture must cover before the text inside it
    /// is asked about, on a page whose text is already read.
    #[cfg(feature = "raster")]
    #[must_use]
    pub fn recovery_mixed_area(mut self, min_area_ratio: f64) -> Self {
        self.recovery_mixed_min_area_ratio = min_area_ratio;
        self
    }
}

/// A converted document, what the conversion could not do, and the image bytes
/// it took out of the PDF.
#[derive(Clone, Debug)]
pub struct Converted {
    /// The document, ready for `strict-ooxml-write`.
    pub document: Document,
    /// What was inferred and what was given up.
    pub report: ConversionReport,
    /// The media parts, with the bytes the document's `MediaIndex` names.
    ///
    /// A converted document references its images by part id and carries no
    /// bytes: the model has nowhere to put them. A caller writing the document
    /// out needs both halves, and this is the other half.
    pub media: Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>,
}

/// A failure that stopped the conversion outright.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ConvertError {
    /// The reader failed; the conversion cannot start.
    Pdf(String),
    /// The page range asked for is outside the document.
    NoSuchPage(usize),
    /// The page has no size, so nothing can be placed on it.
    UnusablePage(usize),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pdf(detail) => write!(f, "the PDF could not be read: {detail}"),
            Self::NoSuchPage(number) => write!(f, "page {number} is not in the document"),
            Self::UnusablePage(number) => {
                write!(f, "page {number} has no usable size or origin")
            }
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<strict_ooxml_pdf::PdfError> for ConvertError {
    fn from(error: strict_ooxml_pdf::PdfError) -> Self {
        Self::Pdf(error.to_string())
    }
}

/// Converts a PDF into a document model.
///
/// # Errors
///
/// Returns [`ConvertError::Pdf`] when a page cannot be read,
/// [`ConvertError::NoSuchPage`] for a page range outside the document and
/// [`ConvertError::UnusablePage`] when a page has no usable geometry.
pub fn convert(pdf: &mut PdfDocument, options: &PdfOptions) -> Result<Converted, ConvertError> {
    let all = pdf
        .pages()
        .map_err(|error| ConvertError::Pdf(error.to_string()))?;
    let pages: Vec<strict_ooxml_pdf::PdfPage> = match options.pages {
        None => all,
        Some((start, end)) => {
            if all.is_empty() || start == 0 || start > all.len() || start > end {
                return Err(ConvertError::NoSuchPage(start));
            }
            let end = end.min(all.len());
            all.get(start - 1..end)
                .ok_or(ConvertError::NoSuchPage(start))?
                .to_vec()
        }
    };
    if pages.is_empty() {
        return Err(ConvertError::NoSuchPage(
            options.pages.map_or(1, |(s, _)| s),
        ));
    }

    let mut report = ConversionReport::new(options.mode);
    for page in &pages {
        if !(page.geometry.width.is_finite() && page.geometry.height.is_finite())
            || page.geometry.width <= 0.0
            || page.geometry.height <= 0.0
        {
            return Err(ConvertError::UnusablePage(page.number));
        }
    }
    for page in &pages {
        report.merge(&page.report);
    }

    // The recovery pass runs after the page reports are folded in and before the
    // blocks are built, because the recovered paragraphs have to land in the
    // page's place in the flow — and because a page we cannot read is a fact
    // about the conversion, not about a paragraph.
    #[cfg(feature = "raster")]
    let recovered = recover::recover_pages(pdf, &pages, options, &mut report);
    #[cfg(not(feature = "raster"))]
    let recovered = {
        recover::record_absent(&pages, &mut report);
        recover::Recovered::default()
    };

    #[cfg_attr(not(feature = "raster"), allow(unused_mut))]
    let (mut document, media) = match options.mode {
        Mode::Semantic => semantic::build(&pages, &recovered, options, &mut report),
        Mode::Visual => visual::build(&pages, &recovered, options, &mut report),
    };
    #[cfg(feature = "raster")]
    if !recovered.is_empty() {
        recover::declare_style(&mut document);
    }
    // AUD-83: PDF text layers can carry C0 controls that XML 1.0 cannot. Strip
    // them from every `w:t` before the writer sees the model, and name the loss
    // here so a conversion report does not depend on the writer being asked.
    sanitize_xml_text(&mut document, &mut report);
    Ok(Converted {
        document,
        report,
        media,
    })
}

/// Removes characters XML 1.0 cannot carry from every text node (AUD-83).
fn sanitize_xml_text(document: &mut Document, report: &mut ConversionReport) {
    use strict_ooxml_core::xml::escape::is_xml_char;
    use strict_ooxml_wml::model::block::Block;
    use strict_ooxml_wml::model::inline::{Inline, RunContent};

    let mut removed = 0usize;
    for block in &mut document.body.blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &mut paragraph.inlines {
            let Inline::Run(run) = inline else {
                continue;
            };
            for content in &mut run.content {
                let RunContent::Text(node) = content else {
                    continue;
                };
                let before = node.text.chars().count();
                node.text = node.text.chars().filter(|&ch| is_xml_char(ch)).collect();
                removed += before - node.text.chars().count();
            }
        }
    }
    if removed > 0 {
        report.record(
            "convert.invalid-xml-char",
            Severity::Lost,
            format!("{removed} character(s) that XML 1.0 cannot carry were removed from the text"),
        );
    }
}

/// Points to twips, rounded to the nearest twentieth of a point.
#[must_use]
pub fn to_twips(points: f64) -> i32 {
    if !points.is_finite() {
        return 0;
    }
    let twips = (points * 20.0).round();
    twips.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// Points to half-points, the unit `w:sz` uses.
#[must_use]
pub fn to_half_points(points: f64) -> i32 {
    if !points.is_finite() {
        return 0;
    }
    let half = (points * 2.0).round();
    half.clamp(2.0, 1638.0) as i32
}

/// Points to points per inch, the inverse of [`to_twips`] for page sizes.
#[must_use]
pub fn points_from_twips(twips: Twips) -> f64 {
    f64::from(twips) / 20.0
}

/// The page size in points a WordprocessingML page size implies.
#[must_use]
pub fn page_points(width_twips: Twips, height_twips: Twips) -> (f64, f64) {
    (
        points_from_twips(width_twips),
        points_from_twips(height_twips),
    )
}

/// Points to twips: one twip is a twentieth of a point.
pub const TWIPS_PER_POINT: f64 = 20.0;

#[cfg(test)]
mod tests {
    use super::{to_half_points, to_twips, Mode};

    #[test]
    fn twips_round_to_the_nearest_twentieth_of_a_point() {
        assert_eq!(to_twips(1.0), 20);
        assert_eq!(to_twips(72.0), 1440);
        assert_eq!(to_twips(0.024), 0);
        assert_eq!(to_twips(0.026), 1);
        // A non-finite measurement is zero, not a panic and not garbage.
        assert_eq!(to_twips(f64::NAN), 0);
        assert_eq!(to_twips(f64::INFINITY), 0);
    }

    #[test]
    fn half_points_clamp_to_the_schemas_range() {
        assert_eq!(to_half_points(12.0), 24);
        // `w:sz` has a domain of 2..1638 half-points (ISO/IEC 29500-1
        // ST_HpsMeasure); anything outside is a document a reader rejects.
        assert_eq!(to_half_points(0.1), 2);
        assert_eq!(to_half_points(10_000.0), 1638);
    }

    #[test]
    fn the_mode_names_itself() {
        assert_eq!(Mode::Semantic.as_str(), "semantic");
        assert_eq!(Mode::Visual.as_str(), "visual");
    }
}
