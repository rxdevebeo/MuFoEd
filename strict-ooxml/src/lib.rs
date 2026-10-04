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
#[cfg(feature = "edit")]
mod editing;
#[cfg(feature = "edit")]
pub use editing::DocumentEditor;
#[cfg(feature = "edit")]
pub use strict_ooxml_edit as edit;

#[cfg(feature = "convert")]
pub use strict_ooxml_convert::{
    convert as convert_pdf, ConversionReport, ConvertError, Converted, Mode, ParagraphRules,
    PdfOptions,
};
pub use strict_ooxml_core::error::StrictError;
pub use strict_ooxml_core::limits::ResourceLimits;
/// How much a normalization removal matters.
///
/// Named apart from the Feature Report's own `Severity`, which grades support
/// of a construct; this one grades what removing it cost.
pub use strict_ooxml_core::normalize::Severity as LossSeverity;
pub use strict_ooxml_core::normalize::{
    DirectionPolicy, ExpansionLimit, InvariantMode, LossRecord, McePolicy, NormalizationReport,
    NormalizerOptions, TransitionalNormalizer, VmlFallback,
};
pub use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions};
#[cfg(feature = "convert")]
pub use strict_ooxml_pdf::{PdfDocument, PdfError, PdfLimits, PdfPage};
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
pub use strict_ooxml_write::package::{MediaBag, NoSource, RelationshipInfo, Source};
#[cfg(feature = "write")]
pub use strict_ooxml_write::{
    dropped_count, verify_no_silent_loss, write_package, WriteOptions, WriteOutput, WriteReport,
};

/// An opened Strict document: the OPC package plus its parsed model.
pub struct StrictDocument {
    package: Package,
    document: Document,
    file: String,
    support_dirty: bool,
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
            support_dirty: false,
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

    /// Returns the document model for mutation.
    ///
    /// The model is owned and every field on it is public (ADR-0004), so
    /// editing is a matter of reaching it - the alternative, "edit a copy and
    /// swap it in", leaves two sources of truth and a way for them to disagree.
    ///
    /// What this accessor does **not** do is make the result coherent: a
    /// mutated document may carry a style id nothing defines, a `w:sectPr`
    /// whose counterpart in `Document::sections` was not updated, or a
    /// `para_id` duplicated by a clone. Coherence is `validate`'s job
    /// (`STAGE-10-TASK.md` E16, phase 10B) and nothing here claims otherwise.
    ///
    /// The `SupportModel` does not follow the edit: it is filled by the parser
    /// and merges additively, so a feature an edit removed still appears in a
    /// report built from it. Recomputing it is part of the same phase.
    pub fn document_mut(&mut self) -> &mut Document {
        self.support_dirty = true;
        &mut self.document
    }
    /// Whether support metadata describes an earlier model revision.
    #[must_use]
    pub fn support_is_stale(&self) -> bool {
        self.support_dirty
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

    /// Builds the Feature Report for this document.
    ///
    /// Available with the `report` feature (enabled by default). The report
    /// records a `Strict` declared target, the package's detected conformance,
    /// and `normalized` from [`Package::was_normalized`]. When a normalizer
    /// was installed, the `normalization` block carries the Loss Report
    /// (AUD-31).
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
        let mut support = self.support().clone();
        if self.support_dirty {
            support.record(
                "edit.support-stale",
                model::SupportStatus::Partial,
                Some(
                    "Document changed; refresh support through the checked editing save pipeline"
                        .to_owned(),
                ),
                None,
            );
        }
        let mut input = ReportInput::new(&self.file, &support)
            .tool(Tool::new("strict-ooxml", env!("CARGO_PKG_VERSION")))
            .conformance(
                Conformance::Strict,
                self.package.conformance(),
                self.package.was_normalized(),
            )
            .fallback_location(Some(fallback));
        if let Some(mut report) = self.package.normalization_report() {
            report.conformance_detected = Some(self.package.conformance());
            input = input.normalization_report(&report);
        }
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
        // AUD-52: `usize::MAX + 1` must not wrap; reject with a clear render error.
        let page = index
            .checked_add(1)
            .ok_or_else(|| StrictError::Render("page index out of range".to_owned()))?;
        let selection = PageSelection::Range {
            start: page,
            end: page,
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
///
/// No conformance policy to carry across any more (AUD-23 / ADR-0016):
/// `Package::open_*` already weighed it through `opc::policy::decide`.
fn parse_options_from(options: &OpenOptions) -> ParseOptions {
    ParseOptions {
        limits: options.limits,
    }
}
