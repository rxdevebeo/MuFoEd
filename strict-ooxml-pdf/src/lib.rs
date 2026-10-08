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
//! of the page, because that is the space the WML model is written in (twips from
//! the top margin) and a converter that had to flip every coordinate on the way
//! out would be a second place to get the flip wrong. PDF's y-up space is an
//! intermediate step inside the interpreter and nowhere else.
//!
//! ## Example
//!
//! ```no_run
//! use strict_ooxml_pdf::{PdfDocument, PdfLimits};
//!
//! let bytes = std::fs::read("document.pdf").expect("readable");
//! let mut pdf = PdfDocument::open(&bytes, PdfLimits::default())?;
//! for page in pdf.pages()? {
//!     println!("page: {} items", page.items().len());
//! }
//! for loss in pdf.report().losses() {
//!     eprintln!("{loss}");
//! }
//! # Ok::<(), strict_ooxml_pdf::PdfError>(())
//! ```
//!
//! ## State of the phase
//!
//! Landed: the resource budget, the font layer, the content interpreter, images
//! and path flattening; the corpus round trip against a PDF this workspace wrote
//! (`O-9`); and the pixel gate over the WPS references (`O-2`), which reads this
//! crate's own output back and rasterizes it. See `STAGE-8-OPEN.md`.
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
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )
)]
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
// PDF dictionaries index glyphs with numbers a producer wrote, so the casts into
// glyph codes are deliberate and clamped; the rest is the model's own f64
// geometry.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    // PDF numbers arrive as `i64` or `f32` and are geometry: a length, a
    // rotation, a colour component, a glyph code. Every one of them is small,
    // and the conversions are the only way to get them into one arithmetic.
    clippy::cast_precision_loss,
    clippy::cast_lossless
)]

pub mod content;
pub mod document;
pub mod error;
pub mod fonts;
pub mod image;
pub mod inline;
mod preload;
#[cfg(feature = "raster")]
pub mod raster;
pub mod report;
pub mod text;

pub use crate::content::{
    Content, Glyph, Item, LineCap, LineJoin, Matrix, PageBudget, PageGeometry, PlacedImage,
    RenderMode, Rgb, SubPath, Vector,
};
pub use crate::document::{PdfDocument, PdfPage, TextLayer};
pub use crate::error::{LimitKind, PdfError, PdfLimits, OBJECT_STREAM_BUDGET_FACTOR};
pub use crate::fonts::{BaseEncoding, PdfFont, Width};
pub use crate::image::{Encoded, Reject};
pub use crate::report::{Loss, ReadReport};
pub use crate::text::{lines as text_lines, GlyphLine};

/// The largest deviation a flattened Bézier may have from the curve, in points.
///
/// SC-9 allows 0.5 pt; the reader uses a quarter of that, because the
/// consumer that draws the result is a *different* renderer and a quarter of
/// the budget spent here is a quarter the consumer never has to.
pub const FLATTEN_TOLERANCE_PT: f64 = 0.125;

pub use document::{PdfDocument as Document, PdfPage as Page};
