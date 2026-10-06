//! A15 / F13: active header/footer regions move the body; inactive refs do not.

#![allow(clippy::expect_used, clippy::format_push_string)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{place_pages, render, Item, RenderOptions, TextItem};

const HEADER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/header";
const FOOTER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footer";
const SETTINGS_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";

/// Ink box from an actual text item (advance width × font-size metric), not a 10 px baseline gap.
fn ink_box(item: &TextItem) -> (f64, f64, f64, f64) {
    let top = item.baseline - item.size_px * 0.9;
    (item.x, top, item.width, item.size_px * 1.2)
}

fn boxes_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64), eps: f64) -> bool {
    a.0 < b.0 + b.2 - eps && a.0 + a.2 > b.0 + eps && a.1 < b.1 + b.3 - eps && a.1 + a.3 > b.1 + eps
}

fn text_items(page: &strict_ooxml_render_svg::PlacedPage) -> Vec<&TextItem> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

fn first_text<'a>(items: &[&'a TextItem], needle: &str) -> &'a TextItem {
    items
        .iter()
        .copied()
        .find(|item| item.text.contains(needle))
        .expect(needle)
}

fn assert_no_region_collision(
    page: &strict_ooxml_render_svg::PlacedPage,
    body: &str,
    region: &str,
) {
    let items = text_items(page);
    let body_item = first_text(&items, body);
    let region_item = first_text(&items, region);
    assert!(
        !boxes_overlap(ink_box(body_item), ink_box(region_item), 0.25),
        "unexpected ink collision: body {:?} vs region {:?}",
        ink_box(body_item),
        ink_box(region_item)
    );
}

fn open_parts(
    body: &str,
    rels: &str,
    parts: &[(&str, Vec<u8>)],
) -> strict_ooxml_wml::model::Document {
    let mut entries = vec![
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into()),
    ];
    for (name, bytes) in parts {
        entries.push((name, bytes.clone()));
    }
    open_bytes(build_docx(&entries)).1
}

fn place_body(
    body: &str,
    rels: &str,
    parts: &[(&str, Vec<u8>)],
) -> Vec<strict_ooxml_render_svg::PlacedPage> {
    let parsed = open_parts(body, rels, parts);
    place_pages(&parsed, &RenderOptions::default(), None).expect("place")
}

