//! What recognition changed, and what it could not do.
//!
//! The rule this crate exists to keep is that no text reaches a document without
//! saying where it came from (O3). A model that fills a gap silently produces a
//! document nobody can audit, and an audit that cannot see a guess is not an
//! audit.

use std::fmt;

/// How much a recorded fact costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Text or a description came from a model, not from the file. The document
    /// says so; a reader can act on it.
    Recovered,
    /// Something needed a model and nothing could answer.
    Missing,
    /// The model answered and the answer was unusable.
    Rejected,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Recovered => "recovered",
            Self::Missing => "missing",
            Self::Rejected => "rejected",
        })
    }
}

/// One recorded fact about recognition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcrLoss {
    /// A stable id, for a report line.
    pub id: String,
    /// What happened.
    pub detail: String,
    /// How much it costs.
    pub severity: Severity,
    /// The model involved, when there was one.
    pub model: Option<String>,
    /// The model's version or digest, when there was one.
    pub version: Option<String>,
}

impl fmt::Display for OcrLoss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.severity, self.id)?;
        if let Some(model) = &self.model {
            write!(f, " ({model}")?;
            if let Some(version) = &self.version {
                write!(f, " {version}")?;
            }
            f.write_str(")")?;
        }
        write!(f, ": {}", self.detail)
    }
}

/// Everything recognition did and did not do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OcrReport {
    losses: Vec<OcrLoss>,
    recovered: u32,
    described: u32,
}

impl OcrReport {
    /// An empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded facts, in discovery order.
    #[must_use]
    pub fn losses(&self) -> &[OcrLoss] {
        &self.losses
    }

    /// How many text answers reached a document.
    #[must_use]
    pub fn recovered(&self) -> u32 {
        self.recovered
    }

    /// How many graphic regions were described.
    #[must_use]
    pub fn described(&self) -> u32 {
        self.described
    }

    /// Records text that came from a model.
    pub fn record_recovered(
        &mut self,
        id: &str,
        detail: impl Into<String>,
        model: &str,
        version: &str,
    ) {
        self.recovered += 1;
        self.losses.push(OcrLoss {
            id: id.to_owned(),
            detail: detail.into(),
            severity: Severity::Recovered,
            model: Some(model.to_owned()),
            version: Some(version.to_owned()),
        });
    }

    /// Records a description of a graphic region.
    pub fn record_described(
        &mut self,
        id: &str,
        detail: impl Into<String>,
        model: &str,
        version: &str,
    ) {
        self.described += 1;
        self.record_recovered(id, detail, model, version);
    }

    /// Records something that needed a model and got nothing.
    pub fn record_missing(&mut self, id: &str, detail: impl Into<String>) {
        self.losses.push(OcrLoss {
            id: id.to_owned(),
            detail: detail.into(),
            severity: Severity::Missing,
            model: None,
            version: None,
        });
    }

    /// Records an answer that arrived and could not be used.
    pub fn record_rejected(&mut self, id: &str, detail: impl Into<String>, model: &str) {
        self.losses.push(OcrLoss {
            id: id.to_owned(),
            detail: detail.into(),
            severity: Severity::Rejected,
            model: Some(model.to_owned()),
            version: None,
        });
    }

    /// Whether anything was recovered or refused.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.losses.is_empty()
    }
}

impl fmt::Display for OcrReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("recognition: nothing was needed or nothing was done\n");
        }
        for loss in &self.losses {
            writeln!(f, "  {loss}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{OcrReport, Severity};

    #[test]
    fn a_recovered_answer_names_its_model_and_version() {
        let mut report = OcrReport::new();
        report.record_recovered("ocr.page", "page 3 text", "qwen3-vl", "8b");
        assert_eq!(report.recovered(), 1);
        let line = report.losses()[0].to_string();
        assert!(line.contains("recovered"), "{line}");
        assert!(line.contains("qwen3-vl 8b"), "{line}");
        assert_eq!(report.losses()[0].severity, Severity::Recovered);
    }

    #[test]
    fn a_gap_and_a_refusal_are_different_facts() {
        let mut report = OcrReport::new();
        report.record_missing("ocr.page", "page 5 has no text layer and no model");
        report.record_rejected("ocr.figure", "the answer had no kind", "qwen3-vl");
        assert_eq!(report.losses().len(), 2);
        assert_eq!(report.losses()[0].severity, Severity::Missing);
        assert_eq!(report.losses()[1].severity, Severity::Rejected);
        assert!(report.losses()[1].to_string().contains("qwen3-vl"));
    }
}
