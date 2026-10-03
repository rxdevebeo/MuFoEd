//! Multi-section geometry and empty page-break tests (AUD-74, AUD-75).

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::format_push_string
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{render, RenderOptions};

const HEADER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/header";

fn render_body(body: &str) -> Vec<strict_ooxml_render_svg::Page> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default()).expect("render")
}

fn view_box(svg: &str) -> (f64, f64) {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let root = document.root_element();
    let view = root.attribute("viewBox").expect("viewBox");
    let parts: Vec<f64> = view
        .split_whitespace()
        .map(|part| part.parse().expect("num"))
        .collect();
    (parts[2], parts[3])
}

#[test]
fn portrait_landscape_portrait_have_matching_viewboxes() {
    // AUD-74: three sections → three pages with portrait / landscape / portrait.
    let body = "\
<w:p><w:r><w:t>P1</w:t></w:r></w:p>\
<w:p><w:pPr><w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:type w:val=\"nextPage\"/>\
</w:sectPr></w:pPr></w:p>\
<w:p><w:r><w:t>L2</w:t></w:r></w:p>\
<w:p><w:pPr><w:sectPr>\
<w:pgSz w:w=\"15840\" w:h=\"12240\" w:orient=\"landscape\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:type w:val=\"nextPage\"/>\
</w:sectPr></w:pPr></w:p>\
<w:p><w:r><w:t>P3</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
</w:sectPr>";
    let pages = render_body(body);
    assert_eq!(pages.len(), 3, "expected three section pages");
    let (w0, h0) = view_box(&pages[0].svg);
    let (w1, h1) = view_box(&pages[1].svg);
    let (w2, h2) = view_box(&pages[2].svg);
    assert!(
        (w0 - 816.0).abs() < 0.5 && (h0 - 1056.0).abs() < 0.5,
        "{w0}x{h0}"
    );
    assert!(
        (w1 - 1056.0).abs() < 0.5 && (h1 - 816.0).abs() < 0.5,
        "{w1}x{h1}"
    );
    assert!(
        (w2 - 816.0).abs() < 0.5 && (h2 - 1056.0).abs() < 0.5,
        "{w2}x{h2}"
    );
}

#[test]
fn odd_page_section_break_inserts_blank_page() {
    // AUD-74: oddPage after page 1 inserts an empty even page.
    let body = "\
<w:p><w:r><w:t>One</w:t></w:r></w:p>\
<w:p><w:pPr><w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:type w:val=\"oddPage\"/>\
</w:sectPr></w:pPr></w:p>\
<w:p><w:r><w:t>Three</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
</w:sectPr>";
    let pages = render_body(body);
    assert_eq!(pages.len(), 3, "oddPage should insert a blank page");
    assert!(pages[0].svg.contains("One"));
    assert!(!pages[1].svg.contains("One") && !pages[1].svg.contains("Three"));
    assert!(pages[2].svg.contains("Three"));
}

#[test]
fn section_two_can_use_its_own_header() {
    // AUD-74: headers differ per section.
    let body = "\
<w:p><w:r><w:t>Body1</w:t></w:r></w:p>\
<w:p><w:pPr><w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH1\"/>\
<w:type w:val=\"nextPage\"/>\
</w:sectPr></w:pPr></w:p>\
<w:p><w:r><w:t>Body2</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH2\"/>\
</w:sectPr>";
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdH1\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/>\
<Relationship Id=\"rIdH2\" Type=\"{HEADER_REL}\" Target=\"header2.xml\"/></Relationships>"
    );
    let header = |text: &str| {
        format!(
            "<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>"
        )
        .into_bytes()
    };
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/header1.xml", header("HDR1")),
        ("word/header2.xml", header("HDR2")),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    let pages = render(&parsed, &RenderOptions::default()).expect("render");
    assert_eq!(pages.len(), 2);
    assert!(pages[0].svg.contains("HDR1"), "{}", pages[0].svg);
    assert!(!pages[0].svg.contains("HDR2"));
    assert!(pages[1].svg.contains("HDR2"), "{}", pages[1].svg);
    assert!(!pages[1].svg.contains("HDR1"));
}

#[test]
fn consecutive_page_breaks_keep_empty_pages() {
    // AUD-75: p, br(page), br(page), p → 3 pages.
    let body = "\
<w:p><w:r><w:t>FIRSTMARK</w:t></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>SECONDMARK</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
</w:sectPr>";
    let pages = render_body(body);
    assert_eq!(
        pages.len(),
        3,
        "two explicit breaks must keep the empty page"
    );
    assert!(pages[0].svg.contains("FIRSTMARK"));
    assert!(
        !pages[1].svg.contains("FIRSTMARK") && !pages[1].svg.contains("SECONDMARK"),
        "middle page must stay empty: {}",
        pages[1].svg
    );
    assert!(pages[2].svg.contains("SECONDMARK"));
}
