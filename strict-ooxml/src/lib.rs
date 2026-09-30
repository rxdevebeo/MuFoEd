//! Public entry point for reading WordprocessingML Strict documents.
//!
//! The meta-crate ties the Stage-1 core ([`strict_ooxml_core`]) to the Stage-2
//! document model ([`strict_ooxml_wml`]). `StrictDocument` owns both the opened
//! OPC package and the parsed, immutable [`Document`].
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml::{OpenOptions, StrictDocument};
//!
//! let doc = StrictDocument::open_path("document.docx", &OpenOptions::default())?;
//! println!("blocks: {}", doc.document().body.blocks.len());
//! println!("{}", doc.support_debug());
//! # Ok::<(), strict_ooxml_core::error::StrictError>(())
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(clippy::doc_markdown)]

use std::io::Read;
use std::path::Path;

use strict_ooxml_core::error::Result;
#[cfg(feature = "report")]
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::Package;
use strict_ooxml_wml::model::support::SupportModel;

pub use strict_ooxml_core::error::StrictError;
pub use strict_ooxml_core::limits::ResourceLimits;
/// How much a normalization removal matters.
///
/// Named apart from the Feature Report's own `Severity`, which grades support
/// of a construct; this one grades what removing it cost.
pub use strict_ooxml_core::normalize::Severity as LossSeverity;
pub use strict_ooxml_core::normalize::{
    InvariantMode, LossRecord, McePolicy, NormalizationReport, NormalizerOptions,
    TransitionalNormalizer,
};
pub use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions};
#[cfg(feature = "pdf")]
pub use strict_ooxml_render_pdf::{
    render as render_pdf_pages, render_with_source as render_pdf_pages_with_source, PdfLoss,
    PdfOutput, PdfReport,
};
#[cfg(feature = "svg")]
pub use strict_ooxml_render_svg::{MediaMode, Page, PageSelection, RenderError, RenderOptions};
#[cfg(feature = "report")]
pub use strict_ooxml_report::{
    Feature, FeatureStatus, Location, OverallStatus, Severity, SupportReport,
    SCHEMA_VERSION as REPORT_SCHEMA_VERSION,
};
pub use strict_ooxml_wml::model;
pub use strict_ooxml_wml::model::Document;
pub use strict_ooxml_wml::ParseOptions as WmlOptions;
pub use strict_ooxml_wml::{parse_document, ParseOptions};
#[cfg(feature = "write")]
pub use strict_ooxml_write::package::{NoSource, RelationshipInfo, Source};
#[cfg(feature = "write")]
pub use strict_ooxml_write::{
    dropped_count, verify_no_silent_loss, write_package, WriteOptions, WriteOutput, WriteReport,
};

/// An opened Strict document: the OPC package plus its parsed model.
pub struct StrictDocument {
    package: Package,
    document: Document,
    file: String,
}

impl StrictDocument {
    /// Parses an already-opened package.
    ///
    /// The file name used in reports is `"<package>"`; use
    /// [`StrictDocument::with_file_name`] to override it.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] if the package is not Strict or the document
    /// cannot be parsed.
    pub fn from_package(package: Package, options: &ParseOptions) -> Result<Self> {
        let document = parse_document(&package, options)?;
        Ok(Self {
            package,
            document,
            file: "<package>".to_owned(),
        })
    }

    /// Opens and parses a `.docx` from a filesystem path.
    ///
    /// # Errors
    ///
    /// See [`StrictDocument::from_package`].
    pub fn open_path(path: impl AsRef<Path>, options: &OpenOptions) -> Result<Self> {
        let path = path.as_ref();
        let package = Package::open_path(path, options)?;
        let mut document = Self::from_package(package, &parse_options_from(options))?;
        document.file = path.display().to_string();
        Ok(document)
    }

    /// Opens and parses a `.docx` from any reader.
    ///
    /// # Errors
    ///
    /// See [`StrictDocument::from_package`].
    pub fn open_reader<R: Read>(reader: R, options: &OpenOptions) -> Result<Self> {
        let package = Package::open_reader(reader, options)?;
        let mut document = Self::from_package(package, &parse_options_from(options))?;
        document.file = String::from("<reader>");
        Ok(document)
    }

