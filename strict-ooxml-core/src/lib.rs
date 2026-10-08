//! Resource-safe raw layer for reading `WordprocessingML` `Strict` packages.
//!
//! `strict-ooxml-core` is the foundation shared by every other crate in the
//! toolkit. It implements the OPC/ZIP packaging layer, a streaming
//! namespace-aware XML reader with hard resource limits, the Strict ↔
//! Transitional namespace registry, conformance detection and the unified error
//! model (see `TZ-STRICT-OOXML-RUST.md` §8, §9, §12, §13).
//!
//! This crate deliberately does **not** build a `WordprocessingML` DOM and does
//! **not** render anything; those concerns belong to later stages.
//!
//! # Module map
//!
//! - [`opc`] — package (`.docx`) access: ZIP reader, `[Content_Types].xml`,
//!   relationships and target-path canonicalization.
//! - [`xml`] — streaming XML tokenizer with namespace resolution and hard
//!   safety limits (no DTD, no external entities).
//! - [`ns`] — Strict ↔ Transitional namespace registry and conformance
//!   detection.
//! - [`normalize`] — extension point for the future Transitional → Strict raw
//!   normalization stages (T1–T8).
//! - [`error`] — the crate-wide error type and result alias.
//! - [`limits`] — the tunable resource budget.
//! - [`part`] — part identifiers and lazy part access.
//!
//! # Stage status
//!
//! Stage 1 task S1.1 delivers this skeleton only; the module contents are
//! implemented by tasks S1.2–S1.13.
// Never-crash (docs/WORDCRAFT_ADOPTION_2026-10-07.md §4.2): library paths
// return errors or degrade with a report; tests may still unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod error;
pub mod limits;
pub mod normalize;
pub mod ns;
pub mod opc;
pub mod part;
pub mod pipeline;
pub mod xml;
