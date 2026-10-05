//! List numbering rendering tests (STAGE-5 S5.10): multi-level counters,
//! `lvlText` substitution, overrides, bullets and level indentation.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::unreadable_literal
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{render, Page, RenderOptions};

const NUMBERING_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";

/// Builds a Strict package with `numbering.xml` and renders the body.
fn build(body: &str, numbering: &str) -> Vec<Page> {
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdN\" Type=\"{NUMBERING_REL}\" Target=\"numbering.xml\"/></Relationships>"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/numbering.xml", numbering.as_bytes().to_vec()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default()).expect("render")
}

/// A numbered paragraph at `ilvl`.
fn item(ilvl: u8, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{ilvl}\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"
    )
}

/// Returns `(text, x)` of every `<text>` node.
fn texts(svg: &str) -> Vec<(String, f64)> {
    let document = roxmltree::Document::parse(svg).expect("valid svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .map(|node| {
            let text = node.text().unwrap_or_default().to_owned();
            let x = node
                .attribute("x")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            (text, x)
        })
        .collect()
}

/// Numbering with two decimal levels (`%1.` and `%1.%2`).
fn two_level_numbering() -> String {
    format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{W}\"><w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl>\
<w:lvl w:ilvl=\"1\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.%2\"/></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
    )
}

#[test]
fn multi_level_markers_and_restart() {
    let body = format!(
        "{}{}{}{}{}",
        item(0, "A"),
        item(1, "B"),
        item(1, "C"),
        item(0, "D"),
        item(1, "E")
    );
    let pages = build(&body, &two_level_numbering());
    let rendered = texts(&pages[0].svg);
    let markers: Vec<&str> = rendered
        .iter()
        .map(|(text, _)| text.as_str())
        .filter(|text| text.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .collect();
    assert_eq!(markers, ["1.", "1.1", "1.2", "2.", "2.1"]);
}

#[test]
fn start_override_applies() {
    let numbering = format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{W}\"><w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"5\"/></w:lvlOverride></w:num></w:numbering>"
    );
    let pages = build(&format!("{}{}", item(0, "A"), item(0, "B")), &numbering);
    let rendered = texts(&pages[0].svg);
    assert!(rendered.iter().any(|(text, _)| text == "5."));
    assert!(rendered.iter().any(|(text, _)| text == "6."));
}

#[test]
fn bullet_level_renders_its_glyph() {
    let numbering = format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{W}\"><w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"\u{2022}\"/></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
    );
    let pages = build(&item(0, "Bullet"), &numbering);
    assert!(texts(&pages[0].svg)
        .iter()
        .any(|(text, _)| text == "\u{2022}"));
}

/// A bullet level whose marker is a private-use character of `font`.
fn symbol_bullet(font: &str, marker: char) -> String {
    format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{W}\"><w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{marker}\"/>\
<w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\"/></w:rPr></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
    )
}

#[test]
fn symbol_private_bullet_renders_as_a_bullet() {
    let pages = build(&item(0, "Item"), &symbol_bullet("Symbol", '\u{F0B7}'));
    let svg = &pages[0].svg;
    assert!(texts(svg).iter().any(|(text, _)| text == "\u{2022}"));
    assert!(!svg.contains('\u{F0B7}'));
    assert!(svg.contains("font-family=\"Carlito\""));
}

#[test]
fn wingdings_square_renders_as_a_square() {
    let pages = build(&item(0, "Item"), &symbol_bullet("Wingdings", '\u{F0A7}'));
    assert!(texts(&pages[0].svg)
        .iter()
        .any(|(text, _)| text == "\u{25AA}"));
}

#[test]
fn wingdings_clock_is_not_turned_into_a_bullet() {
    let pages = build(&item(0, "Item"), &symbol_bullet("Wingdings", '\u{F0B7}'));
    let rendered = texts(&pages[0].svg);
    assert!(rendered.iter().any(|(text, _)| text.contains('\u{F0B7}')));
    assert!(!rendered.iter().any(|(text, _)| text.contains('\u{2022}')));
}

#[test]
fn level_indentation_offsets_the_marker() {
    let numbering = format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{W}\"><w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
<w:pPr><w:ind w:start=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
    );
    let pages = build(&item(0, "Item"), &numbering);
    let rendered = texts(&pages[0].svg);
    let marker_x = rendered
        .iter()
        .find(|(text, _)| text == "1.")
        .map(|(_, x)| *x)
        .expect("marker");
    let text_x = rendered
        .iter()
        .find(|(text, _)| text == "Item")
        .map(|(_, x)| *x)
        .expect("text");
    // hanging = 360 twips = 18 pt = 24 px at 96 DPI.
    assert!(
        (text_x - marker_x - 24.0).abs() < 0.01,
        "marker {marker_x}, text {text_x}"
    );
}
