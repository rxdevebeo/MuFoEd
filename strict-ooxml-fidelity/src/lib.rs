//! The visual-fidelity gate's arithmetic: grayscale pages, SSIM, and the
//! structural ink checks that SSIM is bad at.
//!
//! **This crate exists because two gates compare pixels and must mean the same
//! thing by it.** `strict-ooxml-render-svg/tests/ssim.rs` measures our SVG
//! against the WPS references; `strict-ooxml-pdf/tests/pdf_pixels.rs` measures our
//! PDF, rasterized, against those same references. A second copy of SSIM would be
//! a second definition of SSIM, and a threshold quoted from one gate and applied
//! to the other would then be a number about a different quantity. So the
//! measurement lives here and both tests call it.
//!
//! # What is compared
//!
//! A [`Gray`] page: 8-bit RGB(A) or grayscale, linearized to Rec. 601 luma in
//! `[0, 1]`, composited on white. Both gates therefore compare the same
//! luminance of the same reference file.
//!
//! # What is judged, and why two ways
//!
//! SSIM is a *fidelity* measure and it is nearly blind to sparse ink: a shifted
//! or dropped line of text costs a fraction of a percent, because most of the
//! window is white in both images. The structural checks in [`structure`] are
//! the other half - ink coverage, best whole-page alignment, ink centroid, ink
//! extent and row/column ink profiles - and they are rasterizer-independent:
//! they are computed from thresholds on gray levels, not from anything a
//! particular renderer does at the edges of a glyph.
//!
//! # The numbers are not here
//!
//! Thresholds live in `coverage/render-gates.toml`, next to the reason each one
//! has the value it has (GATE-STRATEGY §6: a constant edited inside a test looks
//! like a test change and gets reviewed as one). [`policy`] parses that file.

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // A page is indexed by pixel, and a pixel index is compared against a signed
    // search window on the way to "which shift lines the profiles up". The
    // alternative is a checked conversion per pixel on every page of every run.
    clippy::cast_possible_wrap,
    clippy::doc_markdown,
    // Threshold comparisons are the whole point of a gate: `score >= 0.95` and
    // `bounds == the class's number` are what it has to be able to say.
    clippy::float_cmp,
    clippy::similar_names
)]

pub mod gray;
pub mod policy;
pub mod ssim;
pub mod structure;

pub use crate::gray::{crop, decode_png_gray, from_rgba8, load_png_gray, Gray, INK_LEVEL};
pub use crate::policy::{GatePolicy, PdfClassLimits, PolicyClass, PolicyOverride, Thresholds};
pub use crate::ssim::{ssim, worst_score, Comparison};
pub use crate::structure::{
    alignment_search_range, best_profile_alignment, column_ink_profile, extent_delta, ink_centroid,
    ink_fraction, ink_rows, measurement_line, row_ink_profile, structural_fidelity,
    structural_fidelity_with, Edge, StructuralFidelity, StructuralLimits,
};
