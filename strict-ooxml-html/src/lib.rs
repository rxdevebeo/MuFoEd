//! HTML fragments imported into the document model. No browser, script engine,
//! network client or filesystem access is used: external images require an
//! explicit caller-provided resolver. Success does not imply browser fidelity;
//! unsupported content is recorded in [`HtmlReport`].
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
mod convert;
mod images;
mod tree;

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;

/// Import budgets, checked independently of a resource resolver's own budgets.
#[derive(Clone, Debug)]
pub struct HtmlOptions {
    /// Maximum UTF-8 input bytes.
    pub max_input_bytes: usize,
    /// Maximum parsed nodes.
    pub max_nodes: usize,
    /// Maximum element nesting.
    pub max_depth: usize,
    /// Maximum number of images.
    pub max_images: usize,
    /// Maximum image bytes in total.
    pub max_media_bytes: usize,
    /// Maximum width times height of an image's intrinsic dimensions.
    pub max_image_pixels: u64,
    /// Base URL for relative image references; a `CF_HTML` caller supplies `SourceURL`.
    pub base_url: Option<String>,
}
impl Default for HtmlOptions {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_nodes: 100_000,
            max_depth: 128,
            max_images: 512,
            max_media_bytes: 32 * 1024 * 1024,
            max_image_pixels: 64 * 1024 * 1024,
            base_url: None,
        }
    }
}

/// Caller-controlled, synchronous external image loading. Implementations must
/// bound I/O and their reads by `max_bytes`; returned bytes are checked again.
pub trait HtmlResolver {
    /// Reads an absolute URL (or the original relative reference without a base).
    fn resolve(&self, source: &str, max_bytes: usize) -> Result<Vec<u8>, String>;
}
impl<F: Fn(&str, usize) -> Result<Vec<u8>, String>> HtmlResolver for F {
    fn resolve(&self, source: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        self(source, max_bytes)
    }
}

/// An import loss, such as unsupported CSS or an unavailable image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlIssue {
    /// Stable feature name.
    pub feature: String,
    /// Human-readable context.
    pub detail: String,
}
/// Named losses; detailed entries are capped while the total remains exact.
#[derive(Clone, Debug, Default)]
pub struct HtmlReport {
    /// Total named losses, including those beyond the detail cap.
    pub loss_count: usize,
    /// First 256 loss details.
    pub issues: Vec<HtmlIssue>,
}
impl HtmlReport {
    pub(crate) fn loss(&mut self, feature: &str, detail: impl Into<String>) {
        self.loss_count += 1;
        if self.issues.len() < 256 {
            self.issues.push(HtmlIssue {
                feature: feature.into(),
                detail: detail.into(),
            });
        }
    }
}
/// Owned model, owned image payloads and import-only losses.
#[derive(Debug)]
pub struct HtmlImported {
    /// Synthetic WML document, ready for the shared writer.
    pub document: Document,
    /// Referenced image bytes, keyed by package part.
    pub media: Vec<(PartId, Vec<u8>)>,
    /// Unsupported input and resource failures.
    pub report: HtmlReport,
}
/// Fatal budget or encoding failure. No partial result is returned.
#[derive(Debug, thiserror::Error)]
pub enum HtmlError {
    /// A configured import budget was exceeded.
    #[error("HTML import limit exceeded: {0}")]
    Limit(&'static str),
    /// HTML input is not valid UTF-8.
    #[error("HTML input is not UTF-8: {0}")]
    Encoding(#[from] std::str::Utf8Error),
}

/// Imports UTF-8 HTML. Data images are owned locally; external images are loaded
/// only through `resolver`. Unsupported constructs produce named losses.
///
/// # Errors
/// Returns [`HtmlError`] for invalid UTF-8 or an exceeded budget.
pub fn import(
    bytes: &[u8],
    options: &HtmlOptions,
    resolver: Option<&dyn HtmlResolver>,
) -> Result<HtmlImported, HtmlError> {
    if bytes.len() > options.max_input_bytes {
        return Err(HtmlError::Limit("input bytes"));
    }
    let html = std::str::from_utf8(bytes)?;
    let tree = tree::parse(html, options)?;
    convert::convert(tree, options, resolver)
}
