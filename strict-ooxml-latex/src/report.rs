//! Import diagnostics: stable feature counters plus a bounded detail list.

use std::collections::BTreeMap;

/// Maximum distinct, bounded diagnostic feature names retained in aggregate counters.
pub(crate) const MAX_FEATURE_COUNT_KEYS: usize = 4096;
/// Feature names longer than this are counted as unattributed to bound report memory.
pub(crate) const MAX_FEATURE_NAME_BYTES: usize = 128;

/// Zero-based, half-open byte range in one LaTeX source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatexSpan {
    /// Inclusive starting offset.
    pub start: usize,
    /// Exclusive ending offset.
    pub end: usize,
}

impl LatexSpan {
    /// Creates a span from its bounds.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Severity is independent of whether parsing succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Non-content metadata or an accepted approximation.
    Info,
    /// Unsupported or replaced content, formatting or semantics.
    Loss,
}

/// One retained diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatexIssue {
    /// Location of the triggering token in the originating source file.
    pub span: LatexSpan,
    /// Effect on fidelity.
    pub severity: Severity,
    /// Stable diagnostic category with the original command or environment name.
    pub feature: String,
    /// Name of the source file the span belongs to.
    pub file: String,
}

/// Counts for one stable import diagnostic feature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatexFeatureCount {
    /// Total loss events for this feature, including events beyond the detail cap.
    pub loss_count: usize,
    /// Total informational events for this feature, including events beyond the detail cap.
    pub info_count: usize,
}

/// Import summary. Aggregate counts remain valid when details are truncated.
#[derive(Clone, Debug, Default)]
pub struct LatexReport {
    /// Retained diagnostic details; the configured maximum limits this list.
    pub issues: Vec<LatexIssue>,
    /// Total loss events, including details omitted by the cap.
    pub loss_count: usize,
    /// Total informational events, including details omitted by the cap.
    pub info_count: usize,
    /// Per-feature totals, including events beyond the retained detail cap.
    pub feature_counts: BTreeMap<String, LatexFeatureCount>,
    /// Loss events whose feature name was too long or exceeded the distinct-feature cap.
    pub unattributed_loss_count: usize,
    /// Informational events whose feature name was too long or exceeded the distinct-feature cap.
    pub unattributed_info_count: usize,
    /// Some feature totals could not be attributed due to their bounded aggregate cap.
    pub feature_counts_truncated: bool,
    /// Some diagnostic details were omitted by the issue cap.
    pub details_truncated: bool,
    /// Maximum lexical tokens, or consumed tokens in a report.
    pub tokens: usize,
    /// Maximum emitted Unicode scalar values, or emitted count in a report.
    pub text_scalars: usize,
    /// Maximum model nodes, or allocated node count in a report.
    pub nodes: usize,
    /// Number of additional source files pulled in through `\input`/`\include`.
    pub included_files: usize,
}

impl LatexReport {
    /// Adapts import diagnostics to the shared Convert stage. LaTeX locations
    /// retain byte ranges and never invent package parts or page numbers.
    #[must_use]
    pub fn pipeline_summary(&self) -> strict_ooxml_core::pipeline::PipelineSummary {
        use strict_ooxml_core::pipeline::{PipelineIssue, PipelineStage, PipelineSummary};
        let mut summary = PipelineSummary::new();
        let mut issues: Vec<_> = self
            .issues
            .iter()
            .map(|i| PipelineIssue {
                stage: PipelineStage::Convert,
                id: format!("latex.{}", i.feature),
                severity: match i.severity {
                    Severity::Info => "info",
                    Severity::Loss => "lossy",
                }
                .into(),
                part: None,
                page: None,
                location: Some(format!("{}:{}..{}", i.file, i.span.start, i.span.end)),
                count: 1,
                detail: i.feature.clone(),
            })
            .collect();
        let retained = self
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Loss)
            .count();
        let omitted = self.loss_count.saturating_sub(retained);
        if omitted > 0 {
            issues.push(PipelineIssue {
                stage: PipelineStage::Convert,
                id: "latex.omitted-loss-details".into(),
                severity: "lossy".into(),
                part: None,
                page: None,
                location: None,
                count: u32::try_from(omitted).unwrap_or(u32::MAX),
                detail: format!("{omitted} import losses omitted by diagnostic cap"),
            });
        }
        summary.extend_issues(issues);
        summary
    }
}

/// Mutable report accumulator shared by the parser and the math translator.
pub(crate) struct ReportBuilder {
    report: LatexReport,
    issue_limit: usize,
}

impl ReportBuilder {
    /// Creates a builder whose detail list holds at most `issue_limit` entries.
    #[must_use]
    pub(crate) fn new(issue_limit: usize) -> Self {
        Self {
            report: LatexReport::default(),
            issue_limit,
        }
    }

    /// Records one diagnostic, updating the aggregate counters unconditionally.
    pub(crate) fn record(
        &mut self,
        file: &str,
        span: LatexSpan,
        severity: Severity,
        feature: &str,
    ) {
        match severity {
            Severity::Loss => self.report.loss_count += 1,
            Severity::Info => self.report.info_count += 1,
        }
        self.count_feature(severity, feature);
        if self.report.issues.len() < self.issue_limit {
            self.report.issues.push(LatexIssue {
                span,
                severity,
                feature: feature.to_owned(),
                file: file.to_owned(),
            });
        } else {
            self.report.details_truncated = true;
        }
    }

    fn count_feature(&mut self, severity: Severity, feature: &str) {
        if feature.len() > MAX_FEATURE_NAME_BYTES {
            match severity {
                Severity::Loss => self.report.unattributed_loss_count += 1,
                Severity::Info => self.report.unattributed_info_count += 1,
            }
            return;
        }
        if !self.report.feature_counts.contains_key(feature)
            && self.report.feature_counts.len() >= MAX_FEATURE_COUNT_KEYS
        {
            self.report.feature_counts_truncated = true;
            match severity {
                Severity::Loss => self.report.unattributed_loss_count += 1,
                Severity::Info => self.report.unattributed_info_count += 1,
            }
            return;
        }
        let entry = self
            .report
            .feature_counts
            .entry(feature.to_owned())
            .or_default();
        match severity {
            Severity::Loss => entry.loss_count += 1,
            Severity::Info => entry.info_count += 1,
        }
    }

    /// Returns the finished report, stamping the resource counters.
    #[must_use]
    pub(crate) fn finish(
        mut self,
        tokens: usize,
        text_scalars: usize,
        nodes: usize,
        included_files: usize,
    ) -> LatexReport {
        self.report.tokens = tokens;
        self.report.text_scalars = text_scalars;
        self.report.nodes = nodes;
        self.report.included_files = included_files;
        self.report
    }
}
