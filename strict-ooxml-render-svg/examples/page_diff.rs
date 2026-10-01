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

/// Decodes a PNG into `(width, height, grayscale)` — the gate's decode, copied
/// because it is ten lines and an example cannot import a test file.
fn load_png_gray(path: &Path) -> (u32, u32, Vec<f64>) {
    let file = std::io::BufReader::new(std::fs::File::open(path).expect("open reference png"));
    let mut reader = png::Decoder::new(file).read_info().expect("read png info");
    let mut buffer = vec![
        0u8;
        reader
            .output_buffer_size()
            .expect("png output size is known")
    ];
    let info = reader.next_frame(&mut buffer).expect("decode png frame");
    assert_eq!(
        info.bit_depth,
        png::BitDepth::Eight,
        "reference must be 8-bit"
    );
    let (width, height) = (info.width, info.height);
    let pixels = (width * height) as usize;
    let mut out = Vec::with_capacity(pixels);
    for index in 0..pixels {
        let (r, g, b) = match info.color_type {
            png::ColorType::Rgb => (
                buffer[index * 3],
                buffer[index * 3 + 1],
                buffer[index * 3 + 2],
            ),
            png::ColorType::Rgba => (
                buffer[index * 4],
                buffer[index * 4 + 1],
                buffer[index * 4 + 2],
            ),
            png::ColorType::Grayscale => {
                let value = buffer[index];
                (value, value, value)
            }
            png::ColorType::GrayscaleAlpha => {
                let value = buffer[index * 2];
                (value, value, value)
            }
            png::ColorType::Indexed => panic!("indexed reference PNG is unsupported"),
        };
        out.push((0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)) / 255.0);
    }
    (width, height, out)
}

/// Rasterizes our SVG at the reference's resolution, through `resvg`.
fn rasterize_gray(svg: &str, width: u32, height: u32, fonts: &Path) -> Vec<f64> {
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
    let data = pixmap.data();
    (0..(width * height) as usize)
        .map(|index| {
            let alpha = u16::from(data[index * 4 + 3]);
            let composite = |channel: u8| f64::from(u16::from(channel) + (255 - alpha));
            let r = composite(data[index * 4]);
            let g = composite(data[index * 4 + 1]);
            let b = composite(data[index * 4 + 2]);
            (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
        })
        .collect()
}

fn write_gray(path: &Path, gray: &[f64], width: u32, height: u32) {
    let mut bytes = Vec::with_capacity(gray.len() * 3);
    for value in gray {
        let level = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        bytes.extend_from_slice(&[level, level, level]);
    }
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(&bytes)
        .expect("png data");
}

/// Both images' ink drawn over each other: red ours, blue the reference.
fn overlay(reference: &[f64], candidate: &[f64]) -> Vec<f64> {
    const INK: f64 = 0.75;
    reference
        .iter()
        .zip(candidate)
        .map(|(reference, candidate)| {
            let (reference, candidate) = (*reference, *candidate);
            match (reference < INK, candidate < INK) {
                (false, false) => 1.0,
                (true, true) => 0.0,
                (true, false) => 1.0 - candidate,
                (false, true) => 1.0 - reference,
            }
        })
        .collect()
}

/// The contiguous runs of rows that carry ink, as `(first, last)`, each with the
/// columns its ink spans.
///
/// The gate's `extent_delta` reads the first and the last of these; printing all
/// of them, with their horizontal span, is what turns «the bottom edge is 16 px
/// high» into «the gap between blocks is 4 px short, four times over» — and a
/// band that is narrow tells you *which part* of the page it is.
fn ink_bands(gray: &[f64], width: usize, height: usize) -> Vec<(usize, usize, usize, usize)> {
    let mut bands: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut start = None;
    let mut left = usize::MAX;
    let mut right = 0usize;
    for row in 0..height {
        let ink = gray[row * width..(row + 1) * width]
            .iter()
            .enumerate()
            .filter(|(_, value)| **value < 0.75)
            .map(|(column, _)| column);
        let mut any = false;
        for column in ink {
            any = true;
            left = left.min(column);
            right = right.max(column);
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

/// The ink centre of mass, the same definition the gate uses.
fn ink_centroid(gray: &[f64], width: usize, height: usize) -> Option<(f64, f64)> {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;
    for row in 0..height {
        for column in 0..width {
            if gray[row * width + column] < 0.75 {
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
    let (width, height, reference) = load_png_gray(&references[page]);
    let candidate = rasterize_gray(&rendered[page].svg, width, height, &fonts_dir());

    let stem = format!("{stem}-p{page}");
    std::fs::write(
        out.join(format!("{stem}.svg")),
        rendered[page].svg.as_bytes(),
    )
    .expect("write our SVG");
    write_gray(
        &out.join(format!("{stem}-ref.png")),
        &reference,
        width,
        height,
    );
    write_gray(
        &out.join(format!("{stem}-ours.png")),
        &candidate,
        width,
        height,
    );
    write_gray(
        &out.join(format!("{stem}-overlay.png")),
        &overlay(&reference, &candidate),
        width,
        height,
    );

    let reference_bands = ink_bands(&reference, width as usize, height as usize);
    let candidate_bands = ink_bands(&candidate, width as usize, height as usize);
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
    match (
        ink_centroid(&reference, width as usize, height as usize),
        ink_centroid(&candidate, width as usize, height as usize),
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
        "usage: cargo run -p strict-ooxml-render-svg --example page_diff -- \
         <document.docx> <page>… [--out <dir>]"
    );
    eprintln!(
        "  pages are 0-based, as the gate numbers them; the reference directory is \
         strict-ooxml-core/tests/strict/refs/<document>/"
    );
    std::process::exit(2);
}
