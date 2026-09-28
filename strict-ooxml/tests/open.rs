//! Integration tests for the public `StrictDocument` entry point.

#![allow(clippy::cast_possible_truncation, missing_docs)]

use std::io::Cursor;

use strict_ooxml::model::inline::{Inline, RunContent};
use strict_ooxml::{OpenOptions, StrictDocument};

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
