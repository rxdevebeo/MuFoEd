//! Structural, determinism and option tests for the SVG renderer.

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::doc_markdown
)]

mod common;

use common::{
    build_docx, content_types, document, open_body, open_bytes, render_body, root_rels,
    without_font_faces, W,
};
use strict_ooxml_render_svg::{render, PageSelection, RenderOptions};

#[test]
fn simple_paragraph_renders_text_and_formatting() {
    let pages =
        render_body("<w:p><w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>Hello world</w:t></w:r></w:p>");
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    assert!(
        svg.contains(">Hello </text>") || svg.contains(">Hello<"),
        "{svg}"
    );
    assert!(svg.contains("world"), "{svg}");
    assert!(svg.contains("font-weight=\"bold\""));
    assert!(svg.contains("font-style=\"italic\""));
    assert!(svg.starts_with("<svg "));
    assert!(svg.ends_with("</svg>\n"));
}

#[test]
fn empty_paragraph_still_produces_a_page() {
    let pages = render_body("<w:p/>");
    assert_eq!(pages.len(), 1);
    assert!(pages[0].svg.contains("viewBox"));
}

#[test]
fn default_page_is_letter_at_96_dpi() {
    let pages = render_body("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    assert!((pages[0].width_px - 816.0).abs() < 0.001);
    assert!((pages[0].height_px - 1056.0).abs() < 0.001);
}

#[test]
fn zero_page_size_falls_back_to_letter() {
    // AUD-73: pgSz w="0" h="0" → non-zero Letter viewBox + warning.
    let pages = render_body(
        "<w:p><w:r><w:t>x</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"0\" w:h=\"0\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr>",
    );
    assert_eq!(pages.len(), 1);
    assert!(pages[0].width_px > 1.0, "viewBox width");
    assert!(pages[0].height_px > 1.0, "viewBox height");
    assert!((pages[0].width_px - 816.0).abs() < 0.001);
    assert!((pages[0].height_px - 1056.0).abs() < 0.001);
    assert!(
        pages[0].svg.contains("viewBox=\"0 0 816 1056\""),
        "{}",
        pages[0].svg
    );
    assert!(
        pages[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("pgSz")),
        "expected pgSz warning: {:?}",
        pages[0].warnings
    );
}

#[test]
fn scale_changes_page_dimensions() {
    let (_package, document) = open_body("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    let pages = render(&document, &RenderOptions::default().scale(48.0)).expect("render");
    assert!((pages[0].width_px - 408.0).abs() < 0.001);
    assert!((pages[0].height_px - 528.0).abs() < 0.001);
}

#[test]
fn two_runs_produce_identical_bytes() {
    let (_package, document) = open_body("<w:p><w:r><w:t>deterministic</w:t></w:r></w:p>");
    let first = render(&document, &Default::default()).expect("render");
    let second = render(&document, &Default::default()).expect("render");
    assert_eq!(first, second);
}

#[test]
fn page_selection_filters_output() {
    let body: String = (0..80)
        .map(|index| format!("<w:p><w:r><w:t>line {index}</w:t></w:r></w:p>"))
        .collect();
    let (_package, document) = open_body(&body);
    let all = render(&document, &Default::default()).expect("render");
    assert!(all.len() >= 2, "expected multiple pages, got {}", all.len());
    let selected = render(
        &document,
        &RenderOptions::default().pages(PageSelection::Range { start: 1, end: 1 }),
    )
    .expect("render");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].index, 0);
}

#[test]
fn table_renders_cells_and_borders() {
    let body = "<w:tbl><w:tblGrid><w:gridCol w:w=\"2880\"/><w:gridCol w:w=\"2880\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let pages = render_body(body);
    let svg = &pages[0].svg;
    assert!(svg.contains(">A<"), "{svg}");
    assert!(svg.contains(">B<"), "{svg}");
}

#[test]
fn no_nan_or_infinite_coordinates() {
    let pages = render_body(
        "<w:p><w:r><w:t>abc</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>",
    );
    for page in &pages {
        let ink = without_font_faces(&page.svg);
        assert!(!ink.contains("NaN"), "{ink}");
        assert!(!ink.contains("inf"), "{ink}");
        assert!(!ink.contains("-0."), "{ink}");
    }
}

#[test]
fn zero_default_tab_stop_advances_text() {
    // AUD-71: defaultTabStop=0 must not produce NaN; text after a tab sits
    // to the right of the line start (Word's 720-twip default).
    const SETTINGS_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";
    let settings = format!(
        "<?xml version=\"1.0\"?><w:settings xmlns:w=\"{W}\"><w:defaultTabStop w:val=\"0\"/></w:settings>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdS\" Type=\"{SETTINGS_REL}\" Target=\"settings.xml\"/></Relationships>"
    );
    let body = "<w:p><w:r><w:tab/><w:t>TABBED</w:t></w:r></w:p>";
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/settings.xml", settings.into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    let pages = render(&parsed, &RenderOptions::default()).expect("render");
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    let ink = without_font_faces(svg);
    assert!(!ink.contains("NaN"), "{ink}");
    assert!(!ink.contains("inf"), "{ink}");
    let document = roxmltree::Document::parse(svg).expect("svg");
    let text_x = document
        .descendants()
        .find(|node| {
            node.is_element()
                && node.tag_name().name() == "text"
                && node.text().is_some_and(|text| text.contains("TABBED"))
        })
        .and_then(|node| node.attribute("x"))
        .and_then(|x| x.parse::<f64>().ok())
        .expect("TABBED x");
    // Content left is 96 px (1"); default tab 720 twips = 48 px → x ≈ 144.
    assert!(
        text_x > 96.0 + 1.0,
        "tabbed text should sit right of the content edge: x={text_x}"
    );
    assert!(
        pages[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("default-tab-stop")),
        "expected default-tab-stop warning: {:?}",
        pages[0].warnings
    );
}

#[test]
fn explicit_page_break_creates_second_page() {
    let pages = render_body(
        "<w:p><w:r><w:t>first</w:t><w:br w:type=\"page\"/><w:t>second</w:t></w:r></w:p>",
    );
    assert!(
        pages.len() >= 2,
        "expected a page break, got {}",
        pages.len()
    );
    assert!(pages[0].svg.contains("first"));
    assert!(pages[1].svg.contains("second"));
}
