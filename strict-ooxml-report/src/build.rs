//! Report assembly: `(SupportModel, Conformance, metadata) → SupportReport`.
//!
//! The build is a pure, infallible function. It sorts features by `feature_id`
//! (then by location) and derives `summary`, `overall_status` and `severity`,
//! which are checked by the invariant/oracle tests (`STAGE-3-TASK.md` §5, §8.2).

use strict_ooxml_core::normalize::{
    NormalizationReport as CoreNormalizationReport, Severity as CoreSeverity,
};
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_wml::model::support::{FeatureUse, SupportModel, SupportStatus};

use crate::model::{
    AppliedTransform, ConformanceBlock, ConformanceName, Feature, FeatureStatus, Location, Loss,
    NormalizationBlock, Severity, SupportReport, Tool, SCHEMA_VERSION, STANDARD,
};
use crate::severity;

/// How many locations to keep on an applied transform or loss entry (AUD-31).
const MAX_LOCATIONS: usize = 16;

/// Inputs to [`build`].
///
/// Use [`ReportInput::new`] for the defaults and the builder methods to override
/// conformance, tool, fallback location or normalization state.
pub struct ReportInput<'a> {
    /// Input file name or path.
    pub file: &'a str,
    /// Producing tool.
    pub tool: Tool,
    /// Conformance the consumer requested (declared target).
    pub declared: Conformance,
    /// Conformance detected in the package.
    pub detected: Conformance,
    /// Whether a normalizer changed any part of the package.
    pub normalized: bool,
    /// The support model to convert.
    pub support: &'a SupportModel,
    /// Location used when a partial/unsupported feature has none recorded.
    ///
    /// The parser always records a location for such features, but this
    /// guarantees the report invariant (`≥ 1` location) for hand-built inputs
    /// too. `None` leaves such a feature without a location (and therefore
    /// schema-invalid), which the oracle tests use as a negative control.
    pub fallback_location: Option<Location>,
    /// Populated Loss Report from Stage-6 normalization (AUD-31).
    pub normalization: NormalizationBlock,
}

impl<'a> ReportInput<'a> {
    /// Creates an input with default metadata: declared/detected `Strict`,
    /// tool `strict-ooxml` at this crate's version and no normalization.
    #[must_use]
    pub fn new(file: &'a str, support: &'a SupportModel) -> Self {
        Self {
            file,
            tool: Tool::new("strict-ooxml", env!("CARGO_PKG_VERSION")),
            declared: Conformance::Strict,
            detected: Conformance::Strict,
            normalized: false,
            support,
            fallback_location: None,
            normalization: NormalizationBlock::default(),
        }
    }

    /// Sets the producing tool.
    #[must_use]
    pub fn tool(mut self, tool: Tool) -> Self {
        self.tool = tool;
        self
    }

    /// Sets the declared/detected conformance and normalization flag.
    #[must_use]
    pub fn conformance(
        mut self,
        declared: Conformance,
        detected: Conformance,
        normalized: bool,
    ) -> Self {
        self.declared = declared;
        self.detected = detected;
        self.normalized = normalized;
        self
    }

    /// Sets the fallback location used for location-less blockers.
    #[must_use]
    pub fn fallback_location(mut self, location: Option<Location>) -> Self {
        self.fallback_location = location;
        self
    }

    /// Sets the normalization block directly.
    #[must_use]
    pub fn normalization(mut self, block: NormalizationBlock) -> Self {
        self.normalization = block;
        self
    }

    /// Fills the normalization block from a core [`NormalizationReport`] (AUD-31).
    #[must_use]
    pub fn normalization_report(mut self, report: &CoreNormalizationReport) -> Self {
        self.normalization = normalization_block_from(report);
        self
    }
}

