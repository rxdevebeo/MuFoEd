//! Integration tests for the public `StrictDocument` entry point.

#![allow(clippy::cast_possible_truncation, missing_docs)]

use std::io::Cursor;

use strict_ooxml::model::inline::{Inline, RunContent};
use strict_ooxml::model::values::Space;
use strict_ooxml::{write_package, OpenOptions, StrictDocument, WriteOptions};

const W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const W_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn docx(namespace: &str, body: &str) -> Vec<u8> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{namespace}\"><w:body>{body}</w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.ms-word.document.main+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ])
}

#[test]
fn opens_and_exposes_model_and_support() {
    let bytes = docx(W_STRICT, "<w:p><w:r><w:t>Hello</w:t></w:r></w:p>");
    let document = StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
        .expect("open strict document");

    let paragraph = document.document().body.blocks[0]
        .as_paragraph()
        .expect("paragraph");
    let Inline::Run(run) = &paragraph.inlines[0] else {
        panic!("expected run");
    };
    let RunContent::Text(text) = &run.content[0] else {
        panic!("expected text");
    };
    assert_eq!(text.text, "Hello");
    // A plain paragraph uses no optional mechanisms, so the summary may be empty
    // but must always be available.
    assert!(document.support_debug().contains("support:"));
}

#[test]
fn rejects_transitional_under_strict_only() {
    let bytes = docx(W_TRANSITIONAL, "<w:p/>");
    let result = StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default());
    assert!(result.is_err(), "Transitional must be rejected");
}

#[cfg(feature = "report")]
#[test]
fn report_contains_unsupported_mechanism() {
    let bytes = docx(W_STRICT, "<w:altChunk/>");
    let document = StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
        .expect("open strict document");
    let report = document.support_report();
    assert_eq!(report.file, "<reader>");
    let chunk = report
        .features
        .iter()
        .find(|feature| feature.feature_id == "w:altChunk")
        .expect("altChunk reported");
    assert_eq!(chunk.status, strict_ooxml::FeatureStatus::Unsupported);
    assert!(!chunk.locations.is_empty());
    assert!(document
        .report_json()
        .contains("\"feature_id\": \"w:altChunk\""));
    assert!(document.report_text().contains("w:altChunk"));
    assert!(report.has_critical_problems());
}

#[cfg(feature = "report")]
#[test]
fn report_file_name_can_be_overridden() {
    let bytes = docx(W_STRICT, "<w:p/>");
    let document = StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
        .expect("open strict document")
        .with_file_name("named.docx");
    assert_eq!(document.file(), "named.docx");
    assert!(document.report_json().contains("\"file\": \"named.docx\""));
}

#[cfg(feature = "svg")]
#[test]
fn renders_pages_to_svg() {
    let bytes = docx(W_STRICT, "<w:p><w:r><w:t>Hello</w:t></w:r></w:p>");
    let document = StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
        .expect("open strict document");
    let pages = document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .expect("render");
    assert_eq!(pages.len(), 1);
    assert!(pages[0].svg.contains("Hello"), "{}", pages[0].svg);
    assert!(document.render_page_svg(0).unwrap().contains("<svg "));
    assert!(document.render_page_svg(99).is_err());
    assert_eq!(document.render_all_svg().unwrap().len(), 1);
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        push_local(&mut local, name, crc32(content), content.len(), content);
    }
    let cd_offset = local.len() as u32;
    for ((name, content), offset) in entries.iter().zip(offsets) {
        push_central(&mut central, name, crc32(content), content.len(), offset);
    }
    let cd_size = central.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn push_local(out: &mut Vec<u8>, name: &str, crc: u32, size: usize, content: &[u8]) {
    out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(content);
}

fn push_central(out: &mut Vec<u8>, name: &str, crc: u32, size: usize, offset: u32) {
    out.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[test]
fn an_edit_reaches_the_written_package() {
    let mut opened = StrictDocument::open_path(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../strict-ooxml-core/tests/strict/strict-text.docx"
        ),
        &OpenOptions::default(),
    )
    .expect("the fixture opens");

    let before = opened.document().body.blocks.len();
    opened
        .document_mut()
        .body
        .blocks
        .push(strict_ooxml_wml::model::block::Block::Paragraph(
            strict_ooxml_wml::model::block::Paragraph {
                props: strict_ooxml_wml::model::props::ParagraphProperties::default(),
                inlines: vec![strict_ooxml_wml::model::inline::Inline::Run(
                    strict_ooxml_wml::model::inline::Run {
                        props: strict_ooxml_wml::model::props::RunProperties::default(),
                        content: vec![strict_ooxml_wml::model::inline::RunContent::Text(
                            strict_ooxml_wml::model::inline::TextNode {
                                text: "added by an edit".to_owned(),
                                space: Space::Default,
                            },
                        )],
                        revision: None,
                        location: strict_ooxml_core::error::SourceLocation::unknown(),
                    },
                )],
                rsids: strict_ooxml::model::values::Rsids::default(),
                para_id: None,
                text_id: None,
                revision: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            },
        ));

    assert_eq!(
        opened.document().body.blocks.len(),
        before + 1,
        "the edit went in"
    );

    let written = write_package(
        opened.document(),
        Some(opened.package()),
        &WriteOptions::default(),
    )
    .expect("the edited document writes");

    // The package is deflated, so asserting on its bytes would prove nothing.
    // What matters is that the edit survives the cycle, which is the whole
    // claim `document_mut` makes.
    let reopened =
        StrictDocument::open_reader(Cursor::new(&written.bytes), &OpenOptions::default())
            .expect("the written package reopens");
    assert_eq!(
        reopened.document().body.blocks.len(),
        before + 1,
        "the edited paragraph is still there after write and read"
    );
}
