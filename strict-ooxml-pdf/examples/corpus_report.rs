//! A report over a directory of PDFs that are **not ours**.
//!
//! ```text
//! cargo run -p strict-ooxml-pdf --example corpus_report -- testdata/pdf
//! cargo run -p strict-ooxml-pdf --features raster --example corpus_report -- testdata/pdf
//! ```
//!
//! # Why this is an example and not a test
//!
//! The corpus is a bug-finding instrument, not a fixture: the files are somebody
//! else's, they grow, and they are not part of the build. A test that skipped
//! when the directory was absent would be the project's own rule about checks
//! that cannot fire; a test that asserted nothing about files it does not own
//! would be worse. So this is a tool somebody runs on purpose, and the permanent
//! guards it earned are `tests/resources.rs` (the two shapes of a resource
//! dictionary) and `tests/raster.rs`.
//!
//! # What to read the output for
//!
//! Three columns are the ones that find bugs, and all three are "a document
//! said something and we did not get it":
//!
//! - **`unreadable`** — pages with glyphs and no character for any of them. A
//!   font we cannot map, and the reason a converter would send the page to a
//!   model.
//! - **`no text`** — pages where no text came out at all, ink or not. With ink on
//!   the page that is a page we cannot read; without ink it is a picture-only
//!   page, which is the case the recovery path exists for.
//! - **`losses`** — what the reader said it could not carry, by kind. A kind that
//!   appears on every file is a feature we do not have; a kind that appears on
//!   one file is worth opening that file and looking.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits, TextLayer};

fn usage() -> ! {
    eprintln!(
        "usage: cargo run -p strict-ooxml-pdf --example corpus_report -- <directory> [--raster] [--losses]"
    );
    std::process::exit(2);
}

/// Every PDF under dir, in a stable order.
///
/// The walk is recursive: a document set kept in a subdirectory - a reference
/// manual split into chunks, say - is as much a part of the corpus as a file at
/// the top of it, and a tool that only saw the first level would quietly report
/// on less than it was given.
fn corpus(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// One file's numbers, and the reading of them.
struct Reading {
    name: String,
    pages: usize,
    glyphs: usize,
    unmapped: usize,
    unreadable: usize,
    no_text: usize,
    blank: usize,
    /// Pictures drawn, how many of them failed to decode, and how many distinct
    /// `XObject`s those draws named.
    ///
    /// The last number is the one that says whether the reader decodes a picture
    /// once or once per draw. A document that draws 102 902 pictures naming 1 160
    /// of them is a document about pictures, and a reader that inflates the same
    /// stream a hundred times is spending its time there (Q-26). The opposite
    /// ratio is just as real: a photo album draws almost every picture once, and
    /// no cache can help a decode that was going to happen anyway.
    pictures: usize,
    broken_pictures: usize,
    distinct_pictures: usize,
    /// Bytes of decoded pictures the reader is still holding, which is the number
    /// the cache ceiling (`PdfLimits::max_cached_image_bytes`) is about.
    cached_bytes: usize,
    millis: u128,
    /// `(kind, count)` for the losses this file reported.
    losses: Vec<(String, usize)>,
    /// `((kind, what the reader said), count)`, for the same losses. The counts
    /// here are the report's own, so a kind that appears on every file and a kind
    /// that appears on one are told apart by the number, and the *reason* is one
    /// line away instead of a file opening away.
    details: Vec<((String, String), usize)>,
    /// `file page` for every page a converter would hand to a model.
    candidates: Vec<String>,
}

fn read(path: &Path) -> Option<Reading> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = std::fs::read(path).ok()?;
    let started = Instant::now();
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).ok()?;
    let pages = document.pages().ok()?;
    let mut reading = Reading {
        name,
        pages: pages.len(),
        glyphs: 0,
        unmapped: 0,
        unreadable: 0,
        no_text: 0,
        blank: 0,
        pictures: 0,
        broken_pictures: 0,
        distinct_pictures: 0,
        cached_bytes: 0,
        millis: 0,
        losses: Vec::new(),
        details: Vec::new(),
        candidates: Vec::new(),
    };
    for (index, page) in pages.iter().enumerate() {
        reading.glyphs += page
            .items()
            .iter()
            .filter(|item| matches!(item, Item::Glyph(_)))
            .count();
        reading.unmapped += page
            .items()
            .iter()
            .filter(|item| matches!(item, Item::Glyph(glyph) if !glyph.mapped))
            .count();
        for item in page.items() {
            if let Item::Image(image) = item {
                reading.pictures += 1;
                if image.missing.is_some() || image.image.is_none() {
                    reading.broken_pictures += 1;
                }
            }
        }
        match page.text_layer() {
            TextLayer::Readable => {}
            TextLayer::Unreadable => reading.unreadable += 1,
            TextLayer::Absent => reading.no_text += 1,
        }
        if !page.has_ink() {
            reading.blank += 1;
        }
        if page.text_layer() != TextLayer::Readable && page.has_ink() {
            reading
                .candidates
                .push(format!("{} page {}", reading.name, index + 1));
        }
    }
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    // The same losses, but with the reader's own words: "which kind" says there is
    // a problem, and only "what did it say" says where to look.
    let mut details: BTreeMap<(String, String), usize> = BTreeMap::new();
    for loss in document.report().losses() {
        *kinds.entry(loss.id.clone()).or_default() += 1;
        *details
            .entry((loss.id.clone(), loss.detail.clone()))
            .or_default() += 1;
    }
    reading.losses = kinds.into_iter().collect();
    reading.details = details.into_iter().collect();
    reading.distinct_pictures = document.distinct_images();
    reading.cached_bytes = document.cached_image_bytes();
    reading.millis = started.elapsed().as_millis();
    Some(reading)
}