/// Builds a [`SupportReport`] from a [`ReportInput`].
#[must_use]
pub fn build(input: ReportInput<'_>) -> SupportReport {
    let fallback = input.fallback_location.as_ref();
    let mut features: Vec<Feature> = input
        .support
        .iter()
        .map(|use_| feature_from(use_, fallback))
        .collect();
    // Error-severity losses surface as `normalization.<feature_id>` features
    // so `overall_status` and the §11.3 "error has a location" rule see them.
    for loss in &input.normalization.losses {
        if loss.severity != Severity::Error {
            continue;
        }
        let mut locations = loss.locations.clone();
        if locations.is_empty() {
            if let Some(fallback) = fallback {
                locations.push(fallback.clone());
            }
        }
        features.push(Feature {
            feature_id: format!("normalization.{}", loss.feature_id),
            status: FeatureStatus::Error,
            severity: Severity::Error,
            message: Some(loss.reason.clone()),
            locations,
            count: 1,
        });
    }
    features.sort_by(|left, right| {
        left.feature_id
            .cmp(&right.feature_id)
            .then_with(|| left.locations.cmp(&right.locations))
    });

    let summary = severity::summarize(&features);
    let overall_status = severity::overall_status(&features);

    SupportReport {
        schema_version: SCHEMA_VERSION.to_owned(),
        file: input.file.to_owned(),
        standard: STANDARD.to_owned(),
        tool: input.tool,
        conformance: ConformanceBlock {
            declared: ConformanceName::from(input.declared),
            detected: ConformanceName::from(input.detected),
            normalized: input.normalized,
        },
        overall_status,
        summary,
        features,
        normalization: input.normalization,
    }
}

/// Converts a core [`NormalizationReport`] into the Feature Report block.
#[must_use]
pub fn normalization_block_from(report: &CoreNormalizationReport) -> NormalizationBlock {
    let applied: Vec<AppliedTransform> = report
        .applied()
        .into_iter()
        .filter(|record| record.count > 0)
        .map(|record| AppliedTransform {
            transform_id: record.id.to_owned(),
            count: record.count,
            // TransformRecord does not store locations today (AUD-31).
            locations: Vec::new(),
        })
        .collect();
    let losses: Vec<Loss> = report
        .losses()
        .into_iter()
        .map(|loss| Loss {
            transform_id: loss.transform_id.to_owned(),
            feature_id: loss.feature_id,
            reason: loss.reason,
            severity: map_loss_severity(loss.severity),
            locations: loss
                .locations
                .iter()
                .take(MAX_LOCATIONS)
                .map(Location::from)
                .collect(),
        })
        .collect();
    let invariants_ok = report.fatal.is_none() && report.verify_no_silent_loss().is_ok();
    NormalizationBlock {
        applied,
        losses,
        invariants_ok,
    }
}

fn map_loss_severity(severity: CoreSeverity) -> Severity {
    match severity {
        CoreSeverity::Info | CoreSeverity::Ignorable => Severity::Info,
        CoreSeverity::Lossy => Severity::Warning,
        CoreSeverity::Error => Severity::Error,
    }
}

/// Converts one aggregated [`FeatureUse`] into a report [`Feature`].
fn feature_from(use_: &FeatureUse, fallback: Option<&Location>) -> Feature {
    let status = feature_status(use_.status);
    let mut locations: Vec<Location> = use_.locations.iter().map(Location::from).collect();
    if locations.is_empty() && status_requires_location(status) {
        if let Some(fallback) = fallback {
            locations.push(fallback.clone());
        }
    }
    Feature {
        feature_id: use_.feature_id.to_string(),
        status,
        severity: severity::severity_for(status),
        message: use_.message.clone(),
        locations,
        count: use_.count.max(1),
    }
}

/// Maps the Stage-2 support status onto the report status vocabulary.
const fn feature_status(status: SupportStatus) -> FeatureStatus {
    match status {
        SupportStatus::Supported => FeatureStatus::Supported,
        SupportStatus::Partial => FeatureStatus::Partial,
        SupportStatus::Unsupported => FeatureStatus::Unsupported,
        SupportStatus::Ignored => FeatureStatus::Ignored,
    }
}

/// Returns `true` for statuses that require at least one location.
const fn status_requires_location(status: FeatureStatus) -> bool {
    matches!(
        status,
        FeatureStatus::Partial | FeatureStatus::Unsupported | FeatureStatus::Error
    )
}

#[cfg(test)]
mod tests {
    use super::{build, ReportInput};
    use crate::model::{ConformanceName, FeatureStatus, Location, OverallStatus, Severity, Tool};
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::ns::Conformance;
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_wml::model::support::{SupportModel, SupportStatus};

    fn location(line: u32) -> SourceLocation {
        SourceLocation::new(PartId::new("/word/document.xml"), line, 1, u64::from(line))
    }

