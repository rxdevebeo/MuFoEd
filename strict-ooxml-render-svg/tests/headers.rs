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

use common::{build_docx, content_types, document, open_bytes, root_rels, A, W, WP, WPS};
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

/// A footer whose only content is a floating (`wp:anchor`) text box, offset
/// `offset_emu` below its paragraph. Such a footer has no flow height.
fn footer_with_text_box(text: &str, offset_emu: i64) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\" xmlns:wp=\"{WP}\" xmlns:a=\"{A}\" xmlns:wps=\"{WPS}\">\
<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:align>center</wp:align></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>{offset_emu}</wp:posOffset></wp:positionV>\
<wp:extent cx=\"2743200\" cy=\"914400\"/><wp:docPr id=\"1\" name=\"box\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"2743200\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p></w:ftr>"
    )
    .into_bytes()
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
fn title_page_without_first_leaves_first_page_blank() {
    // `titlePg` set but no `first` reference: the first page must have no
    // header and no footer (no Default/Even fallback).
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/><w:titlePg/>",
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
            ("word/header1.xml", header("DEFHDR")),
            ("word/footer1.xml", footer("DEFTR")),
        ],
    );
    assert_eq!(pages.len(), 2);
    assert!(
        !pages[0].svg.contains("DEFHDR"),
        "first page must not use the default header"
    );
    assert!(
        !pages[0].svg.contains("DEFTR"),
        "first page must not use the default footer"
    );
    // Later pages keep using the default header/footer.
    assert!(pages[1].svg.contains("DEFHDR"));
    assert!(pages[1].svg.contains("DEFTR"));
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
fn a_footer_of_floating_objects_keeps_its_text_on_the_page() {
    // The footer is a single floating text box: the region has no flow height
    // (`y` stays 0), so placing it by `y` put the box at the page edge and its
    // text below it (CC0/019 drew a footer box 15 pt under the page). The
    // footer is placed by the ink it draws.
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"0\" w:footer=\"0\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/>",
    );
    let rels = doc_rels(&[("rIdF", FOOTER_REL, "footer1.xml")]);
    let pages = build(
        &body,
        &rels,
        None,
        &[("word/footer1.xml", footer_with_text_box("FTBOX", 228_600))],
    );
    let page = &pages[0];
    let y = text_y(&page.svg, "FTBOX").expect("footer text box baseline");
    assert!(
        y > page.height_px / 2.0,
        "the footer box is in the lower half: y={y}"
    );
    assert!(
        y < page.height_px,
        "the footer box must stay on the page: y={y}, height={}",
        page.height_px
    );
}

#[test]
fn a_bare_footer_box_does_not_lift_the_footer_text() {
    // The footer has a real text line and, below it, a floating text box whose
    // declared extent is large but which draws nothing (`a:noFill`, no line) and
    // holds only a short label. Counting that invisible rectangle as ink lifted
    // the whole footer into the body (CC0_DOCX_MATH/095). Only drawn ink counts.
    let body = two_page_body(
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"0\" w:footer=\"0\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/>",
    );
    let rels = doc_rels(&[("rIdF", FOOTER_REL, "footer1.xml")]);
    let pages = build(
        &body,
        &rels,
        None,
        &[(
            "word/footer1.xml",
            footer_with_flow_and_bare_box("FTREAL", "lbl"),
        )],
    );
    let page = &pages[0];
    let y = text_y(&page.svg, "FTREAL").expect("footer flow text");
    assert!(
        y > page.height_px - 120.0,
        "the footer text stays at the foot, not lifted by the bare box: y={y}, height={}",
        page.height_px
    );
    assert!(y < page.height_px, "footer must stay on the page: y={y}");
}

/// A footer with one flow text line and a large, invisible (`a:noFill`) text box
/// anchored below it.
fn footer_with_flow_and_bare_box(flow: &str, box_text: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\" xmlns:wp=\"{WP}\" xmlns:a=\"{A}\" xmlns:wps=\"{WPS}\">\
<w:p><w:r><w:t>{flow}</w:t></w:r></w:p>\
<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:align>center</wp:align></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
<wp:extent cx=\"2743200\" cy=\"2743200\"/><wp:docPr id=\"1\" name=\"box\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"2743200\" cy=\"2743200\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/>\
<a:ln><a:noFill/></a:ln></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>{box_text}</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p></w:ftr>"
    )
    .into_bytes()
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

/// A header picture positioned from the page margin keeps that position when
/// the header distance changes. The distance only slides the header's own
/// lines (`021_Ljudi`: the logo was measured from `w:header` and left the page).
#[test]
fn a_margin_anchored_header_picture_ignores_the_header_distance() {
    let logo = header_margin_logo();
    let y_at = |header: i32| {
        let body = format!(
            "<w:p><w:r><w:t>Body</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1417\" w:right=\"1417\" w:bottom=\"1417\" w:left=\"1417\" w:header=\"{header}\" w:footer=\"708\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/></w:sectPr>"
        );
        let rels = doc_rels(&[("rIdH", HEADER_REL, "header1.xml")]);
        let pages = build(&body, &rels, None, &[("word/header1.xml", logo.clone())]);
        text_y(&pages[0].svg, "LOGO").expect("logo")
    };
    let near = y_at(708);
    let far = y_at(1440);
    assert!(
        (near - far).abs() < 0.6,
        "the header distance moved the logo: {near} vs {far}"
    );
    // Top margin 1417 twips is 94.5 px; the offset −578485 EMU is −60.7 px.
    // The label sits one line inside a box whose top is about 33.7 px.
    assert!(
        (30.0..70.0).contains(&near),
        "logo baseline {near} is not on the page margin"
    );
}

/// Header picture from `021`: `positionV` is the page margin, not the header box.
fn header_margin_logo() -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\" xmlns:wp=\"{WP}\" xmlns:a=\"{A}\" xmlns:wps=\"{WPS}\">\
<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"margin\"><wp:posOffset>0</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"margin\"><wp:posOffset>-578485</wp:posOffset></wp:positionV>\
<wp:extent cx=\"1828800\" cy=\"533400\"/><wp:docPr id=\"1\" name=\"logo\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"533400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>LOGO</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p></w:hdr>"
    )
    .into_bytes()
}