/// One file's row of numbers.
///
/// Its own function, because the row and the header must agree on their columns
/// and a table nobody can line up is a table nobody reads.
fn print_row(reading: &Reading) {
    println!(
        "{:<40} {:>5} {:>8} {:>7} {:>5} {:>4} {:>6} {:>8} {:>8} {:>7}",
        reading.name,
        reading.pages,
        reading.glyphs,
        reading.unreadable,
        reading.no_text,
        reading.blank,
        reading.pictures,
        reading.broken_pictures,
        reading.distinct_pictures,
        reading.millis
    );
}

/// How the reading time and the picture cache add up.
///
/// Integer arithmetic, because a byte count is not a number to hand to a float:
/// the point of the line is to be read exactly.
fn print_picture_summary(drawn: usize, distinct: usize, cached: usize) {
    const MIB: usize = 1024 * 1024;
    println!(
        "pictures: {drawn} drawn, {distinct} distinct, {} MiB {} KiB still held by the \
         picture cache",
        cached / MIB,
        (cached % MIB) / 1024
    );
}

/// What the reader said it could not carry: a count per kind, and under each kind
/// the reasons it gave, most frequent first.
///
/// A kind with one reason is one bug. A kind with a hundred reasons is a missing
/// feature wearing a loss record as a disguise, and the two look identical in a
/// column of counts.
fn print_losses(kinds: &BTreeMap<String, usize>, details: &BTreeMap<(String, String), usize>) {
    if kinds.is_empty() {
        return;
    }
    println!("what the reader said it could not carry, by kind:");
    let mut kinds: Vec<(&String, &usize)> = kinds.iter().collect();
    kinds.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
    for (id, count) in kinds {
        println!("  {count:>5}  {id}");
        let mut reasons: Vec<(&String, &usize)> = details
            .iter()
            .filter(|((kind, _), _)| kind == id)
            .map(|((_, reason), count)| (reason, count))
            .collect();
        reasons.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
        for (reason, times) in reasons.iter().take(6) {
            println!("           {times:>5}  {reason}");
        }
        if reasons.len() > 6 {
            println!("           ... and {} more", reasons.len() - 6);
        }
    }
}

/// One file's own losses, under its row.
///
/// The aggregate answers "what did the reader lose across the corpus"; this
/// answers "in which file", which is the question that has to be answered before
/// a single one of them is opened.
fn print_file_losses(reading: &Reading) {
    if reading.losses.is_empty() {
        return;
    }
    println!("  losses in {}:", reading.name);
    let kinds: BTreeMap<String, usize> = reading.losses.iter().cloned().collect();
    let details: BTreeMap<(String, String), usize> = reading
        .details
        .iter()
        .map(|(key, count)| (key.clone(), *count))
        .collect();
    print_losses(&kinds, &details);
    println!();
}

