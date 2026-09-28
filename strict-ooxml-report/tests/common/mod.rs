//! Shared helpers for the `strict-ooxml-report` integration tests.
//!
//! The module builds synthetic [`SupportModel`]s, embeds the JSON Schema and
//! exposes the independent `jsonschema` validator (ADR-0005) plus a small
//! oracle that checks a report against the model it came from
//! (`STAGE-3-TASK.md` §8.2).

#![allow(dead_code)]

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_report::{build, FeatureStatus, OverallStatus, ReportInput, SupportReport, Tool};
use strict_ooxml_wml::model::support::{SupportModel, SupportStatus};

/// A deterministic location in `/word/document.xml`.
pub(crate) fn location(line: u32, column: u32) -> SourceLocation {
    SourceLocation::new(
        PartId::new("/word/document.xml"),
        line,
        column,
        u64::from(line) * 100,
    )
}

/// A deterministic location in a named part.
pub(crate) fn location_in(part: &str, line: u32, column: u32) -> SourceLocation {
    SourceLocation::new(PartId::new(part.to_owned()), line, column, 0)
}

/// A model with a supported, a partial, an unsupported and an ignored feature.
pub(crate) fn mixed_model() -> SupportModel {
    let mut support = SupportModel::new();
    support.record(
        "w:tbl",
        SupportStatus::Supported,
        None,
        Some(location(1, 1)),
    );
    support.record(
        "w:drawing",
        SupportStatus::Partial,
        Some("only inline images are supported".to_owned()),
        Some(location(4, 7)),
    );
    support.record(
        "wp:anchor",
        SupportStatus::Unsupported,
        Some("anchored drawings are not positioned".to_owned()),
        Some(location(9, 3)),
    );
    support.record(
        "m:oMath",
        SupportStatus::Unsupported,
        None,
        Some(location(12, 1)),
    );
    support.record("w:comment", SupportStatus::Ignored, None, None);
    support
}

/// A model that is fully supported (no blockers).
pub(crate) fn clean_model() -> SupportModel {
    let mut support = SupportModel::new();
    support.record("w:p", SupportStatus::Supported, None, Some(location(1, 1)));
    support.record("w:r", SupportStatus::Supported, None, Some(location(1, 4)));
    support.record("w:t", SupportStatus::Supported, None, Some(location(1, 7)));
    support
}

/// Builds a report for a model with the fallback location configured.
pub(crate) fn build_report(file: &str, support: &SupportModel) -> SupportReport {
    build(
        ReportInput::new(file, support)
            .tool(Tool::new("strict-ooxml-test", "0.1.0"))
            .fallback_location(Some(location(1, 1).into())),
    )
}

/// Parses the embedded schema into a JSON value.
pub(crate) fn schema_value() -> serde_json::Value {
    serde_json::from_str(strict_ooxml_report::schema_json()).expect("schema is valid JSON")
}

/// Validates a JSON value against the embedded schema with `jsonschema`.
pub(crate) fn validate(value: &serde_json::Value) -> Result<(), String> {
    let schema = schema_value();
    jsonschema::validate(&schema, value).map_err(|error| error.to_string())
}

/// Validates a report by serializing it to JSON first.
pub(crate) fn validate_report(report: &SupportReport) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(&strict_ooxml_report::json::to_json(report)).expect("valid JSON");
    validate(&value)
}

/// Oracle: checks every Stage-3 §5 invariant of `report` against `model`.
///
/// Returns `Err` with a human-readable reason on the first violation. The
/// schema-level oracle is separate ([`validate_report`]); this one cross-checks
/// the report against the input `SupportModel` so both cannot be wrong in the
/// same way.
pub(crate) fn oracle_check(report: &SupportReport, model: &SupportModel) -> Result<(), String> {
    let mut expected: Vec<&str> = model.iter().map(|use_| use_.feature_id.as_ref()).collect();
    expected.sort_unstable();
    let mut actual: Vec<&str> = report
        .features
        .iter()
        .map(|feature| feature.feature_id.as_str())
        .collect();
    actual.sort_unstable();
    if expected != actual {
        return Err(format!(
            "feature_id set mismatch: {actual:?} != {expected:?}"
        ));
    }

    for feature in &report.features {
        let source = model
            .get(&feature.feature_id)
            .ok_or_else(|| format!("extra feature {}", feature.feature_id))?;
        if feature.count != source.count {
            return Err(format!(
                "count mismatch for {}: {} != {}",
                feature.feature_id, feature.count, source.count
            ));
        }
        if feature.message != source.message {
            return Err(format!("message mismatch for {}", feature.feature_id));
        }
        match feature.status {
            FeatureStatus::Partial | FeatureStatus::Unsupported | FeatureStatus::Error => {
                if feature.locations.is_empty() {
                    return Err(format!("{} has no location", feature.feature_id));
                }
            }
            FeatureStatus::Supported | FeatureStatus::Ignored => {}
        }
        // Locations are derived from the model's own locations, in order.
        for (index, location) in source.locations.iter().enumerate() {
            let rendered = location.to_string();
            if feature
                .locations
                .get(index)
                .map(strict_ooxml_report::Location::as_str)
                != Some(rendered.as_str())
            {
                return Err(format!(
                    "location mismatch for {} at {index}",
                    feature.feature_id
                ));
            }
        }
    }

    // Summary must equal the actual per-status counts.
    let mut summary = strict_ooxml_report::Summary::default();
    for feature in &report.features {
        match feature.status {
            FeatureStatus::Supported => summary.supported += 1,
            FeatureStatus::Partial => summary.partial += 1,
            FeatureStatus::Unsupported => summary.unsupported += 1,
            FeatureStatus::Ignored => summary.ignored += 1,
            FeatureStatus::Error => summary.error += 1,
        }
    }
    if summary != report.summary {
        return Err(format!("summary mismatch: {:?}", report.summary));
    }

    // overall_status must be the worst-of aggregate.
    let expected_overall = if report.summary.unsupported > 0 || report.summary.error > 0 {
        OverallStatus::Unsupported
    } else if report.summary.partial > 0 {
        OverallStatus::Partial
    } else {
        OverallStatus::Supported
    };
    if report.overall_status != expected_overall {
        return Err(format!(
            "overall_status mismatch: {}",
            report.overall_status
        ));
    }

    // Features must be sorted by feature_id (then location).
    let mut sorted = report.features.clone();
    sorted.sort_by(|left, right| {
        left.feature_id
            .cmp(&right.feature_id)
            .then_with(|| left.locations.cmp(&right.locations))
    });
    if sorted != report.features {
        return Err("features are not sorted".to_owned());
    }

    Ok(())
}
