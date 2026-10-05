//! A14 / F12: paragraph shading is painted behind every line of the paragraph.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, RenderOptions};

#[test]
fn f12_paragraph_shading_reaches_output() {
    let body = "<w:p><w:pPr><w:shd w:val=\"clear\" w:fill=\"F7F7F7\"/></w:pPr>\
<w:r><w:t>alpha beta gamma delta epsilon zeta eta theta iota kappa</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"3600\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>";
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let pages = render(&doc, &RenderOptions::default()).expect("render");
    let svg = &pages[0].svg;
    assert!(svg.contains("alpha"), "the paragraph text is kept");
    let document = roxmltree::Document::parse(svg).expect("svg");
    let shades: Vec<(f64, f64, f64, f64)> = document
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "rect"
                && node.attribute("fill") == Some("#f7f7f7")
        })
        .map(|node| {
            let num = |name| {
                node.attribute(name)
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            (num("x"), num("y"), num("width"), num("height"))
        })
        .collect();
    assert!(
        !shades.is_empty(),
        "the grey paragraph background is painted"
    );
    let texts: Vec<(f64, f64)> = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .map(|node| {
            let num = |name| {
                node.attribute(name)
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            (num("x"), num("y"))
        })
        .collect();
    assert!(
        texts.len() > 1,
        "the paragraph wraps onto more than one line"
    );
    let page_width = 3600.0 / 15.0;
    for (x, y, width, _) in &shades {
        assert!(
            (*x).abs() <= 0.25,
            "shading starts at the content edge, x={x}"
        );
        assert!(
            (width - page_width).abs() <= 0.25,
            "shading spans the content box, width={width}"
        );
        let _ = y;
    }
    for (x, baseline) in &texts {
        let covered = shades
            .iter()
            .any(|(_, y, _, height)| *baseline + 0.25 >= *y && *baseline <= *y + *height + 0.25);
        assert!(covered, "text at ({x}, {baseline}) is outside the shading");
    }
}
