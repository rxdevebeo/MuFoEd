//! Golden SVG snapshots on synthetic Strict documents
//! (`STAGE-4-TASK.md` §8.1).
//!
//! Regenerate with `UPDATE_GOLDEN=1 cargo test -p strict-ooxml-render-svg --test golden`.

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::doc_markdown
)]

mod common;

use common::{open_body, open_with_image, render_body};
use std::path::PathBuf;
use strict_ooxml_render_svg::{render_with_media, RenderOptions};

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
fn paragraph_golden() {
    let pages = render_body(
        "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val=\"32\"/></w:rPr><w:t>Centered bold</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>",
    );
    assert_golden("paragraphs.svg", &pages[0].svg);
}

#[test]
fn table_golden() {
    let pages = render_body(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"2880\"/><w:gridCol w:w=\"2880\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>Left</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Right</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    assert_golden("table.svg", &pages[0].svg);
}

#[test]
fn image_golden() {
    let (package, document) = open_with_image();
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    assert_golden("image.svg", &pages[0].svg);
    let _ = open_body("<w:p/>");
}
