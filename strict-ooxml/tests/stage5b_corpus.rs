//! End-to-end test over the Stage-5B Strict fixture
//! `strict-ooxml-core/tests/strict/strict-stage5b.docx` (STAGE-5B §7.3).
//!
//! The fixture exercises the 5B drawing subsystem: anchored shape, group,
//! text box, anchored picture and page borders.

#![allow(clippy::expect_used, missing_docs)]

use std::path::PathBuf;

use strict_ooxml::model::SupportStatus;
use strict_ooxml::{OpenOptions, StrictDocument};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-stage5b.docx")
}

#[test]
fn parses_stage5b_drawing_mechanisms() {
    let document =
        StrictDocument::open_path(fixture(), &OpenOptions::default()).expect("open fixture");
    let model = document.document();
    for id in [
        "wp:anchor",
        "wps:wsp",
        "wpg:wgp",
        "w:txbxContent",
        "w:pgBorders",
    ] {
        let entry = model.support.get(id).unwrap_or_else(|| panic!("no {id}"));
        assert_eq!(
            entry.status,
            SupportStatus::Supported,
            "{id} is not supported"
        );
    }
    assert!(model
        .sections
        .last()
        .expect("section")
        .properties
        .page_borders
        .is_some());
}

#[cfg(feature = "svg")]
#[test]
fn renders_stage5b_floating_objects() {
    let document =
        StrictDocument::open_path(fixture(), &OpenOptions::default()).expect("open fixture");
    let pages = document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .expect("render");
    assert_eq!(pages.len(), 1, "expected a one-page fixture");
    let svg = &pages[0].svg;
    assert!(svg.contains("<path "), "no shape paths");
    assert!(svg.contains("#4472c4"), "shape fill missing");
    assert!(
        svg.contains("#70ad47") && svg.contains("#ed7d31"),
        "group fills missing"
    );
    assert!(svg.contains("Floating"), "text box content missing");
    assert!(svg.contains("<image "), "anchored picture missing");
    assert!(svg.contains("#1f3864"), "page border missing");
}
