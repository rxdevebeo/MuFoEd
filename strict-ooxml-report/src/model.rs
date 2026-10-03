//! The report data model (`TZ-STRICT-OOXML-RUST.md` §11.1, schema v2).
//!
//! Every type here is a plain data carrier: [`serde`] derives render it exactly
//! as prescribed by the JSON Schema in `schema/support-report.schema.json`.
//! Field declaration order is the JSON field order, and all collections are
//! ordered `Vec`s, so serialization is deterministic (ADR-0005).

use std::fmt;

use serde::{Deserialize, Serialize};
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::ns::Conformance;

/// The report schema version emitted by this crate (`TZ` §11.1).
pub const SCHEMA_VERSION: &str = "2.0";

/// The standard identified by every report (`TZ` §11.1).
pub const STANDARD: &str = "ISO/IEC 29500-1:2008 Strict";

/// Reports the tool that produced a report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name, for example `strict-ooxml`.
    pub name: String,
    /// Tool version, for example `0.1.0`.
    pub version: String,
}

impl Tool {
    /// Creates a tool descriptor.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Conformance name used by the `conformance` block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConformanceName {
    /// Strict (`purl.oclc.org/ooxml/...`) markup.
    Strict,
    /// Transitional (`schemas.openxmlformats.org/...`) markup.
    Transitional,
    /// Both families observed in one package.
    Mixed,
    /// No conclusive signal.
    Unknown,
}

impl ConformanceName {
    /// Returns a stable, machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Transitional => "transitional",
            Self::Mixed => "mixed",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ConformanceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<Conformance> for ConformanceName {
    fn from(value: Conformance) -> Self {
        match value {
            Conformance::Strict => Self::Strict,
            Conformance::Transitional => Self::Transitional,
            Conformance::Mixed => Self::Mixed,
            Conformance::Unknown => Self::Unknown,
        }
    }
}

/// The `conformance` block: declared target, detected markup and whether
/// normalization was applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConformanceBlock {
    /// Conformance the consumer requested (the declared target standard).
    pub declared: ConformanceName,
    /// Conformance detected in the package.
    pub detected: ConformanceName,
    /// Whether a normalizer changed any part of the package (`was_normalized`).
    pub normalized: bool,
}

/// A location rendered as `"<part>:<line>:<column>"` (`TZ` §7.4, §11.1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Location(String);

impl Location {
    /// Creates a location from its string form.
    #[must_use]
    pub fn new(rendered: impl Into<String>) -> Self {
        Self(rendered.into())
    }

    /// Returns the rendered form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&SourceLocation> for Location {
    fn from(value: &SourceLocation) -> Self {
        Self(value.to_string())
    }
}

impl From<SourceLocation> for Location {
    fn from(value: SourceLocation) -> Self {
        Self(value.to_string())
    }
}

/// Status of one mechanism in the report (`TZ` §11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureStatus {
    /// Fully parsed into the model.
    ///
    /// Under the Stage-3 semantics (ADR-0005) the parser records a mechanism as
    /// `supported` only when it explicitly reports it; fully supported core
    /// markup is **not** enumerated, so this status is rare in production and
    /// `summary.supported` is normally `0`.
    Supported,
    /// Recognised, but only partially represented.
    Partial,
    /// Not represented; recorded for reporting.
    Unsupported,
    /// Deliberately ignored.
    Ignored,
    /// Report-assembly failure (reserved; unused in Stage 3).
    Error,
}

impl FeatureStatus {
    /// Returns a stable, machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
            Self::Ignored => "ignored",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for FeatureStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Severity of one mechanism (`TZ` §11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational.
    Info,
    /// A warning that does not block.
    Warning,
    /// A critical error.
    Error,
}

impl Severity {
    /// Returns a stable, machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Aggregated support of the whole document (`TZ` §11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverallStatus {
    /// Everything requested is supported.
    Supported,
    /// Some mechanisms are only partial.
    Partial,
    /// At least one mechanism is unsupported or errored.
    Unsupported,
}

impl OverallStatus {
    /// Returns a stable, machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
        }
    }
}

