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
const SSIM_DOCS: &[&str] = &["strict-text", "strict-text-grid"];

/// Minimum ink fraction for a page to count as having content.
const MIN_INK_FRACTION: f64 = 0.001;
/// Allowed candidate/reference ink ratio (catches blank or over-inked pages).
const MIN_INK_RATIO: f64 = 0.5;
/// Upper bound of the candidate/reference ink ratio.
const MAX_INK_RATIO: f64 = 2.0;
/// Maximum allowed alignment shift in px on either axis (2 passes, 3 fails).
const MAX_ALIGNMENT_SHIFT_PX: isize = 2;
/// Minimum Pearson correlation of the row-ink profiles.
const MIN_ROW_CORRELATION: f64 = 0.9;
/// Minimum Pearson correlation of the column-ink profiles.
const MIN_COLUMN_CORRELATION: f64 = 0.85;
/// Maximum allowed shift of the ink centroid in px (either axis).
const MAX_CENTROID_SHIFT_PX: f64 = 2.5;

/// Structural (rasterizer-independent) fidelity of one page (B-3/R-1).
#[derive(Debug, Clone, Copy)]
pub struct StructuralFidelity {
    /// Best vertical shift of the candidate relative to the reference.
    pub shift_y_px: isize,
    /// Best horizontal shift of the candidate relative to the reference.
    pub shift_x_px: isize,
    /// Correlation of the row-ink profiles at `shift_y_px`.
    pub correlation_y: f64,
    /// Correlation of the column-ink profiles at `shift_x_px`.
    pub correlation_x: f64,
    /// Signed horizontal shift of the ink centroid, in px.
    pub centroid_x_delta: f64,
    /// Signed vertical shift of the ink centroid, in px.
    pub centroid_y_delta: f64,
    /// Reference ink fraction.
    pub reference_ink: f64,
    /// Candidate ink fraction.
    pub candidate_ink: f64,
}

/// Checks a page structurally: similar ink coverage, no vertical or horizontal
/// drift and well-correlated row/column ink profiles.
///
/// Complements SSIM, which barely penalises a missing or shifted sparse text
/// layer (`S4F-REWORK-2` B-3/R-1). Deterministic and rasterizer independent.
///
/// # Errors
///
/// Returns a human-readable reason when any structural bound is violated.
pub fn structural_fidelity(
    reference: &[f64],
    candidate: &[f64],
    width: usize,
    height: usize,
) -> Result<StructuralFidelity, String> {
    let reference_ink = ink_fraction(reference);
    let candidate_ink = ink_fraction(candidate);
    if reference_ink <= MIN_INK_FRACTION {
        return Err(format!("reference ink {reference_ink:.5} is empty"));
    }
    if candidate_ink < reference_ink * MIN_INK_RATIO {
        return Err(format!(
            "candidate ink {candidate_ink:.5} << reference {reference_ink:.5}"
        ));
    }
    if candidate_ink > reference_ink * MAX_INK_RATIO {
        return Err(format!(
            "candidate ink {candidate_ink:.5} >> reference {reference_ink:.5}"
        ));
    }
    let (shift_y, correlation_y) = best_profile_alignment(
        &row_ink_profile(reference, width, height),
        &row_ink_profile(candidate, width, height),
        12,
    );
    let correlation_y = correlation_y.ok_or_else(|| "no row structure to compare".to_owned())?;
    if shift_y.abs() > MAX_ALIGNMENT_SHIFT_PX {
        return Err(format!("vertical shift {shift_y}px exceeds tolerance"));
    }
    if correlation_y < MIN_ROW_CORRELATION {
        return Err(format!("row-ink correlation {correlation_y:.3} too low"));
    }
    let (shift_x, correlation_x) = best_profile_alignment(
        &column_ink_profile(reference, width, height),
        &column_ink_profile(candidate, width, height),
        12,
    );
    let correlation_x = correlation_x.ok_or_else(|| "no column structure to compare".to_owned())?;
    if shift_x.abs() > MAX_ALIGNMENT_SHIFT_PX {
        return Err(format!("horizontal shift {shift_x}px exceeds tolerance"));
    }
    if correlation_x < MIN_COLUMN_CORRELATION {
        return Err(format!("column-ink correlation {correlation_x:.3} too low"));
    }
    let (centroid_x_delta, centroid_y_delta) = centroid_delta(reference, candidate, width, height);
    if centroid_x_delta.abs() > MAX_CENTROID_SHIFT_PX {
        return Err(format!(
            "horizontal centroid shift {centroid_x_delta:.2}px exceeds tolerance"
        ));
    }
    if centroid_y_delta.abs() > MAX_CENTROID_SHIFT_PX {
        return Err(format!(
            "vertical centroid shift {centroid_y_delta:.2}px exceeds tolerance"
        ));
    }
    Ok(StructuralFidelity {
        shift_y_px: shift_y,
        shift_x_px: shift_x,
        correlation_y,
        correlation_x,
        centroid_x_delta,
        centroid_y_delta,
        reference_ink,
        candidate_ink,
    })
}

