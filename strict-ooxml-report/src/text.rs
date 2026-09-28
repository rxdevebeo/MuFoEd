//! Human-readable rendering of a [`SupportReport`] (`STAGE-3-TASK.md` §7.1).
//!
//! The layout is deterministic and English-only (`TZ` §14); problems
//! (`partial`/`unsupported`/`error`) are listed separately at the end.

use std::fmt::Write as _;

use crate::model::{Feature, FeatureStatus, SupportReport};

/// Renders a report for humans, terminated by `\n`.
#[must_use]
pub fn render(report: &SupportReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Feature report");
    let _ = writeln!(out, "{:<12} {}", "file:", report.file);
    let _ = writeln!(out, "{:<12} {}", "standard:", report.standard);
    let _ = writeln!(
        out,
        "{:<12} {} {}",
        "tool:", report.tool.name, report.tool.version
    );
    let _ = writeln!(
        out,
        "{:<12} declared={} detected={} normalized={}",
        "conformance:",
        report.conformance.declared,
        report.conformance.detected,
        report.conformance.normalized
    );
    let _ = writeln!(out, "{:<12} {}", "overall:", report.overall_status);
    let _ = writeln!(
        out,
        "{:<12} supported={} partial={} unsupported={} ignored={} error={}",
        "summary:",
        report.summary.supported,
        report.summary.partial,
        report.summary.unsupported,
        report.summary.ignored,
        report.summary.error,
    );
    // Feature semantics (ADR-0005): `features` lists the mechanisms the parser
    // recorded — normally the ones needing attention — not a full coverage map.
    let _ = writeln!(
        out,
        "{:<12} only mechanisms with a notable status are recorded; fully supported markup is not enumerated",
        "note:",
    );

    let _ = writeln!(out, "{:<12} {}", "features:", report.features.len());
    for feature in &report.features {
        let _ = writeln!(out, "  {}", format_feature(feature));
    }

    let problems: Vec<&Feature> = report
        .features
        .iter()
        .filter(|feature| is_problem(feature.status))
        .collect();
    let _ = writeln!(out, "{:<12} {}", "problems:", problems.len());
    for feature in problems {
        let _ = writeln!(out, "  {}", format_problem(feature));
    }
    out
}

/// Renders the one-line feature-list entry.
fn format_feature(feature: &Feature) -> String {
    format!(
        "[{}] {} {} x{}{}",
        feature.severity,
        feature.status,
        feature.feature_id,
        feature.count,
        format_locations(feature),
    )
}

/// Renders the one-line problem entry.
fn format_problem(feature: &Feature) -> String {
    let message = feature
        .message
        .as_deref()
        .map_or_else(String::new, |message| format!(" — {message}"));
    format!(
        "[{}] {} ({}){}{}",
        feature.severity,
        feature.feature_id,
        feature.status,
        format_locations(feature),
        message,
    )
}

/// Renders `" @ loc1, loc2"` for a feature's locations, or an empty string.
fn format_locations(feature: &Feature) -> String {
    if feature.locations.is_empty() {
        return String::new();
    }
    let joined = feature
        .locations
        .iter()
        .map(crate::model::Location::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    format!(" @ {joined}")
}

/// Returns `true` for statuses reported as problems.
const fn is_problem(status: FeatureStatus) -> bool {
    matches!(
        status,
        FeatureStatus::Partial | FeatureStatus::Unsupported | FeatureStatus::Error
    )
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::build::{build, ReportInput};
    use crate::model::{FeatureStatus, Location};
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_wml::model::support::{SupportModel, SupportStatus};

    #[test]
    fn renders_empty_report() {
        let support = SupportModel::new();
        let report = build(ReportInput::new("empty.docx", &support));
        let text = render(&report);
        assert!(text.contains("Feature report"));
        assert!(text.contains("file:        empty.docx"));
        assert!(text.contains("features:    0"));
        assert!(text.contains("problems:    0"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn renders_features_and_problems() {
        let mut support = SupportModel::new();
        let at = |line: u32| SourceLocation::new(PartId::new("/word/document.xml"), line, 3, 0);
        support.record("w:tbl", SupportStatus::Supported, None, Some(at(1)));
        support.record(
            "wp:anchor",
            SupportStatus::Unsupported,
            Some("anchored drawings are not positioned".to_owned()),
            Some(at(7)),
        );
        let report = build(
            ReportInput::new("doc.docx", &support)
                .fallback_location(Some(Location::new("/word/document.xml:1:1"))),
        );
        let text = render(&report);
        assert!(text.contains("[info] supported w:tbl x1"));
        assert!(text.contains("[warning] unsupported wp:anchor x1 @ /word/document.xml:7:3"));
        assert!(text.contains("problems:    1"));
        assert!(text.contains("— anchored drawings are not positioned"));
    }

    #[test]
    fn problem_without_location_renders_cleanly() {
        let mut support = SupportModel::new();
        support.record("w:x", SupportStatus::Partial, None, None);
        let report = build(ReportInput::new("doc.docx", &support));
        let text = render(&report);
        assert!(text.contains("[warning] partial w:x x1"));
        assert!(text.contains("[warning] w:x (partial)"));
        assert!(!text.contains(" @ "));
        assert_eq!(report.features[0].status, FeatureStatus::Partial);
    }
}
