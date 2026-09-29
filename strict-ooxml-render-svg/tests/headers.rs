//! Header/footer rendering tests (STAGE-5 S5.3).
//!
//! Synthetic Strict packages exercise default/first/even selection, the
//! `w:pgMar` header/footer distances and the deterministic paint order.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::format_push_string
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{render, Page, RenderOptions};

const HEADER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/header";
const FOOTER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footer";
const SETTINGS_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";

/// Relationships part for the document, from `(id, type, target)` triples.
fn doc_rels(entries: &[(&str, &str, &str)]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (id, type_uri, target) in entries {
        xml.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{type_uri}\" Target=\"{target}\"/>"
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

/// A minimal Strict header part.
fn header(text: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>"
    )
    .into_bytes()
}

/// A minimal Strict footer part.
fn footer(text: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:ftr>"
    )
    .into_bytes()
}

/// A body with an explicit page break, yielding two pages.
fn two_page_body(sect_pr: &str) -> String {
    format!(
        "<w:p><w:r><w:t>Body one</w:t></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>Body two</w:t></w:r></w:p>\
<w:sectPr>{sect_pr}</w:sectPr>"
    )
}

/// Builds and parses a Strict package from a body, rels, settings and extra parts.
fn build(body: &str, rels: &str, settings: Option<&str>, parts: &[(&str, Vec<u8>)]) -> Vec<Page> {
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes().to_vec()),
    ];
    if let Some(settings) = settings {
        entries.push(("word/settings.xml", settings.as_bytes().to_vec()));
    }
    for (name, bytes) in parts {
        entries.push((name, bytes.clone()));
    }
    let (package, parsed) = open_bytes(build_docx(&entries));
    let _ = package;
    render(&parsed, &RenderOptions::default()).expect("render")
}

/// Returns the `y` baseline of the first `<text>` whose content contains `needle`.
fn text_y(svg: &str, needle: &str) -> Option<f64> {
    let document = roxmltree::Document::parse(svg).expect("valid svg");
    for node in document.descendants() {
        if node.is_element()
            && node.tag_name().name() == "text"
            && node.text().is_some_and(|text| text.contains(needle))
        {
            return node.attribute("y").and_then(|y| y.parse().ok());
        }
    }
    None
}

#[test]
fn default_header_and_footer_paint_on_every_page() {
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/>",
    );
    let rels = doc_rels(&[
        ("rIdH", HEADER_REL, "header1.xml"),
        ("rIdF", FOOTER_REL, "footer1.xml"),
    ]);
    let pages = build(
        &body,
        &rels,
        None,
        &[
            ("word/header1.xml", header("HDRMARK")),
            ("word/footer1.xml", footer("FTRMARK")),
        ],
    );
    assert_eq!(pages.len(), 2);
    for page in &pages {
        assert!(
            page.svg.contains("HDRMARK"),
            "missing header on page {}",
            page.index
        );
        assert!(
            page.svg.contains("FTRMARK"),
            "missing footer on page {}",
            page.index
        );
    }
}

#[test]
fn first_page_header_wins_when_title_page() {
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdDefault\"/>\
<w:headerReference w:type=\"first\" r:id=\"rIdFirst\"/><w:titlePg/>",
    );
    let rels = doc_rels(&[
        ("rIdDefault", HEADER_REL, "header1.xml"),
        ("rIdFirst", HEADER_REL, "header2.xml"),
    ]);
    let pages = build(
        &body,
        &rels,
        None,
        &[
            ("word/header1.xml", header("DEFHDR")),
            ("word/header2.xml", header("FIRSTHDR")),
        ],
    );
    assert_eq!(pages.len(), 2);
    assert!(
        pages[0].svg.contains("FIRSTHDR"),
        "first page uses the first header"
    );
    assert!(
        !pages[0].svg.contains("DEFHDR"),
        "first page must not use the default header"
    );
    assert!(
        pages[1].svg.contains("DEFHDR"),
        "later pages use the default header"
    );
    assert!(
        !pages[1].svg.contains("FIRSTHDR"),
        "later pages must not use the first header"
    );
}

#[test]
fn even_odd_headers_follow_settings() {
    let settings = format!(
        "<?xml version=\"1.0\"?><w:settings xmlns:w=\"{W}\"><w:evenAndOddHeaders/></w:settings>"
    );
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdDefault\"/>\
<w:headerReference w:type=\"even\" r:id=\"rIdEven\"/>",
    );
    let rels = doc_rels(&[
        ("rIdDefault", HEADER_REL, "header1.xml"),
        ("rIdEven", HEADER_REL, "header2.xml"),
        ("rIdSettings", SETTINGS_REL, "settings.xml"),
    ]);
    let pages = build(
        &body,
        &rels,
        Some(&settings),
        &[
            ("word/header1.xml", header("ODDHDR")),
            ("word/header2.xml", header("EVENHDR")),
        ],
    );
    assert_eq!(pages.len(), 2);
    assert!(
        pages[0].svg.contains("ODDHDR"),
        "odd page uses the default header"
    );
    assert!(
        pages[1].svg.contains("EVENHDR"),
        "even page uses the even header"
    );
    assert!(!pages[1].svg.contains("ODDHDR"));
}

#[test]
fn header_offset_follows_page_margin() {
    let build_with_margin = |header_twips: i32| {
        let body = two_page_body(&format!(
            "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"{header_twips}\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>"
        ));
        let rels = doc_rels(&[("rIdH", HEADER_REL, "header1.xml")]);
        let pages = build(
            &body,
            &rels,
            None,
            &[("word/header1.xml", header("HDRMARK"))],
        );
        text_y(&pages[0].svg, "HDRMARK").expect("header baseline")
    };
    let half_inch = build_with_margin(720);
    let one_inch = build_with_margin(1440);
    // 720 twips = 0.5 inch = 48 px at 96 DPI.
    assert!(
        (one_inch - half_inch - 48.0).abs() < 0.01,
        "header offset delta was {}",
        one_inch - half_inch
    );
}

#[test]
fn footer_sits_below_the_body_margin() {
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/>",
    );
    let rels = doc_rels(&[("rIdF", FOOTER_REL, "footer1.xml")]);
    let pages = build(
        &body,
        &rels,
        None,
        &[("word/footer1.xml", footer("FTRMARK"))],
    );
    let page = &pages[0];
    let y = text_y(&page.svg, "FTRMARK").expect("footer baseline");
    assert!(
        y > page.height_px / 2.0,
        "footer should be in the lower half: y={y}"
    );
    assert!(y < page.height_px, "footer must stay on the page: y={y}");
}
