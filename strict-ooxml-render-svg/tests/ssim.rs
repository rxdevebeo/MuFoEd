//! SSIM harness (`STAGE-4-TASK.md` §8.2, §11.2).
//!
//! The structural/determinism/oracle checks fully gate rendering. The SSIM gate
//! against *approved external* references is **waived** in this environment
//! (see `docs/stage-4-report.md` and ADR-0006): producing those references
//! needs Word/LibreOffice plus an external rasterizer, neither of which is
//! available here. This file implements the SSIM metric and its self-check, and
//! a harness entry point that consumes caller-provided grayscale buffers so the
//! gate can be enabled unchanged once references are supplied.

#![allow(clippy::expect_used, clippy::float_cmp, clippy::cast_precision_loss)]

/// Computes the global SSIM of two equal-length grayscale buffers in `[0, 1]`.
///
/// Uses the standard constants `C1 = (0.01·L)²`, `C2 = (0.03·L)²` with dynamic
/// range `L = 1`.
///
/// # Panics
///
/// Panics if the inputs differ in length or are empty.
#[must_use]
pub fn ssim(reference: &[f64], candidate: &[f64]) -> f64 {
    const C1: f64 = 0.01 * 0.01;
    const C2: f64 = 0.03 * 0.03;
    assert_eq!(reference.len(), candidate.len(), "length mismatch");
    assert!(!reference.is_empty(), "empty input");
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
    ((2.0 * mean_a * mean_b + C1) * (2.0 * cov + C2))
        / ((mean_a * mean_a + mean_b * mean_b + C1) * (var_a + var_b + C2))
}

/// Pairs a reference with a candidate buffer.
pub struct Comparison {
    /// Reference grayscale image.
    pub reference: Vec<f64>,
    /// Candidate grayscale image.
    pub candidate: Vec<f64>,
}

/// Runs every comparison and returns the worst (minimum) SSIM score.
///
/// Returns `None` when there is nothing to compare (the references have not
/// been supplied yet); callers treat that as the documented waiver.
#[must_use]
pub fn worst_score(comparisons: &[Comparison]) -> Option<f64> {
    comparisons
        .iter()
        .map(|pair| ssim(&pair.reference, &pair.candidate))
        .reduce(f64::min)
}

#[test]
fn identical_images_score_one() {
    let image: Vec<f64> = (0..256).map(|index| f64::from(index % 16) / 15.0).collect();
    assert!((ssim(&image, &image) - 1.0).abs() < 1e-12);
}

#[test]
fn perturbed_images_score_below_one() {
    let reference: Vec<f64> = (0..256).map(|index| f64::from(index % 16) / 15.0).collect();
    let candidate: Vec<f64> = reference
        .iter()
        .enumerate()
        .map(|(index, value)| if index % 3 == 0 { 1.0 - value } else { *value })
        .collect();
    assert!(ssim(&reference, &candidate) < 1.0);
}

#[test]
fn worst_score_aggregates() {
    let image: Vec<f64> = vec![0.5; 16];
    assert_eq!(worst_score(&[]), None);
    let good = Comparison {
        reference: image.clone(),
        candidate: image.clone(),
    };
    assert!((worst_score(&[good]).expect("score") - 1.0).abs() < 1e-12);
}