fn render_body(body: &str, rels: &str, parts: &[(&str, Vec<u8>)]) -> String {
    let parsed = open_parts(body, rels, parts);
    render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

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

fn hdr(text: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>")
        .into_bytes()
}

fn ftr(text: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:ftr>")
        .into_bytes()
}

fn tall_ftr(marker: &str, lines: usize) -> Vec<u8> {
    let mut xml = format!("<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\">");
    for index in 0..lines {
        xml.push_str(&format!("<w:p><w:r><w:t>{marker}{index}</w:t></w:r></w:p>"));
    }
    xml.push_str("</w:ftr>");
    xml.into_bytes()
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
    let pages = place_body(body, &rels, &[("word/header1.xml", hdr("HEADERTEXT"))]);
    assert_eq!(pages.len(), 1);
    assert_no_region_collision(&pages[0], "BODYTEXT", "HEADERTEXT");
    let svg = render_body(body, &rels, &[("word/header1.xml", hdr("HEADERTEXT"))]);
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
    let pages = place_body(&body, &rels, &[("word/footer1.xml", ftr("FOOTERTEXT"))]);
    let last = pages.last().expect("page");
    let items = text_items(last);
    let footer = first_text(&items, "FOOTERTEXT");
    let mut lowest_body: Option<&TextItem> = None;
    for item in &items {
        if item.text.starts_with("LINE") {
            lowest_body = Some(match lowest_body {
                Some(prev) if prev.baseline >= item.baseline => prev,
                _ => item,
            });
        }
    }
    let lowest = lowest_body.expect("body lines must paint");
    assert!(
        !boxes_overlap(ink_box(lowest), ink_box(footer), 0.25),
        "footer ink {:?} must not collide with body {:?}",
        ink_box(footer),
        ink_box(lowest)
    );
}

#[test]
fn f13_inactive_tall_footer_does_not_shrink_other_page() {
    // Inverse of tallest-of-all-refs: title page uses a one-line first footer;
    // the default footer is many lines and must not steal body capacity from page 1.
    let mut paragraphs = String::new();
    for index in 0..18 {
        paragraphs.push_str(&format!("<w:p><w:r><w:t>FLOW{index}</w:t></w:r></w:p>"));
    }
    let body = format!(
        "{paragraphs}\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"5040\"/>\
<w:pgMar w:top=\"567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:footerReference w:type=\"first\" r:id=\"rIdFirst\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdDefault\"/>\
<w:titlePg/>\
</w:sectPr>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdFirst\" Type=\"{FOOTER_REL}\" Target=\"footerFirst.xml\"/>\
<Relationship Id=\"rIdDefault\" Type=\"{FOOTER_REL}\" Target=\"footerDefault.xml\"/></Relationships>"
    );
    let pages = place_body(
        &body,
        &rels,
        &[
            ("word/footerFirst.xml", ftr("SHORTFTR")),
            ("word/footerDefault.xml", tall_ftr("TALLFTR", 12)),
        ],
    );
    assert!(pages.len() >= 2);
    let page1_flow: Vec<_> = text_items(&pages[0])
        .into_iter()
        .filter(|item| item.text.starts_with("FLOW"))
        .map(|item| item.text.as_str())
        .collect();
    assert!(
        page1_flow.iter().any(|text| text == &"FLOW10"),
        "short first-page footer must leave room for FLOW10 on page 1, got {page1_flow:?}"
    );
    assert!(
        !text_items(&pages[0])
            .iter()
            .any(|item| item.text.contains("TALLFTR")),
        "page 1 must not paint the inactive tall footer"
    );
    assert!(
        text_items(&pages[1])
            .iter()
            .any(|item| item.text.contains("TALLFTR")),
        "later pages use the tall default footer"
    );
}

#[test]
fn f13_numpages_digit_rollover_uses_field_env_height() {
    // Without FieldEnv, NUMPAGES measures as "1". Two-digit totals must wrap the
    // header and still keep body ink out of the actual header box.
    let mut paragraphs = String::new();
    for index in 0..48 {
        paragraphs.push_str(&format!("<w:p><w:r><w:t>BODY{index}</w:t></w:r></w:p>"));
    }
    let body = format!(
        "{paragraphs}\
<w:sectPr>\
<w:pgSz w:w=\"3600\" w:h=\"1800\"/>\
<w:pgMar w:top=\"240\" w:right=\"240\" w:bottom=\"240\" w:left=\"240\" w:header=\"240\" w:footer=\"240\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>\
</w:sectPr>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdH\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/></Relationships>"
    );
    let header = format!(
        "<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\">\
<w:p>\
<w:r><w:t>HDRWRAP AAA BBB CCC DDD </w:t></w:r>\
<w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
<w:r><w:t>/</w:t></w:r>\
<w:fldSimple w:instr=\" NUMPAGES \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
<w:r><w:t>/</w:t></w:r>\
<w:fldSimple w:instr=\" SECTIONPAGES \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
</w:p></w:hdr>"
    );
    let parsed = open_parts(&body, &rels, &[("word/header1.xml", header.into_bytes())]);
    let pages = place_pages(&parsed, &RenderOptions::default(), None).expect("place");
    assert!(
        pages.len() >= 10,
        "fixture must cross the two-digit NUMPAGES threshold, got {}",
        pages.len()
    );
    let painted = render(&parsed, &RenderOptions::default()).expect("render");
    let total = painted.len().to_string();
    for (index, page) in pages.iter().enumerate() {
        let items = text_items(page);
        assert!(
            items.iter().any(|item| item.text.contains(&total)),
            "page {index}: NUMPAGES must paint the converged total {total}"
        );
        let body_item = items
            .iter()
            .copied()
            .find(|item| item.text.starts_with("BODY"))
            .expect("body");
        for item in items {
            if item.text.contains("HDRWRAP")
                || item.text.contains(&total) && !item.text.starts_with("BODY")
            {
                assert!(
                    !boxes_overlap(ink_box(body_item), ink_box(item), 0.25),
                    "page {index}: body {:?} collides header {:?}",
                    ink_box(body_item),
                    ink_box(item)
                );
            }
        }
    }
}

#[test]
fn f13_section_change_uses_that_section_active_header() {
    let body = "\
<w:p><w:r><w:t>SEC1BODY</w:t></w:r></w:p>\
<w:p><w:pPr><w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"7920\"/>\
<w:pgMar w:top=\"567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH1\"/>\
<w:type w:val=\"nextPage\"/>\
</w:sectPr></w:pPr></w:p>\
<w:p><w:r><w:t>SEC2BODY</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"7920\"/>\
<w:pgMar w:top=\"567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH2\"/>\
</w:sectPr>";
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdH1\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/>\
<Relationship Id=\"rIdH2\" Type=\"{HEADER_REL}\" Target=\"header2.xml\"/></Relationships>"
    );
    let mut tall = String::from("<?xml version=\"1.0\"?><w:hdr xmlns:w=\"");
    tall.push_str(W);
    tall.push_str("\">");
    for index in 0..8 {
        tall.push_str(&format!("<w:p><w:r><w:t>TALLH{index}</w:t></w:r></w:p>"));
    }
    tall.push_str("</w:hdr>");
    let pages = place_body(
        body,
        &rels,
        &[
            ("word/header1.xml", hdr("SHORTH")),
            ("word/header2.xml", tall.into_bytes()),
        ],
    );
    assert_eq!(pages.len(), 2);
    let y1 = first_text(&text_items(&pages[0]), "SEC1BODY").baseline;
    let y2 = first_text(&text_items(&pages[1]), "SEC2BODY").baseline;
    assert!(
        y2 > y1 + 20.0,
        "section 2 tall header must push its body (sec1 {y1}, sec2 {y2})"
    );
    assert_no_region_collision(&pages[0], "SEC1BODY", "SHORTH");
    assert_no_region_collision(&pages[1], "SEC2BODY", "TALLH0");
}

