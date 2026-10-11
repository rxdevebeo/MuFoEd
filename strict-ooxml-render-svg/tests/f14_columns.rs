//! Section columns (`w:cols`) flow content across a page.
//!
//! Geometry for the fixtures: page 6000x4000 twips, left margin 1000, no other
//! margins, two equal columns with a 200-twip gap. The content box is
//! 1000..6000 twips, so the columns are 2400 twips each and start at 1000 and
//! 3600 twips = 66.67 and 240 px at 96 dpi.

#![allow(clippy::expect_used)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, without_font_faces};
use strict_ooxml_render_svg::{render, RenderOptions};

const SECT: &str = "<w:sectPr><w:pgSz w:w=\"6000\" w:h=\"4000\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"1000\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
<w:cols w:num=\"2\" w:space=\"200\"/></w:sectPr>";

fn render_pages(body: &str) -> Vec<String> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    render(&doc, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .map(|page| without_font_faces(&page.svg))
        .collect()
}

/// The `(first-x, text)` of every painted text fragment on a page.
fn texts(svg: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for part in svg.split("<text ").skip(1) {
        let x = part
            .split("x=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok());
        let text = part
            .split('>')
            .nth(1)
            .and_then(|rest| rest.split('<').next())
            .unwrap_or_default()
            .to_owned();
        if let Some(x) = x {
            out.push((x, text));
        }
    }
    out
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0
}

#[test]
fn column_break_moves_the_rest_to_the_next_column() {
    let body = format!(
        "<w:p><w:r><w:t>Alpha</w:t></w:r>\
<w:r><w:br w:type=\"column\"/></w:r>\
<w:r><w:t>Beta</w:t></w:r></w:p>{SECT}"
    );
    let pages = render_pages(&body);
    assert_eq!(pages.len(), 1, "a column break stays on the page");
    let found = texts(&pages[0]);
    let alpha = found
        .iter()
        .find(|(_, text)| text == "Alpha")
        .map(|(x, _)| *x);
    let beta = found
        .iter()
        .find(|(_, text)| text == "Beta")
        .map(|(x, _)| *x);
    let alpha = alpha.expect("Alpha is painted");
    let beta = beta.expect("Beta is painted");
    assert!(near(alpha, 66.67), "Alpha starts the first column: {alpha}");
    assert!(near(beta, 240.0), "Beta starts the second column: {beta}");
}

#[test]
fn overflowing_text_flows_into_the_second_column() {
    let words = "lorem ipsum dolor sit amet consectetur adipiscing elit ".repeat(10);
    let body = format!("<w:p><w:r><w:t>{words}</w:t></w:r></w:p>{SECT}");
    let pages = render_pages(&body);
    let found = texts(&pages[0]);
    let first = found.iter().filter(|(x, _)| near(*x, 66.67)).count();
    let second = found.iter().filter(|(x, _)| near(*x, 240.0)).count();
    assert!(first > 1, "lines start in the first column: {first}");
    assert!(
        second > 1,
        "the overflow fills the second column on the same page: {second}"
    );
}

#[test]
fn a_single_column_section_keeps_the_page_content_box() {
    let body = "<w:p><w:r><w:t>Alpha</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"6000\" w:h=\"4000\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"1000\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>";
    let pages = render_pages(body);
    let found = texts(&pages[0]);
    let alpha = found
        .iter()
        .find(|(_, text)| text == "Alpha")
        .map(|(x, _)| *x)
        .expect("Alpha is painted");
    assert!(
        near(alpha, 66.67),
        "the single-column fast path keeps the margin: {alpha}"
    );
}
