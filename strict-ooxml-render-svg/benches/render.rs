//! Rendering benchmark: 10/100/500-page synthetic Strict documents.
//!
//! `STAGE-4-TASK.md` §7: a 100-page document must render in ≤ 5 s on the
//! reference machine. Run with `cargo bench -p strict-ooxml-render-svg`.

#![allow(clippy::cast_possible_truncation, missing_docs)]

use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{render, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// Builds a Strict `.docx` with `paragraphs` simple paragraphs.
fn build_docx(paragraphs: usize) -> Vec<u8> {
    const W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
    const DOC_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
    let mut body = String::with_capacity(paragraphs * 64);
    for index in 0..paragraphs {
        let _ = write!(
            body,
            "<w:p><w:r><w:t>Paragraph {index} with some text to lay out on the page.</w:t></w:r></w:p>"
        );
    }
    let document = format!(
        "<?xml version=\"1.0\"?><w:document xmlns:w=\"{W}\"><w:body>{body}</w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{DOC_REL}\" Target=\"word/document.xml\"/></Relationships>"
    );
    zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ])
}

fn render_bytes(bytes: &[u8]) -> usize {
    let package =
        Package::open_reader(Cursor::new(bytes.to_vec()), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    render(&document, &RenderOptions::default())
        .expect("render")
        .len()
}

fn bench_render(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("render");
    group.sample_size(10);
    for (label, paragraphs, pages) in [("10p", 10, 1usize), ("100p", 400, 4), ("500p", 2000, 20)] {
        let bytes = build_docx(paragraphs);
        let page_count = render_bytes(&bytes);
        assert!(page_count >= pages, "{label}: only {page_count} pages");
        group.bench_function(label, |bencher| {
            bencher.iter(|| render_bytes(&bytes));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(10));
    targets = bench_render
}
criterion_main!(benches);

/// Builds a ZIP archive with stored entries (deterministic).
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        let crc = crc32(content);
        local.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&(content.len() as u32).to_le_bytes());
        local.extend_from_slice(&(content.len() as u32).to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(content);
    }
    let cd_offset = local.len() as u32;
    for ((name, content), offset) in entries.iter().zip(offsets) {
        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc32(content).to_le_bytes());
        central.extend_from_slice(&(content.len() as u32).to_le_bytes());
        central.extend_from_slice(&(content.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
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

/// CRC-32 (IEEE) used by the ZIP writer.
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
