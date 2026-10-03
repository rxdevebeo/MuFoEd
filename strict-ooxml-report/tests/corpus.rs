//! Corpus run over `strict-ooxml-core/tests/samples/` (`STAGE-3-TASK.md` §8.1).
//!
//! Strict documents that parse yield a schema-valid, oracle-consistent report;
//! Transitional packages are refused (ADR-0005) and therefore produce no
//! report. Damaged/unexpected inputs are counted but never fail the test.
//!
//! AUD-23 / ADR-0016 moved the Transitional refusal from the WML parser
//! (`parse_document`'s own gate on `StrictError::TransitionalNotSupported`)
//! to `Package::open_path` itself: `Permissive` without a `RawNormalizer` now
//! refuses to open Transitional or Mixed content at all, rather than opening
//! it and relying on a second gate downstream to reject it. There is only one
//! decision point left, so this corpus run now expects the refusal there.

#![allow(clippy::expect_used)]

mod common;

use std::path::Path;

use common::{build_report, oracle_check, validate_report};
use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_wml::{parse_document, ParseOptions};

#[test]
fn corpus_reports_are_consistent() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/samples");
    if !dir.is_dir() {
        eprintln!("corpus directory not present; skipped");
        return;
    }

    let permissive = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    let mut strict = 0u32;
    let mut transitional_rejected_at_open = 0u32;
    let mut other = 0u32;
    for entry in std::fs::read_dir(&dir).expect("read corpus directory") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sample.docx")
            .to_owned();
        let package = match Package::open_path(&path, &permissive) {
            Ok(package) => package,
            Err(StrictError::Unsupported(_)) => {
                // `Permissive` with no normalizer refuses Transitional and
                // Mixed content before any part is read (`opc::policy`).
                transitional_rejected_at_open += 1;
                continue;
            }
            Err(error) => {
                eprintln!("{name}: open failed: {error}");
                other += 1;
                continue;
            }
        };
        match parse_document(&package, &ParseOptions::default()) {
            Ok(document) => {
                let report = build_report(&name, document.support());
                if let Err(error) = oracle_check(&report, document.support()) {
                    panic!("{name}: oracle failed: {error}");
                }
                if let Err(error) = validate_report(&report) {
                    panic!("{name}: schema invalid: {error}");
                }
                strict += 1;
            }
            Err(error) => {
                eprintln!("{name}: model failed: {error}");
                other += 1;
            }
        }
    }

    eprintln!(
        "corpus: strict={strict} transitional_rejected_at_open={transitional_rejected_at_open} other={other}"
    );
    assert!(
        strict + transitional_rejected_at_open + other > 0,
        "expected at least one sample"
    );
    assert!(
        transitional_rejected_at_open > 0,
        "the Stage-1 corpus is Transitional; the open-time refusal path must be exercised"
    );
}
