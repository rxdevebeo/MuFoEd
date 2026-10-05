//! A15: a header or footer that does not fit in the margin moves the body.

#![allow(clippy::expect_used, clippy::format_push_string)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{render, RenderOptions};

const HEADER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/header";
const FOOTER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footer";

fn text_y(svg: &str, needle: &str) -> f64 {
    let document = roxmltree::Document::parse(svg).expect("svg");
    document
        .descendants()
        .find(|node| {
            node.is_element()
                && node.tag_name().name() == "text"
                && node.text().is_some_and(|text| text.contains(needle))
        })
        .and_then(|node| node.attribute("y"))
        .and_then(|y| y.parse().ok())
        .expect(needle)
}

fn render_body(body: &str, rels: &str, parts: &[(&str, Vec<u8>)]) -> String {
    let mut entries = vec![
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into()),
    ];
    for (name, bytes) in parts {
        entries.push((name, bytes.clone()));
    }
    let (_package, parsed) = open_bytes(build_docx(&entries));
    render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

#[test]
fn f13_body_respects_tall_header() {
    // 567 twips = 37.8 px. The one-line header starts at the same distance as
    // the top margin, so it extends into the body unless the body moves down.
    let body = "<w:p><w:r><w:t>BODYTEXT</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>\
</w:sectPr>";
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdH\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/></Relationships>"
    );
    let header = format!(
        "<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\"><w:p><w:r><w:t>HEADERTEXT</w:t></w:r></w:p></w:hdr>"
    );
    let svg = render_body(body, &rels, &[("word/header1.xml", header.into_bytes())]);
    let header_y = text_y(&svg, "HEADERTEXT");
    let body_y = text_y(&svg, "BODYTEXT");
    assert!(
        body_y > header_y + 10.0,
        "body {body_y} must sit a line below header {header_y}"
    );
}

#[test]
fn f13_body_stays_above_a_tall_footer() {
    let mut paragraphs = String::new();
    for index in 0..12 {
        paragraphs.push_str(&format!("<w:p><w:r><w:t>LINE{index}</w:t></w:r></w:p>"));
    }
    let body = format!(
        "{paragraphs}\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"3600\"/>\
<w:pgMar w:top=\"567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/>\
</w:sectPr>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdF\" Type=\"{FOOTER_REL}\" Target=\"footer1.xml\"/></Relationships>"
    );
    let footer = format!(
        "<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\"><w:p><w:r><w:t>FOOTERTEXT</w:t></w:r></w:p></w:ftr>"
    );
    let svg = render_body(&body, &rels, &[("word/footer1.xml", footer.into_bytes())]);
    let footer_y = text_y(&svg, "FOOTERTEXT");
    let mut lowest = 0.0_f64;
    for index in 0..12 {
        if svg.contains(&format!("LINE{index}")) {
            lowest = lowest.max(text_y(&svg, &format!("LINE{index}")));
        }
    }
    assert!(lowest > 0.0, "body lines must paint");
    assert!(
        footer_y > lowest + 10.0,
        "footer {footer_y} must sit a line below the last body line {lowest}"
    );
}
