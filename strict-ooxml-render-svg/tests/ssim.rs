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

/// Minimum acceptable worst-page SSIM on the approved references.
const SSIM_THRESHOLD: f64 = 0.95;

/// The margin a document must clear above [`SSIM_THRESHOLD`] to pass outright.
///
/// Without it, "passes" and "passes because the threshold was set from the
/// result" look identical. The corpus shows why this matters: a document at
/// 0.9533 clears a 0.95 gate by three thousandths, and the gate runs on three
/// operating systems in CI, so whether it is a stable property of the layout
/// or of one rasterizer build is unknowable from the number alone. A document
/// at 0.96 has cleared a meaningful part of that uncertainty.
const SSIM_MARGIN: f64 = 0.01;

/// Documents known to clear [`SSIM_THRESHOLD`] but not [`SSIM_MARGIN`].
///
/// This list is the amber register, and it is the whole of the policy: a
/// document *not* on it that lands in the margin band fails the gate, so a new
/// fixture cannot join the gate "just below the line" without being written
/// down here. Each entry is outstanding debt with a named defect, not an
/// accepted tolerance. Removing an entry is progress; adding one is a decision
/// that has to be made on purpose.
///
/// * `strict-stage5c` — 0.9533. The display-block box is 17/18 px taller than
///   WPS's (§4.2 of the Stage-5C report); `EXTENT_RATCHET` tracks it.
/// * `07-strict-drawingml-shapes` — 0.9568. The same block-geometry family, on
///   the opposite sign: the page is 16 px *shorter* than the reference.
const SSIM_AMBER: &[&str] = &["strict-stage5c", "07-strict-drawingml-shapes"];

/// Documents compared page by page with SSIM.
const SSIM_DOCS: &[&str] = &[
    "strict-text",
    "strict-text-grid",
    "strict-stage5",
    "strict-stage5b",
    "strict-stage5c",
    "05-strict-math-simple",
    "06-strict-math-display",
    "07-strict-drawingml-shapes",
    "10-strict-math-eqarr",
];

/// Documents compared for the page-count invariant only.
///
/// Their content is raster content this renderer cannot produce (a DrawingML
/// chart part, `strict-ooxml-core/tests/strict/README.md`), so a per-pixel
/// comparison would measure the placeholder, not the layout. `strict-profile`
/// has been page-count only since Stage 4; `09-strict-math-drawing-chart` is
/// the real-Strict counterpart of the same mechanism (`STAGE-5C-TASK.md` §3.2
/// puts chart rasterization out of scope).
const PAGE_COUNT_ONLY: &[&str] = &["strict-profile", "09-strict-math-drawing-chart"];

/// Relaxed structural limits for the mixed Stage-5 fixture
/// (`STAGE-5-REWORK-1` R5-1): the rasterizer-independent checks stay enforced
/// (ink coverage / blank page, alignment shift, ink centroid); only the
/// row/column profile-correlation thresholds are loosened, because the sparse
/// mixed table/footnote content makes them fall slightly below the text-only
/// thresholds.
const STAGE5_LIMITS: StructuralLimits = StructuralLimits {
    max_shift_px: 2,
    max_centroid_px: 3.0,
    min_row_correlation: 0.75,
    min_column_correlation: 0.70,
    max_extent_px: 2.0,
};

/// Relaxed structural limits for the Stage-5B floating-drawing fixture
/// (`STAGE-5B-REWORK-1` 5B-2). The page is dominated by large solid fills whose
/// row/column ink profiles correlate only weakly with the WPS raster (the
/// correlation thresholds are lowered), and the WPS rasterizer's fill/edge
/// antialiasing and text-metric differences put the ink centroid ~6 px off
/// while the profile alignment is exact (`dx = dy = 0`). Ink/blank and the
/// profile-shift bounds stay tight; the centroid bound is widened to `8 px`
/// but is still enforced (a blank or materially shifted page is rejected).
const STAGE5B_LIMITS: StructuralLimits = StructuralLimits {
    max_shift_px: 2,
    max_centroid_px: 8.0,
    min_row_correlation: 0.60,
    min_column_correlation: 0.50,
    max_extent_px: 2.0,
};

