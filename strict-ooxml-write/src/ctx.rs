//! Per-part writing context: the loss report, the relationship-id maps and the
//! counters a part needs.

use std::collections::BTreeMap;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::normalize::report::{LossRecord, NormalizationReport, Severity};
use strict_ooxml_core::part::PartId;

/// Stable id of the writer's own loss class.
///
/// It is namespaced with `W` so it can never collide with a normalization
/// stage id (`T1`…`T8`) in a report that merges both.
pub const WRITE_LOSS_ID: &str = "W1.unserializable";

/// Stable id of the writer's "written only in part" class.
pub const WRITE_PARTIAL_ID: &str = "W2.partial";

/// State shared by every part writer.
#[derive(Debug)]
pub struct Ctx<'a> {
    report: &'a mut NormalizationReport,
    next_doc_pr_id: u32,
    /// Old hyperlink relationship id → the id this write emits.
    hyperlinks: BTreeMap<String, String>,
    /// Media part id → the relationship id this write emits.
    media: BTreeMap<String, String>,
    /// Header/footer part id → the relationship id this write emits.
    header_footers: BTreeMap<String, String>,
}

impl<'a> Ctx<'a> {
    /// Creates a context writing into `report`.
    #[must_use]
    pub fn new(report: &'a mut NormalizationReport) -> Self {
        Self {
            report,
            // `wp:docPr/@id` must be unique within a part and non-zero; 1 is the
            // first value Word uses.
            next_doc_pr_id: 1,
            hyperlinks: BTreeMap::new(),
            media: BTreeMap::new(),
            header_footers: BTreeMap::new(),
        }
    }

    /// Records the relationship-id maps a package write computed.
    #[must_use]
    pub fn with_relationships(
        mut self,
        hyperlinks: BTreeMap<String, String>,
        media: BTreeMap<String, String>,
        header_footers: BTreeMap<String, String>,
    ) -> Self {
        self.hyperlinks = hyperlinks;
        self.media = media;
        self.header_footers = header_footers;
        self
    }

    /// Returns the relationship id to write for a hyperlink that referenced
    /// `old_id` in the parsed document.
    #[must_use]
    pub fn hyperlink_rel(&self, old_id: &str) -> Option<&str> {
        self.hyperlinks.get(old_id).map(String::as_str)
    }

    /// Returns the relationship id to write for a picture that referenced
    /// `part`.
    #[must_use]
    pub fn media_rel(&self, part: Option<&PartId>) -> Option<&str> {
        part.and_then(|part| self.media.get(part.as_str()))
            .map(String::as_str)
    }

    /// Returns the relationship id to write for a header/footer part.
    #[must_use]
    pub fn header_footer_rel(&self, part: &PartId) -> Option<&str> {
        self.header_footers.get(part.as_str()).map(String::as_str)
    }

    /// Returns the mutable report.
    pub fn report(&mut self) -> &mut NormalizationReport {
        self.report
    }

    /// Allocates the next `wp:docPr/@id`.
    pub fn next_doc_pr_id(&mut self) -> u32 {
        let id = self.next_doc_pr_id;
        self.next_doc_pr_id += 1;
        id
    }

    /// Records a construct that could not be written at all.
    pub fn report_unsupported(
        &mut self,
        feature_id: &str,
        reason: &str,
        location: &SourceLocation,
    ) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_LOSS_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
        self.report.count_reported_removal(1);
    }

    /// Records a construct that was written only in part.
    pub fn report_partial(&mut self, feature_id: &str, reason: &str, location: &SourceLocation) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_PARTIAL_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Ignorable,
            locations: vec![location.clone()],
        });
    }

    /// Records an informational rewrite that changes no meaning.
    pub fn report_info(&mut self, feature_id: &str, reason: &str, location: &SourceLocation) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_PARTIAL_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Info,
            locations: vec![location.clone()],
        });
    }

    /// Consumes the context, returning the report it wrote into.
    #[must_use]
    pub fn into_report(self) -> NormalizationReport {
        self.report.clone()
    }
}
