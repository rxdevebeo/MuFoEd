//! Footnote/endnote rendering tests (STAGE-5 S5.5).

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

const FOOTNOTES_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footnotes";
const ENDNOTES_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/endnotes";

/// Relationships part for the document.
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

/// Builds a package with body, rels and extra parts; renders it.
fn build(body: &str, rels: &str, parts: &[(&str, Vec<u8>)]) -> Vec<Page> {
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes().to_vec()),
    ];
    for (name, bytes) in parts {
        entries.push((name, bytes.clone()));
    }
    let (_package, parsed) = open_bytes(build_docx(&entries));
    render(&parsed, &RenderOptions::default()).expect("render")
}

/// A footnotes part with one normal note.
fn footnotes(inner: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><w:footnotes xmlns:w=\"{W}\">{inner}</w:footnotes>")
        .into_bytes()
}

/// An endnotes part with one normal note.
fn endnotes(inner: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><w:endnotes xmlns:w=\"{W}\">{inner}</w:endnotes>").into_bytes()
}

/// A normal footnote definition whose body starts with the note marker.
fn note(id: i32, marker: &str, text: &str) -> String {
    note_element("footnote", id, marker, text)
}

/// A normal endnote definition whose body starts with the note marker.
fn endnote(id: i32, marker: &str, text: &str) -> String {
    note_element("endnote", id, marker, text)
}

/// A normal note definition (`w:footnote`/`w:endnote`).
fn note_element(element: &str, id: i32, marker: &str, text: &str) -> String {
    format!(
        "<w:{element} w:id=\"{id}\"><w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:{marker}/></w:r><w:r><w:t xml:space=\"preserve\"> {text}</w:t></w:r></w:p></w:{element}>"
    )
}

/// Returns `(text, font_size)` of every `<text>` node.
fn texts(svg: &str) -> Vec<(String, f64)> {
    let document = roxmltree::Document::parse(svg).expect("valid svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .map(|node| {
            let text = node.text().unwrap_or_default().to_owned();
            let size = node
                .attribute("font-size")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.0);
            (text, size)
        })
        .collect()
}

/// Returns the horizontal length of every `<line>` in the SVG.
fn line_lengths(svg: &str) -> Vec<f64> {
    let document = roxmltree::Document::parse(svg).expect("valid svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "line")
        .map(|node| {
            let x1: f64 = node
                .attribute("x1")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let x2: f64 = node
                .attribute("x2")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            (x2 - x1).abs()
        })
        .collect()
}

const PAGE_SETUP: &str = "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>";

#[test]
fn footnote_renders_marker_and_note_area() {
    let body = format!(
        "<w:p><w:r><w:t>Body</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
<w:sectPr>{PAGE_SETUP}</w:sectPr>"
    );
    let rels = doc_rels(&[("rIdFn", FOOTNOTES_REL, "footnotes.xml")]);
    let pages = build(
        &body,
        &rels,
        &[(
            "word/footnotes.xml",
            footnotes(&note(1, "footnoteRef", "NoteBodyText")),
        )],
    );
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    assert!(svg.contains("NoteBodyText"), "footnote text missing");
    let rendered = texts(svg);
    // The body marker and the note-area marker both render the number "1".
    assert!(rendered.iter().filter(|(text, _)| text == "1").count() >= 2);
    // The marker is superscript (smaller than the 11pt body text at 96 DPI).
    let body_size = rendered
        .iter()
        .find(|(text, _)| text == "Body")
        .map(|(_, size)| *size)
        .expect("body text");
    let marker_size = rendered
        .iter()
        .find(|(text, _)| text == "1")
        .map(|(_, size)| *size)
        .expect("marker");
    assert!(
        marker_size < body_size,
        "marker {marker_size} vs body {body_size}"
    );
    // A separator line is drawn.
    assert!(!line_lengths(svg).is_empty());
}

#[test]
fn endnote_renders_at_document_end() {
    let body = format!(
        "<w:p><w:r><w:t>Body</w:t></w:r><w:r><w:endnoteReference w:id=\"1\"/></w:r></w:p>\
<w:sectPr>{PAGE_SETUP}</w:sectPr>"
    );
    let rels = doc_rels(&[("rIdEn", ENDNOTES_REL, "endnotes.xml")]);
    let pages = build(
        &body,
        &rels,
        &[(
            "word/endnotes.xml",
            endnotes(&endnote(1, "endnoteRef", "EndBodyText")),
        )],
    );
    let last = pages.last().expect("a page");
    assert!(last.svg.contains("EndBodyText"), "endnote text missing");
    // Endnotes default to lowerRoman numbering.
    assert!(
        texts(&last.svg).iter().any(|(text, _)| text == "i"),
        "expected lowerRoman endnote marker"
    );
}

#[test]
fn long_footnote_continues_on_next_page() {
    // A short page: a multi-line note cannot fit alongside the body.
    let body = "<w:p><w:r><w:t>Body</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"2600\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"720\" w:left=\"1440\" w:header=\"720\" w:footer=\"360\"/></w:sectPr>";
    let rels = doc_rels(&[("rIdFn", FOOTNOTES_REL, "footnotes.xml")]);
    let long_note = format!(
        "<w:footnote w:id=\"1\">\
<w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> ContinuedNoteText</w:t></w:r></w:p>{}\
</w:footnote>",
        "<w:p><w:r><w:t>extra note line</w:t></w:r></w:p>".repeat(5)
    );
    let pages = build(
        body,
        &rels,
        &[("word/footnotes.xml", footnotes(&long_note))],
    );
    assert_eq!(pages.len(), 2, "expected the note to force a second page");
    assert!(!pages[0].svg.contains("ContinuedNoteText"));
    assert!(pages[1].svg.contains("ContinuedNoteText"));
    // Page 2 starts with a continued note, so its separator spans the full width.
    let content_width = 12240.0 / 1440.0 * 96.0 - 2.0 * 96.0;
    let lengths = line_lengths(&pages[1].svg);
    assert!(
        lengths
            .iter()
            .any(|length| (length - content_width).abs() < 1.0),
        "expected a full-width continuation separator, got {lengths:?}"
    );
}
