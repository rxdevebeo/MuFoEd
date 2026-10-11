//! Paragraph `w:pBdr` edges and cell diagonals reach the paint output.

#![allow(clippy::expect_used)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, RenderOptions};

fn lines(svg: &str) -> usize {
    let parsed = roxmltree::Document::parse(svg).expect("svg");
    parsed
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "line")
        .count()
}

fn render_body(body: &str) -> String {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    render(&doc, &RenderOptions::default()).expect("render")[0]
        .svg
        .clone()
}

#[test]
fn paragraph_borders_are_stroked() {
    let svg = render_body(
        "<w:p><w:pPr><w:pBdr>\
<w:top w:val=\"single\" w:sz=\"12\" w:space=\"0\" w:color=\"FF0000\"/>\
<w:bottom w:val=\"double\" w:sz=\"8\" w:space=\"0\" w:color=\"0000FF\"/>\
<w:left w:val=\"single\" w:sz=\"12\" w:space=\"0\" w:color=\"FF0000\"/>\
<w:right w:val=\"single\" w:sz=\"12\" w:space=\"0\" w:color=\"FF0000\"/>\
</w:pBdr></w:pPr><w:r><w:t>boxed line</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"3600\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>",
    );
    assert!(
        svg.contains("boxed") && svg.contains("line"),
        "the text is kept: {svg}"
    );
    assert!(
        lines(&svg) >= 4,
        "the four `w:pBdr` edges are stroked: {svg}"
    );
    assert!(svg.contains("#ff0000") || svg.contains("#FF0000"));
}

#[test]
fn cell_diagonals_are_stroked() {
    let svg = render_body(
        "<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/><w:tcBorders>\
<w:top w:val=\"single\" w:sz=\"8\" w:color=\"000000\"/>\
<w:left w:val=\"single\" w:sz=\"8\" w:color=\"000000\"/>\
<w:bottom w:val=\"single\" w:sz=\"8\" w:color=\"000000\"/>\
<w:right w:val=\"single\" w:sz=\"8\" w:color=\"000000\"/>\
<w:tl2br w:val=\"single\" w:sz=\"8\" w:color=\"FF0000\"/>\
<w:tr2bl w:val=\"single\" w:sz=\"8\" w:color=\"0000FF\"/>\
</w:tcBorders></w:tcPr><w:p><w:r><w:t>X</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:sectPr><w:pgSz w:w=\"3600\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>",
    );
    // Four cell edges plus two diagonals.
    assert!(
        lines(&svg) >= 6,
        "the cell edges and both diagonals are stroked: {svg}"
    );
}
