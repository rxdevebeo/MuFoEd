//! LaTeX source to an owned `WordprocessingML` model, with explicit import losses.
//!
//! The importer reads a bounded subset of LaTeX: sectioning, paragraph text and
//! character formatting, lists, tables, figures and images, footnotes,
//! hyperlinks, verbatim blocks and `$...$`/`\[...\]`/`equation`/`align` math
//! (translated to OMML). `\input` and `\include` are resolved through a
//! [`SourceProvider`]; every construct it cannot express is a named diagnostic
//! in the [`LatexReport`], never a silent drop.
//!
//! ```
//! use strict_ooxml_latex::{import, LatexOptions};
//! let imported = import(b"\\section{Intro}\nHello \\textbf{world}.", &LatexOptions::default()).unwrap();
//! assert_eq!(imported.document.body.blocks.len(), 2);
//! ```
#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::many_single_char_names,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::redundant_else,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    clippy::unused_self
)]

mod lexer;
mod math;
mod media;
mod parser;
mod report;
mod source;
mod table;

use std::path::Path;

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;

pub use report::{LatexFeatureCount, LatexIssue, LatexReport, LatexSpan, Severity};
pub use source::{FsSource, MemorySource, SourceProvider};

/// Unsupported construct handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnsupportedPolicy {
    /// Keep supported content and record every unsupported construct.
    #[default]
    ReportAndContinue,
    /// Stop at the first unsupported construct.
    Reject,
}

/// Allocation and work budgets. Limits count input bytes, lexical tokens,
/// emitted Unicode scalars, model nodes and include nesting.
#[derive(Clone, Debug)]
pub struct LatexLimits {
    /// Maximum input length in bytes, per file.
    pub input_bytes: usize,
    /// Maximum number of simultaneously open `\input`/`\include` files.
    pub depth: usize,
    /// Maximum nesting of groups, environments, lists and math. Bounds the
    /// recursive descent so deeply nested input cannot overflow the stack.
    pub nesting: usize,
    /// Maximum lexical tokens across every file.
    pub tokens: usize,
    /// Maximum emitted Unicode scalar values, or emitted count in a report.
    pub text_scalars: usize,
    /// Maximum model nodes, or allocated node count in a report.
    pub nodes: usize,
    /// Retained diagnostic details; the configured maximum limits this list.
    pub issues: usize,
    /// Maximum files pulled in through `\input`/`\include`.
    pub include_files: usize,
    /// Maximum table rows across the document.
    pub table_rows: usize,
    /// Maximum table cells across the document.
    pub table_cells: usize,
    /// Maximum columns one table may declare, before any layout is allocated.
    pub table_columns: usize,
    /// Maximum bytes in one embedded image.
    pub media_part_bytes: usize,
    /// Maximum embedded image bytes across the document.
    pub media_total_bytes: usize,
}

impl Default for LatexLimits {
    fn default() -> Self {
        Self {
            input_bytes: 64 * 1024 * 1024,
            depth: 32,
            nesting: 32,
            tokens: 10_000_000,
            text_scalars: 16_000_000,
            nodes: 1_000_000,
            issues: 10_000,
            include_files: 1024,
            table_rows: 100_000,
            table_cells: 1_000_000,
            table_columns: 4096,
            media_part_bytes: 32 * 1024 * 1024,
            media_total_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Import configuration; no application, network or font dependency.
#[derive(Clone, Debug)]
pub struct LatexOptions {
    /// Independent work and allocation budgets.
    pub limits: LatexLimits,
    /// Whether unsupported constructs are recorded or rejected.
    pub unsupported_policy: UnsupportedPolicy,
    /// Whether a plain page-number footer is added, like LaTeX's default style.
    pub page_numbers: bool,
}

impl Default for LatexOptions {
    fn default() -> Self {
        Self {
            limits: LatexLimits::default(),
            unsupported_policy: UnsupportedPolicy::default(),
            page_numbers: true,
        }
    }
}

/// Successful import. Success does not imply complete fidelity: check the report.
#[derive(Debug)]
pub struct LatexImported {
    /// Owned synthetic WML document; XML locations are unknown.
    pub document: Document,
    /// Owned media payloads keyed by package part identifier.
    pub media: Vec<(PartId, Vec<u8>)>,
    /// Import-only diagnostics; writer diagnostics remain separate.
    pub report: LatexReport,
}

/// Fatal error, including its source byte offset where applicable.
#[derive(Debug, thiserror::Error)]
pub enum LatexError {
    /// Input is not valid UTF-8.
    #[error("input {file} is not valid UTF-8 at byte {offset}")]
    InvalidEncoding {
        /// Input byte offset of the first invalid byte.
        offset: usize,
        /// Name of the offending file.
        file: String,
    },
    /// Configured resource budget exceeded.
    #[error("limit {limit} exceeded: {observed}")]
    LimitExceeded {
        /// Name of the exceeded budget.
        limit: &'static str,
        /// Attempted usage.
        observed: usize,
    },
    /// Caller requested rejection of unsupported constructs.
    #[error("unsupported {feature}")]
    UnsupportedRejected {
        /// Unsupported category.
        feature: String,
    },
    /// A named source file was not found by the provider.
    #[error("source file not found: {name}")]
    SourceNotFound {
        /// Requested file name.
        name: String,
    },
    /// An `\includegraphics` target is not a supported image container.
    #[error("unsupported image container: {name}")]
    UnsupportedImage {
        /// Requested image file name.
        name: String,
    },
    /// Reading a source file from the provider failed.
    #[error("cannot read {name}: {source}")]
    Io {
        /// Requested file name.
        name: String,
        /// Underlying reader error.
        source: std::io::Error,
    },
}

/// Imports a LaTeX document from bytes. `\input` and `\include` are recorded
/// as losses because no external source is available.
///
/// ```
/// use strict_ooxml_latex::{import, LatexOptions};
/// let imported = import(b"Hello, $x^2 + y^2 = z^2$.", &LatexOptions::default()).unwrap();
/// assert_eq!(imported.document.body.blocks.len(), 1);
/// ```
///
/// # Errors
///
/// Returns a typed error for invalid text, exceeded budgets or, when
/// [`UnsupportedPolicy::Reject`] is set, the first unsupported construct.
pub fn import(bytes: &[u8], options: &LatexOptions) -> Result<LatexImported, LatexError> {
    parser::import_bytes(bytes, options)
}

/// Imports a LaTeX document, resolving `\input`/`\include` through `provider`.
///
/// # Errors
///
/// As [`import`]; additionally, unreadable includes are losses unless
/// rejection is requested.
pub fn import_with_source(
    name: &str,
    bytes: &[u8],
    provider: &dyn SourceProvider,
    options: &LatexOptions,
) -> Result<LatexImported, LatexError> {
    parser::import_entry(name, bytes, provider, options)
}

/// Reads `path` and imports it, resolving includes relative to its directory.
///
/// # Errors
///
/// Returns [`LatexError::Io`] when the entry file cannot be read, and the same
/// errors as [`import_with_source`] otherwise.
pub fn import_file(
    path: impl AsRef<Path>,
    options: &LatexOptions,
) -> Result<LatexImported, LatexError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| LatexError::Io {
        name: path.display().to_string(),
        source,
    })?;
    let root = path
        .parent()
        .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf);
    let provider = FsSource::new(root);
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    import_with_source(&name, &bytes, &provider, options)
}
