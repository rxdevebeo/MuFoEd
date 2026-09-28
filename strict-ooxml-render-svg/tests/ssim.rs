//! SSIM harness against WPS-produced reference PNGs
//! (`STAGE-4-RENDER-FIDELITY.md` S4F.5).
//!
//! Our SVG is rasterized with `resvg` using the bundled metric-compatible fonts,
//! converted to grayscale and compared page-by-page with the committed WPS
//! references (`strict-ooxml-core/tests/strict/refs/<doc>/page_N.png`). The
//! gate is the *worst* per-page SSIM plus the page-count invariant.
//!
//! References for chart/diagram documents (`strict-profile`) are page-count
//! checked only: Stage 4 cannot rasterize DrawingML charts, so per-pixel
//! comparison is not meaningful there (see `docs/stage-4-report.md`).

#![allow(
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::similar_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::needless_borrows_for_generic_args
)]

use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{render_with_media, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// Minimum acceptable worst-page SSIM on the approved (text) references.
const SSIM_THRESHOLD: f64 = 0.95;

/// Documents compared with SSIM. Others are checked for page count only.
const SSIM_DOCS: &[&str] = &["strict-text"];

/// SSIM window edge (an 11×11 window, `σ = 1.5`, as in the reference paper).
const SSIM_WINDOW: usize = 11;
/// SSIM Gaussian standard deviation.
const SSIM_SIGMA: f64 = 1.5;
/// SSIM stabilising constant `C1 = (0.01·L)²` for dynamic range `L = 1`.
const SSIM_C1: f64 = 0.01 * 0.01;
/// SSIM stabilising constant `C2 = (0.03·L)²` for dynamic range `L = 1`.
const SSIM_C2: f64 = 0.03 * 0.03;

/// Computes the mean SSIM of two equal-size grayscale images in `[0, 1]`.
///
/// Uses the standard constants `C1 = (0.01·L)²`, `C2 = (0.03·L)²` with dynamic
/// range `L = 1` and an 11×11 Gaussian window; the per-window map is averaged
/// over the interior (border windows are excluded). Falls back to the single
/// global window for images smaller than the window.
///
/// # Panics
///
/// Panics if the dimensions do not match the buffer lengths or are empty.
#[must_use]
pub fn ssim(reference: &[f64], candidate: &[f64], width: usize, height: usize) -> f64 {
    assert_eq!(reference.len(), width * height, "reference size mismatch");
    assert_eq!(candidate.len(), width * height, "candidate size mismatch");
    assert!(width > 0 && height > 0, "empty image");
    if width < SSIM_WINDOW || height < SSIM_WINDOW {
        return global_ssim(reference, candidate);
    }
    let kernel = gaussian_kernel(SSIM_WINDOW, SSIM_SIGMA);
    let square = |values: &[f64]| values.iter().map(|value| value * value).collect::<Vec<_>>();
    let product: Vec<f64> = reference
        .iter()
        .zip(candidate)
        .map(|(a, b)| a * b)
        .collect();
    let mu_a = blur(reference, width, height, &kernel);
    let mu_b = blur(candidate, width, height, &kernel);
    let mu_aa = blur(&square(reference), width, height, &kernel);
    let mu_bb = blur(&square(candidate), width, height, &kernel);
    let mu_ab = blur(&product, width, height, &kernel);

    let radius = SSIM_WINDOW / 2;
    let mut total = 0.0;
    let mut count = 0u64;
    for row in radius..height - radius {
        for col in radius..width - radius {
            let index = row * width + col;
            let var_a = mu_aa[index] - mu_a[index] * mu_a[index];
            let var_b = mu_bb[index] - mu_b[index] * mu_b[index];
            let cov = mu_ab[index] - mu_a[index] * mu_b[index];
            total += ((2.0 * mu_a[index] * mu_b[index] + SSIM_C1) * (2.0 * cov + SSIM_C2))
                / ((mu_a[index] * mu_a[index] + mu_b[index] * mu_b[index] + SSIM_C1)
                    * (var_a + var_b + SSIM_C2));
            count += 1;
        }
    }
    if count == 0 {
        global_ssim(reference, candidate)
    } else {
        total / count as f64
    }
}

/// Single-window (global) SSIM, used for images smaller than the window.
fn global_ssim(reference: &[f64], candidate: &[f64]) -> f64 {
    let n = reference.len() as f64;
    let mean_a = reference.iter().sum::<f64>() / n;
    let mean_b = candidate.iter().sum::<f64>() / n;
    let var_a = reference
        .iter()
        .map(|value| (value - mean_a).powi(2))
        .sum::<f64>()
        / n;
    let var_b = candidate
        .iter()
        .map(|value| (value - mean_b).powi(2))
        .sum::<f64>()
        / n;
    let cov = reference
        .iter()
        .zip(candidate)
        .map(|(a, b)| (a - mean_a) * (b - mean_b))
        .sum::<f64>()
        / n;
    ((2.0 * mean_a * mean_b + SSIM_C1) * (2.0 * cov + SSIM_C2))
        / ((mean_a * mean_a + mean_b * mean_b + SSIM_C1) * (var_a + var_b + SSIM_C2))
}

/// Returns a normalized 1-D Gaussian kernel.
fn gaussian_kernel(size: usize, sigma: f64) -> Vec<f64> {
    let radius = (size / 2) as f64;
    let mut kernel: Vec<f64> = (0..size)
        .map(|index| {
            let offset = index as f64 - radius;
            (-(offset * offset) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f64 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= sum;
    }
    kernel
}

/// Separable Gaussian blur with edge clamping.
fn blur(source: &[f64], width: usize, height: usize, kernel: &[f64]) -> Vec<f64> {
    let radius = (kernel.len() / 2) as isize;
    let mut horizontal = vec![0.0; source.len()];
    for row in 0..height {
        for col in 0..width {
            let mut acc = 0.0;
            for (offset, weight) in kernel.iter().enumerate() {
                let sample =
                    (col as isize + offset as isize - radius).clamp(0, width as isize - 1) as usize;
                acc += weight * source[row * width + sample];
            }
            horizontal[row * width + col] = acc;
        }
    }
    let mut output = vec![0.0; source.len()];
    for row in 0..height {
        for col in 0..width {
            let mut acc = 0.0;
            for (offset, weight) in kernel.iter().enumerate() {
                let sample = (row as isize + offset as isize - radius).clamp(0, height as isize - 1)
                    as usize;
                acc += weight * horizontal[sample * width + col];
            }
            output[row * width + col] = acc;
        }
    }
    output
}

/// Pairs a reference with a candidate image.
pub struct Comparison {
    /// Reference grayscale image.
    pub reference: Vec<f64>,
    /// Candidate grayscale image.
    pub candidate: Vec<f64>,
    /// Image width in px.
    pub width: usize,
    /// Image height in px.
    pub height: usize,
}

/// Runs every comparison and returns the worst (minimum) SSIM score.
///
/// Returns `None` when there is nothing to compare.
#[must_use]
pub fn worst_score(comparisons: &[Comparison]) -> Option<f64> {
    comparisons
        .iter()
        .map(|pair| ssim(&pair.reference, &pair.candidate, pair.width, pair.height))
        .reduce(f64::min)
}

/// Repository path to the real Strict corpus (`tests/strict/`).
fn strict_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

/// Repository path to the bundled fonts.
fn fonts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts")
}

/// Rasterizes `svg` to `width × height` grayscale via `resvg` with bundled fonts.
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

/// Decodes a PNG into `(width, height, grayscale)` (8-bit input).
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

/// Returns the reference PNGs of a document directory, sorted by page number.
fn reference_pages(dir: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read reference dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect();
    pages.sort();
    pages
}

#[test]
fn matches_wps_references() {
    let refs = strict_dir().join("refs");
    assert!(refs.is_dir(), "missing references at {}", refs.display());
    let fonts = fonts_dir();
    let mut checked = 0u32;
    let mut gated = 0u32;
    for entry in std::fs::read_dir(&refs).expect("read refs dir") {
        let dir = entry.expect("refs entry").path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|value| value.to_str())
            .expect("document name")
            .to_owned();
        let docx = strict_dir().join(format!("{name}.docx"));
        assert!(docx.is_file(), "missing fixture {}", docx.display());
        let package = Package::open_path(&docx, &OpenOptions::default()).expect("open package");
        let document = parse_document(&package, &ParseOptions::default()).expect("parse document");
        let pages = render_with_media(&document, &RenderOptions::default(), Some(&package))
            .expect("render document");
        let ref_pages = reference_pages(&dir);
        assert_eq!(
            pages.len(),
            ref_pages.len(),
            "{name}: rendered page count differs from the reference"
        );

        let mut comparisons = Vec::new();
        for (page, reference_path) in pages.iter().zip(&ref_pages) {
            let (width, height, reference) = load_png_gray(reference_path);
            let candidate = rasterize_gray(&page.svg, width, height, &fonts);
            assert_eq!(
                reference.len(),
                candidate.len(),
                "{name}: raster size mismatch"
            );
            comparisons.push(Comparison {
                reference,
                candidate,
                width: width as usize,
                height: height as usize,
            });
        }
        checked += 1;

        if SSIM_DOCS.contains(&name.as_str()) {
            let worst = worst_score(&comparisons).expect("at least one comparison");
            eprintln!("{name}: worst SSIM = {worst:.4}");
            assert!(
                worst >= SSIM_THRESHOLD,
                "{name}: worst SSIM {worst:.4} < {SSIM_THRESHOLD}"
            );
            gated += 1;
        }
    }
    assert!(checked > 0, "no reference documents were checked");
    assert!(gated > 0, "no SSIM-gated documents were present");
}

#[test]
fn rasterization_is_deterministic() {
    let refs = strict_dir().join("refs/strict-text");
    let Some(first_page) = reference_pages(&refs).into_iter().next() else {
        return;
    };
    let package = Package::open_path(
        &strict_dir().join("strict-text.docx"),
        &OpenOptions::default(),
    )
    .expect("open package");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    let (width, height, _) = load_png_gray(&first_page);
    let a = rasterize_gray(&pages[0].svg, width, height, &fonts_dir());
    let b = rasterize_gray(&pages[0].svg, width, height, &fonts_dir());
    assert_eq!(a, b, "rasterization must be byte-deterministic");
}

#[test]
fn identical_images_score_one() {
    let image: Vec<f64> = (0..256).map(|index| f64::from(index % 16) / 15.0).collect();
    assert!((ssim(&image, &image, 16, 16) - 1.0).abs() < 1e-12);
}

#[test]
fn perturbed_images_score_below_one() {
    let reference: Vec<f64> = (0..256).map(|index| f64::from(index % 16) / 15.0).collect();
    let candidate: Vec<f64> = reference
        .iter()
        .enumerate()
        .map(|(index, value)| if index % 3 == 0 { 1.0 - value } else { *value })
        .collect();
    assert!(ssim(&reference, &candidate, 16, 16) < 1.0);
}

#[test]
fn worst_score_aggregates() {
    let image: Vec<f64> = vec![0.5; 16];
    assert_eq!(worst_score(&[]), None);
    let good = Comparison {
        reference: image.clone(),
        candidate: image.clone(),
        width: 4,
        height: 4,
    };
    assert!((worst_score(&[good]).expect("score") - 1.0).abs() < 1e-12);
}
