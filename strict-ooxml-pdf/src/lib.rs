//! PDF reading: pages, text with geometry, fonts, images and vector paths
//! (`STAGE-8-TASK.md` §5, phase 8C).
//!
//! The object model, the cross-reference table, the stream filters and the
//! content-stream *tokenizer* come from `lopdf`. Everything that decides what a
//! page means is here: the font layer (encodings, `ToUnicode`, widths), the
//! graphics-state machine, the placement of glyphs, the flattening of curves
//! and the choice of what to do with a construct that cannot be represented.
//!
//! ## Coordinates
//!
//! Output is in **points** (1/72 inch) with **y measured downwards from the top**
//! of the page, because that is the space the WML model is written in
//! (twips from the top margin) and a converter that had to flip every coordinate
//! on the way out would be a second place to get the flip wrong. PDF's own
//! y-up space is only an intermediate step inside the interpreter.
//!
//! ## State of the phase
//!
//! Landed: the resource budget ([`error::PdfLimits`]) and the font layer
//! ([`fonts`]). The content interpreter, the page assembly, images and vector
//! paths are the next increments; see `STAGE-8-OPEN.md` (O-4).

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
// PDF dictionaries index glyphs with numbers a producer wrote, so the casts
// into glyph codes are deliberate and clamped by `to_code`; the rest is the
// model's own arithmetic on f64 geometry.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown
)]

pub mod error;
pub mod fonts;
