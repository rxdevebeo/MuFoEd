//! XML scanning benchmarks (rework P1 / R1).
//!
//! Guards against a regression to quadratic location bookkeeping by reporting
//! throughput at several element counts.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

fn make_flat(n: usize) -> Vec<u8> {
    let mut xml = String::from("<w:document xmlns:w=\"urn:w\">");
    for _ in 0..n {
        xml.push_str("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    }
    xml.push_str("</w:document>");
    xml.into_bytes()
}

fn scan(bytes: &[u8]) -> usize {
    let mut reader = XmlReader::new(
        bytes,
        PartId::new("/word/document.xml"),
        &ResourceLimits::default(),
    )
    .expect("valid reader");
    let mut events = 0usize;
    loop {
        match reader.next_event().expect("valid xml") {
            XmlEvent::Eof => break,
            _ => events += 1,
        }
    }
    events
}

fn xml_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("xml_scan");
    for n in [1_000usize, 8_000, 32_000, 128_000] {
        let bytes = make_flat(n);
        group.bench_function(format!("n={n}"), |b| {
            b.iter(|| black_box(scan(black_box(&bytes))));
        });
    }
    group.finish();
}

fn xml_deep(c: &mut Criterion) {
    let mut xml = String::from("<w:document xmlns:w=\"urn:w\">");
    for _ in 0..200 {
        xml.push_str("<w:p>");
    }
    for _ in 0..200 {
        xml.push_str("</w:p>");
    }
    xml.push_str("</w:document>");
    let bytes = xml.into_bytes();
    c.bench_function("xml_deep/depth=200", |b| {
        b.iter(|| black_box(scan(black_box(&bytes))));
    });
}

criterion_group!(benches, xml_scan, xml_deep);
criterion_main!(benches);
