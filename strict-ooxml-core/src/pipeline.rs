//! Combined result of the read, normalize, convert, write, and render stages.
//!
//! Each stage keeps its own report. This module only folds those reports into
//! one outcome so a caller can tell a written file that lost something from a
//! clean file and from a failure. Issues are not merged when two stages reuse
//! an id: a conversion loss and a write loss are different facts.

use std::fmt;

use crate::normalize::{LossRecord, NormalizationReport};

/// Which stage produced an issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipelineStage {
    /// Opening the input failed, or the input itself is the subject.
    Input,
    /// Transitional to Strict normalization.
    Normalize,
    /// PDF to `WordprocessingML` conversion.
    Convert,
    /// Writing a Strict package.
    Write,
    /// SVG or PDF rendering.
    Render,
}

impl fmt::Display for PipelineStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "input",
            Self::Normalize => "normalize",
            Self::Convert => "convert",
            Self::Write => "write",
            Self::Render => "render",
        })
    }
}

/// What the whole chain may claim about its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipelineOutcome {
    /// Nothing substantial was lost and no invariant failed.
    Clean,
    /// A result exists, and something substantial was lost or only recovered.
    Degraded,
    /// The chain failed or broke a no-silent-loss invariant.
    Failed,
}

impl fmt::Display for PipelineOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Clean => "clean",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        })
    }
}

/// One issue carried into the combined result.
///
/// Absent `part`, `page`, and `location` mean the source report had no such
/// fact. Callers must not invent them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineIssue {
    /// Stage that recorded the issue.
    pub stage: PipelineStage,
    /// Stable id from that stage.
    pub id: String,
    /// The stage's own severity word, unchanged.
    pub severity: String,
    /// Package part, when the stage named one.
    pub part: Option<String>,
    /// Page number, when the stage named one.
    pub page: Option<u32>,
    /// Location text, when the stage named one.
    pub location: Option<String>,
    /// How many times this issue was counted.
    pub count: u32,
    /// The stage's own explanation.
    pub detail: String,
}

/// Every issue from the stages that ran, and the outcome they imply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineSummary {
    /// Issues in stable order: stage, part, page, location, id, detail.
    pub issues: Vec<PipelineIssue>,
    /// Combined outcome.
    pub outcome: PipelineOutcome,
}

impl Default for PipelineSummary {
    fn default() -> Self {
        Self {
            issues: Vec::new(),
            outcome: PipelineOutcome::Clean,
        }
    }
}

impl PipelineSummary {
    /// An empty clean summary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds a normalization or write report into a summary for `stage`.
    ///
    /// `Lossy` degrades the outcome. `Error`, a fatal stop, and a broken
    /// no-silent-loss check fail it. `Info` and `Ignorable` stay clean.
    /// Matching removal counters is not treated as proof that nothing was lost.
    #[must_use]
    pub fn from_normalization(stage: PipelineStage, report: &NormalizationReport) -> Self {
        let mut summary = Self::new();
        if let Some(fatal) = &report.fatal {
            summary.issues.push(PipelineIssue {
                stage,
                id: String::from("fatal"),
                severity: String::from("error"),
                part: None,
                page: None,
                location: None,
                count: 1,
                detail: fatal.clone(),
            });
        }
        if let Err(reason) = report.verify_no_silent_loss() {
            summary.issues.push(PipelineIssue {
                stage,
                id: String::from("silent-loss"),
                severity: String::from("error"),
                part: None,
                page: None,
                location: None,
                count: 1,
                detail: reason,
            });
        }
        for loss in report.losses() {
            summary.issues.push(issue_from_loss(stage, &loss));
        }
        summary.finish();
        summary
    }

    /// Appends issues and recomputes the outcome.
    pub fn extend_issues(&mut self, issues: impl IntoIterator<Item = PipelineIssue>) {
        self.issues.extend(issues);
        self.finish();
    }

    /// Appends another summary. Issues that share an id both remain.
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.issues.extend(other.issues);
        self.finish();
        self
    }

    fn finish(&mut self) {
        self.issues.sort_by(|left, right| {
            left.stage
                .cmp(&right.stage)
                .then_with(|| left.part.cmp(&right.part))
                .then_with(|| left.page.cmp(&right.page))
                .then_with(|| left.location.cmp(&right.location))
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.detail.cmp(&right.detail))
        });
        self.outcome = outcome_of(&self.issues);
    }
}

