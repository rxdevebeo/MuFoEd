//! Save and support refresh use the existing writer/parser and pipeline outcome.
use crate::{EditError, Editor};
use strict_ooxml_core::{
    error::StrictError,
    opc::{OpenOptions, Package},
    pipeline::{PipelineOutcome, PipelineStage, PipelineSummary},
};
use strict_ooxml_wml::{model::SupportModel, parse_document, ParseOptions};
use strict_ooxml_write::{package::Source, WriteOptions};
/// Whether a caller accepts a reported lossy result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SavePolicy {
    /// Refuse degraded and failed outcomes.
    #[default]
    Lossless,
    /// Return degraded output with its complete report; failures still refuse.
    AllowDegraded,
}
/// Bytes and evidence returned by the common save pipeline.
#[derive(Debug)]
pub struct SavedDocument {
    /// Strict DOCX bytes; the caller chooses where to publish them.
    pub bytes: Vec<u8>,
    /// Input, normalization/conversion and writer issues are retained.
    pub pipeline: PipelineSummary,
    /// Fresh parser support for the written representation.
    pub support: SupportModel,
}
/// Save refusal preserves the document and editing history.
#[derive(Debug)]
pub enum SaveError {
    /// Revision or model error.
    Edit(EditError),
    /// Serialization, package or parse error.
    Pipeline(StrictError),
    /// Output exists internally but its losses violate the requested policy.
    Rejected(PipelineSummary),
}
impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SaveError {}
impl Editor<'_> {
    /// Serializes and reopens the current model without mutating it or the source.
    /// Media and foreign relationships are resolved through the original Source.
    pub fn save(
        &self,
        revision: u64,
        source: Option<&dyn Source>,
        options: &WriteOptions,
        policy: SavePolicy,
    ) -> Result<SavedDocument, SaveError> {
        if revision != self.revision() {
            return Err(SaveError::Edit(EditError::StaleRevision));
        }
        self.validate().map_err(SaveError::Edit)?;
        let output = strict_ooxml_write::write_package(self.document, source, options)
            .map_err(SaveError::Pipeline)?;
        let pipeline = self
            .input_pipeline
            .clone()
            .merge(PipelineSummary::from_normalization(
                PipelineStage::Write,
                &output.report,
            ));
        if pipeline.outcome == PipelineOutcome::Failed
            || (policy == SavePolicy::Lossless && pipeline.outcome != PipelineOutcome::Clean)
        {
            return Err(SaveError::Rejected(pipeline));
        }
        let open = OpenOptions {
            limits: options.limits,
            ..OpenOptions::default()
        };
        let package =
            Package::open_reader(output.bytes.as_slice(), &open).map_err(SaveError::Pipeline)?;
        let parse = ParseOptions {
            limits: options.limits,
        };
        let reopened = parse_document(&package, &parse).map_err(SaveError::Pipeline)?;
        Ok(SavedDocument {
            bytes: output.bytes,
            pipeline,
            support: reopened.support,
        })
    }
    /// Refreshes support from a checked written representation. A degraded write
    /// is refused: otherwise dropped mechanisms could disappear from the report.
    pub fn refresh_support(
        &mut self,
        revision: u64,
        source: Option<&dyn Source>,
        options: &WriteOptions,
    ) -> Result<(), SaveError> {
        let saved = self.save(revision, source, options, SavePolicy::Lossless)?;
        self.document.support = saved.support;
        self.support_stale = false;
        Ok(())
    }
}