/// Ink pixels counted from a page edge before the content is considered to
/// have started.
///
/// The extent is where the page's content *begins*, not where the first
/// antialiased pixel lands, so a single stray pixel one row outside the real
/// content must not move it — a bound that tight would measure rasterizer
/// noise instead of layout.
///
/// The threshold is accumulated *across* rows rather than required within one.
/// A per-row threshold is blind to a vertical rule: a 3 px page border
/// contributes about three ink pixels to each of the thousand rows it runs
/// down, so a per-row filter discards it and a border running off the bottom
/// of the page looks exactly like one that stops at the right place. That is
/// not hypothetical — it is how `strict-stage5b` shipped a page border whose
/// vertical rules ran the full page height, past the horizontal ones, with
/// every structural bound satisfied. Forty pixels is a few glyph strokes: too
/// much for antialiasing, far less than any real rule or text line.
const MIN_EXTENT_INK_PX: usize = 40;

/// Extra pixels searched beyond [`StructuralLimits::max_shift_px`] when
/// looking for the best profile alignment.
///
/// The search range must strictly exceed the bound it is checked against,
/// otherwise the reported shift is clamped to the search window and a page
/// that drifted *further* than the bound can report a shift *inside* it —
/// the check then cannot fail. This margin is what makes the bound a real
/// bound; [`alignment_search_range_is_wider_than_every_bound`] pins the
/// invariant.
const ALIGNMENT_SEARCH_MARGIN_PX: isize = 8;

/// The alignment search range for `limits`: the bound plus the margin.
fn alignment_search_range(limits: &StructuralLimits) -> isize {
    limits.max_shift_px + ALIGNMENT_SEARCH_MARGIN_PX
}

/// Structural limits for the Stage-5C formula fixtures
/// (`STAGE-5C-REWORK-1` C1/C4): `strict-stage5c` and the real-Strict
/// repro `05`/`06`/`07`/`10`.
///
/// The pages are sparse — a handful of short lines with small constructs
/// between them — so the row/column ink profiles carry little structure to
/// correlate, and each construct differs from the WPS rendering by a fraction
/// of a line. The rasterizer-independent checks stay enforced: the ink ratio
/// (a blank or materially different page is rejected), the best-alignment shift,
/// the ink centroid and the ink extent.
///
/// `max_shift_px` is `12 px` on a 1056 px page (≈ 1.1 %). Before
/// `STAGE-5C-REWORK-1` the worst case drifted by 400 px (an `m:eqArr` row
/// spacing read as points instead of twips), so the bound is a two-order-of-
/// magnitude tightening of the defect it replaces. The correlation floors
/// relax only the measure a sparse page cannot satisfy.
///
/// `max_extent_px` is the bound that sees the remaining defect, and it is the
/// one number here that is a **ratchet on a known defect**, not an accepted
/// tolerance. Measured against the pinned references, the ink extent of the
/// five 5C fixtures disagrees with WPS by 0 px (`05`), −3 px (`10`),
/// −10 px (`06`), −16 px (`07`) and +1/+17 px (`strict-stage5c` page 1),
/// while every text, mixed and 5B page sits at ≤ 1 px.
///
/// A bound of `2 px` — the default, and what the other three classes use — is
/// what the layout should meet; `structural_extent_ratchet_pins_the_known_drift`
/// pins today's per-fixture values so the drift cannot grow, and closing it
/// is Stage-5C rework, not a gate change. Relaxing this number to make a
/// failure pass is exactly what the gate exists to prevent.
const STAGE5C_LIMITS: StructuralLimits = StructuralLimits {
    max_shift_px: 12,
    max_centroid_px: 24.0,
    min_row_correlation: 0.45,
    min_column_correlation: 0.40,
    max_extent_px: 18.0,
};

