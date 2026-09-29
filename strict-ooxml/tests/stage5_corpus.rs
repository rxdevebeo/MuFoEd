//! End-to-end test over the Stage-5 Strict fixture
//! `strict-ooxml-core/tests/strict/strict-stage5.docx` (S5.12).
//!
//! The fixture exercises every 5A subsystem: headers/footers, footnotes and
//! endnotes, computed fields, a complex table (gridSpan/vMerge/tblHeader),
//! a theme and multi-level numbering.

#![allow(clippy::expect_used, missing_docs)]

use std::path::PathBuf;

use strict_ooxml::model::props::HeaderFooterKind;
use strict_ooxml::{OpenOptions, StrictDocument};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-stage5.docx")
}

#[test]
fn parses_all_stage5_subsystems() {
    let document =
        StrictDocument::open_path(fixture(), &OpenOptions::default()).expect("open fixture");
    let model = document.document();

    assert!(!model.body.blocks.is_empty(), "empty body");
    assert!(model.source.theme.is_some(), "theme part not discovered");
    assert!(model.theme.is_some(), "theme not parsed");
    assert!(
        !model.headers_footers.is_empty(),
        "no header/footer parts parsed"
    );
    assert!(!model.footnotes.is_empty(), "footnotes not parsed");
    assert!(!model.endnotes.is_empty(), "endnotes not parsed");
    assert!(!model.numbering.is_empty(), "no numbering definitions");

    let section = model.sections.last().expect("a section");
    let kinds: Vec<HeaderFooterKind> = section
        .properties
        .headers
        .iter()
        .map(|reference| reference.kind)
        .collect();
    assert!(kinds.contains(&HeaderFooterKind::Default));
    assert!(kinds.contains(&HeaderFooterKind::Even));
    assert!(section.properties.title_page);
}

#[test]
fn report_lists_stage5_mechanisms_without_unsupported() {
    let document =
        StrictDocument::open_path(fixture(), &OpenOptions::default()).expect("open fixture");
    let report = document.support_report();
    assert!(
        !report.has_critical_problems(),
        "no unsupported/error mechanism is expected: {:?}",
        report
            .features
            .iter()
            .filter(|feature| feature.status.as_str() == "unsupported")
            .map(|feature| &feature.feature_id)
            .collect::<Vec<_>>()
    );
    for id in [
        "w:headerReference",
        "w:footerReference",
        "w:footnoteReference",
        "w:endnoteReference",
        "w:fldSimple",
        "w:gridSpan",
        "w:vMerge",
        "w:tblHeader",
        "a:theme",
        "w:numbering",
    ] {
        assert!(
            report
                .features
                .iter()
                .any(|feature| feature.feature_id == id),
            "report is missing mechanism {id}"
        );
    }
}

#[cfg(feature = "svg")]
#[test]
fn renders_two_valid_pages_with_stage5_content() {
    let document =
        StrictDocument::open_path(fixture(), &OpenOptions::default()).expect("open fixture");
    let pages = document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .expect("render");
    assert_eq!(pages.len(), 2, "expected a two-page fixture");

    let first = &pages[0].svg;
    let second = &pages[1].svg;
    for svg in [first, second] {
        assert!(svg.contains("<svg "), "invalid SVG");
        assert!(!svg.contains("NaN"), "NaN in SVG");
    }
    // Text is wrapped into word tokens, so assert on individual words.
    assert!(
        first.contains("Default"),
        "default header missing on page 1"
    );
    assert!(second.contains("Even"), "even header missing on page 2");
    assert!(first.contains("Footnote"), "footnote missing");
    assert!(second.contains("Endnote"), "endnote missing");
    assert!(first.contains("Merged"), "table missing");
    assert!(first.contains(">1.</text>"), "level-0 marker missing");
    assert!(first.contains(">1.1</text>"), "level-1 marker missing");
    assert!(
        first.contains("Page") && first.contains("of"),
        "fields missing"
    );
}
