//! Independent oracle: report invariants cross-checked against the input
//! `SupportModel` (`STAGE-3-TASK.md` §5, §8.2).
//!
//! The oracle function itself is exercised against deliberately corrupted
//! reports so that its failure modes are proven (self-check).

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{build_report, clean_model, mixed_model, oracle_check};
use strict_ooxml_report::{FeatureStatus, Severity};

#[test]
fn oracle_passes_on_generated_reports() {
    for model in [clean_model(), mixed_model()] {
        let report = build_report("document.docx", &model);
        if let Err(error) = oracle_check(&report, &model) {
            panic!("oracle failed: {error}");
        }
    }
}

#[test]
fn unsupported_mechanism_is_reported_with_severity_and_location() {
    let model = mixed_model();
    let report = build_report("document.docx", &model);
    let anchor = report
        .features
        .iter()
        .find(|feature| feature.feature_id == "wp:anchor")
        .expect("wp:anchor reported");
    assert_eq!(anchor.status, FeatureStatus::Unsupported);
    assert_eq!(anchor.severity, Severity::Warning);
    assert!(!anchor.locations.is_empty());
    assert_eq!(anchor.locations[0].as_str(), "/word/document.xml:9:3");
}

#[test]
fn oracle_detects_a_missing_location() {
    let model = mixed_model();
    let mut report = build_report("document.docx", &model);
    let partial = report
        .features
        .iter_mut()
        .find(|feature| feature.status == FeatureStatus::Partial)
        .expect("partial feature");
    partial.locations.clear();
    assert!(oracle_check(&report, &model).is_err());
}

#[test]
fn oracle_detects_a_wrong_count() {
    let model = mixed_model();
    let mut report = build_report("document.docx", &model);
    report.features[0].count += 1;
    assert!(oracle_check(&report, &model).is_err());
}

#[test]
fn oracle_detects_an_extra_feature() {
    let model = mixed_model();
    let mut report = build_report("document.docx", &model);
    let mut extra = report.features[0].clone();
    extra.feature_id = String::from("w:extra");
    report.features.push(extra);
    assert!(oracle_check(&report, &model).is_err());
}

#[test]
fn oracle_detects_a_broken_summary() {
    let model = mixed_model();
    let mut report = build_report("document.docx", &model);
    report.summary.supported += 1;
    assert!(oracle_check(&report, &model).is_err());
}
