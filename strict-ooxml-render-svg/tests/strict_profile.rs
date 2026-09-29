//! End-to-end Stage-5B check over the real Strict fixture
//! `strict-ooxml-core/tests/strict/strict-profile.docx` (`STAGE-5B-TASK.md`
//! §7.3, acceptance criterion 4).
//!
//! The file is a real Word-produced Strict document (MIT, `kklimuk/docx-cli`)
//! whose only text lives in an anchored text box; Stage 5B must render it.

#![allow(clippy::expect_used)]

use std::path::PathBuf;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{render_with_media, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-profile.docx")
}

#[test]
fn strict_profile_renders_anchored_text_box() {
    let path = fixture();
    assert!(path.is_file(), "missing fixture {}", path.display());
    let package = Package::open_path(&path, &OpenOptions::default()).expect("open package");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    assert_eq!(pages.len(), 2, "strict-profile is a two-page document");

    let svg: String = pages.iter().map(|page| page.svg.clone()).collect();
    assert!(
        svg.contains("Your") && svg.contains("text") && svg.contains("here"),
        "anchored text box content was not rendered"
    );
    assert!(svg.contains("<path "), "anchored shape path missing");
    // The anchor is positioned relative to the column at x = 48 px (margin).
    assert!(
        svg.contains("transform=\"translate(48 "),
        "anchored object position missing"
    );
}
