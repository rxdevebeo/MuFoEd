//! Golden JSON and text reports on synthetic Strict documents
//! (`STAGE-3-TASK.md` §8.1).
//!
//! Purpose: lock the **serialization format** (field order, spacing, the
//! `note:` line), not production coverage. The synthetic models include
//! `supported`/`ignored` features so every status the model can serialize is
//! exercised; under the Stage-3 semantics (ADR-0005, acceptance finding F1/F5)
//! the parser records only mechanisms needing attention, so a production
//! report normally has `summary.supported == 0`. A real Strict example
//! (unsupported `w:altChunk`) is covered by `strict-ooxml/tests/open.rs`.
//!
//! Regenerate with `UPDATE_GOLDEN=1 cargo test -p strict-ooxml-report --test golden`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{build_report, clean_model, mixed_model};
use std::path::PathBuf;
use strict_ooxml_report::{json, text};

fn golden_path(name: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("golden dir")).expect("create dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden {}", path.display()));
    assert_eq!(actual, expected, "golden {} changed", path.display());
}

#[test]
fn clean_document_golden() {
    let report = build_report("clean.docx", &clean_model());
    assert_golden("clean.json", &json::to_json(&report));
    assert_golden("clean.txt", &text::render(&report));
}

#[test]
fn mixed_document_golden() {
    let report = build_report("mixed.docx", &mixed_model());
    assert_golden("mixed.json", &json::to_json(&report));
    assert_golden("mixed.txt", &text::render(&report));
}
