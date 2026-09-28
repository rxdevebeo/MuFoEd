//! Independent JSON-Schema validation of every report (`STAGE-3-TASK.md` §8.1,
//! §8.2): the schema is an external contract validated with `jsonschema`, not a
//! structure copied from the code.
//!
//! A corrupted report must be **rejected** (oracle self-check).

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{build_report, clean_model, mixed_model, validate, validate_report};

#[test]
fn generated_reports_are_schema_valid() {
    for model in [clean_model(), mixed_model()] {
        let report = build_report("document.docx", &model);
        if let Err(error) = validate_report(&report) {
            panic!("generated report is invalid: {error}");
        }
    }
}

#[test]
fn empty_report_is_schema_valid() {
    let report = build_report(
        "empty.docx",
        &strict_ooxml_wml::model::support::SupportModel::new(),
    );
    validate_report(&report).expect("empty report valid");
}

#[test]
fn golden_files_are_schema_valid() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let entries = std::fs::read_dir(&dir).expect("golden dir");
    let mut checked = 0;
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read golden");
        let value: serde_json::Value = serde_json::from_str(&text).expect("golden json");
        validate(&value).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        checked += 1;
    }
    assert!(checked >= 2, "expected at least two golden JSON files");
}

#[test]
fn corrupted_reports_are_rejected() {
    // A partial feature with no location violates the conditional schema.
    let mut report = build_report("document.docx", &mixed_model());
    let partial = report
        .features
        .iter_mut()
        .find(|feature| feature.status == strict_ooxml_report::FeatureStatus::Partial)
        .expect("partial feature");
    partial.locations.clear();
    assert!(
        validate_report(&report).is_err(),
        "location-less partial must be rejected"
    );

    // A status/severity mismatch violates the severity table. The first
    // feature is `m:oMath` (`unsupported`), which must be `warning`.
    let mut report = build_report("document.docx", &mixed_model());
    report.features[0].severity = strict_ooxml_report::Severity::Error;
    assert!(
        validate_report(&report).is_err(),
        "severity mismatch must be rejected"
    );

    // An unknown field violates `additionalProperties: false`.
    let mut value: serde_json::Value =
        serde_json::from_str(&strict_ooxml_report::json::to_json(&report)).expect("json");
    value["surprise"] = serde_json::json!(true);
    assert!(validate(&value).is_err(), "unknown field must be rejected");
}