/// The per-fixture ink-extent drift each gated page is allowed to keep, in px.
///
/// This is the ratchet: it records the measured drift of every gated page
/// against its pinned WPS reference, so a layout change that makes a page
/// *worse* fails even while the 5C bound is still loose. Each value must be
/// the current measurement — lowering one is the way to claim progress, and it
/// is the number the Stage-5C rework has to move down.
const EXTENT_RATCHET: &[(&str, usize, f64, f64)] = &[
    ("strict-text", 0, 0.0, 1.0),
    ("strict-text-grid", 0, 0.0, 1.0),
    ("strict-stage5", 0, 0.0, -1.0),
    ("strict-stage5", 1, 1.0, 0.0),
    ("strict-stage5b", 0, -1.0, 1.0),
    ("strict-stage5c", 0, 1.0, 17.0),
    ("strict-stage5c", 1, 9.0, 18.0),
    ("05-strict-math-simple", 0, 0.0, -2.0),
    ("06-strict-math-display", 0, 0.0, -10.0),
    ("07-strict-drawingml-shapes", 0, 0.0, -16.0),
    ("10-strict-math-eqarr", 0, 0.0, -3.0),
];

/// Returns the structural limits for a reference document.
fn limits_for(name: &str) -> StructuralLimits {
    match name {
        "strict-stage5" => STAGE5_LIMITS,
        "strict-stage5b" => STAGE5B_LIMITS,
        "strict-stage5c"
        | "05-strict-math-simple"
        | "06-strict-math-display"
        | "07-strict-drawingml-shapes"
        | "10-strict-math-eqarr" => STAGE5C_LIMITS,
        _ => StructuralLimits::default(),
    }
}

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
/// Maximum allowed drift of the page's ink extent in px (either edge).
const MAX_EXTENT_DRIFT_PX: f64 = 2.0;

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
    /// Signed drift of the first substantial ink row, in px.
    pub top_delta: f64,
    /// Signed drift of the last substantial ink row, in px.
    pub bottom_delta: f64,
    /// Reference ink fraction.
    pub reference_ink: f64,
    /// Candidate ink fraction.
    pub candidate_ink: f64,
}

/// Structural bounds for [`structural_fidelity_with`].
#[derive(Clone, Copy, Debug)]
pub struct StructuralLimits {
    /// Maximum tolerated best-alignment shift, in px.
    pub max_shift_px: isize,
    /// Maximum tolerated ink-centroid drift, in px.
    pub max_centroid_px: f64,
    /// Minimum row-ink profile correlation.
    pub min_row_correlation: f64,
    /// Minimum column-ink profile correlation.
    pub min_column_correlation: f64,
    /// Maximum tolerated drift of the ink extent, in px (either edge).
    pub max_extent_px: f64,
}

impl Default for StructuralLimits {
    fn default() -> Self {
        Self {
            max_shift_px: MAX_ALIGNMENT_SHIFT_PX,
            max_centroid_px: MAX_CENTROID_SHIFT_PX,
            min_row_correlation: MIN_ROW_CORRELATION,
            min_column_correlation: MIN_COLUMN_CORRELATION,
            max_extent_px: MAX_EXTENT_DRIFT_PX,
        }
    }
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
    structural_fidelity_with(
        reference,
        candidate,
        width,
        height,
        &StructuralLimits::default(),
    )
}

