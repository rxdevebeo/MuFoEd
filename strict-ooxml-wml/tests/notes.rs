#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for footnotes/endnotes (`w:footnotes`/`w:endnotes`).

mod common;

use strict_ooxml_wml::model::inline::RunContent;
use strict_ooxml_wml::model::notes::NoteKind;

use common::{document_parts, parse_parts, rels, W_NS};

const FOOTNOTES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footnotes";
const ENDNOTES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/endnotes";
const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";

fn footnotes_xml() -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:footnotes xmlns:w=\"{W_NS}\">\
<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>\
<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>\
<w:footnote w:id=\"1\"><w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> note one</w:t></w:r></w:p></w:footnote>\
</w:footnotes>"
    )
    .into_bytes()
}

fn endnotes_xml() -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><w:endnotes xmlns:w=\"{W_NS}\">\
<w:endnote w:id=\"2\"><w:p><w:r><w:endnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> end one</w:t></w:r></w:p></w:endnote>\
</w:endnotes>"
    )
    .into_bytes()
}

#[test]
fn parses_footnotes_and_endnotes_parts() {
    let body =
        "<w:p><w:r><w:footnoteReference w:id=\"1\"/><w:endnoteReference w:id=\"2\"/></w:r></w:p>";
    let rels = rels(&[
        ("rIdFn", FOOTNOTES, "footnotes.xml"),
        ("rIdEn", ENDNOTES, "endnotes.xml"),
    ]);
    let parts = document_parts(
        body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/footnotes.xml", footnotes_xml()),
            ("word/endnotes.xml", endnotes_xml()),
        ],
    );
    let document = parse_parts(&parts).expect("parse");

    assert_eq!(document.footnotes.len(), 3);
    assert!(document.footnotes.separator().is_some());
    assert!(document.footnotes.continuation_separator().is_some());
    let note = document.footnotes.get(1).expect("note 1");
    assert_eq!(note.kind, NoteKind::Normal);
    assert_eq!(note.blocks.len(), 1);

    // The note body's `w:footnoteRef` is modelled.
    let text = format!("{:?}", note.blocks);
    assert!(text.contains("NoteRef"), "note body marker missing: {text}");

    assert_eq!(document.endnotes.len(), 1);
    assert_eq!(document.endnotes.get(2).unwrap().kind, NoteKind::Normal);

    assert!(document.source.footnotes.is_some());
    assert!(document.source.endnotes.is_some());
    assert!(document.support.get("w:footnotes").is_some());
    assert!(document.support.get("w:endnotes").is_some());
}

#[test]
fn parses_note_numbering_properties() {
    let body = "<w:p/>";
    let settings = format!(
        "<?xml version=\"1.0\"?><w:settings xmlns:w=\"{W_NS}\">\
<w:footnotePr><w:pos w:val=\"pageBottom\"/><w:numFmt w:val=\"lowerRoman\"/><w:numStart w:val=\"3\"/><w:numRestart w:val=\"eachPage\"/></w:footnotePr>\
<w:endnotePr><w:numFmt w:val=\"decimal\"/></w:endnotePr>\
</w:settings>"
    );
    let rels = rels(&[("rIdS", SETTINGS, "settings.xml")]);
    let parts = document_parts(
        body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/settings.xml", settings.into_bytes()),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let footnote = &document.settings.footnote_properties;
    assert_eq!(footnote.position.as_deref(), Some("pageBottom"));
    assert_eq!(footnote.num_format.as_deref(), Some("lowerRoman"));
    assert_eq!(footnote.num_start, Some(3));
    assert_eq!(footnote.num_restart.as_deref(), Some("eachPage"));
    assert_eq!(
        document.settings.endnote_properties.num_format.as_deref(),
        Some("decimal")
    );
}

#[test]
fn parses_note_ref_marker_in_run() {
    let body = "<w:p><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>";
    let rels = rels(&[("rIdFn", FOOTNOTES, "footnotes.xml")]);
    let parts = document_parts(
        body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/footnotes.xml", footnotes_xml()),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let note = document.footnotes.get(1).unwrap();
    let strict_ooxml_wml::model::Block::Paragraph(paragraph) = &note.blocks[0] else {
        panic!("expected paragraph");
    };
    let has_note_ref = paragraph.inlines.iter().any(|inline| match inline {
        strict_ooxml_wml::model::Inline::Run(run) => run
            .content
            .iter()
            .any(|content| matches!(content, RunContent::NoteRef)),
        _ => false,
    });
    assert!(
        has_note_ref,
        "expected a RunContent::NoteRef in the note body"
    );
}
