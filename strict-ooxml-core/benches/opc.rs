//! OPC opening and part-reading benchmarks (rework P2/P3/P5).

#![allow(missing_docs)]

use std::hint::black_box;
use std::io::Cursor;

use criterion::{criterion_group, criterion_main, Criterion};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;

mod common;

use common::build_zip;

const CONTENT_TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
const ROOT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const DOCUMENT: &str = r#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body/></w:document>"#;

fn docx(extra_parts: usize) -> Vec<u8> {
    let mut owned: Vec<(String, Vec<u8>, bool)> = vec![
        (
            "[Content_Types].xml".to_owned(),
            CONTENT_TYPES.as_bytes().to_vec(),
            true,
        ),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELS.as_bytes().to_vec(),
            true,
        ),
        (
            "word/document.xml".to_owned(),
            DOCUMENT.as_bytes().to_vec(),
            true,
        ),
    ];
    for i in 0..extra_parts {
        owned.push((format!("word/part{i}.xml"), b"<a/>".to_vec(), true));
    }
    let entries: Vec<(&str, &[u8], bool)> = owned
        .iter()
        .map(|(name, data, deflate)| (name.as_str(), data.as_slice(), *deflate))
        .collect();
    build_zip(&entries)
}

fn opc_open(c: &mut Criterion) {
    let mut group = c.benchmark_group("opc_open");
    for parts in [0usize, 47] {
        let bytes = docx(parts);
        group.bench_function(format!("parts={}", parts + 3), |b| {
            b.iter(|| {
                let options = OpenOptions::default();
                let package =
                    Package::open_reader(Cursor::new(black_box(bytes.clone())), &options).unwrap();
                black_box(package.parts().count())
            });
        });
    }
    group.finish();
}

fn part_read(c: &mut Criterion) {
    let payload = vec![b'x'; 1 << 20];
    let mut owned: Vec<(String, Vec<u8>, bool)> = vec![
        (
            "[Content_Types].xml".to_owned(),
            CONTENT_TYPES.as_bytes().to_vec(),
            true,
        ),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELS.as_bytes().to_vec(),
            true,
        ),
        (
            "word/document.xml".to_owned(),
            DOCUMENT.as_bytes().to_vec(),
            true,
        ),
        ("word/data.bin".to_owned(), payload, true),
    ];
    let entries: Vec<(&str, &[u8], bool)> = owned
        .iter_mut()
        .map(|(name, data, deflate)| (name.as_str(), data.as_slice(), *deflate))
        .collect();
    let archive = build_zip(&entries);
    let options = OpenOptions::default();
    let package = Package::open_reader(Cursor::new(archive), &options).unwrap();
    let id = PartId::new("/word/data.bin");
    c.bench_function("part_read/deflate_1MiB", |b| {
        b.iter(|| {
            let bytes = package.read_part(black_box(&id)).unwrap();
            black_box(bytes.len())
        });
    });
}

criterion_group!(benches, opc_open, part_read);
criterion_main!(benches);
