//! Look at a page of the **PDF** pipeline instead of a number: the WPS
//! reference, our PDF rasterized by `hayro`, the two drawn over each other, and
//! the ink bands of both.
//!
//! ```text
//! cargo run -p strict-ooxml-pdf --features raster --example pdf_page_diff -- \
//!     <document> <page>… [--out <dir>]
//! ```
//!
//! # Why this exists, and why it is a twin
//!
//! `strict-ooxml-render-svg`'s `page_diff` answers *what differs* for the SVG
//! backend, because a gate's failure message answers *which bound* and that is
//! not the same question. Stage 8 has two renderers and two pixel gates (SC-6),
//! so it has two questions to ask of a page, and the pair has to print the same
//! things: otherwise a number quoted from one of them cannot be read against a
//! picture from the other.
//!
//! The path through the page is the whole pipeline, not the rasterizer alone:
//! `docx -> place_pages -> PDF -> PdfDocument -> Rasterizer -> PNG`. The last
//! step is `hayro` reading back the file this workspace wrote, so what the tool
//! shows is what a reader of our PDF would see, not a second rendering of the
//! layout.
//!
//! It deliberately does not decide whether a page passes. The verdict belongs to
//! `tests/pdf_pixels.rs`, and a second copy of its arithmetic in an example is a
//! second thing to keep in step with it.
//!
//! # Scale
//!
//! The reference PNGs are 816 × 1056 — Letter at 96 dpi. The page is rasterized
//! at whatever scale puts it on **exactly** that grid and the outermost pixel is
//! cropped if the rasterizer rounds up, so the two images are pixel-for-pixel
//! comparable rather than approximately the same size.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_fidelity::{crop, decode_png_gray, load_png_gray, ssim, Gray};
use strict_ooxml_pdf::raster::{RasterOptions, Rasterizer};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The graded fixtures, in the order the gate walks them.
fn strict_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