    #[test]
    fn input_defaults_and_builders() {
        let support = SupportModel::new();
        let input = ReportInput::new("document.docx", &support)
            .tool(Tool::new("tool", "9.9"))
            .conformance(Conformance::Strict, Conformance::Transitional, true)
            .fallback_location(Some(Location::new("/word/document.xml:1:1")));
        assert_eq!(input.file, "document.docx");
        assert_eq!(input.tool.name, "tool");
        assert_eq!(input.declared, Conformance::Strict);
        assert_eq!(input.detected, Conformance::Transitional);
        assert!(input.normalized);
        assert!(input.fallback_location.is_some());
        assert_eq!(input.support.len(), 0);

        let report = build(input);
        assert_eq!(report.file, "document.docx");
        assert_eq!(report.tool.version, "9.9");
        assert_eq!(report.conformance.declared, ConformanceName::Strict);
        assert_eq!(report.conformance.detected, ConformanceName::Transitional);
        assert!(report.conformance.normalized);
        assert_eq!(report.standard, crate::model::STANDARD);
        assert!(report.features.is_empty());
        assert_eq!(report.overall_status, OverallStatus::Supported);
    }

    #[test]
    fn build_sorts_features_and_derives_severity() {
        let mut support = SupportModel::new();
        support.record("w:zebra", SupportStatus::Supported, None, Some(location(9)));
        support.record(
            "w:alpha",
            SupportStatus::Unsupported,
            Some("not represented".to_owned()),
            Some(location(2)),
        );
        support.record("w:mid", SupportStatus::Partial, None, Some(location(5)));
        support.record("w:ignored", SupportStatus::Ignored, None, None);

        let report = build(ReportInput::new("f.docx", &support));
        let ids: Vec<&str> = report
            .features
            .iter()
            .map(|feature| feature.feature_id.as_str())
            .collect();
        assert_eq!(ids, ["w:alpha", "w:ignored", "w:mid", "w:zebra"]);

        let alpha = &report.features[0];
        assert_eq!(alpha.status, FeatureStatus::Unsupported);
        assert_eq!(alpha.severity, Severity::Warning);
        assert_eq!(alpha.message.as_deref(), Some("not represented"));
        assert_eq!(alpha.locations.len(), 1);
        assert_eq!(alpha.count, 1);

        assert_eq!(report.features[1].status, FeatureStatus::Ignored);
        assert_eq!(report.features[1].severity, Severity::Info);
        assert_eq!(report.summary.supported, 1);
        assert_eq!(report.summary.partial, 1);
        assert_eq!(report.summary.unsupported, 1);
        assert_eq!(report.summary.ignored, 1);
        assert_eq!(report.summary.error, 0);
        assert_eq!(report.overall_status, OverallStatus::Unsupported);
    }

    #[test]
    fn fallback_location_fills_locationless_blockers() {
        let mut support = SupportModel::new();
        support.record("wp:anchor", SupportStatus::Unsupported, None, None);
        let report = build(
            ReportInput::new("f.docx", &support)
                .fallback_location(Some(Location::new("/word/document.xml:1:1"))),
        );
        assert_eq!(report.features[0].locations.len(), 1);
        assert_eq!(
            report.features[0].locations[0].as_str(),
            "/word/document.xml:1:1"
        );
    }

    #[test]
    fn no_fallback_leaves_blockers_unlocated() {
        let mut support = SupportModel::new();
        support.record("wp:anchor", SupportStatus::Unsupported, None, None);
        let report = build(ReportInput::new("f.docx", &support));
        assert!(report.features[0].locations.is_empty());
    }

    #[test]
    fn multiple_locations_are_carried_over() {
        let mut support = SupportModel::new();
        support.record("w:tab", SupportStatus::Partial, None, Some(location(1)));
        support.record("w:tab", SupportStatus::Partial, None, Some(location(4)));
        let report = build(ReportInput::new("f.docx", &support));
        assert_eq!(report.features[0].count, 2);
        assert_eq!(report.features[0].locations.len(), 2);
        assert_eq!(
            report.features[0].locations[0].as_str(),
            "/word/document.xml:1:1"
        );
        assert_eq!(
            report.features[0].locations[1].as_str(),
            "/word/document.xml:4:1"
        );
    }
}
