//! RTF adapter into the regular package-backed public document API.
use crate::{
    MediaBag, OpenOptions, RtfOptions, RtfReport, StrictDocument, StrictError, WriteOptions,
    WriteReport,
};
use std::{fmt, io::Read, path::Path};

/// The shared pipeline outcome, exposed under an import-oriented name.
pub use strict_ooxml_core::pipeline::PipelineOutcome as RtfStatus;
/// Imported document and the two independent fidelity reports.
pub struct RtfOpened {
    /// Initial serialized Strict package; later model edits do not change it.
    pub strict_bytes: Vec<u8>,
    /// Package-backed document, ready for the normal render/write APIs.
    pub document: StrictDocument,
    /// Diagnostics at original RTF byte offsets.
    pub import_report: RtfReport,
    /// Diagnostics produced while constructing the Strict package.
    pub write_report: WriteReport,
}
impl RtfOpened {
    /// Returns the outcome using aggregate counts, even if details were capped.
    #[must_use]
    pub fn status(&self) -> RtfStatus {
        self.pipeline_summary().outcome
    }
    /// Combines Convert and Write issues without merging their identities.
    #[must_use]
    pub fn pipeline_summary(&self) -> strict_ooxml_core::pipeline::PipelineSummary {
        use strict_ooxml_core::pipeline::{PipelineStage, PipelineSummary};
        self.import_report
            .pipeline_summary()
            .merge(PipelineSummary::from_normalization(
                PipelineStage::Write,
                &self.write_report,
            ))
    }
}
/// Failure before a complete public document is available.
#[derive(Debug)]
pub enum RtfOpenError {
    /// Invalid or unsupported RTF, or a resource limit violation.
    Import(strict_ooxml_rtf::RtfError),
    /// Package serialization or reopening failed.
    Package(StrictError),
    /// Reading an input path failed.
    Io(std::io::Error),
}
impl fmt::Display for RtfOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Import(e) => e.fmt(f),
            Self::Package(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for RtfOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(e) => Some(e),
            Self::Package(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}
impl StrictDocument {
    /// Imports RTF bytes through an owned Strict package. Import and writer
    /// losses remain available separately; success does not imply fidelity.
    pub fn import_rtf(
        bytes: &[u8],
        options: &RtfOptions,
    ) -> std::result::Result<RtfOpened, RtfOpenError> {
        let imported = strict_ooxml_rtf::import(bytes, options).map_err(RtfOpenError::Import)?;
        let mut media = MediaBag::new();
        for (part, bytes) in imported.media {
            media.insert(part, bytes);
        }
        let written =
            crate::write_package(&imported.document, Some(&media), &WriteOptions::default())
                .map_err(RtfOpenError::Package)?;
        written.report.verify_no_silent_loss().map_err(|e| {
            RtfOpenError::Package(StrictError::Unsupported(format!(
                "RTF writer invariant: {e}"
            )))
        })?;
        let document = Self::open_reader(&written.bytes[..], &OpenOptions::default())
            .map_err(RtfOpenError::Package)?
            .with_file_name("<rtf>");
        Ok(RtfOpened {
            document,
            strict_bytes: written.bytes,
            import_report: imported.report,
            write_report: written.report,
        })
    }
    /// Reads at most the configured input budget plus one byte, then imports.
    /// No application, network access or external converter is used.
    pub fn import_rtf_path(
        path: impl AsRef<Path>,
        options: &RtfOptions,
    ) -> std::result::Result<RtfOpened, RtfOpenError> {
        let path = path.as_ref();
        let file = std::fs::File::open(path).map_err(RtfOpenError::Io)?;
        let mut bytes = vec![];
        file.take(
            u64::try_from(options.limits.input_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .map_err(RtfOpenError::Io)?;
        let mut opened = Self::import_rtf(&bytes, options)?;
        opened.document = opened.document.with_file_name(path.display().to_string());
        Ok(opened)
    }
}