fn main() {
    let mut raster = false;
    let mut per_file_losses = false;
    let mut directory: Option<PathBuf> = None;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--raster" => raster = true,
            // Which file a loss is in: the aggregate says a reader met 53 unknown
            // operators, and that is one question too many to open 46 files for.
            "--losses" => per_file_losses = true,
            "--help" | "-h" => usage(),
            other => directory = Some(PathBuf::from(other)),
        }
    }
    let Some(directory) = directory else { usage() };
    let files = corpus(&directory);
    if files.is_empty() {
        eprintln!("no PDFs in {}", directory.display());
        usage();
    }

    println!(
        "{:<40} {:>5} {:>8} {:>7} {:>5} {:>4} {:>6} {:>8} {:>8} {:>7}",
        "file",
        "pages",
        "glyphs",
        "unread",
        "no tx",
        "blank",
        "pics",
        "broken",
        "distinct",
        "read ms"
    );
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut totals = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut candidates: Vec<String> = Vec::new();
    let mut details: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut drawn = 0usize;
    let mut distinct = 0usize;
    let mut cached = 0usize;
    for path in &files {
        let Some(reading) = read(path) else {
            println!("{:<40} refused or unreadable", path.display());
            totals.8 += 1;
            continue;
        };
        for (kind, count) in &reading.losses {
            *kinds.entry(kind.clone()).or_default() += count;
        }
        for (key, count) in &reading.details {
            *details.entry(key.clone()).or_default() += count;
        }
        candidates.extend(reading.candidates.clone());
        totals.0 += reading.pages;
        totals.1 += reading.glyphs;
        totals.2 += reading.unmapped;
        totals.3 += reading.unreadable;
        totals.4 += reading.no_text;
        totals.5 += reading.blank;
        totals.6 += reading.pictures;
        totals.7 += reading.broken_pictures;
        drawn += reading.pictures;
        distinct += reading.distinct_pictures;
        cached += reading.cached_bytes;
        print_row(&reading);
        if per_file_losses {
            print_file_losses(&reading);
        }
    }

    println!();
    println!(
        "{} file(s): {} page(s), {} glyph(s), {} unmapped, {} unreadable, {} without text, \
         {} blank, {} picture(s) of which {} broken, {} refused",
        files.len(),
        totals.0,
        totals.1,
        totals.2,
        totals.3,
        totals.4,
        totals.5,
        totals.6,
        totals.7,
        totals.8
    );
    println!();
    print_picture_summary(drawn, distinct, cached);
    println!();
    print_losses(&kinds, &details);
    print_candidates(&candidates);
    if raster {
        raster_pages(&files);
    }
}

/// The pages a converter would offer to a model, and the first few of them.
fn print_candidates(candidates: &[String]) {
    if candidates.is_empty() {
        return;
    }
    println!();
    println!(
        "pages a converter would hand to a model (ink, no readable text): {}",
        candidates.len()
    );
    for candidate in candidates.iter().take(20) {
        println!("  {candidate}");
    }
    if candidates.len() > 20 {
        println!("  ... and {} more", candidates.len() - 20);
    }
}

/// The rasterizer half of the report, which only exists with the `raster` feature.
#[cfg(feature = "raster")]
fn raster_pages(files: &[PathBuf]) {
    println!();
    println!("rasterizer (feature `raster`) on the first page of each file:");
    for path in files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        match strict_ooxml_pdf::raster::Rasterizer::new(&bytes, PdfLimits::default()) {
            Ok(rasterizer) => {
                let options = strict_ooxml_pdf::raster::RasterOptions {
                    scale: 1.0,
                    ..strict_ooxml_pdf::raster::RasterOptions::default()
                };
                let started = Instant::now();
                match rasterizer.page_png(1, &options) {
                    Ok(png) => println!(
                        "  {name:<44} {:>8} KiB in {:>6} ms",
                        png.len() / 1024,
                        started.elapsed().as_millis()
                    ),
                    Err(error) => println!("  {name:<44} failed: {error}"),
                }
            }
            Err(error) => println!("  {name:<44} no rasterizer: {error}"),
        }
    }
}

/// Without the `raster` feature there is nothing to report here, and saying so
/// beats a flag that silently does nothing.
#[cfg(not(feature = "raster"))]
fn raster_pages(_files: &[PathBuf]) {
    println!();
    println!("(pass --raster and build with `--features raster` for the rasterizer half)");
}
