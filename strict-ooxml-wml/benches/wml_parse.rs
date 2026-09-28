//! `wml_parse` — end-to-end parse benchmark (STAGE-2 S2.18).
//!
//! Builds synthetic Strict `.docx` packages in memory and measures
//! `Package::open_reader` + `parse_document` for 10/100/500-page documents.

#![allow(clippy::cast_possible_truncation)]
#![allow(missing_docs)]

use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::{parse_document, ParseOptions};

const W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

/// Builds a Strict `document.xml` body with `paragraphs` paragraphs.
fn document_xml(paragraphs: usize) -> Vec<u8> {
    let mut xml = String::with_capacity(paragraphs * 120 + 256);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>");
    let _ = write!(xml, "<w:document xmlns:w=\"{W_NS}\"><w:body>");
    for index in 0..paragraphs {
        let _ = write!(
            xml,
            "<w:p><w:pPr><w:jc w:val=\"start\"/></w:pPr><w:r><w:t xml:space=\"preserve\">Paragraph number {index} with some representative body text.</w:t></w:r></w:p>"
        );
    }
    xml.push_str("</w:body></w:document>");
    xml.into_bytes()
}

/// Builds a minimal Strict package containing the given document part.
fn build_strict_docx(document: &[u8]) -> Vec<u8> {
    const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.ms-word.document.main+xml\"/></Types>";
    const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("word/document.xml", document),
    ])
}

/// Builds a stored (uncompressed) ZIP archive.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        let crc = crc32(content);
        push_local(&mut local, name, crc, content.len(), content);
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

fn parse(bytes: &[u8]) {
    let package =
        Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open package");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse document");
    std::hint::black_box(&document);
}

fn bench_wml_parse(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("wml_parse");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));
    for pages in [10usize, 100, 500] {
        let xml = document_xml(pages * 10);
        let docx = build_strict_docx(&xml);
        group.throughput(Throughput::Bytes(docx.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(pages),
            &docx,
            |bencher, docx| {
                bencher.iter(|| parse(docx));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_wml_parse);
criterion_main!(benches);
