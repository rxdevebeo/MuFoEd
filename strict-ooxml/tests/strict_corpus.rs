//! End-to-end test over the real Strict corpus in
//! `strict-ooxml-core/tests/strict/` (REWORK-WML-1, finding C-3).
//!
//! A real Word-produced Strict document places the declaration and the root
//! element on separate lines. It must open, produce a Feature Report and render
//! at least one page.

#![allow(clippy::expect_used, missing_docs)]

use std::path::PathBuf;

use strict_ooxml::{OpenOptions, StrictDocument};

fn strict_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-profile.docx")
}

#[test]
fn opens_real_strict_profile() {
    let path = strict_fixture();
    assert!(path.is_file(), "missing corpus fixture {}", path.display());
    let document =
        StrictDocument::open_path(&path, &OpenOptions::default()).expect("open strict fixture");
    assert!(
        !document.document().body.blocks.is_empty(),
        "real Strict document parsed to an empty body"
    );
    assert!(document.file().ends_with("strict-profile.docx"));
}

#[cfg(feature = "report")]
#[test]
fn produces_valid_feature_report() {
    let path = strict_fixture();
    let document =
        StrictDocument::open_path(&path, &OpenOptions::default()).expect("open strict fixture");
    let report = document.support_report();
    assert_eq!(report.file, path.to_string_lossy());
    assert!(!report.features.is_empty(), "report has no features");
    let json = document.report_json();
    assert!(json.contains("\"overall_status\""));
    assert!(document.report_text().contains("Feature report"));
}

#[cfg(feature = "svg")]
#[test]
fn renders_at_least_one_page() {
    let path = strict_fixture();
    let document =
        StrictDocument::open_path(&path, &OpenOptions::default()).expect("open strict fixture");
    let pages = document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .expect("render strict fixture");
    assert!(!pages.is_empty(), "expected at least one rendered page");
    for page in &pages {
        assert!(page.svg.contains("<svg "), "invalid SVG: {}", page.svg);
    }
}
