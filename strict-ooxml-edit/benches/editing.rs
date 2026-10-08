//! `editing` — the cost of editor transactions against document size
//! (`docs/EDITING_ROADMAP_2026-10-08.md` §0.1).
//!
//! Each scenario times only the transactions: the document is parsed once per
//! size, and the copy and `Editor::new` of every iteration are outside the
//! measured span (`iter_custom`). A transaction whose cost is independent of
//! the document is the roadmap's stage-1 target, so the three sizes are the
//! point of the benchmark.

#![allow(missing_docs)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{Address, Edit, EditLimits, Editor, FormatPatch};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Block, Document, TriState};
use strict_ooxml_wml::{parse_document, ParseOptions};

const SIZES: [usize; 3] = [100, 1_000, 10_000];

/// `paragraphs` body paragraphs, with a two-cell table every hundred.
fn document(paragraphs: usize) -> Document {
    let mut body = String::new();
    for index in 0..paragraphs {
        let _ = write!(
            body,
            "<w:p><w:r><w:t xml:space=\"preserve\">Paragraph {index} with some representative body text.</w:t></w:r></w:p>"
        );
        if index % 100 == 99 {
            body.push_str(
                "<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
                 <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>\
                 <w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
            );
        }
    }
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

/// Times `steps` against a fresh editor over a copy of `document`, `iters` times.
fn timed(document: &Document, iters: u64, steps: impl Fn(&mut Editor<'_>)) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut copy = document.clone();
        let mut editor = Editor::new(&mut copy, EditLimits::default()).expect("editor");
        let start = Instant::now();
        steps(&mut editor);
        total += start.elapsed();
        std::hint::black_box(editor.document());
    }
    total
}

/// The body paragraph at `index`, or the nearest one before it.
fn paragraph_near(document: &Document, index: usize) -> usize {
    let mut at = index;
    while at > 0 && !matches!(document.body.blocks.get(at), Some(Block::Paragraph(_))) {
        at -= 1;
    }
    at
}

fn transact(editor: &mut Editor<'_>, edit: Edit) {
    let revision = editor.revision();
    editor.transact(revision, &[edit]).expect("transact");
}

/// Twenty one-character insertions into the middle paragraph.
fn typing(editor: &mut Editor<'_>, at: usize) {
    for offset in 0..20 {
        transact(
            editor,
            Edit::Text {
                at: Address::body(at),
                range: offset..offset,
                text: "x".to_owned(),
            },
        );
    }
}

/// Ten bold patches over overlapping three-character ranges.
fn formatting(editor: &mut Editor<'_>, at: usize) {
    for step in 0..10 {
        let patch = FormatPatch {
            bold: Some(TriState::On),
            ..FormatPatch::default()
        };
        let range = step..step + 3;
        let at = Address::body(at);
        transact(editor, Edit::Format { at, range, patch });
    }
}

/// Five splits of a paragraph, each joined back.
fn split_join(editor: &mut Editor<'_>, index: usize) {
    for _ in 0..5 {
        let (at, offset) = (Address::body(index), 4);
        transact(editor, Edit::Split { at, offset });
        let at = Address::body(index);
        transact(editor, Edit::Join { at });
    }
}

/// Twenty undos and twenty redos after [`typing`], timing only those.
fn undo_redo(document: &Document, iters: u64, at: usize) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut copy = document.clone();
        let mut editor = Editor::new(&mut copy, EditLimits::default()).expect("editor");
        typing(&mut editor, at);
        let start = Instant::now();
        for _ in 0..20 {
            let revision = editor.revision();
            editor.undo(revision).expect("undo");
        }
        for _ in 0..20 {
            let revision = editor.revision();
            editor.redo(revision).expect("redo");
        }
        total += start.elapsed();
    }
    total
}

fn bench_editing(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("editing");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));
    for size in SIZES {
        let document = document(size);
        let at = paragraph_near(&document, document.body.blocks.len() / 2);
        let doc = &document;
        group.bench_function(BenchmarkId::new("typing_20", size), |b| {
            b.iter_custom(|iters| timed(doc, iters, |editor| typing(editor, at)));
        });
        group.bench_function(BenchmarkId::new("format_10", size), |b| {
            b.iter_custom(|iters| timed(doc, iters, |editor| formatting(editor, at)));
        });
        group.bench_function(BenchmarkId::new("split_join_5", size), |b| {
            b.iter_custom(|iters| timed(doc, iters, |editor| split_join(editor, at)));
        });
        group.bench_function(BenchmarkId::new("undo_redo_20", size), |b| {
            b.iter_custom(|iters| undo_redo(doc, iters, at));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_editing);
criterion_main!(benches);
