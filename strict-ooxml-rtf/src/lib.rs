//! RTF bytes to an owned `WordprocessingML` model, with explicit import losses.
#![forbid(unsafe_code)]

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;
mod parser;
mod picture;

/// Unsupported construct handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnsupportedPolicy {
    /// Keep supported content and record every unsupported construct.
    #[default]
    ReportAndContinue,
    /// Stop at the first unsupported construct.
    Reject,
}
/// Invalid text handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvalidTextPolicy {
    /// Reject invalid byte sequences and unpaired UTF-16 surrogates.
    #[default]
    Error,
    /// Substitute U+FFFD and report the loss.
    ReplaceAndReport,
}
/// Interpretation of noncanonical positive UTF-16 control parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnicodeParameterPolicy {
    /// Require the signed 16-bit representation specified by RTF.
    #[default]
    StrictSigned,
    /// Accept positive values up to 65535 and record each repaired parameter.
    AllowUnsignedAndReport,
}
/// Handling of U+0000, which cannot be stored in XML 1.0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NulPolicy {
    /// Reject NUL text; the default never discards input characters.
    #[default]
    Reject,
    /// Discard only NUL, record a loss and count every discarded character.
    DropAndReport,
}
/// Application-specific interpretation of otherwise unsupported RTF syntax.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompatibilityProfile {
    /// Preserve format semantics and report unknown controls.
    #[default]
    Standard,
    /// Reproduce independently checked WPS text behavior: leading raw group
    /// line endings, specific inline Apple/Cocoa words and discretionary hyphens.
    WpsText,
}
/// Allocation and work budgets. Limits count input bytes, lexical tokens,
/// emitted Unicode scalars and model nodes (paragraph/run/content).
#[derive(Clone, Debug)]
pub struct RtfLimits {
    /// Maximum input length in bytes.
    pub input_bytes: usize,
    /// Maximum simultaneously open groups, including the root.
    pub depth: usize,
    /// Maximum lexical tokens, or consumed tokens in a report.
    pub tokens: usize,
    /// Maximum emitted Unicode scalar values, or emitted count in a report.
    pub text_scalars: usize,
    /// Maximum model nodes, or allocated node count in a report.
    pub nodes: usize,
    /// Retained diagnostic details; the configured maximum limits this list.
    pub issues: usize,
    /// Maximum table rows across the document.
    pub table_rows: usize,
    /// Maximum table cells and declarations across the document.
    pub table_cells: usize,
    /// Maximum font or color definitions.
    pub definitions: usize,
    /// Maximum bytes in one embedded image.
    pub media_part_bytes: usize,
    /// Maximum embedded image bytes across the document.
    pub media_total_bytes: usize,
}
impl Default for RtfLimits {
    fn default() -> Self {
        Self {
            input_bytes: 64 * 1024 * 1024,
            depth: 256,
            tokens: 10_000_000,
            text_scalars: 16_000_000,
            nodes: 1_000_000,
            issues: 10_000,
            table_rows: 100_000,
            table_cells: 1_000_000,
            definitions: 100_000,
            media_part_bytes: 32 * 1024 * 1024,
            media_total_bytes: 64 * 1024 * 1024,
        }
    }
}
/// Import configuration; no application, network or filesystem dependency.
#[derive(Clone, Debug, Default)]
pub struct RtfOptions {
    /// Independent work and allocation budgets.
    pub limits: RtfLimits,
    /// Whether unsupported constructs are recorded or rejected.
    pub unsupported_policy: UnsupportedPolicy,
    /// Whether invalid text is rejected or explicitly replaced.
    pub invalid_text_policy: InvalidTextPolicy,
    /// Whether legacy unsigned UTF-16 parameters are accepted.
    pub unicode_parameter_policy: UnicodeParameterPolicy,
    /// Explicit policy for XML-incompatible NUL characters.
    pub nul_policy: NulPolicy,
    /// Application-specific behavior; never enabled implicitly.
    pub compatibility_profile: CompatibilityProfile,
}
/// Zero-based, half-open RTF input byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtfSpan {
    /// Inclusive starting offset.
    pub start: usize,
    /// Exclusive ending offset.
    pub end: usize,
}
/// Severity is independent of whether parsing succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Non-content metadata or permitted trailing NUL.
    Info,
    /// Unsupported or replaced content, formatting or semantics.
    Loss,
}
/// One retained diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtfIssue {
    /// Location of the triggering token in the RTF input.
    pub span: RtfSpan,
    /// Effect on fidelity.
    pub severity: Severity,
    /// Stable diagnostic category with the original control/destination name.
    pub feature: String,
}
/// Import summary. Aggregate counts remain valid when details are truncated.
#[derive(Clone, Debug, Default)]
pub struct RtfReport {
    /// NUL characters explicitly discarded by the selected policy.
    pub discarded_nuls: usize,
    /// Retained diagnostic details; the configured maximum limits this list.
    pub issues: Vec<RtfIssue>,
    /// Total loss events, including details omitted by the cap.
    pub loss_count: usize,
    /// Total informational events, including omitted details.
    pub info_count: usize,
    /// Some diagnostic details were omitted by the issue cap.
    pub details_truncated: bool,
    /// Maximum lexical tokens, or consumed tokens in a report.
    pub tokens: usize,
    /// Maximum emitted Unicode scalar values, or emitted count in a report.
    pub text_scalars: usize,
    /// Maximum model nodes, or allocated node count in a report.
    pub nodes: usize,
}
/// Successful import. Success does not imply complete fidelity: check the report.
#[derive(Debug)]
pub struct RtfImported {
    /// Owned synthetic WML document; XML locations are unknown.
    pub document: Document,
    /// Owned media payloads keyed by package part identifier.
    pub media: Vec<(PartId, Vec<u8>)>,
    /// Import-only diagnostics; writer diagnostics remain separate.
    pub report: RtfReport,
}
/// Fatal error, including its RTF byte offset where applicable.
#[derive(Debug, thiserror::Error)]
pub enum RtfError {
    /// Missing or unsupported root header.
    #[error("expected an RTF 1 root header")]
    InvalidHeader,
    /// Invalid lexical structure.
    #[error("malformed RTF at byte {offset}: {reason}")]
    Malformed {
        /// Input byte offset.
        offset: usize,
        /// Structural failure.
        reason: &'static str,
    },
    /// Text cannot be decoded or represented faithfully.
    #[error("invalid encoding at byte {offset}: {reason}")]
    InvalidEncoding {
        /// Input byte offset.
        offset: usize,
        /// Text failure.
        reason: &'static str,
    },
    /// Configured resource budget exceeded.
    #[error("limit {limit} exceeded at byte {offset}: {observed}")]
    LimitExceeded {
        /// Input byte offset.
        offset: usize,
        /// Name of the exceeded budget.
        limit: &'static str,
        /// Attempted usage.
        observed: usize,
    },
    /// Caller requested rejection of unsupported constructs.
    #[error("unsupported {feature} at bytes {span:?}")]
    UnsupportedRejected {
        /// Triggering input token.
        span: RtfSpan,
        /// Unsupported category.
        feature: String,
    },
}
/// Import an RTF document. Errors never return a success-shaped partial document.
///
/// ```
/// use strict_ooxml_rtf::{import, RtfOptions};
/// let imported = import(br"{\rtf1 Hello {\b world}!}", &RtfOptions::default()).unwrap();
/// assert_eq!(imported.report.loss_count, 0);
/// assert_eq!(imported.document.body.blocks.len(), 1);
/// ```
///
/// # Errors
///
/// Returns a typed error for invalid syntax, encoding or exceeded budgets.
/// Unsupported constructs produce errors when rejection is requested; unsafe
/// structural fallbacks, including nested tables, are always rejected.
pub fn import(bytes: &[u8], options: &RtfOptions) -> Result<RtfImported, RtfError> {
    parser::parse(bytes, options)
}