fn issue_from_loss(stage: PipelineStage, loss: &LossRecord) -> PipelineIssue {
    let location = loss.locations.first();
    PipelineIssue {
        stage,
        id: loss.feature_id.clone(),
        severity: loss.severity.to_string(),
        part: location.map(|place| place.part.as_str().to_owned()),
        page: None,
        location: location.map(ToString::to_string),
        count: 1,
        detail: loss.reason.clone(),
    }
}

fn outcome_of(issues: &[PipelineIssue]) -> PipelineOutcome {
    let mut outcome = PipelineOutcome::Clean;
    for issue in issues {
        if is_failure(&issue.severity) {
            return PipelineOutcome::Failed;
        }
        if degrades(&issue.severity) {
            outcome = PipelineOutcome::Degraded;
        }
    }
    outcome
}

fn is_failure(severity: &str) -> bool {
    matches!(severity, "error" | "fatal")
}

/// Confirmed lossless notes (`info`, `ignorable`, `inferred`) stay clean.
/// Everything else, including `recovered`, is a substantial gap.
fn degrades(severity: &str) -> bool {
    !matches!(severity, "info" | "ignorable" | "inferred")
}

#[cfg(test)]
mod tests {
    use super::{PipelineIssue, PipelineOutcome, PipelineStage, PipelineSummary};
    use crate::normalize::{LossRecord, NormalizationReport, Severity};

    fn issue(stage: PipelineStage, id: &str, severity: &str) -> PipelineIssue {
        PipelineIssue {
            stage,
            id: id.to_owned(),
            severity: severity.to_owned(),
            part: None,
            page: None,
            location: None,
            count: 1,
            detail: id.to_owned(),
        }
    }

    #[test]
    fn lossy_normalization_is_degraded_even_when_counters_match() {
        let mut report = NormalizationReport::new();
        report.record_loss(LossRecord {
            transform_id: "T5.removal",
            feature_id: String::from("v:shape"),
            reason: String::from("vml drawing dropped"),
            severity: Severity::Lossy,
            locations: Vec::new(),
        });
        report.count_reported_removal(1);
        let summary = PipelineSummary::from_normalization(PipelineStage::Normalize, &report);
        assert_eq!(summary.outcome, PipelineOutcome::Degraded);
        assert!(summary.issues.iter().any(|item| item.id == "v:shape"));
    }

    #[test]
    fn info_and_inferred_stay_clean_and_recovered_does_not() {
        let mut summary = PipelineSummary::new();
        summary.extend_issues([
            issue(PipelineStage::Normalize, "namespace", "info"),
            issue(PipelineStage::Convert, "paragraph", "inferred"),
        ]);
        assert_eq!(summary.outcome, PipelineOutcome::Clean);
        summary.extend_issues([issue(PipelineStage::Convert, "ocr", "recovered")]);
        assert_eq!(summary.outcome, PipelineOutcome::Degraded);
    }

    #[test]
    fn silent_loss_and_error_fail() {
        let mut silent = NormalizationReport::new();
        silent.count_unreported_removal(2);
        let summary = PipelineSummary::from_normalization(PipelineStage::Write, &silent);
        assert_eq!(summary.outcome, PipelineOutcome::Failed);
        assert!(summary.issues.iter().any(|item| item.id == "silent-loss"));

        let mut fatal = NormalizationReport::new();
        fatal.record_loss(LossRecord {
            transform_id: "T0",
            feature_id: String::from("broken"),
            reason: String::from("cannot continue"),
            severity: Severity::Error,
            locations: Vec::new(),
        });
        let summary = PipelineSummary::from_normalization(PipelineStage::Normalize, &fatal);
        assert_eq!(summary.outcome, PipelineOutcome::Failed);
    }

    #[test]
    fn the_same_id_on_two_stages_is_kept_twice() {
        let mut convert = PipelineSummary::new();
        convert.extend_issues([issue(PipelineStage::Convert, "media", "inferred")]);
        let mut write = PipelineSummary::new();
        write.extend_issues([issue(PipelineStage::Write, "media", "lossy")]);
        let merged = convert.merge(write);
        assert_eq!(merged.issues.len(), 2);
        assert_eq!(merged.outcome, PipelineOutcome::Degraded);
        assert!(merged
            .issues
            .iter()
            .any(|item| { item.stage == PipelineStage::Convert && item.id == "media" }));
        assert!(merged
            .issues
            .iter()
            .any(|item| item.stage == PipelineStage::Write && item.id == "media"));
    }
}