/// Like [`structural_fidelity`] but with explicit [`StructuralLimits`].
///
/// # Errors
///
/// See [`structural_fidelity`].
pub fn structural_fidelity_with(
    reference: &[f64],
    candidate: &[f64],
    width: usize,
    height: usize,
    limits: &StructuralLimits,
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
        alignment_search_range(limits),
    );
    let correlation_y = correlation_y.ok_or_else(|| "no row structure to compare".to_owned())?;
    if shift_y.abs() > limits.max_shift_px {
        return Err(format!("vertical shift {shift_y}px exceeds tolerance"));
    }
    if correlation_y < limits.min_row_correlation {
        return Err(format!("row-ink correlation {correlation_y:.3} too low"));
    }
    let (shift_x, correlation_x) = best_profile_alignment(
        &column_ink_profile(reference, width, height),
        &column_ink_profile(candidate, width, height),
        alignment_search_range(limits),
    );
    let correlation_x = correlation_x.ok_or_else(|| "no column structure to compare".to_owned())?;
    if shift_x.abs() > limits.max_shift_px {
        return Err(format!("horizontal shift {shift_x}px exceeds tolerance"));
    }
    if correlation_x < limits.min_column_correlation {
        return Err(format!("column-ink correlation {correlation_x:.3} too low"));
    }
    let (centroid_x_delta, centroid_y_delta) = centroid_delta(reference, candidate, width, height);
    if centroid_x_delta.abs() > limits.max_centroid_px {
        return Err(format!(
            "horizontal centroid shift {centroid_x_delta:.2}px exceeds tolerance"
        ));
    }
    if centroid_y_delta.abs() > limits.max_centroid_px {
        return Err(format!(
            "vertical centroid shift {centroid_y_delta:.2}px exceeds tolerance"
        ));
    }
    // The extent is the outermost *substantial* ink on each edge. Unlike the
    // centroid it does not average over the page: a block that is a whole line
    // too tall moves the last ink row without moving the mean, which is exactly
    // the defect the centroid tolerates.
    let top_delta = extent_delta(reference, candidate, width, height, Edge::Top);
    if top_delta.abs() > limits.max_extent_px {
        return Err(format!(
            "top ink edge drifted {top_delta:.2}px (bound {:.2})",
            limits.max_extent_px
        ));
    }
    let bottom_delta = extent_delta(reference, candidate, width, height, Edge::Bottom);
    if bottom_delta.abs() > limits.max_extent_px {
        return Err(format!(
            "bottom ink edge drifted {bottom_delta:.2}px (bound {:.2})",
            limits.max_extent_px
        ));
    }
    Ok(StructuralFidelity {
        shift_y_px: shift_y,
        shift_x_px: shift_x,
        correlation_y,
        correlation_x,
        centroid_x_delta,
        centroid_y_delta,
        top_delta,
        bottom_delta,
        reference_ink,
        candidate_ink,
    })
}

/// Which page edge an [`extent_delta`] reads.
#[derive(Debug, Clone, Copy)]
enum Edge {
    /// The first substantial ink row.
    Top,
    /// The last substantial ink row.
    Bottom,
}

/// Returns the first/last row where the page's content starts, or `None` for a
/// blank image.
///
/// Rows are counted cumulatively from each edge; see
/// [`MIN_EXTENT_INK_PX`]. A page too sparse to reach the threshold falls back
/// to its outermost ink pixel rather than reporting no extent.
fn ink_rows(gray: &[f64], width: usize, height: usize) -> Option<(usize, usize)> {
    let row_ink: Vec<usize> = (0..height)
        .map(|row| {
            (0..width)
                .filter(|&col| gray[row * width + col] < INK_LEVEL)
                .count()
        })
        .collect();
    let outermost = || {
        let first = row_ink.iter().position(|&ink| ink > 0)?;
        let last = row_ink.iter().rposition(|&ink| ink > 0)?;
        Some((first, last))
    };
    let mut from_top = 0usize;
    let mut top = None;
    let mut from_bottom = 0usize;
    let mut bottom = None;
    for row in 0..height {
        if top.is_none() {
            from_top += row_ink[row];
            if from_top >= MIN_EXTENT_INK_PX {
                top = Some(row);
            }
        }
        let reversed = height - 1 - row;
        if bottom.is_none() {
            from_bottom += row_ink[reversed];
            if from_bottom >= MIN_EXTENT_INK_PX {
                bottom = Some(reversed);
            }
        }
        if top.is_some() && bottom.is_some() {
            break;
        }
    }
    match (top, bottom) {
        (Some(top), Some(bottom)) => Some((top, bottom)),
        _ => outermost(),
    }
}

