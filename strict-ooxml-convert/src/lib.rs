//! PDF to WordprocessingML Strict, in two modes (`STAGE-8-TASK.md` §5.1).
//!
//! One parsed PDF produces two different documents, and the difference is worth
//! stating plainly because it decides which one a caller should ask for.
//!
//! - **`Semantic`** reconstructs what the document *is*: paragraphs, runs,
//!   headings, lists and tables, with the geometry left to the layout engine.
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
//! image boxes exactly. It states nothing about paragraphs, headings, lists or
//! tables. Every one of those is inferred here, by a named rule with a stated
//! threshold, and every inference that fails is recorded in
//! [`ConversionReport`] rather than hidden. A conversion that quietly drops a
//! diagram is worse than one that says it could not place it.
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

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

mod geometry;
mod media;
mod report;
mod semantic;
mod visual;

use strict_ooxml_pdf::PdfDocument;
use strict_ooxml_wml::model::drawing::MediaIndex;
use strict_ooxml_wml::model::notes::NoteTable;
use strict_ooxml_wml::model::numbering::NumberingTable;
use strict_ooxml_wml::model::settings::Settings;
use strict_ooxml_wml::model::styles::StyleTable;
use strict_ooxml_wml::model::support::SupportModel;
use strict_ooxml_wml::model::Document;

pub use report::{ConversionLoss, ConversionReport, Severity};
pub use semantic::ParagraphRules;

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
    /// Pages to convert; `None` means all of them.
    pub pages: Option<(usize, usize)>,
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
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            paragraphs: Rules::default(),
            pages: None,
            embed_images: true,
            figure_classifier: None,
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
            all[start - 1..end].to_vec()
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

    let (document, media) = match options.mode {
        Mode::Semantic => semantic::build(&pages, options, &mut report),
        Mode::Visual => visual::build(&pages, options, &mut report),
    };
    Ok(Converted {
        document,
        report,
        media,
    })
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