impl fmt::Display for OverallStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reported mechanism.
///
/// `features` is the list of mechanisms the parser **recorded** — normally the
/// ones that need attention (`partial`/`unsupported`/`ignored`), not a complete
/// coverage map of the document (ADR-0005).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    /// Feature identifier, for example `w:tbl` or `wp:anchor`.
    pub feature_id: String,
    /// Aggregated status.
    pub status: FeatureStatus,
    /// Severity derived from the status (ADR-0005).
    pub severity: Severity,
    /// Optional human-readable message (first occurrence).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Locations (`≥ 1` for partial/unsupported/error).
    pub locations: Vec<Location>,
    /// Number of occurrences recorded.
    pub count: u32,
}

/// Counts of features per status.
///
/// These are counters over the **recorded** [`Feature`]s (see the Stage-3
/// semantics in ADR-0005), not coverage percentages. In particular
/// `supported` counts only mechanisms the parser explicitly reported as
/// supported and is normally `0`; it does **not** mean "no markup is supported".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Number of `supported` features (explicitly recorded; usually `0`).
    pub supported: u32,
    /// Number of `partial` features.
    pub partial: u32,
    /// Number of `unsupported` features.
    pub unsupported: u32,
    /// Number of `ignored` features.
    pub ignored: u32,
    /// Number of `error` features.
    pub error: u32,
}

/// One applied normalization transform.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedTransform {
    /// Transform identifier, for example `T1.namespace`.
    pub transform_id: String,
    /// Number of applications.
    pub count: u32,
    /// Locations (may be empty).
    pub locations: Vec<Location>,
}

/// One loss recorded during normalization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loss {
    /// Transform identifier that dropped or degraded content.
    pub transform_id: String,
    /// Affected feature identifier.
    pub feature_id: String,
    /// Human-readable reason.
    pub reason: String,
    /// Severity of the loss.
    pub severity: Severity,
    /// Locations (may be empty).
    pub locations: Vec<Location>,
}

/// The `normalization` block (`TZ` §11.1). Empty when no normalizer ran.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizationBlock {
    /// Applied transforms.
    pub applied: Vec<AppliedTransform>,
    /// Recorded losses.
    pub losses: Vec<Loss>,
    /// Whether the Stage-10.10 invariants held.
    pub invariants_ok: bool,
}

impl Default for NormalizationBlock {
    fn default() -> Self {
        Self {
            applied: Vec::new(),
            losses: Vec::new(),
            invariants_ok: true,
        }
    }
}

/// A complete Feature Report (`TZ` §11).
///
/// The report is a **problem/attention view**, not a coverage map: `features`
/// lists the mechanisms the parser recorded and `overall_status` aggregates
/// them (ADR-0005). `overall_status == supported` therefore means "no recorded
/// limitation", not "every mechanism was verified as supported".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportReport {
    /// Report schema version (`"2.0"`).
    pub schema_version: String,
    /// Input file name or path.
    pub file: String,
    /// Standard the report was produced against.
    pub standard: String,
    /// Producing tool.
    pub tool: Tool,
    /// Conformance block.
    pub conformance: ConformanceBlock,
    /// Aggregated status of the **recorded** mechanisms (worst-of).
    ///
    /// `supported` means no `partial`/`unsupported`/`error` mechanism was
    /// recorded; it is not a completeness claim.
    pub overall_status: OverallStatus,
    /// Status counters over the recorded mechanisms (not coverage).
    pub summary: Summary,
    /// Recorded mechanisms, sorted by `feature_id` (then location).
    ///
    /// Normally the mechanisms needing attention (see [`Feature`]).
    pub features: Vec<Feature>,
    /// Normalization block (empty in Stage 3).
    pub normalization: NormalizationBlock,
}

impl SupportReport {
    /// Returns `true` if the report contains at least one `unsupported` or
    /// `error` blocker (`STAGE-3-TASK.md` §7.2).
    #[must_use]
    pub fn has_critical_problems(&self) -> bool {
        self.summary.unsupported > 0 || self.summary.error > 0
    }

    /// Serializes the report to deterministic JSON (`json::to_json`).
    #[must_use]
    pub fn to_json(&self) -> String {
        crate::json::to_json(self)
    }

