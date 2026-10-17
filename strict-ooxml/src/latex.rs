//! LaTeX adapter into the regular package-backed public document API.
use crate::{
    LatexOptions, LatexReport, MediaBag, OpenOptions, StrictDocument, StrictError, WriteOptions,
    WriteReport,
};
use std::{fmt, io::Read, path::Path};

/// The shared pipeline outcome, exposed under an import-oriented name.
pub use strict_ooxml_core::pipeline::PipelineOutcome as LatexStatus;
/// Imported document and the two independent fidelity reports.
pub struct LatexOpened {
    /// Initial serialized Strict package; later model edits do not change it.
    pub strict_bytes: Vec<u8>,
    /// Package-backed document, ready for the normal render/write APIs.
    pub document: StrictDocument,
    /// Diagnostics at original LaTeX byte offsets.
    pub import_report: LatexReport,
    /// Diagnostics produced while constructing the Strict package.
    pub write_report: WriteReport,
}
impl LatexOpened {
    /// Returns the outcome using aggregate counts, even if details were capped.
    #[must_use]
    pub fn status(&self) -> LatexStatus {
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
pub enum LatexOpenError {
    /// Invalid or unsupported LaTeX, or a resource limit violation.
    Import(strict_ooxml_latex::LatexError),
    /// Package serialization or reopening failed.
    Package(StrictError),
    /// Reading an input path failed.
    Io(std::io::Error),
}
impl fmt::Display for LatexOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Import(e) => e.fmt(f),
            Self::Package(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for LatexOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(e) => Some(e),
            Self::Package(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}
impl StrictDocument {
    /// Imports LaTeX bytes through an owned Strict package. Import and writer
    /// losses remain available separately; success does not imply fidelity.
    ///
    /// No external source is available, so `\input`/`\include` are recorded as
    /// import losses. Use [`Self::import_latex_path`] to resolve them.
    pub fn import_latex(
        bytes: &[u8],
        options: &LatexOptions,
    ) -> std::result::Result<LatexOpened, LatexOpenError> {
        let imported =
            strict_ooxml_latex::import(bytes, options).map_err(LatexOpenError::Import)?;
        Self::package_latex(imported, "<latex>")
    }

    /// Reads at most the configured input budget plus one byte, then imports,
    /// resolving `\input`/`\include` relative to the input file's directory.
    /// No application, network access or external converter is used.
    pub fn import_latex_path(
        path: impl AsRef<Path>,
        options: &LatexOptions,
    ) -> std::result::Result<LatexOpened, LatexOpenError> {
        let path = path.as_ref();
        let file = std::fs::File::open(path).map_err(LatexOpenError::Io)?;
        let mut bytes = vec![];
        file.take(
            u64::try_from(options.limits.input_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .map_err(LatexOpenError::Io)?;
        let root = path
            .parent()
            .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf);
        let provider = strict_ooxml_latex::FsSource::new(root);
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let imported = strict_ooxml_latex::import_with_source(&name, &bytes, &provider, options)
            .map_err(LatexOpenError::Import)?;
        let mut opened = Self::package_latex(imported, &name)?;
        opened.document = opened.document.with_file_name(path.display().to_string());
        Ok(opened)
    }

    fn package_latex(
        imported: strict_ooxml_latex::LatexImported,
        file_name: &str,
    ) -> std::result::Result<LatexOpened, LatexOpenError> {
        let mut media = MediaBag::new();
        for (part, bytes) in imported.media {
            media.insert(part, bytes);
        }
        let written =
            crate::write_package(&imported.document, Some(&media), &WriteOptions::default())
                .map_err(LatexOpenError::Package)?;
        written.report.verify_no_silent_loss().map_err(|e| {
            LatexOpenError::Package(StrictError::Unsupported(format!(
                "LaTeX writer invariant: {e}"
            )))
        })?;
        let document = Self::open_reader(&written.bytes[..], &OpenOptions::default())
            .map_err(LatexOpenError::Package)?
            .with_file_name(file_name);
        Ok(LatexOpened {
            document,
            strict_bytes: written.bytes,
            import_report: imported.report,
            write_report: written.report,
        })
    }
}
