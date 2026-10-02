//! Structural similarity between two pages (`STAGE-4-RENDER-FIDELITY.md` S4F.5,
//! Wang et al. 2004).
//!
//! The *fidelity* half of the gate. It says the pages carry the same detail in
//! the same places; [`crate::structure`] says the pages carry the same ink. Both
//! are needed: SSIM barely penalises sparse text that moved, and the structural
//! checks cannot tell a substituted face from a correct one.

use crate::gray::Gray;

/// SSIM window edge (an 11×11 window, `σ = 1.5`, as in the reference paper).
const SSIM_WINDOW: usize = 11;
/// SSIM Gaussian standard deviation.
const SSIM_SIGMA: f64 = 1.5;
/// SSIM stabilising constant `C1 = (0.01·L)²` for dynamic range `L = 1`.
const SSIM_C1: f64 = 0.01 * 0.01;
/// SSIM stabilising constant `C2 = (0.03·L)²` for dynamic range `L = 1`.
const SSIM_C2: f64 = 0.03 * 0.03;

/// Computes the mean SSIM of two pages.
///
/// Uses the standard constants `C1 = (0.01·L)²`, `C2 = (0.03·L)²` with dynamic
/// range `L = 1` and an 11×11 Gaussian window; the per-window map is averaged
/// over the interior (border windows are excluded). Falls back to the single
/// global window for pages smaller than the window.
///
/// # Panics
///
/// Panics if the pages have different sizes.
#[must_use]
pub fn ssim(reference: &Gray, candidate: &Gray) -> f64 {
    assert_eq!(
        (reference.width(), reference.height()),
        (candidate.width(), candidate.height()),
        "the pages must be the same size"
    );
    let (width, height) = (reference.width(), reference.height());
    let (reference, candidate) = (reference.pixels(), candidate.pixels());
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

/// Single-window (global) SSIM, used for pages smaller than the window.
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

/// A reference page paired with a candidate page of the same size.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// The committed WPS reference.
    pub reference: Gray,
    /// What this workspace produced.
    pub candidate: Gray,
}

impl Comparison {
    /// Pairs two pages.
    ///
    /// # Panics
    ///
    /// Panics if the pages are not the same size; [`ssim`] requires it.
    #[must_use]
    pub fn new(reference: Gray, candidate: Gray) -> Self {
        assert_eq!(
            (reference.width(), reference.height()),
            (candidate.width(), candidate.height()),
            "a comparison pairs pages of the same size"
        );
        Self {
            reference,
            candidate,
        }
    }

    /// The SSIM of this pair.
    #[must_use]
    pub fn score(&self) -> f64 {
        ssim(&self.reference, &self.candidate)
    }
}

/// Runs every comparison and returns the worst (minimum) SSIM score.
///
/// The gate is the *worst* page, not the mean: a document whose second page lost
/// its text has not rendered correctly on average.
///
/// Returns `None` when there is nothing to compare.
#[must_use]
pub fn worst_score(comparisons: &[Comparison]) -> Option<f64> {
    comparisons.iter().map(Comparison::score).reduce(f64::min)
}

#[cfg(test)]
mod tests {
    use super::{ssim, worst_score, Comparison};
    use crate::gray::Gray;

    #[test]
    fn identical_pages_score_one() {
        let pixels: Vec<f64> = (0..256).map(|index| f64::from(index % 16) / 15.0).collect();
        let page = Gray::new(16, 16, pixels);
        assert!((ssim(&page, &page) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_perturbed_page_scores_below_one() {
        let pixels: Vec<f64> = (0..256).map(|index| f64::from(index % 16) / 15.0).collect();
        let reference = Gray::new(16, 16, pixels.clone());
        let other: Vec<f64> = pixels
            .iter()
            .enumerate()
            .map(|(index, value)| if index % 3 == 0 { 1.0 - value } else { *value })
            .collect();
        assert!(ssim(&reference, &Gray::new(16, 16, other)) < 1.0);
    }

    #[test]
    fn a_page_smaller_than_the_window_falls_back_to_one_global_map() {
        // Two flat pages of the same level are the degenerate case the global
        // fallback exists for: there is no window to slide, and the answer has
        // to be 1 rather than a division by zero variance.
        let reference = Gray::filled(4, 4, 1.0);
        assert!((ssim(&reference, &Gray::filled(4, 4, 1.0)) - 1.0).abs() < 1e-12);
        // Two flat pages of different levels are still perfectly flat, so the
        // mean term carries the whole score - which is below 1, not a crash.
        assert!(ssim(&reference, &Gray::filled(4, 4, 0.5)) < 1.0);
    }

    #[test]
    fn worst_score_is_the_minimum_not_the_mean() {
        let blank = Gray::filled(16, 16, 1.0);
        let inked = {
            let mut pixels = vec![1.0; 16 * 16];
            for row in 4..12 {
                for column in 2..14 {
                    pixels[row * 16 + column] = 0.0;
                }
            }
            Gray::new(16, 16, pixels)
        };
        let comparisons = [
            Comparison::new(inked.clone(), inked.clone()),
            Comparison::new(inked.clone(), blank),
        ];
        let worst = worst_score(&comparisons).expect("scores");
        assert!(worst < 1.0, "the blank page must drag the score down");
        assert_eq!(worst_score(&[]), None);
    }
}