#[test]
fn f13_negative_margins_keep_source_overlap() {
    let body = "<w:p><w:r><w:t>BODYNEG</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"-567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>\
</w:sectPr>";
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdH\" Type=\"{HEADER_REL}\" Target=\"header1.xml\"/></Relationships>"
    );
    let parsed = open_parts(body, &rels, &[("word/header1.xml", hdr("HDRNEG"))]);
    let pages = place_pages(&parsed, &RenderOptions::default(), None).expect("place");
    let painted = render(&parsed, &RenderOptions::default()).expect("render");
    assert!(
        painted[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("negative-page-margin")),
        "negative margins need an explicit diagnostic, got {:?}",
        painted[0].warnings
    );
    let body_item = first_text(&text_items(&pages[0]), "BODYNEG");
    let header_item = first_text(&text_items(&pages[0]), "HDRNEG");
    // Source overlap is allowed: do not invent a gap that separates the inks.
    assert!(
        body_item.baseline < header_item.baseline + header_item.size_px * 2.0,
        "negative top margin must not auto-grow an extra body gap (body {}, header {})",
        body_item.baseline,
        header_item.baseline
    );
}

#[test]
fn f13_even_header_selection_is_not_the_odd_tallest() {
    let settings = format!(
        "<?xml version=\"1.0\"?><w:settings xmlns:w=\"{W}\"><w:evenAndOddHeaders/></w:settings>"
    );
    let body = "<w:p><w:r><w:t>ODDBODY</w:t></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>EVENBODY</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"7920\"/>\
<w:pgMar w:top=\"567\" w:right=\"567\" w:bottom=\"567\" w:left=\"567\" w:header=\"567\" w:footer=\"567\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdOdd\"/>\
<w:headerReference w:type=\"even\" r:id=\"rIdEven\"/>\
</w:sectPr>";
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdOdd\" Type=\"{HEADER_REL}\" Target=\"headerOdd.xml\"/>\
<Relationship Id=\"rIdEven\" Type=\"{HEADER_REL}\" Target=\"headerEven.xml\"/>\
<Relationship Id=\"rIdSettings\" Type=\"{SETTINGS_REL}\" Target=\"settings.xml\"/></Relationships>"
    );
    let mut tall = format!("<?xml version=\"1.0\"?><w:hdr xmlns:w=\"{W}\">");
    for index in 0..10 {
        tall.push_str(&format!("<w:p><w:r><w:t>EVENH{index}</w:t></w:r></w:p>"));
    }
    tall.push_str("</w:hdr>");
    let pages = place_body(
        body,
        &rels,
        &[
            ("word/settings.xml", settings.into_bytes()),
            ("word/headerOdd.xml", hdr("ODDH")),
            ("word/headerEven.xml", tall.into_bytes()),
        ],
    );
    assert_eq!(pages.len(), 2);
    let odd_y = first_text(&text_items(&pages[0]), "ODDBODY").baseline;
    let even_y = first_text(&text_items(&pages[1]), "EVENBODY").baseline;
    assert!(
        odd_y < 90.0,
        "odd page must reserve only the short default header, not the even tallest (odd {odd_y})"
    );
    assert!(
        even_y > 180.0,
        "even page must reserve its own tall header (even {even_y})"
    );
    assert_no_region_collision(&pages[0], "ODDBODY", "ODDH");
}
