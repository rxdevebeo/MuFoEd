//! Serializes a [`Document`] back to a `.docx` package.
//!
//! The crate is the answer to a gap the specification left open: `TZ` §3.2 kept
//! «saving in Strict» out of the MVP, so every stage up to 7 could read and
//! render but not write. Stage 8A closes that (`STAGE-8-TASK.md` §3).
//!
//! Three properties are load-bearing:
//!
//! 1. **Strict only.** Every namespace and every relationship type written is
//!    the `purl.oclc.org/ooxml` one, so a written package never carries a
//!    Transitional signal and never needs a second pass.
//! 2. **Nothing is dropped silently.** A construct the writer cannot express is
//!    recorded in the report ([`WriteReport`]) and counted, and
//!    [`verify_no_silent_loss`] asserts the counts agree (SC-10).
//! 3. **Reproducible.** The same [`Document`] produces the same bytes (SC-1).
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml_core::opc::{OpenOptions, Package};
//! use strict_ooxml_wml::{parse_document, ParseOptions};
//! use strict_ooxml_write::{verify_no_silent_loss, write_package, WriteOptions};
//!
//! let package = Package::open_path("document.docx", &OpenOptions::default())?;
//! let document = parse_document(&package, &ParseOptions::default())?;
//! let written = write_package(&document, Some(&package), &WriteOptions::default())?;
//! std::fs::write("rewritten.docx", &written.bytes)?;
//! for loss in written.report.losses() {
//!     eprintln!("{loss}");
//! }
//! assert_eq!(verify_no_silent_loss(&written.report), Ok(()));
//! # Ok::<(), strict_ooxml_core::error::StrictError>(())
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
// The writer walks a model whose value types are newtypes over integers, and it
// mirrors schema element order literally; both make the lints below noise
// rather than signal. `doc_markdown` fires on OOXML element names in prose,
// `ref_option` on the model's `Option` fields being borrowed (the model owns
// them), `default_trait_access` on `Default::default()` for a type with no
// `new()`, and `single_match_else`/`manual_let_else` on the explicit
// Some/None arms that keep the schema order readable.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::manual_let_else,
    clippy::ref_option,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref
)]

pub mod body;
pub mod ctx;
pub mod drawing;
pub mod math;
pub mod package;
pub mod parts;
pub mod props;
pub mod xml;

mod passthrough;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::normalize::report::NormalizationReport;
use strict_ooxml_wml::model::Document;

pub use package::{write_package, WriteOptions, WriteOutput};
pub use xml::WriteError;

/// The change report a write produces.
///
/// This is the Stage-6 [`NormalizationReport`] under a name that fits the
/// direction: a write changes nothing semantically on its own, so its records
/// are exactly the constructs it could not express and the parts it rewrote.
/// Reusing the type rather than defining a parallel one is deliberate — the
/// «no silent loss» invariant ([`verify_no_silent_loss`]) is the project's
/// mechanism for SC-10 and must not exist twice.
pub type WriteReport = NormalizationReport;

/// Returns the number of constructs a write could not express at all.
///
/// # Errors
///
/// Never; the function is total.
#[must_use]
pub fn dropped_count(report: &WriteReport) -> usize {
    report
        .losses()
        .iter()
        .filter(|loss| loss.transform_id == ctx::WRITE_LOSS_ID)
        .count()
}

/// Verifies that every construct the writer dropped reached the report (SC-10).
///
/// # Errors
///
/// Returns a description of the discrepancy when the invariant is broken.
pub fn verify_no_silent_loss(report: &WriteReport) -> std::result::Result<(), String> {
    report.verify_no_silent_loss()
}

/// Serializes `document` and returns only the bytes.
///
/// # Errors
///
/// Same as [`write_package`].
pub fn write_bytes(
    document: &Document,
    source: Option<&dyn package::Source>,
    options: &WriteOptions,
) -> Result<Vec<u8>> {
    Ok(write_package(document, source, options)?.bytes)
}
