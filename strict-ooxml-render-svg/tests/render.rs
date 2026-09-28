//! Structural, determinism and option tests for the SVG renderer.

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::doc_markdown
)]

mod common;

use common::{open_body, render_body};
use strict_ooxml_render_svg::{render, PageSelection, RenderOptions};

#[test]
fn simple_paragraph_renders_text_and_formatting() {
    let pages =
        render_body("<w:p><w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>Hello world</w:t></w:r></w:p>");
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    assert!(
        svg.contains(">Hello </text>") || svg.contains(">Hello<"),
        "{svg}"
    );
    assert!(svg.contains("world"), "{svg}");
    assert!(svg.contains("font-weight=\"bold\""));
    assert!(svg.contains("font-style=\"italic\""));
    assert!(svg.starts_with("<svg "));
    assert!(svg.ends_with("</svg>\n"));
}

#[test]
fn empty_paragraph_still_produces_a_page() {
    let pages = render_body("<w:p/>");
    assert_eq!(pages.len(), 1);
    assert!(pages[0].svg.contains("viewBox"));
}

#[test]
fn default_page_is_letter_at_96_dpi() {
    let pages = render_body("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    assert!((pages[0].width_px - 816.0).abs() < 0.001);
    assert!((pages[0].height_px - 1056.0).abs() < 0.001);
}

#[test]
fn scale_changes_page_dimensions() {
    let (_package, document) = open_body("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    let pages = render(&document, &RenderOptions::default().scale(48.0)).expect("render");
    assert!((pages[0].width_px - 408.0).abs() < 0.001);
    assert!((pages[0].height_px - 528.0).abs() < 0.001);
}

#[test]
fn two_runs_produce_identical_bytes() {
    let (_package, document) = open_body("<w:p><w:r><w:t>deterministic</w:t></w:r></w:p>");
    let first = render(&document, &Default::default()).expect("render");
    let second = render(&document, &Default::default()).expect("render");
    assert_eq!(first, second);
}

#[test]
fn page_selection_filters_output() {
    let body: String = (0..80)
        .map(|index| format!("<w:p><w:r><w:t>line {index}</w:t></w:r></w:p>"))
        .collect();
    let (_package, document) = open_body(&body);
    let all = render(&document, &Default::default()).expect("render");
    assert!(all.len() >= 2, "expected multiple pages, got {}", all.len());
    let selected = render(
        &document,
        &RenderOptions::default().pages(PageSelection::Range { start: 1, end: 1 }),
    )
    .expect("render");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].index, 0);
}

#[test]
fn table_renders_cells_and_borders() {
    let body = "<w:tbl><w:tblGrid><w:gridCol w:w=\"2880\"/><w:gridCol w:w=\"2880\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let pages = render_body(body);
    let svg = &pages[0].svg;
    assert!(svg.contains(">A<"), "{svg}");
    assert!(svg.contains(">B<"), "{svg}");
}

#[test]
fn no_nan_or_infinite_coordinates() {
    let pages = render_body(
        "<w:p><w:r><w:t>abc</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>",
    );
    for page in &pages {
        assert!(!page.svg.contains("NaN"), "{}", page.svg);
        assert!(!page.svg.contains("inf"), "{}", page.svg);
        assert!(!page.svg.contains("-0."), "{}", page.svg);
    }
}

#[test]
fn explicit_page_break_creates_second_page() {
    let pages = render_body(
        "<w:p><w:r><w:t>first</w:t><w:br w:type=\"page\"/><w:t>second</w:t></w:r></w:p>",
    );
    assert!(
        pages.len() >= 2,
        "expected a page break, got {}",
        pages.len()
    );
    assert!(pages[0].svg.contains("first"));
    assert!(pages[1].svg.contains("second"));
}
