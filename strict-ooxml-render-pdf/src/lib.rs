//! Deterministic PDF rendering for WordprocessingML Strict documents.
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown
)]

pub mod document;
pub mod font;
pub mod image;
pub mod matrix;
pub mod path;
pub mod report;
pub mod units;

pub use document::{render, render_with_source, PdfOutput};
pub use report::{PdfLoss, PdfReport, Severity as LossSeverity};
