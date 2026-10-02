//! Where reading time goes on one file, with pictures — and how much of it
//! decoding actually is.
//!
//! ```text
//! cargo run --release -p strict-ooxml-pdf --example probe_pictures -- <file.pdf>
//! ```
//!
//! # What it does
//!
//! Reads one PDF three times under three `PdfLimits` and prints the wall clock
//! for each:
//!
//! 1. **as the reader behaves now** — every distinct image decoded once, cached;
//! 2. **decoding refused** (`max_image_bytes = 1`), which is the floor and
//!    therefore the **ceiling of what decoding lazily can win** for a consumer
//!    that never looks at image bytes;
//! 3. **cache ceiling at 0** — a re-decode on every draw, which is the price a
//!    design without a cache would pay.
//!
//! # Why the order matters, and why the file is read twice first
//!
//! A file of 139 MiB read from disk costs the configuration that happens to run
//! first about a second more than the others, which is the same size as the thing
//! being measured. So the file is read once to warm the page cache before any
//! measurement, and the order of the three configurations is **rotated between
//! passes** so that each one is also measured first once. Without both, the
//! numbers are a story about disk I/O.
//!
//! # What it found
//!
//! On the most picture-heavy file in `testdata/pdf` — 160 pages, 102 902 picture
//! draws naming 72 661 distinct `XObject`s — decoding is **inside the noise**:
//! the floor and the current behaviour agree, and on a text-heavy file all three
//! overlap completely. The picture cache is worth ~2 % there, and it is worth
//! that little because a cache hit clones the whole `Encoded`.
//!
//! That is the answer to «should images be fetched on demand», and it is no: the
//! ceiling on that question is zero. The numbers and the conclusion are in
//! `STAGE-8-OPEN.md` §3c Q-29. What *is* left is the content-stream interpreter,
//! and locating that needs instrumentation inside `interpret` rather than here.
//!
//! # Why this is an example and not a test
//!
//! It prints measurements and asserts nothing, for the same reason
//! `corpus_report` does: it runs on files the project does not own, and a test
//! that asserted a duration would fail on somebody else's machine. The claims it
//! supports are numbers in the registry, and the reason those numbers are ranges
//! is written there.

#![allow(clippy::print_stdout, clippy::cast_precision_loss)]

use std::path::PathBuf;
use std::time::Instant;

use strict_ooxml_pdf::{PdfDocument, PdfLimits};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: probe_pictures <file.pdf>");
    let path = PathBuf::from(path);
    let bytes = std::fs::read(&path).expect("read");
    println!(
        "{}: {} MiB",
        path.display(),
        bytes.len() as f64 / (1024.0 * 1024.0)
    );

    // Warm the file, so a configuration that happens to run first does not pay
    // for reading it off disk while the others read it from the page cache.
    std::fs::read(&path).expect("read");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let configurations: [(&str, PdfLimits); 3] = [
        (
            "now: decode each distinct image once, cached",
            PdfLimits::default(),
        ),
        (
            "decoding refused (max_image_bytes = 1): the floor",
            PdfLimits {
                // One byte, so `samples.len() > max_image_bytes` refuses every real
                // picture. Zero would mean *no limit* — the check is `>`, not `>=`.
                max_image_bytes: 1,
                ..PdfLimits::default()
            },
        ),
        (
            "decode allowed, cache ceiling 0: a re-decode per draw",
            PdfLimits {
                max_cached_image_bytes: 0,
                ..PdfLimits::default()
            },
        ),
    ];

    for pass in 1..=3 {
        // Rotated every pass, so each configuration is measured first once too.
        for (label, limits) in configurations[(pass - 1) % 3..]
            .iter()
            .chain(configurations[..(pass - 1) % 3].iter())
        {
            let started = Instant::now();
            let mut document = PdfDocument::open(&bytes, *limits).expect("open");
            let pages = document.pages().expect("pages");
            let mut glyphs = 0usize;
            let mut items = 0usize;
            let mut pictures = 0usize;
            let mut jpeg = 0usize;
            let mut raw = 0usize;
            let mut broken = 0usize;
            for page in &pages {
                for item in page.items() {
                    items += 1;
                    match item {
                        strict_ooxml_pdf::Item::Glyph(_) => glyphs += 1,
                        strict_ooxml_pdf::Item::Image(image) => {
                            pictures += 1;
                            // What the consumer is actually handed, which is the
                            // thing a lazy design would stop handing over.
                            match &image.image {
                                None => broken += 1,
                                Some(strict_ooxml_pdf::Encoded::Jpeg { .. }) => jpeg += 1,
                                Some(strict_ooxml_pdf::Encoded::Raw { .. }) => raw += 1,
                            }
                        }
                        strict_ooxml_pdf::Item::Vector(_) => {}
                    }
                }
            }
            let elapsed = started.elapsed();
            println!(
                "  pass {pass}  {label}\n    {} page(s), {glyphs} glyph(s), {pictures} \
                 picture(s), {items} item(s) in {:.2} s = {:.1} page(s)/s; {} distinct \
                 image(s), {} MiB cached; handed out: {jpeg} jpeg, {raw} raw, {broken} broken",
                pages.len(),
                elapsed.as_secs_f64(),
                pages.len() as f64 / elapsed.as_secs_f64(),
                document.distinct_images(),
                document.cached_image_bytes() as f64 / (1024.0 * 1024.0),
            );
        }
    }
}