/// Mean `(x, y)` of the ink pixels, or `None` when the image has no ink.
#[must_use]
pub fn ink_centroid(gray: &[f64], width: usize, height: usize) -> Option<(f64, f64)> {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;
    for row in 0..height {
        for col in 0..width {
            if gray[row * width + col] < INK_LEVEL {
                sum_x += col as f64;
                sum_y += row as f64;
                count += 1;
            }
        }
    }
    (count > 0).then(|| (sum_x / count as f64, sum_y / count as f64))
}

/// Signed centroid shift `candidate - reference` on `(x, y)`.
fn centroid_delta(reference: &[f64], candidate: &[f64], width: usize, height: usize) -> (f64, f64) {
    match (
        ink_centroid(reference, width, height),
        ink_centroid(candidate, width, height),
    ) {
        (Some((rx, ry)), Some((cx, cy))) => (cx - rx, cy - ry),
        _ => (0.0, 0.0),
    }
}

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

/// A pixel at or below this gray level counts as "ink" (text).
const INK_LEVEL: f64 = 0.5;

/// Fraction of the image covered by ink.
#[must_use]
pub fn ink_fraction(gray: &[f64]) -> f64 {
    let dark = gray.iter().filter(|value| **value < INK_LEVEL).count();
    dark as f64 / gray.len() as f64
}

/// Per-row ink fraction, used for vertical-alignment comparison.
#[must_use]
pub fn row_ink_profile(gray: &[f64], width: usize, height: usize) -> Vec<f64> {
    (0..height)
        .map(|row| {
            let dark = (0..width)
                .filter(|&col| gray[row * width + col] < INK_LEVEL)
                .count();
            dark as f64 / width as f64
        })
        .collect()
}

/// Per-column ink fraction, used for horizontal-alignment comparison.
#[must_use]
pub fn column_ink_profile(gray: &[f64], width: usize, height: usize) -> Vec<f64> {
    (0..width)
        .map(|col| {
            let dark = (0..height)
                .filter(|&row| gray[row * width + col] < INK_LEVEL)
                .count();
            dark as f64 / height as f64
        })
        .collect()
}

/// Pearson correlation of two equal-length samples, or `None` if either is
/// constant (no structure to correlate).
fn pearson(left: &[f64], right: &[f64]) -> Option<f64> {
    let n = left.len() as f64;
    let mean_left = left.iter().sum::<f64>() / n;
    let mean_right = right.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_left = 0.0;
    let mut var_right = 0.0;
    for (a, b) in left.iter().zip(right) {
        let da = a - mean_left;
        let db = b - mean_right;
        cov += da * db;
        var_left += da * da;
        var_right += db * db;
    }
    if var_left <= 0.0 || var_right <= 0.0 {
        return None;
    }
    Some(cov / (var_left.sqrt() * var_right.sqrt()))
}

