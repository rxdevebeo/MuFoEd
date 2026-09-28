//! Corpus run over `strict-ooxml-core/tests/samples/` (`STAGE-3-TASK.md` §8.1).
//!
//! Strict documents that parse yield a schema-valid, oracle-consistent report;
//! Transitional packages are opened permissively but **refused** by the parser
//! and therefore produce no report (ADR-0005). Damaged/unexpected inputs are
//! counted but never fail the test.

#![allow(clippy::expect_used)]

mod common;

use std::path::Path;

use common::{build_report, oracle_check, validate_report};
use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::ns::Conformance;
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
    let mut transitional = 0u32;
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
        let Ok(package) = Package::open_path(&path, &permissive) else {
            other += 1;
            continue;
        };
        let result = parse_document(&package, &ParseOptions::default());
        match (package.conformance(), result) {
            (Conformance::Transitional, Err(StrictError::TransitionalNotSupported { .. })) => {
                transitional += 1;
            }
            (_, Ok(document)) => {
                let report = build_report(&name, document.support());
                if let Err(error) = oracle_check(&report, document.support()) {
                    panic!("{name}: oracle failed: {error}");
                }
                if let Err(error) = validate_report(&report) {
                    panic!("{name}: schema invalid: {error}");
                }
                strict += 1;
            }
            (_, Err(error)) => {
                eprintln!("{name}: model failed: {error}");
                other += 1;
            }
        }
    }

    eprintln!("corpus: strict={strict} transitional={transitional} other={other}");
    assert!(
        strict + transitional + other > 0,
        "expected at least one sample"
    );
    assert!(
        transitional > 0,
        "the Stage-1 corpus is Transitional; the refusal path must be exercised"
    );
}
