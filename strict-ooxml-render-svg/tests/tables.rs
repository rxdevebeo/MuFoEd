//! Complex-table rendering tests (STAGE-5 S5.7–S5.8): gridSpan, vMerge,
//! nesting, page breaks and `w:tblHeader` repetition.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::unreadable_literal
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, Page, RenderOptions};

/// Builds a Strict package with the given body and renders it.
fn build(body: &str) -> Vec<Page> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default()).expect("render")
}

/// Returns every `<rect>`'s `(fill, height)`.
fn rects(svg: &str) -> Vec<(String, f64)> {
    let document = roxmltree::Document::parse(svg).expect("valid svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "rect")
        .map(|node| {
            let fill = node.attribute("fill").unwrap_or_default().to_owned();
            let height = node
                .attribute("height")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.0);
            (fill, height)
        })
        .collect()
}

#[test]
fn grid_span_renders_a_single_wide_cell() {
    let body = "<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"3\"/></w:tcPr><w:p><w:r><w:t>Wide</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>";
    let pages = build(body);
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    assert!(svg.contains("Wide") && svg.contains(">A<") && svg.contains(">C<"));
}

#[test]
fn vertical_merge_spans_the_merged_region() {
    let body = "<w:tbl><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid>\
<w:tr><w:trPr><w:trHeight w:val=\"400\" w:hRule=\"atLeast\"/></w:trPr>\
<w:tc><w:tcPr><w:vMerge w:val=\"restart\"/><w:shd w:val=\"clear\" w:fill=\"FF0000\"/></w:tcPr>\
<w:p><w:r><w:t>Merged</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:trPr><w:trHeight w:val=\"400\" w:hRule=\"atLeast\"/></w:trPr>\
<w:tc><w:tcPr><w:vMerge w:val=\"continue\"/></w:tcPr><w:p/></w:tc></w:tr>\
</w:tbl>";
    let pages = build(body);
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    assert!(svg.contains("Merged"));
    // The restart cell's fill spans both rows (a single row is far shorter).
    let merged = rects(svg)
        .into_iter()
        .filter(|(fill, _)| fill == "#ff0000")
        .map(|(_, height)| height)
        .fold(0.0_f64, f64::max);
    assert!(
        merged > 50.0,
        "merged region height {merged} should span two rows"
    );
}

#[test]
fn nested_table_renders() {
    let body = "<w:tbl><w:tblGrid><w:gridCol w:w=\"4000\"/></w:tblGrid>\
<w:tr><w:tc><w:p><w:r><w:t>Outer</w:t></w:r></w:p>\
<w:tbl><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>Inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
</w:tc></w:tr></w:tbl>";
    let pages = build(body);
    assert_eq!(pages.len(), 1);
    assert!(pages[0].svg.contains("Inner"));
    assert!(pages[0].svg.contains("Outer"));
}

#[test]
fn table_breaks_across_pages() {
    let mut rows = String::new();
    for index in 0..20 {
        rows.push_str(&format!(
            "<w:tr><w:tc><w:p><w:r><w:t>row{index}</w:t></w:r></w:p></w:tc></w:tr>"
        ));
    }
    let body = format!(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"6000\"/></w:tblGrid>{rows}</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"4000\"/>\
<w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\"/></w:sectPr>"
    );
    let pages = build(&body);
    assert!(pages.len() >= 2, "expected a multi-page table");
    assert!(pages.last().is_some_and(|page| page.svg.contains("row19")));
}

#[test]
fn header_row_repeats_on_each_page() {
    let mut rows = String::new();
    for index in 0..12 {
        rows.push_str(&format!(
            "<w:tr><w:tc><w:p><w:r><w:t>data{index}</w:t></w:r></w:p></w:tc></w:tr>"
        ));
    }
    let body = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"4000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"4000\"/></w:tblGrid>\
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>HEADER</w:t></w:r></w:p></w:tc></w:tr>\
{rows}</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"4000\"/>\
<w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\"/></w:sectPr>"
    );
    let pages = build(&body);
    assert!(pages.len() >= 2, "expected the table to span pages");
    let header_pages = pages
        .iter()
        .filter(|page| page.svg.contains("HEADER"))
        .count();
    assert!(
        header_pages >= 2,
        "expected the header row to repeat, got {header_pages} page(s)"
    );
}
