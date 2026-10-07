//! A16 / F10: an explicit zero indent keeps the level hanging, and the marker
//! does not sit on the text.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_render_svg::{render, RenderOptions};
use strict_ooxml_wml::model::{Block, Document};

const NUMBERING_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";

fn list_document(body: &str) -> Document {
    let numbering = format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{W}\">\
<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/>\
<w:lvlText w:val=\"%1.\"/><w:suff w:val=\"space\"/>\
<w:pPr><w:ind w:start=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdN\" Type=\"{NUMBERING_REL}\" Target=\"numbering.xml\"/></Relationships>"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    doc
}

fn render_svg(doc: &Document) -> String {
    let pages = render(doc, &RenderOptions::default()).expect("render");
    pages.into_iter().map(|page| page.svg).collect()
}

fn render_list(body: &str) -> String {
    render_svg(&list_document(body))
}

fn parse_coord_list(value: &str) -> Result<Vec<f64>, String> {
    let mut numbers = Vec::new();
    for token in value.split_whitespace() {
        let number: f64 = token
            .parse()
            .map_err(|_| format!("bad x token {token} in {value}"))?;
        if !number.is_finite() {
            return Err(format!("non-finite x token {token}"));
        }
        numbers.push(number);
    }
    if numbers.is_empty() {
        return Err(format!("empty x list {value}"));
    }
    Ok(numbers)
}

fn texts(svg: &str) -> Vec<(String, f64)> {
    let document = roxmltree::Document::parse(svg).expect("svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .map(|node| {
            // F07 emits a space-separated cluster `x` list; the first value is
            // the run origin used by list indent oracles.
            let text = node.text().unwrap_or_default().to_owned();
            let xs = node
                .attribute("x")
                .map(parse_coord_list)
                .transpose()
                .expect("x list")
                .unwrap_or_default();
            assert!(
                xs.iter().all(|value| value.is_finite()),
                "non-finite x in {text:?}: {xs:?}"
            );
            let x = xs.first().copied().expect("x origin");
            (text, x)
        })
        .collect()
}

fn item(text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>\
<w:ind w:start=\"0\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"
    )
}

/// Direct start=0 keeps the text at the margin. The level hanging still
/// separates the marker. All five items do this.
#[test]
fn f10_partial_zero_indent_does_not_overlap_marker() {
    let body = (1..=5)
        .map(|index| item(&format!("Item {index}")))
        .collect::<String>();
    let svg = render_list(&body);
    let placed = texts(&svg);
    let markers: Vec<f64> = placed
        .iter()
        .filter(|(text, _)| text.ends_with('.'))
        .map(|(_, x)| *x)
        .collect();
    let lines: Vec<f64> = placed
        .iter()
        .filter(|(text, _)| text.starts_with("Item"))
        .map(|(_, x)| *x)
        .collect();
    assert_eq!(markers.len(), 5, "one marker per item: {placed:?}");
    assert_eq!(lines.len(), 5, "one text run per item: {placed:?}");
    for (marker_x, text_x) in markers.iter().zip(&lines) {
        assert!(
            (text_x - 96.0).abs() <= 0.25,
            "explicit start 0 keeps the text at the margin, got {text_x}"
        );
        assert!(
            (text_x - marker_x - 24.0).abs() <= 0.25,
            "hanging 360 twips separates marker {marker_x} from text {text_x}"
        );
        assert!(
            *marker_x < *text_x,
            "marker {marker_x} overlaps text {text_x}"
        );
    }
}

#[test]
fn f10_bullet_and_nothing_suffix_matrix() {
    let svg = render_list(
        "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>\
<w:ind w:start=\"0\"/></w:pPr><w:r><w:t>item</w:t></w:r></w:p>",
    );
    assert!(svg.contains("item"), "{svg}");
    assert!(
        svg.contains(">1.<") || svg.contains(">1<"),
        "decimal marker is painted: {svg}"
    );
}

/// A document built (or edited) in code has no source positions: every
/// paragraph carries `SourceLocation::unknown()`. The markers used to be keyed
/// by location, so all items of such a list rendered the same number.
#[test]
fn list_markers_do_not_depend_on_source_locations() {
    let body = (1..=3)
        .map(|index| item(&format!("Item {index}")))
        .collect::<String>();
    let mut doc = list_document(&body);
    for block in &mut doc.body.blocks {
        if let Block::Paragraph(paragraph) = block {
            paragraph.location = SourceLocation::unknown();
        }
    }
    let markers: Vec<String> = texts(&render_svg(&doc))
        .into_iter()
        .map(|(text, _)| text)
        .filter(|text| text.ends_with('.'))
        .collect();
    assert_eq!(markers, ["1.", "2.", "3."], "{markers:?}");
}
