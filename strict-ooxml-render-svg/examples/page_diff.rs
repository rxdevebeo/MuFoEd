//! Look at a page instead of a number: the WPS reference, our render, the two drawn
//! over each other, and the ink bands of both.
//!
//! ```text
//! cargo run -p strict-ooxml-render-svg --example page_diff -- <document.docx> <page>…
//! ```
//!
//! # Why this exists
//!
//! The fidelity gate (`tests/ssim.rs`) answers **whether** a page is inside its
//! class bounds, and its failure message answers **which bound**. Neither answers
//! **what differs**, and on a formula page the answer is not a number: a centroid
//! moved 24 px can be one operator three times too big, or a block 4 px too close
//! to its neighbour four times over, and the two need different fixes.
//!
//! So this prints the evidence, in the order a reader needs it:
//!
//! - `<doc>-p<page>-ref.png` — the committed WPS reference, re-encoded as
//!   grayscale from the same decode the gate uses;
//! - `<doc>-p<page>-ours.png` — our SVG rasterized by `resvg` with the **bundled**
//!   fonts, at the reference's resolution;
//! - `<doc>-p<page>-overlay.png` — red is ink we drew and WPS did not, blue is ink
//!   WPS drew and we did not, black is both. A word one line too low is a red word
//!   above a blue one; a page shifted sideways is red left of blue everywhere;
//! - `<doc>-p<page>.svg` — our SVG, which is where a wrong number becomes a
//!   readable coordinate;
//! - the **ink bands** of both images: the contiguous runs of rows carrying ink,
//!   as `(first, last)`. The gate measures the first and the last; a page whose
//!   content is four bands where the reference has one says so here in one line.
//!
//! # What it deliberately does not do
//!
//! It does not decide whether a page passes. The verdict belongs to the gate, and
//! a second copy of its arithmetic in an example is a second thing to keep in step
//! with it. This prints measurements; `tests/ssim.rs` judges them.
//!
//! It is also not a test: it writes files and it wants to be run by somebody
//! looking at a page.

