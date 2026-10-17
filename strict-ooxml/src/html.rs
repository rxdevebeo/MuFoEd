//! HTML adapter over the shared writer and package-backed document API.
use crate::{
    HtmlOptions, HtmlReport, HtmlResolver, MediaBag, OpenOptions, StrictDocument, StrictError,
    WriteOptions, WriteReport,
};

/// An imported HTML document and separate import and serialization reports.
pub struct HtmlOpened {
    /// Package-backed document, with owned image resources.
    pub document: StrictDocument,
    /// Initial Strict package, before subsequent edits.
    pub strict_bytes: Vec<u8>,
    /// Unsupported input and image failures.
    pub import_report: HtmlReport,
    /// Serialization losses.
    pub write_report: WriteReport,
}
/// Failure before a complete package-backed HTML import is available.
#[derive(Debug)]
pub enum HtmlOpenError {
    /// Encoding or import budget failure.
    Import(crate::HtmlError),
    /// Writing or reopening the Strict package failed.
    Package(StrictError),
}
impl std::fmt::Display for HtmlOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(e) => e.fmt(f),
            Self::Package(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for HtmlOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(e) => Some(e),
            Self::Package(e) => Some(e),
        }
    }
}
impl StrictDocument {
    /// Imports HTML through an owned Strict package. External resources require
    /// the caller's resolver; inspect both reports before claiming fidelity.
    ///
    /// # Errors
    /// Returns an import, serialization or package-opening error.
    pub fn import_html(
        bytes: &[u8],
        options: &HtmlOptions,
        resolver: Option<&dyn HtmlResolver>,
    ) -> Result<HtmlOpened, HtmlOpenError> {
        let imported =
            strict_ooxml_html::import(bytes, options, resolver).map_err(HtmlOpenError::Import)?;
        let mut media = MediaBag::new();
        for (part, bytes) in imported.media {
            media.insert(part, bytes);
        }
        let written =
            crate::write_package(&imported.document, Some(&media), &WriteOptions::default())
                .map_err(HtmlOpenError::Package)?;
        written.report.verify_no_silent_loss().map_err(|e| {
            HtmlOpenError::Package(StrictError::Unsupported(format!(
                "HTML writer invariant: {e}"
            )))
        })?;
        let document = Self::open_reader(&written.bytes[..], &OpenOptions::default())
            .map_err(HtmlOpenError::Package)?
            .with_file_name("<html>");
        Ok(HtmlOpened {
            document,
            strict_bytes: written.bytes,
            import_report: imported.report,
            write_report: written.report,
        })
    }
}