fn reference_pages(dir: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("a reference directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect();
    pages.sort();
    pages
}

/// The page's size in points.
///
/// `Rasterizer::page_size` reports pixels at the scale it is given, so asking at
/// scale 1.0 asks for points — one point per pixel, no conversion to undo
/// afterwards.
fn page_points(rasterizer: &Rasterizer, number: usize) -> (f64, f64) {
    let size = rasterizer
        .page_size(
            number,
            &RasterOptions {
                scale: 1.0,
                ..RasterOptions::default()
            },
        )
        .expect("page size");
    (f64::from(size.width), f64::from(size.height))
}

/// The scale that puts a page of `points` onto a `width × height` grid.
///
/// `hayro` computes its pixmap as `floor(points * scale)`, so the scale that
/// lands *on* a grid is `width / points` up to a floating-point hair, and one
/// pixel short is as likely as one pixel long. Half a pixel of slack removes the
/// question: the result is at least the grid in both axes, and
/// [`crop`](strict_ooxml_fidelity::crop) takes the window back to it.
///
/// The grid is not the same for every fixture, and that is a fact about the
/// corpus rather than a bug: the references are 816 × 1056, which is Letter at
/// 96 dpi for a page that *is* Letter, and 144 dpi for `strict-stage5b`, whose
/// page is 408 × 528 pt. Asking each page for the scale that reproduces its own
/// reference is what keeps the comparison pixel-for-pixel.
fn scale_for(points: (f64, f64), width: usize, height: usize) -> f64 {
    let by_width = (width as f64 + 0.5) / points.0;
    let by_height = (height as f64 + 0.5) / points.1;
    by_width.min(by_height)
}

fn write_gray(path: &Path, page: &Gray) {
    let mut bytes = Vec::with_capacity(page.pixels().len() * 3);
    for value in page.pixels() {
        let level = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        bytes.extend_from_slice(&[level, level, level]);
    }
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        page.width() as u32,
        page.height() as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(&bytes)
        .expect("png data");
}

/// Both images' ink drawn over each other: red ours, blue the reference.
fn overlay(reference: &Gray, candidate: &Gray) -> Gray {
    const INK: f64 = 0.5;
    let pixels = reference
        .pixels()
        .iter()
        .zip(candidate.pixels())
        .map(
            |(reference, candidate)| match (reference < &INK, candidate < &INK) {
                (false, false) => 1.0,
                (true, true) => 0.0,
                (true, false) => 1.0 - candidate,
                (false, true) => 1.0 - reference,
            },
        )
        .collect();
    Gray::new(reference.width(), reference.height(), pixels)
}

/// The contiguous runs of rows that carry ink, as `(first, last, x0, x1)`, each
/// with the columns its ink spans.
///
/// The gate's `extent_delta` reads the first and the last of these; printing all
/// of them, with their horizontal span, is what turns «the bottom edge is 16 px
/// high» into «the gap between blocks is 4 px short, four times over».
fn ink_bands(page: &Gray) -> Vec<(usize, usize, usize, usize)> {
    bands_where(page, |value| value < strict_ooxml_fidelity::INK_LEVEL)
}

/// The bands of rows where `predicate` holds, with the columns it holds in.
///
/// One predicate for both jobs — "this pixel is ink" and "these two pages differ"
/// — because a tool that described ink in one way and differences in another
/// would answer two questions with two definitions.
fn bands_where(page: &Gray, predicate: impl Fn(f64) -> bool) -> Vec<(usize, usize, usize, usize)> {
    let (width, height) = (page.width(), page.height());
    let mut bands: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut start = None;
    let mut left = usize::MAX;
    let mut right = 0usize;
    for row in 0..height {
        let mut any = false;
        for column in 0..width {
            if predicate(page.at(column, row)) {
                any = true;
                left = left.min(column);
                right = right.max(column);
            }
        }
        match (any, start) {
            (true, None) => {
                start = Some(row);
                left = usize::MAX;
                right = 0;
            }
            (false, Some(first)) => {
                bands.push((first, row - 1, left, right));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start {
        bands.push((first, height - 1, left, right));
    }
    bands
}

/// The gap between each pair of bands, which is the number that accumulates.
fn band_gaps(bands: &[(usize, usize, usize, usize)]) -> Vec<usize> {
    bands
        .windows(2)
        .map(|pair| pair[1].0.saturating_sub(pair[0].1))
        .collect()
}

fn main() {
    let mut out: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--out" || argument == "-o" {
            out = arguments.next().map(PathBuf::from);
        } else {
            positional.push(argument);
        }
    }
    let [document, pages @ ..] = positional.as_slice() else {
        usage();
    };

    let out = out.unwrap_or_else(|| PathBuf::from(".page-diff-pdf").join(document));
    std::fs::create_dir_all(&out).expect("create the output directory");

    let docx = strict_dir().join(format!("{document}.docx"));
    let package = Package::open_path(&docx, &OpenOptions::default()).unwrap_or_else(|error| {
        eprintln!("cannot open {}: {error}", docx.display());
        std::process::exit(2);
    });
    let parsed = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let placed = place_pages(&parsed, &options, Some(&package)).expect("place");
    let output = render_with_source(&placed, &options, Some(&package)).expect("render");
    if !output.report.is_clean() {
        eprintln!("render report:\n{}", indent(&output.report.to_string()));
    }

    let mut document_pdf =
        PdfDocument::open(&output.bytes, PdfLimits::default()).expect("read back");
    for index in 1..=document_pdf.page_count() {
        let page = document_pdf.page(index).expect("page");
        eprintln!(
            "page {index}: {} items, {} chars, text layer {:?}",
            page.items().len(),
            page.text().chars().count(),
            page.text_layer()
        );
    }
    let losses: Vec<String> = document_pdf
        .report()
        .losses()
        .iter()
        .map(ToString::to_string)
        .collect();
    if !losses.is_empty() {
        eprintln!("read report:\n{}", indent(&losses.join("\n")));
    }
    let rasterizer: Rasterizer = document_pdf.rasterizer().expect("rasterizer");

    let references = reference_pages(&strict_dir().join(format!("refs/{document}")));
    for page in pages {
        let Ok(page) = page.parse::<usize>() else {
            eprintln!("`{page}` is not a page number");
            std::process::exit(2);
        };
        if page >= references.len() {
            eprintln!(
                "{document}: {} reference page(s); page {page} was asked for",
                references.len()
            );
            std::process::exit(2);
        }
        write_page(document, page, &out, &references[page], &rasterizer);
    }
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The rows where the two pages differ by more than a quarter of a gray level.
///
/// A quarter of the range is chosen because it is well above any rasterizer's
/// antialiasing on one page and well below any real difference in a mark: a glyph
/// a half-pixel off is a band of edges, and a shape a line off is the whole shape.
fn difference_bands(reference: &Gray, candidate: &Gray) -> Vec<(usize, usize, usize, usize)> {
    const DIFFERENCE: f64 = 0.25;
    let pixels = reference
        .pixels()
        .iter()
        .zip(candidate.pixels())
        .map(|(a, b)| (a - b).abs())
        .collect();
    bands_where(
        &Gray::new(reference.width(), reference.height(), pixels),
        |value| value > DIFFERENCE,
    )
}

/// Writes the four files for one page and prints what the gate would measure.
fn write_page(
    document: &str,
    page: usize,
    out: &Path,
    reference_path: &Path,
    rasterizer: &Rasterizer,
) {
    let reference = load_png_gray(reference_path).expect("reference");
    let (width, height) = (reference.width(), reference.height());
    let number = page + 1;
    let options = RasterOptions {
        scale: scale_for(page_points(rasterizer, number), width, height),
        ..RasterOptions::default()
    };
    let png = rasterizer.page_png(number, &options).expect("rasterize");
    let raw = decode_png_gray(&png).expect("decode our raster");
    let candidate = crop(&raw, width, height).unwrap_or_else(|| {
        eprintln!(
            "{document} page {page}: rasterized {}x{}, reference {width}x{height}",
            raw.width(),
            raw.height()
        );
        std::process::exit(2);
    });

    let stem = format!("{document}-p{page}");
    write_gray(&out.join(format!("{stem}-ref.png")), &reference);
    write_gray(&out.join(format!("{stem}-ours.png")), &candidate);
    write_gray(
        &out.join(format!("{stem}-overlay.png")),
        &overlay(&reference, &candidate),
    );

    println!(
        "{stem}  {width}x{height}  ssim {:.4}",
        ssim(&reference, &candidate)
    );
    let reference_bands = ink_bands(&reference);
    let candidate_bands = ink_bands(&candidate);
    println!("  reference bands (row0, row1, x0, x1):");
    for band in &reference_bands {
        println!("    {band:?}");
    }
    println!("    gaps           : {:?}", band_gaps(&reference_bands));
    println!("  our bands (row0, row1, x0, x1):");
    for band in &candidate_bands {
        println!("    {band:?}");
    }
    println!("    gaps           : {:?}", band_gaps(&candidate_bands));
    let difference = difference_bands(&reference, &candidate);
    println!("  where the two differ (bands beyond 0.25 of gray):");
    for band in &difference {
        println!("    {band:?}");
    }
    match (
        strict_ooxml_fidelity::ink_centroid(&reference),
        strict_ooxml_fidelity::ink_centroid(&candidate),
    ) {
        (Some((rx, ry)), Some((cx, cy))) => println!(
            "  centroid: ours ({cx:.2},{cy:.2}) - reference ({rx:.2},{ry:.2}) = ({:.2},{:.2})",
            cx - rx,
            cy - ry
        ),
        _ => println!("  centroid: one of the two pages carries no ink"),
    }
    println!("  -> {}", out.display());
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo run -p strict-ooxml-pdf --features raster --example pdf_page_diff -- \
         <document> <page>… [--out <dir>]"
    );
    eprintln!(
        "  pages are 0-based, as the gate numbers them; the reference directory is \
         strict-ooxml-core/tests/strict/refs/<document>/"
    );
    std::process::exit(2);
}