    /// Overrides the file name recorded in reports.
    #[must_use]
    pub fn with_file_name(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    /// Returns the file name recorded in reports.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the parsed document model.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Returns the opened OPC package.
    #[must_use]
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// Returns the support model for this document.
    #[must_use]
    pub fn support(&self) -> &SupportModel {
        self.document.support()
    }

    /// Renders a human-readable support summary.
    ///
    /// The full JSON Feature Report is delivered in Stage 3.
    #[must_use]
    pub fn support_debug(&self) -> String {
        self.document.support_debug()
    }

    /// Builds the Stage-3 Feature Report for this document.
    ///
    /// Available with the `report` feature (enabled by default). The report
    /// records the actual detected conformance and a `Strict` declared target;
    /// normalization is always `false` until Stage 6.
    ///
    /// The report is a **problem/attention view** (ADR-0005): `features` lists
    /// the mechanisms the parser recorded (normally those needing attention),
    /// and `overall_status: supported` means "no recorded limitation", not a
    /// complete coverage claim.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use strict_ooxml::{OpenOptions, StrictDocument};
    ///
    /// let doc = StrictDocument::open_path("document.docx", &OpenOptions::default())?;
    /// let report = doc.support_report();
    /// println!("{}", report.overall_status);
    /// # Ok::<(), strict_ooxml_core::error::StrictError>(())
    /// ```
    #[cfg(feature = "report")]
    #[must_use]
    pub fn support_report(&self) -> SupportReport {
        use strict_ooxml_report::{build, Location, ReportInput, Tool};

        let main = self
            .package
            .main_document_part()
            .map_or("/word/document.xml", |part| part.as_str());
        let fallback = Location::new(format!("{main}:1:1"));
        let input = ReportInput::new(&self.file, self.support())
            .tool(Tool::new("strict-ooxml", env!("CARGO_PKG_VERSION")))
            .conformance(Conformance::Strict, self.package.conformance(), false)
            .fallback_location(Some(fallback));
        build(input)
    }

    /// Serializes the Feature Report to deterministic JSON.
    ///
    /// Available with the `report` feature (enabled by default).
    #[cfg(feature = "report")]
    #[must_use]
    pub fn report_json(&self) -> String {
        self.support_report().to_json()
    }

    /// Renders the Feature Report for humans.
    ///
    /// Available with the `report` feature (enabled by default).
    #[cfg(feature = "report")]
    #[must_use]
    pub fn report_text(&self) -> String {
        self.support_report().to_text()
    }

    /// Renders the document to SVG pages.
    ///
    /// Available with the `svg` feature (enabled by default). Images are
    /// resolved from the opened package, so `RenderOptions::media` controls
    /// whether they are embedded as `data:` URIs, referenced by file name or
    /// replaced with placeholders.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] if output limits are exceeded or rendering
    /// fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use strict_ooxml::{OpenOptions, RenderOptions, StrictDocument};
    ///
    /// let doc = StrictDocument::open_path("document.docx", &OpenOptions::default())?;
    /// let pages = doc.render_svg(&RenderOptions::default())?;
    /// println!("{} page(s)", pages.len());
    /// # Ok::<(), strict_ooxml_core::error::StrictError>(())
    /// ```
    #[cfg(feature = "svg")]
    pub fn render_svg(&self, options: &RenderOptions) -> Result<Vec<Page>> {
        strict_ooxml_render_svg::render_with_media(&self.document, options, Some(&self.package))
    }

    /// Renders a single page to an SVG string (zero-based `index`).
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] if `index` is out of range or rendering fails.
    #[cfg(feature = "svg")]
    pub fn render_page_svg(&self, index: usize) -> Result<String> {
        let selection = PageSelection::Range {
            start: index + 1,
            end: index + 1,
        };
        self.render_svg(&RenderOptions::default().pages(selection))?
            .into_iter()
            .next()
            .map(|page| page.svg)
            .ok_or_else(|| StrictError::Render(format!("page {index} is out of range")))
    }

    /// Renders every page to an SVG string.
    ///
    /// # Errors
    ///
    /// See [`StrictDocument::render_svg`].
    #[cfg(feature = "svg")]
    pub fn render_all_svg(&self) -> Result<Vec<String>> {
        Ok(self
            .render_svg(&RenderOptions::default())?
            .into_iter()
            .map(|page| page.svg)
            .collect())
    }

    /// Renders the document to a PDF (`STAGE-8-TASK.md` §4).
    ///
    /// The text is real text: every used face is subsetted and embedded as a CID
    /// font with a `ToUnicode` CMap, so the result is selectable and searchable
    /// rather than a page of drawn glyphs. What the render could not represent
    /// is reported in [`PdfOutput::report`] — a lossy render says so.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] if a media part cannot be read or a limit is
    /// exceeded.
    #[cfg(feature = "pdf")]
    pub fn render_pdf(&self, options: &RenderOptions) -> Result<PdfOutput> {
        let pages =
            strict_ooxml_render_svg::place_pages(&self.document, options, Some(&self.package))?;
        strict_ooxml_render_pdf::render_with_source(&pages, options, Some(&self.package))
    }
}

/// Builds Stage-2 parse options from the core open options.
fn parse_options_from(options: &OpenOptions) -> ParseOptions {
    ParseOptions {
        conformance: options.conformance,
        limits: options.limits,
    }
}
