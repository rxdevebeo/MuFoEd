//! What a PDF render could not represent.
//!
//! A PDF is a different language from a WML document, and some things do not
//! cross: a shape that Word draws with a raster effect, an image format the
//! filter chain does not cover, a font family the bundle does not carry. None of
//! that is a reason to fail the render, and all of it is a reason to say so —
//! the same «no silent loss» rule the rest of the toolkit follows (SC-10).

use std::fmt;

use crate::font::{FaceKey, FontError};
use crate::image::{ImageReject, RejectKind};

/// One thing the render could not reproduce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfLoss {
    /// A stable machine-readable id, e.g. `pdf.image.unsupported`.
    pub id: &'static str,
    /// What was lost, in prose.
    pub detail: String,
    /// How much it matters.
    pub severity: Severity,
}

/// How much a PDF loss matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A placeholder stands in for it; the page is still readable.
    Placeholder,
    /// Text that is missing from the PDF and cannot be selected or searched.
    Text,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Placeholder => "placeholder",
            Self::Text => "text",
        })
    }
}

/// The losses of one render, in discovery order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PdfReport {
    losses: Vec<PdfLoss>,
}

impl PdfReport {
    /// Returns an empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The losses, in the order they were found.
    #[must_use]
    pub fn losses(&self) -> &[PdfLoss] {
        &self.losses
    }

    /// Whether nothing was lost.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.losses.is_empty()
    }

    /// Counts of each severity, for a one-line summary.
    #[must_use]
    pub fn severity_counts(&self) -> Vec<(Severity, usize)> {
        let mut out = vec![(Severity::Placeholder, 0), (Severity::Text, 0)];
        for loss in &self.losses {
            for (severity, count) in &mut out {
                if *severity == loss.severity {
                    *count += 1;
                }
            }
        }
        out.into_iter().filter(|(_, count)| *count > 0).collect()
    }

    /// Records an image that could not be embedded.
    pub fn record_image_reject(&mut self, reject: &ImageReject) {
        self.push(
            match reject.kind {
                RejectKind::UnsupportedFormat => "pdf.image.unsupported",
                RejectKind::Damaged => "pdf.image.damaged",
                RejectKind::TooLarge => "pdf.image.too_large",
            },
            format!("{} ({})", reject.part, reject.reason()),
            Severity::Placeholder,
        );
    }

    /// Records a picture drawn without its bytes.
    pub fn record_placeholder(&mut self, alt: &str) {
        let what = if alt.is_empty() {
            "a picture".to_owned()
        } else {
            format!("picture '{alt}'")
        };
        self.push(
            "pdf.image.no_source",
            format!("{what} has no media source, so a placeholder was drawn"),
            Severity::Placeholder,
        );
    }

    /// Records a character the face does not cover.
    pub fn record_missing_glyph(&mut self, family: &str, ch: char) {
        self.push(
            "pdf.font.no_glyph",
            format!("'{ch}' is not in the bundled face for '{family}' and was not drawn"),
            Severity::Text,
        );
    }

    /// Records a face the bundle does not carry.
    pub fn record_missing_face(&mut self, family: &str) {
        self.push(
            "pdf.font.missing",
            format!("no bundled face for '{family}'; its text is missing from the PDF"),
            Severity::Text,
        );
    }

    /// Records a face that could not be subset or embedded.
    pub fn record_font_error(&mut self, key: &FaceKey, error: &FontError) {
        self.push(
            "pdf.font.embed_failed",
            format!(
                "{} {}: {error}",
                key.family,
                if key.bold { "bold" } else { "regular" }
            ),
            Severity::Text,
        );
    }

    fn push(&mut self, id: &'static str, detail: String, severity: Severity) {
        // The same family is requested by every run on the page; one entry per
        // distinct fact keeps the report readable and reproducible.
        if self
            .losses
            .iter()
            .any(|loss| loss.id == id && loss.detail == detail)
        {
            return;
        }
        self.losses.push(PdfLoss {
            id,
            detail,
            severity,
        });
    }
}

impl fmt::Display for PdfReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.losses.is_empty() {
            return f.write_str("nothing lost\n");
        }
        for (severity, count) in self.severity_counts() {
            writeln!(f, "  {severity}: {count}")?;
        }
        for loss in &self.losses {
            writeln!(f, "  [{}] {}: {}", loss.id, loss.severity, loss.detail)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::image::{ImageReject, RejectKind};

    use super::{PdfReport, Severity};

    #[test]
    fn a_clean_report_says_so() {
        let report = PdfReport::new();
        assert!(report.is_clean());
        assert_eq!(report.to_string(), "nothing lost\n");
        assert!(report.severity_counts().is_empty());
    }

    #[test]
    fn the_same_fact_is_recorded_once() {
        let mut report = PdfReport::new();
        for _ in 0..5 {
            report.record_missing_face("Segoe UI");
        }
        assert_eq!(report.losses().len(), 1);
        assert_eq!(report.losses()[0].severity, Severity::Text);
    }

    #[test]
    fn severities_are_summarised_separately() {
        let mut report = PdfReport::new();
        report.record_image_reject(&ImageReject {
            kind: RejectKind::UnsupportedFormat,
            part: "/word/media/a.gif".to_owned(),
        });
        report.record_placeholder("");
        report.record_missing_face("Segoe UI");
        assert_eq!(
            report.severity_counts(),
            vec![(Severity::Placeholder, 2), (Severity::Text, 1)]
        );
        assert!(report.to_string().contains("pdf.font.missing"));
    }
}
