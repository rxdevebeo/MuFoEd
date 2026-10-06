//! A17: a vertical merge keeps a fragment on the next page.

#![allow(clippy::expect_used, clippy::format_push_string)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, RenderOptions};

fn pages() -> Vec<String> {
    let row = |merge: &str, text: &str| {
        format!(
            "<w:tr><w:trPr><w:trHeight w:val=\"600\"/><w:cantSplit/></w:trPr>\
<w:tc><w:tcPr><w:vMerge w:val=\"{merge}\"/>\
<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"CCCCCC\"/>\
<w:tcW w:w=\"2500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:tcW w:w=\"2500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>side</w:t></w:r></w:p></w:tc></w:tr>"
        )
    };
    let body = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2500\"/><w:gridCol w:w=\"2500\"/></w:tblGrid>\
{}{}{}\
</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"1440\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>",
        row("restart", "merged"),
        row("continue", ""),
        row("continue", "")
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .map(|page| page.svg)
        .collect()
}

fn grey_height(svg: &str) -> f64 {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let page: f64 = document
        .root_element()
        .attribute("height")
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    document
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "rect"
                && node.attribute("fill") == Some("#cccccc")
        })
        .map(|node| {
            let y: f64 = node.attribute("y").unwrap_or("0").parse().unwrap_or(0.0);
            let height: f64 = node
                .attribute("height")
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0);
            assert!(
                y + height <= page + 0.25,
                "grey fragment {y}+{height} leaves the page {page}"
            );
            height
        })
        .sum()
}

#[test]
fn f17_vmerge_has_continuation_on_next_page() {
    let pages = pages();
    assert!(pages.len() >= 2, "the third row must move to the next page");
    let first = grey_height(&pages[0]);
    let second = grey_height(&pages[1]);
    assert!(
        (first - 80.0).abs() <= 0.25,
        "page 1 fragment {first} px, want 80"
    );
    assert!(
        (second - 40.0).abs() <= 0.25,
        "page 2 fragment {second} px, want 40"
    );
    assert!(pages[0].contains("merged"));
    assert!(!pages[1].contains("merged"), "restart text is not repeated");
    assert!(pages[0].contains("side") && pages[1].contains("side"));
}

#[test]
fn f17_gridspan_vmerge_and_header_repeat() {
    let body = "<w:tbl>\
<w:tblPr><w:tblW w:w=\"5000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2500\"/><w:gridCol w:w=\"2500\"/></w:tblGrid>\
<w:tr><w:trPr><w:tblHeader/></w:trPr>\
<w:tc><w:p><w:r><w:t>head</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>col</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/><w:vMerge w:val=\"restart\"/></w:tcPr>\
<w:p><w:r><w:t>span</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"1440\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    let pages: Vec<_> = render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .map(|page| page.svg)
        .collect();
    assert!(pages.iter().any(|svg| svg.contains("span")), "{pages:?}");
    assert!(pages.iter().any(|svg| svg.contains("head")), "{pages:?}");
}