/// Finds the shift of `candidate` relative to `reference` that best aligns
/// their ink profiles, returning `(shift_px, correlation)`.
///
/// A positive shift means the candidate content sits lower/further right. Pass
/// `row_ink_profile` for the vertical axis and `column_ink_profile` for the
/// horizontal axis. Returns `(0, None)` when either profile has no structure.
#[must_use]
pub fn best_profile_alignment(
    reference: &[f64],
    candidate: &[f64],
    max_shift: isize,
) -> (isize, Option<f64>) {
    let n = reference.len() as isize;
    let mut best_shift = 0isize;
    let mut best_corr: Option<f64> = None;
    for shift in -max_shift..=max_shift {
        let start = (-shift).max(0);
        let end = n - shift.max(0);
        if start >= end {
            continue;
        }
        let left = &reference[start as usize..end as usize];
        let right = &candidate[(start + shift) as usize..(end + shift) as usize];
        if let Some(corr) = pearson(left, right) {
            let better = match best_corr {
                None => true,
                Some(current) => corr > current,
            };
            if better {
                best_corr = Some(corr);
                best_shift = shift;
            }
        }
    }
    (best_shift, best_corr)
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
        let mut structures = Vec::new();
        for (page, reference_path) in pages.iter().zip(&ref_pages) {
            let (width, height, reference) = load_png_gray(reference_path);
            let candidate = rasterize_gray(&page.svg, width, height, &fonts);
            assert_eq!(
                reference.len(),
                candidate.len(),
                "{name}: raster size mismatch"
            );
            let (width, height) = (width as usize, height as usize);
            structures.push(structural_fidelity(&reference, &candidate, width, height));
            comparisons.push(Comparison {
                reference,
                candidate,
                width,
                height,
            });
        }
        checked += 1;

        if SSIM_DOCS.contains(&name.as_str()) {
            let worst = worst_score(&comparisons).expect("at least one comparison");
            eprintln!("{name}: worst SSIM = {worst:.4}");
            for (page, structure) in structures.iter().enumerate() {
                match structure {
                    Ok(structure) => eprintln!(
                        "  page {page}: dy={}px corr_y={:.3} | dx={}px corr_x={:.3} | centroid d=({:.2},{:.2}) | ink ref={:.5} cand={:.5}",
                        structure.shift_y_px,
                        structure.correlation_y,
                        structure.shift_x_px,
                        structure.correlation_x,
                        structure.centroid_x_delta,
                        structure.centroid_y_delta,
                        structure.reference_ink,
                        structure.candidate_ink
                    ),
                    Err(reason) => panic!("{name} page {page}: structural check failed: {reason}"),
                }
            }
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

/// Builds a tiny text-like image: two dark rows of "ink", shifted by
/// `(dy, dx)` px.
fn text_like(width: usize, height: usize, rows: [usize; 2], dx: isize) -> Vec<f64> {
    let mut image = vec![1.0; width * height];
    for row in rows {
        for col in 0..width {
            let source = col as isize - dx;
            if source >= 10 && (source as usize) < width - 10 {
                image[row * width + col] = 0.0;
            }
        }
    }
    image
}

#[test]
fn structural_check_rejects_blank_and_shifted_pages() {
    let (width, height) = (64, 96);
    let reference = text_like(width, height, [20, 60], 0);
    // Identical candidate passes.
    assert!(structural_fidelity(&reference, &reference, width, height).is_ok());
    // A blank page is rejected (ink ratio).
    let blank = vec![1.0; width * height];
    assert!(structural_fidelity(&reference, &blank, width, height).is_err());
    // A 3px vertical shift is rejected.
    assert!(structural_fidelity(
        &reference,
        &text_like(width, height, [23, 63], 0),
        width,
        height
    )
    .is_err());
    // A 2px vertical shift is still within tolerance.
    assert!(structural_fidelity(
        &reference,
        &text_like(width, height, [22, 62], 0),
        width,
        height
    )
    .is_ok());
    // A 3px horizontal shift is rejected (R-1).
    assert!(structural_fidelity(
        &reference,
        &text_like(width, height, [20, 60], 3),
        width,
        height
    )
    .is_err());
    // A 2px horizontal shift is still within tolerance.
    assert!(structural_fidelity(
        &reference,
        &text_like(width, height, [20, 60], 2),
        width,
        height
    )
    .is_ok());
}

/// Renders `name` page `page` and returns `(width, height, reference, candidate)`
/// grayscale buffers at the reference resolution.
fn rasterize_reference_pair(name: &str, page: usize) -> (usize, usize, Vec<f64>, Vec<f64>) {
    let dir = strict_dir().join(format!("refs/{name}"));
    let (width, height, reference) = load_png_gray(&reference_pages(&dir)[page]);
    let package = Package::open_path(
        &strict_dir().join(format!("{name}.docx")),
        &OpenOptions::default(),
    )
    .expect("open package");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    let candidate = rasterize_gray(&pages[page].svg, width, height, &fonts_dir());
    (width as usize, height as usize, reference, candidate)
}

/// Shifts an image horizontally by `dx` px (fills the exposed columns white).
fn shift_horizontal(gray: &[f64], width: usize, height: usize, dx: isize) -> Vec<f64> {
    let mut out = vec![1.0; width * height];
    for row in 0..height {
        for col in 0..width {
            let source = col as isize - dx;
            if source >= 0 && (source as usize) < width {
                out[row * width + col] = gray[row * width + source as usize];
            }
        }
    }
    out
}

#[test]
fn structural_check_rejects_real_horizontal_shift() {
    let (width, height, reference, candidate) = rasterize_reference_pair("strict-text", 0);
    // The unshifted render passes.
    assert!(structural_fidelity(&reference, &candidate, width, height).is_ok());
    // A horizontal drift of >= 3 px is rejected (R-1 criterion 1).
    for dx in [3isize, 5, 8, 10] {
        let shifted = shift_horizontal(&candidate, width, height, dx);
        assert!(
            structural_fidelity(&reference, &shifted, width, height).is_err(),
            "dx={dx} must be rejected"
        );
    }
}

#[test]
fn structural_check_rejects_real_vertical_shift() {
    let (width, height, reference, candidate) = rasterize_reference_pair("strict-text-grid", 0);
    let shifted: Vec<f64> = {
        let mut out = vec![1.0; width * height];
        for row in 3..height {
            out[row * width..(row + 1) * width]
                .copy_from_slice(&candidate[(row - 3) * width..(row - 2) * width]);
        }
        out
    };
    assert!(
        structural_fidelity(&reference, &shifted, width, height).is_err(),
        "a 3px vertical shift must be rejected"
    );
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
