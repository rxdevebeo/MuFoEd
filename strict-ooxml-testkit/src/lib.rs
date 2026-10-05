//! Test-only builders for the workspace (`REWORK-AUDIT-2026-10.md`, AUD-01).
//!
//! Every hostile or synthetic input a test needs is built here, in code, rather
//! than committed as a binary: a fixture that is a function can be read,
//! reviewed and varied, and one that is a blob cannot.
//!
//! * [`zip`] — a ZIP writer with its own CRC-32, independent of the reader under
//!   test;
//! * [`docx`] — [`DocxBuilder`], a minimal valid package for either conformance
//!   family, with every part replaceable;
//! * [`pdf`] — [`PdfBuilder`], a PDF 1.7 with a correct cross-reference table;
//! * [`xml`] — generators for deep and wide markup;
//! * [`harness`] — runs a closure on a small stack under a timeout, so a test can
//!   tell a returned error from a panic and from a hang;
//! * [`audit`] — one generated DOCX or PDF per confirmed audit mechanism;
//! * [`inspect`] — a ZIP/XML and PDF-structure check that does not use the
//!   production parsers.
//!
//! The crate depends on no other crate of the workspace and is never published.

pub mod audit;
pub mod docx;
pub mod harness;
pub mod inspect;
pub mod pdf;
pub mod xml;
pub mod zip;

pub use audit::{scenarios, Scenario, ScenarioKind, SCENARIOS};
pub use docx::{DocxBuilder, Family};
pub use harness::{assert_survives, bounded, bounded_with, Outcome};
pub use inspect::{inspect_docx, inspect_pdf, InspectError};
pub use pdf::PdfBuilder;
pub use zip::{Method, ZipBuilder};
