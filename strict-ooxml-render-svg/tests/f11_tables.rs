//! A12 / F11: a zero preferred width uses the grid, not a 1 px table.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, RenderOptions};

#[test]
fn f11_zero_table_width_uses_grid() {
    let body = "<w:tbl>\
<w:tblPr><w:tblW w:w=\"0\" w:type=\"dxa\"/>\
<w:tblBorders><w:top w:val=\"nil\"/><w:left w:val=\"nil\"/><w:bottom w:val=\"nil\"/><w:right w:val=\"nil\"/><w:insideH w:val=\"nil\"/><w:insideV w:val=\"nil\"/></w:tblBorders>\
</w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"4000\"/><w:gridCol w:w=\"5355\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:shd w:val=\"clear\" w:fill=\"FF0000\"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:shd w:val=\"clear\" w:fill=\"00FF00\"/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>\
<w:sectPr><w:pgSz w:w=\"14400\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>";
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let pages = render(&doc, &RenderOptions::default()).expect("render");
    let svg = &pages[0].svg;
    let document = roxmltree::Document::parse(svg).expect("svg");
    let mut widths: Vec<f64> = document
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "rect"
                && node.attribute("fill").is_some_and(|fill| fill != "#ffffff")
        })
        .filter_map(|node| node.attribute("width").and_then(|value| value.parse().ok()))
        .collect();
    widths.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let sum: f64 = widths.iter().sum();
    let expected = 9355.0 / 15.0;
    assert!(
        (sum - expected).abs() <= 0.25,
        "grid 9355 twips is {expected} px, cells summed to {sum} ({widths:?})"
    );
    assert!(
        (widths[0] / 4000.0 - widths[1] / 5355.0).abs() < 1e-6,
        "columns keep the grid proportions: {widths:?}"
    );
}