// Pixel arithmetic over buffers: a page is 816 × 1056, far below 2^53, and a
// gray value is clamped to 0..=1 before it is cast. The gate's test file allows
// the same three lints for the same reason.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_fidelity::Gray;
use strict_ooxml_render_svg::{render_with_media, Page, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The graded fixtures, in the order the gate walks them.
fn strict_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

/// The bundled metric-compatible faces, which is what the gate rasterizes with.
///
/// **Not** `strict-ooxml-core/tests/fonts`: a directory that does not exist loads
/// no fonts, and every glyph the substituted faces do not cover disappears. That
/// is not a hypothetical: the first version of this tool pointed there and its
/// output showed a page with no mathematics on it, which looked like a catastrophic
/// renderer defect and was a wrong directory.
fn fonts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts")
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

/// Decodes a PNG into a [`Gray`] page — the gate's decode.
///
/// It is `strict_ooxml_fidelity::load_png_gray` and not a copy of it: this tool
/// exists to be read next to the gate's numbers, and a tool that decodes a
/// reference slightly differently from the gate that judged it is a tool whose
/// pictures and its numbers disagree.
fn load_png_gray(path: &Path) -> Gray {
    strict_ooxml_fidelity::load_png_gray(path).unwrap_or_else(|error| panic!("{error}"))
}

/// Rasterizes our SVG at the reference's resolution, through `resvg`.
fn rasterize_gray(svg: &str, width: u32, height: u32, fonts: &Path) -> Gray {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_fonts_dir(fonts);
    let tree = resvg::usvg::Tree::from_str(svg, &options).expect("parse our SVG");
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).expect("allocate raster target");
    let transform = resvg::tiny_skia::Transform::from_scale(
        f32::from(u16::try_from(width).expect("width fits u16")) / size.width(),
        f32::from(u16::try_from(height).expect("height fits u16")) / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    strict_ooxml_fidelity::from_rgba8(pixmap.data(), width as usize, height as usize)
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
    const INK: f64 = 0.75;
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
/// high» into «the gap between blocks is 4 px short, four times over» — and a
/// band that is narrow tells you *which part* of the page it is.
fn ink_bands(page: &Gray) -> Vec<(usize, usize, usize, usize)> {
    let (width, height) = (page.width(), page.height());
    let mut bands: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut start = None;
    let mut left = usize::MAX;
    let mut right = 0usize;
    for row in 0..height {
        let mut any = false;
        for column in 0..width {
            if page.at(column, row) < 0.75 {
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

/// The ink centre of mass at this tool's own ink level.
///
/// The gate uses `INK_LEVEL = 0.5`; this tool uses 0.75 because it is looking at
/// *shapes* as much as text, and a light fill counts as content here. The
/// difference is stated rather than hidden, because a number printed next to the
/// gate's numbers has to say which of them it is.
fn ink_centroid(page: &Gray) -> Option<(f64, f64)> {
    let (width, height) = (page.width(), page.height());
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;
    for row in 0..height {
        for column in 0..width {
            if page.at(column, row) < 0.75 {
                sum_x += column as f64;
                sum_y += row as f64;
                count += 1;
            }
        }
    }
    (count > 0).then(|| (sum_x / count as f64, sum_y / count as f64))
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

    let stem = Path::new(document).file_stem().map_or_else(
        || "page".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let out = out.unwrap_or_else(|| PathBuf::from(".page-diff").join(&stem));
    std::fs::create_dir_all(&out).expect("create the output directory");

    let package = Package::open_path(
        strict_dir().join(format!("{document}.docx")),
        &OpenOptions::default(),
    )
    .unwrap_or_else(|error| {
        eprintln!("cannot open {document}.docx: {error}");
        std::process::exit(2);
    });
    let parsed = parse_document(&package, &ParseOptions::default()).expect("parse");
    let rendered =
        render_with_media(&parsed, &RenderOptions::default(), Some(&package)).expect("render");
    let references = reference_pages(&strict_dir().join(format!("refs/{document}")));

    for page in pages {
        let Ok(page) = page.parse::<usize>() else {
            eprintln!("`{page}` is not a page number");
            std::process::exit(2);
        };
        if page >= references.len() || page >= rendered.len() {
            eprintln!(
                "{document}: {} reference page(s), {} rendered page(s); page {page} was asked for",
                references.len(),
                rendered.len()
            );
            std::process::exit(2);
        }
        write_page(&stem, page, &out, &references, &rendered);
    }
}

/// Writes the four files for one page and prints what the gate would measure.
fn write_page(stem: &str, page: usize, out: &Path, references: &[PathBuf], rendered: &[Page]) {
    let reference = load_png_gray(&references[page]);
    let (width, height) = (reference.width() as u32, reference.height() as u32);
    let candidate = rasterize_gray(&rendered[page].svg, width, height, &fonts_dir());

    let stem = format!("{stem}-p{page}");
    std::fs::write(
        out.join(format!("{stem}.svg")),
        rendered[page].svg.as_bytes(),
    )
    .expect("write our SVG");
    write_gray(&out.join(format!("{stem}-ref.png")), &reference);
    write_gray(&out.join(format!("{stem}-ours.png")), &candidate);
    write_gray(
        &out.join(format!("{stem}-overlay.png")),
        &overlay(&reference, &candidate),
    );

    let reference_bands = ink_bands(&reference);
    let candidate_bands = ink_bands(&candidate);
    println!("{stem}  {width}x{height}");
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
    match (ink_centroid(&reference), ink_centroid(&candidate)) {
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
        "usage: cargo run -p strict-ooxml-render-svg --example page_diff -- \
         <document.docx> <page>… [--out <dir>]"
    );
    eprintln!(
        "  pages are 0-based, as the gate numbers them; the reference directory is \
         strict-ooxml-core/tests/strict/refs/<document>/"
    );
    std::process::exit(2);
}
