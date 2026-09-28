//! Determinism: byte-identical output across runs and insertion orders
//! (`STAGE-3-TASK.md` §6, acceptance criterion 5).

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{build_report, location, mixed_model};
use strict_ooxml_report::{json, text};
use strict_ooxml_wml::model::support::{SupportModel, SupportStatus};

#[test]
fn two_runs_produce_identical_bytes() {
    let model = mixed_model();
    let first = build_report("document.docx", &model);
    let second = build_report("document.docx", &model);
    assert_eq!(json::to_json(&first), json::to_json(&second));
    assert_eq!(text::render(&first), text::render(&second));
}

#[test]
fn output_is_independent_of_insertion_order() {
    let mut forward = SupportModel::new();
    forward.record("aa", SupportStatus::Supported, None, Some(location(3, 1)));
    forward.record(
        "zz",
        SupportStatus::Unsupported,
        Some("x".to_owned()),
        Some(location(1, 1)),
    );
    forward.record("mm", SupportStatus::Partial, None, Some(location(2, 1)));

    let mut reverse = SupportModel::new();
    reverse.record("mm", SupportStatus::Partial, None, Some(location(2, 1)));
    reverse.record(
        "zz",
        SupportStatus::Unsupported,
        Some("x".to_owned()),
        Some(location(1, 1)),
    );
    reverse.record("aa", SupportStatus::Supported, None, Some(location(3, 1)));

    assert_eq!(
        json::to_json(&build_report("f.docx", &forward)),
        json::to_json(&build_report("f.docx", &reverse)),
    );
}

#[test]
fn location_order_within_a_feature_is_stable() {
    let mut model = SupportModel::new();
    for line in [5, 1, 3] {
        model.record(
            "w:tab",
            SupportStatus::Partial,
            None,
            Some(location(line, 1)),
        );
    }
    let report = build_report("f.docx", &model);
    let lines: Vec<&str> = report.features[0]
        .locations
        .iter()
        .map(strict_ooxml_report::Location::as_str)
        .collect();
    assert_eq!(
        lines,
        [
            "/word/document.xml:5:1",
            "/word/document.xml:1:1",
            "/word/document.xml:3:1",
        ]
    );
    assert_eq!(
        json::to_json(&report),
        json::to_json(&build_report("f.docx", &model))
    );
}