/// Signed drift of one ink edge, `candidate - reference`, in px.
///
/// Zero when either image has no substantial ink; the ink-ratio check above
/// has already rejected a blank page by this point.
fn extent_delta(
    reference: &[f64],
    candidate: &[f64],
    width: usize,
    height: usize,
    edge: Edge,
) -> f64 {
    let pick = |rows: Option<(usize, usize)>| match edge {
        Edge::Top => rows.map(|value| value.0),
        Edge::Bottom => rows.map(|value| value.1),
    };
    match (
        pick(ink_rows(reference, width, height)),
        pick(ink_rows(candidate, width, height)),
    ) {
        (Some(reference_edge), Some(candidate_edge)) => {
            candidate_edge as f64 - reference_edge as f64
        }
        _ => 0.0,
    }
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

/// Prints the structural numbers of every page of a document.
///
/// The numbers are printed whether or not the page passes, so a regression is
/// readable from the log rather than only from the panic.
///
/// # Panics
///
/// If any page violated a structural bound.
fn report_structure(name: &str, structures: &[(usize, Result<StructuralFidelity, String>)]) {
    for (page, structure) in structures {
        match structure {
            Ok(structure) => eprintln!(
                "  page {page}: dy={}px corr_y={:.3} | dx={}px corr_x={:.3} | centroid d=({:.2},{:.2}) | edge d=({:+.0},{:+.0}) | ink ref={:.5} cand={:.5}",
                structure.shift_y_px,
                structure.correlation_y,
                structure.shift_x_px,
                structure.correlation_x,
                structure.centroid_x_delta,
                structure.centroid_y_delta,
                structure.top_delta,
                structure.bottom_delta,
                structure.reference_ink,
                structure.candidate_ink
            ),
            Err(reason) => panic!("{name} page {page}: structural check failed: {reason}"),
        }
    }
}

/// Grades one document's worst-page SSIM; returns `true` when it is amber.
///
/// Below [`SSIM_THRESHOLD`] is a regression and panics. In
/// `[SSIM_THRESHOLD, SSIM_THRESHOLD + SSIM_MARGIN)` the document passes only
/// because it is written down in [`SSIM_AMBER`], so a new fixture cannot join
/// the gate just below the line without being registered on purpose.
///
/// # Panics
///
/// If `worst` is below the threshold, or is inside the margin band without
/// being registered.
fn grade_ssim(name: &str, worst: f64) -> bool {
    assert!(
        worst >= SSIM_THRESHOLD,
        "{name}: worst SSIM {worst:.4} < {SSIM_THRESHOLD}"
    );
    let margin = worst - SSIM_THRESHOLD;
    if margin >= SSIM_MARGIN {
        return false;
    }
    assert!(
        SSIM_AMBER.contains(&name),
        "{name}: worst SSIM {worst:.4} clears {SSIM_THRESHOLD} by only {margin:.4}, \
         below the {SSIM_MARGIN:.2} margin, and is not registered in SSIM_AMBER"
    );
    true
}

#[test]
fn matches_wps_references() {
    let refs = strict_dir().join("refs");
    assert!(refs.is_dir(), "missing references at {}", refs.display());
    let fonts = fonts_dir();
    let mut checked = 0u32;
    let mut gated = 0u32;
    let mut amber: Vec<String> = Vec::new();
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
        let mut structures: Vec<(usize, Result<StructuralFidelity, String>)> = Vec::new();
        for (page, reference_path) in pages.iter().zip(&ref_pages) {
            let (width, height, reference) = load_png_gray(reference_path);
            let candidate = rasterize_gray(&page.svg, width, height, &fonts);
            assert_eq!(
                reference.len(),
                candidate.len(),
                "{name}: raster size mismatch"
            );
            let (width, height) = (width as usize, height as usize);
            if PAGE_COUNT_ONLY.contains(&name.as_str()) {
                // The page-count invariant is all that is meaningful here: the
                // content is raster data this renderer does not produce, so the
                // reference and our render legitimately differ completely.
                eprintln!(
                    "  page {}: page count only (raster content out of scope)",
                    page.index
                );
                continue;
            }
            let limits = limits_for(&name);
            structures.push((
                page.index,
                structural_fidelity_with(&reference, &candidate, width, height, &limits),
            ));
            comparisons.push(Comparison {
                reference,
                candidate,
                width,
                height,
            });
        }
        checked += 1;

        if SSIM_DOCS.contains(&name.as_str()) && !comparisons.is_empty() {
            let worst = worst_score(&comparisons).expect("at least one comparison");
            eprintln!(
                "{name}: worst SSIM = {worst:.4} (margin {:+.4}, needs {SSIM_MARGIN:+.4})",
                worst - SSIM_THRESHOLD
            );
            report_structure(&name, &structures);
            if grade_ssim(&name, worst) {
                amber.push(name.clone());
            }
            gated += 1;
        }
    }
    assert!(checked > 0, "no reference documents were checked");
    assert!(gated > 0, "no SSIM-gated documents were present");
    if !amber.is_empty() {
        eprintln!(
            "AMBER (clears the threshold, not the {SSIM_MARGIN:.2} margin): {}",
            amber.join(", ")
        );
    }
    // The register has to be exact in both directions, or it rots: a stale
    // entry keeps a fixed document looking like outstanding debt, and a missing
    // one lets a new near-miss through. The per-document check above rejects
    // the unregistered case; this rejects the stale one.
    amber.sort();
    let mut registered = SSIM_AMBER.to_vec();
    registered.sort_unstable();
    assert_eq!(
        amber, registered,
        "SSIM_AMBER is out of date: the documents actually inside the margin band \
         have changed (amber is the observed set)"
    );
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

#[test]
fn structural_check_rejects_blank_stage5_page() {
    // The relaxed Stage-5 limits must still reject a blank render (the hole
    // R5-1 flagged): the rasterizer-independent ink-coverage check applies.
    let (width, height, reference, _candidate) = rasterize_reference_pair("strict-stage5", 0);
    let blank = vec![1.0; width * height];
    assert!(
        structural_fidelity_with(&reference, &blank, width, height, &STAGE5_LIMITS).is_err(),
        "a blank candidate must be rejected even with the relaxed Stage-5 limits"
    );
}

#[test]
fn structural_check_rejects_blank_stage5b_page() {
    // STAGE-5B-REWORK-1 5B-2: even with the relaxed 5B limits, a blank render
    // (or one with no shape ink) must be rejected by the mandatory ink check.
    let (width, height, reference, _candidate) = rasterize_reference_pair("strict-stage5b", 0);
    let blank = vec![1.0; width * height];
    assert!(
        structural_fidelity_with(&reference, &blank, width, height, &STAGE5B_LIMITS).is_err(),
        "a blank candidate must be rejected even with the relaxed Stage-5B limits"
    );
    // A page-border-only render (shapes/group/text/picture missing) is also
    // under-inked relative to the reference and must be rejected.
    let mut border_only = vec![1.0; width * height];
    for y in 0..height {
        for x in 0..width {
            let on_border = (32..34).contains(&y)
                || (height.saturating_sub(34)..height.saturating_sub(32)).contains(&y)
                || (32..34).contains(&x)
                || (width.saturating_sub(34)..width.saturating_sub(32)).contains(&x);
            if on_border {
                border_only[y * width + x] = 0.0;
            }
        }
    }
    assert!(
        structural_fidelity_with(&reference, &border_only, width, height, &STAGE5B_LIMITS).is_err(),
        "a page-border-only candidate must be rejected (shape ink missing)"
    );
}

/// STAGE-5C-REWORK-1 C4: the relaxed 5C limits must still reject a blank page
/// and a materially shifted one, on the formula fixture and on a real-Strict
/// repro. The gate only means something if it can fail.
#[test]
fn structural_check_rejects_blank_and_shifted_stage5c_pages() {
    for (document, page) in [("strict-stage5c", 0usize), ("10-strict-math-eqarr", 0)] {
        let (width, height, reference, candidate) = rasterize_reference_pair(document, page);
        // The unshifted render passes.
        assert!(
            structural_fidelity_with(&reference, &candidate, width, height, &STAGE5C_LIMITS)
                .is_ok(),
            "{document} page {page} must pass its own gate"
        );
        // A blank render is rejected by the ink ratio.
        let blank = vec![1.0; width * height];
        assert!(
            structural_fidelity_with(&reference, &blank, width, height, &STAGE5C_LIMITS).is_err(),
            "{document}: a blank render must be rejected"
        );
        // A vertical drift beyond the 12 px alignment bound is rejected.
        let shifted: Vec<f64> = {
            let mut out = vec![1.0; width * height];
            for row in 20..height {
                out[row * width..(row + 1) * width]
                    .copy_from_slice(&candidate[(row - 20) * width..(row - 19) * width]);
            }
            out
        };
        assert!(
            structural_fidelity_with(&reference, &shifted, width, height, &STAGE5C_LIMITS).is_err(),
            "{document}: a 20px vertical drift must be rejected"
        );
    }
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

/// Shifts an image down by `dy` px (fills the exposed rows white).
fn shift_vertical(gray: &[f64], width: usize, height: usize, dy: isize) -> Vec<f64> {
    let mut out = vec![1.0; width * height];
    for row in 0..height {
        let source = row as isize - dy;
        if source >= 0 && (source as usize) < height {
            out[row * width..(row + 1) * width]
                .copy_from_slice(&gray[source as usize * width..(source as usize + 1) * width]);
        }
    }
    out
}

/// Stretches an image vertically by `by` px about its middle row: the top half
/// moves up, the bottom half down. The ink centroid barely moves (the two
/// halves cancel) while both ink edges do, which is the point of the extent
/// check.
fn stretch_vertical(gray: &[f64], width: usize, height: usize, by: isize) -> Vec<f64> {
    let mid = (height / 2) as isize;
    let mut out = vec![1.0; width * height];
    for row in 0..height {
        let source = if (row as isize) < mid {
            row as isize - by
        } else {
            row as isize + by
        };
        if source >= 0 && (source as usize) < height {
            out[row * width..(row + 1) * width]
                .copy_from_slice(&gray[source as usize * width..(source as usize + 1) * width]);
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

/// The alignment search range must be strictly wider than the bound it is
/// checked against, or the shift check can never fail: the reported shift is
/// clamped to the search window, so a page that drifted further than the bound
/// would report a shift inside it. This pins the invariant for every limit set
/// rather than trusting the arithmetic at each call site.
#[test]
fn alignment_search_range_is_wider_than_every_bound() {
    for (name, limits) in [
        ("default", StructuralLimits::default()),
        ("5A", STAGE5_LIMITS),
        ("5B", STAGE5B_LIMITS),
        ("5C", STAGE5C_LIMITS),
    ] {
        assert!(
            alignment_search_range(&limits) > limits.max_shift_px,
            "{name}: search range {} must exceed the bound {}",
            alignment_search_range(&limits),
            limits.max_shift_px
        );
    }
}

/// The shift bound must reject a drift *beyond* the bound, not merely a drift
/// inside the search window. The 5C limits are the widest, so they are the
/// case that previously could not fail: a 20 px vertical drift is well outside
/// the ±12 px window the gate used to search, and the old check reported it as
/// an in-bounds shift.
#[test]
fn a_drift_beyond_the_shift_bound_is_rejected() {
    let (width, height, reference, candidate) = rasterize_reference_pair("strict-stage5c", 1);
    for drift in [20isize, 40, 80] {
        let shifted = shift_vertical(&candidate, width, height, drift);
        let result = structural_fidelity_with(&reference, &shifted, width, height, &STAGE5C_LIMITS);
        assert!(
            result.is_err(),
            "a {drift}px vertical drift must be rejected, got {result:?}"
        );
    }
}

/// The ink extent is what catches a block that is a whole line too tall: the
/// centroid averages over the page and barely moves, and the correlation is
/// computed after the best alignment, so neither sees it. This is the real
/// defect behind the relaxed 5C limits.
#[test]
fn the_extent_check_sees_drift_the_centroid_does_not() {
    let (width, height, reference, candidate) = rasterize_reference_pair("strict-stage5c", 1);
    // Every bound except the extent one is disabled, so the extent check is
    // the only thing that can reject this page. The shift bound is large
    // rather than unbounded so the alignment search range stays in range.
    let without_extent = StructuralLimits {
        max_shift_px: 4096,
        min_row_correlation: 0.0,
        min_column_correlation: 0.0,
        max_centroid_px: f64::MAX,
        max_extent_px: f64::MAX,
    };
    assert!(
        structural_fidelity_with(&reference, &candidate, width, height, &without_extent).is_ok(),
        "with the extent bound disabled the fixture must pass, \
         otherwise it is rejected by an unrelated bound and the test proves nothing"
    );
    // Stretched by 10 px about the middle: the two halves cancel in the
    // centroid, but both ink edges move.
    let stretched = stretch_vertical(&candidate, width, height, 5);
    let just_extent = StructuralLimits {
        max_extent_px: 2.0,
        ..without_extent
    };
    let error = structural_fidelity_with(&reference, &stretched, width, height, &just_extent)
        .expect_err("a 10px vertical stretch must be caught by the extent bound");
    assert!(
        error.contains("ink edge"),
        "the extent bound must be what rejects the stretch, got: {error}"
    );
}

/// The extent ratchet: every gated page's ink-extent drift is pinned to its
/// current measured value against the pinned WPS reference.
///
/// The class bounds in [`StructuralLimits`] are coarse — the 5C set is
/// carried by a single loose `max_extent_px` so that `05` (0 px) and
/// `strict-stage5c` page 2 (18 px) share one number. This test is the fine
/// grain: it fails the moment any page drifts *further*, so a layout change
/// cannot hide inside the loose bound, and it fails when a page gets better
/// and the pinned value was not lowered, which is how progress is claimed.
#[test]
fn structural_extent_ratchet_pins_the_known_drift() {
    for (document, page, top, bottom) in EXTENT_RATCHET {
        let (width, height, reference, candidate) = rasterize_reference_pair(document, *page);
        let top_delta = extent_delta(&reference, &candidate, width, height, Edge::Top);
        let bottom_delta = extent_delta(&reference, &candidate, width, height, Edge::Bottom);
        assert_eq!(
            top_delta, *top,
            "{document} page {page}: top ink edge moved (was {top}, now {top_delta})"
        );
        assert_eq!(
            bottom_delta, *bottom,
            "{document} page {page}: bottom ink edge moved (was {bottom}, now {bottom_delta})"
        );
    }
}

/// The ratchet must cover every gated page: a fixture added to the gate
/// without a pinned extent would escape the fine-grained check.
#[test]
fn every_gated_page_is_pinned_by_the_extent_ratchet() {
    let mut gated: Vec<(String, usize)> = Vec::new();
    for name in SSIM_DOCS {
        if PAGE_COUNT_ONLY.contains(name) {
            continue;
        }
        let dir = strict_dir().join(format!("refs/{name}"));
        for (index, _) in reference_pages(&dir).into_iter().enumerate() {
            gated.push(((*name).to_owned(), index));
        }
    }
    let pinned: std::collections::BTreeSet<(String, usize)> = EXTENT_RATCHET
        .iter()
        .map(|(name, page, _, _)| ((*name).to_owned(), *page))
        .collect();
    for entry in gated {
        assert!(
            pinned.contains(&entry),
            "{entry:?} is gated but not pinned in EXTENT_RATCHET"
        );
    }
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
