//! AUD-31: Loss Report appears in the Feature Report for Transitional packages.
//!
//! Skips when the local (gitignored) `Manual.docx` corpus file is absent.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Arc;

use common::validate_report;
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions};
use strict_ooxml_report::{build, json, ReportInput, Tool};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn manual_docx() -> Option<std::path::PathBuf> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx/Manual.docx");
    path.is_file().then_some(path)
}

fn report_for(path: &Path) -> strict_ooxml_report::SupportReport {
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone());
    let package = strict_ooxml_core::opc::Package::open_path(path, &options).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let mut norm = normalizer.report();
    norm.conformance_detected = Some(package.conformance());
    build(
        ReportInput::new(path.to_string_lossy().as_ref(), document.support())
            .tool(Tool::new("strict-ooxml-test", "0.1.0"))
            .conformance(
                Conformance::Strict,
                package.conformance(),
                package.was_normalized(),
            )
            .normalization_report(&norm),
    )
}

#[test]
fn manual_docx_transitional_report_carries_the_loss_report() {
    let Some(path) = manual_docx() else {
        eprintln!("skipping: Manual.docx not present");
        return;
    };
    let report = report_for(&path);
    assert!(report.conformance.normalized);
    assert_eq!(
        report.conformance.detected,
        strict_ooxml_report::ConformanceName::Transitional
    );
    assert!(
        !report.normalization.applied.is_empty(),
        "applied transforms must be present"
    );
    assert!(
        !report.normalization.losses.is_empty(),
        "losses must be present"
    );
    validate_report(&report).expect("schema-valid");

    let first = json::to_json(&report);
    let second = json::to_json(&report_for(&path));
    assert_eq!(first, second, "two runs must produce equal JSON bytes");
}

#[test]
fn strict_sample_has_empty_normalization_block() {
    // A Strict sample from the versioned corpus: SC-1 — no normalization.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict01-sdk.docx");
    if !path.is_file() {
        eprintln!("skipping: strict01-sdk.docx not present");
        return;
    }
    let options = OpenOptions::default();
    let package = strict_ooxml_core::opc::Package::open_path(&path, &options).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let report = build(
        ReportInput::new("strict01-sdk.docx", document.support())
            .tool(Tool::new("strict-ooxml-test", "0.1.0"))
            .conformance(
                Conformance::Strict,
                package.conformance(),
                package.was_normalized(),
            ),
    );
    assert!(!report.conformance.normalized);
    assert!(report.normalization.applied.is_empty());
    assert!(report.normalization.losses.is_empty());
    assert!(report.normalization.invariants_ok);
    validate_report(&report).expect("schema-valid");
}
