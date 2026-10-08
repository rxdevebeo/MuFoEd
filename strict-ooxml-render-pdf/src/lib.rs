//! Deterministic PDF rendering for WordprocessingML Strict documents.
//!
//! The second backend over the layout `strict-ooxml-render-svg` produces
//! (ADR-0008): same placed pages, PDF operators instead of SVG elements. It reads
//! nothing but the placed pages it is given
//! ([`strict_ooxml_render_svg::place_pages`]), so it cannot disagree with the SVG about
//! where a line breaks.
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml_core::opc::{OpenOptions, Package};
//! use strict_ooxml_render_pdf::render_with_source;
//! use strict_ooxml_render_svg::{place_pages, RenderOptions};
//! use strict_ooxml_wml::{parse_document, ParseOptions};
//!
//! let package = Package::open_path("document.docx", &OpenOptions::default())?;
//! let document = parse_document(&package, &ParseOptions::default())?;
//! let options = RenderOptions::default();
//! let pages = place_pages(&document, &options, Some(&package))?;
//! let pdf = render_with_source(&pages, &options, Some(&package))?;
//! std::fs::write("out.pdf", &pdf.bytes)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # What the report is for, and how to read it
//!
//! A PDF is a different language from a WML document and some things do not cross
//! (`SC-10`). Nothing is dropped silently: [`PdfReport`] carries every one of
//! them. Read it with [`PdfReport::losses_by_severity`] — worst first, which is
//! the order a person needs — or with [`PdfReport::losses`], which is the order
//! the losses were found and is what `Display` used to print. The two differ
//! because a document that lost its text and one that lost a picture are found in
//! an order that says nothing about which matters.
//!
//! # What is deliberately not here
//!
//! - **The document's `title` and `author`.** `/Info` is not written, so a PDF
//!   reader shows no title and `pdfinfo` prints an empty one. This is a
//!   conscious omission (`STAGE-8-OPEN.md` Q-8), not an oversight: `/Info` is
//!   *metadata about a producer*, and this backend has no opinion about who
//!   produced the `.docx` — it never read `docProps/core.xml`, and inventing an
//!   author would be worse than writing none. Carrying it through means reading
//!   `docProps` and deciding what to do with a date, which `SC-1` forbids
//!   (`dcterms:modified` is a wall clock, and two renders of one document must be
//!   byte-identical).
//! - **Inline images** (`BI`…`ID`…`EI`) when *reading* a PDF, which is
//!   `strict-ooxml-pdf`'s `L-9`, not this crate's. Writing is a different
//!   operation and a picture placed by the layout becomes a normal `XObject`.
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
        clippy::unreachable
    )
)]
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