    /// Renders the report for humans (`text::render`).
    #[must_use]
    pub fn to_text(&self) -> String {
        crate::text::render(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConformanceBlock, ConformanceName, Feature, FeatureStatus, Location, NormalizationBlock,
        OverallStatus, Severity, Summary, SupportReport, Tool, SCHEMA_VERSION, STANDARD,
    };
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::ns::Conformance;
    use strict_ooxml_core::part::PartId;

    fn location() -> SourceLocation {
        SourceLocation::new(PartId::new("/word/document.xml"), 3, 5, 10)
    }

    fn report() -> SupportReport {
        SupportReport {
            schema_version: SCHEMA_VERSION.to_owned(),
            file: "document.docx".to_owned(),
            standard: STANDARD.to_owned(),
            tool: Tool::new("strict-ooxml", "0.1.0"),
            conformance: ConformanceBlock {
                declared: ConformanceName::Strict,
                detected: ConformanceName::Transitional,
                normalized: false,
            },
            overall_status: OverallStatus::Partial,
            summary: Summary {
                partial: 1,
                ..Summary::default()
            },
            features: vec![Feature {
                feature_id: "w:drawing".to_owned(),
                status: FeatureStatus::Partial,
                severity: Severity::Warning,
                message: None,
                locations: vec![Location::from(&location())],
                count: 2,
            }],
            normalization: NormalizationBlock::default(),
        }
    }

    #[test]
    fn conformance_name_every_variant() {
        for (value, expected) in [
            (ConformanceName::Strict, "strict"),
            (ConformanceName::Transitional, "transitional"),
            (ConformanceName::Mixed, "mixed"),
            (ConformanceName::Unknown, "unknown"),
        ] {
            assert_eq!(value.as_str(), expected);
            assert_eq!(value.to_string(), expected);
        }
        assert_eq!(
            ConformanceName::from(Conformance::Strict),
            ConformanceName::Strict
        );
        assert_eq!(
            ConformanceName::from(Conformance::Transitional),
            ConformanceName::Transitional
        );
        assert_eq!(
            ConformanceName::from(Conformance::Mixed),
            ConformanceName::Mixed
        );
        assert_eq!(
            ConformanceName::from(Conformance::Unknown),
            ConformanceName::Unknown
        );
    }

    #[test]
    fn feature_status_every_variant() {
        assert_eq!(FeatureStatus::Supported.as_str(), "supported");
        assert_eq!(FeatureStatus::Partial.as_str(), "partial");
        assert_eq!(FeatureStatus::Unsupported.as_str(), "unsupported");
        assert_eq!(FeatureStatus::Ignored.as_str(), "ignored");
        assert_eq!(FeatureStatus::Error.as_str(), "error");
        assert_eq!(FeatureStatus::Error.to_string(), "error");
    }

    #[test]
    fn severity_every_variant_and_order() {
        assert_eq!(Severity::Info.as_str(), "info");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Error.to_string(), "error");
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn overall_status_every_variant() {
        assert_eq!(OverallStatus::Supported.as_str(), "supported");
        assert_eq!(OverallStatus::Partial.as_str(), "partial");
        assert_eq!(OverallStatus::Unsupported.as_str(), "unsupported");
        assert_eq!(OverallStatus::Unsupported.to_string(), "unsupported");
    }

    #[test]
    fn location_round_trips_and_renders() {
        let rendered = Location::new("/word/document.xml:3:5");
        assert_eq!(rendered.as_str(), "/word/document.xml:3:5");
        assert_eq!(rendered.to_string(), "/word/document.xml:3:5");
        assert_eq!(
            Location::from(location()).as_str(),
            "/word/document.xml:3:5"
        );
        assert_eq!(
            Location::from(&location()).as_str(),
            "/word/document.xml:3:5"
        );
    }

    #[test]
    fn report_helpers_and_serde() {
        let report = report();
        assert!(!report.has_critical_problems());
        assert!(report.to_json().contains("\"schema_version\": \"2.0\""));
        assert!(report.to_text().contains("Feature report"));

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("transitional"));
        let round_tripped: SupportReport = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, report);
    }

    #[test]
    fn critical_report_is_detected() {
        let mut report = report();
        report.summary.error = 1;
        assert!(report.has_critical_problems());
        report.summary.error = 0;
        report.summary.unsupported = 1;
        assert!(report.has_critical_problems());
    }

    #[test]
    fn normalization_default_is_empty_and_ok() {
        let block = NormalizationBlock::default();
        assert!(block.applied.is_empty());
        assert!(block.losses.is_empty());
        assert!(block.invariants_ok);
        let _ = NormalizationBlock::default().clone();
    }
}
