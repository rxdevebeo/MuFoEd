#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for footnotes/endnotes (`w:footnotes`/`w:endnotes`).

mod common;

use strict_ooxml_wml::model::drawing::MediaKind;
use strict_ooxml_wml::model::inline::RunContent;
use strict_ooxml_wml::model::notes::NoteKind;

use common::{document_parts, parse_parts, rels, A_NS, PIC_NS, R_NS, WP_NS, W_NS};

const FOOTNOTES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footnotes";
const ENDNOTES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/endnotes";
const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";
const IMAGE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/image";

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

/// AUD-48: a picture inside a footnote lands in `document.media`.
#[test]
fn footnote_media_is_merged_into_document() {
    let footnotes = format!(
        "<?xml version=\"1.0\"?>\
<w:footnotes xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" xmlns:wp=\"{WP_NS}\" \
xmlns:a=\"{A_NS}\" xmlns:pic=\"{PIC_NS}\">\
<w:footnote w:id=\"1\"><w:p><w:r><w:drawing><wp:inline>\
<wp:extent cx=\"914400\" cy=\"457200\"/>\
<wp:docPr id=\"1\" name=\"fn\"/>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"i.png\"/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg\"/></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"457200\"/></a:xfrm></pic:spPr>\
</pic:pic></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p></w:footnote>\
</w:footnotes>"
    )
    .into_bytes();
    let doc_rels = rels(&[("rIdFn", FOOTNOTES, "footnotes.xml")]);
    let fn_rels = rels(&[("rIdImg", IMAGE, "media/fn.png")]);
    let parts = document_parts(
        "<w:p><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>",
        &[
            ("word/_rels/document.xml.rels", doc_rels),
            ("word/footnotes.xml", footnotes),
            ("word/_rels/footnotes.xml.rels", fn_rels),
            ("word/media/fn.png", vec![0x89, b'P', b'N', b'G']),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    assert_eq!(document.media.len(), 1);
    let item = document.media.iter().next().unwrap();
    assert_eq!(item.kind, MediaKind::Png);
    assert_eq!(item.part.as_str(), "/word/media/fn.png");
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
