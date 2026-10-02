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
    ///
    /// **That order is discovery order, and it is not the order to read them in.**
    /// It is what SC-1 requires — two renders of one document produce the same
    /// bytes, which this order makes true — and it says nothing about how much a
    /// loss matters: a document that dropped the whole body and a document that
    /// lost one picture produce reports whose severity order is the reverse of
    /// their importance order. Use [`Self::losses_by_severity`] to read it, and
    /// keep this one for reproducibility and for tests.
    #[must_use]
    pub fn losses(&self) -> &[PdfLoss] {
        &self.losses
    }

    /// The losses, worst first, ties in discovery order.
    ///
    /// Sorting is stable, so two losses of the same severity keep the order they
    /// were found in and the output is as reproducible as the input.
    #[must_use]
    pub fn losses_by_severity(&self) -> Vec<&PdfLoss> {
        let mut out: Vec<&PdfLoss> = self.losses.iter().collect();
        out.sort_by_key(|loss| std::cmp::Reverse(loss.severity));
        out
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

    /// Records a shape whose outline did not resolve.
    ///
    /// This exists because "it cannot happen" is not a report. The content stream
    /// writer has no arc operator, so an outline that still holds an arc there
    /// would be silently undrawable; the arm that would have to draw it calls
    /// this instead, and the render's losses then say so.
    pub fn record_unresolved_outline(&mut self, reason: &str) {
        self.push(
            "pdf.shape.unresolved_outline",
            format!("{reason}, so its outline is not in the PDF"),
            Severity::Placeholder,
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
        // Worst first: this is the text a person reads, and the whole point of a
        // severity is that it orders what to look at. The counts above carry the
        // same numbers, so nothing is lost by showing the lines out of order.
        for loss in self.losses_by_severity() {
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

    #[test]
    fn the_report_is_read_worst_first() {
        // A document that lost its text entirely and one that lost a picture:
        // discovery order puts the placeholder second, which is the wrong order
        // to read them in.
        let mut report = PdfReport::new();
        report.record_placeholder("");
        report.record_missing_face("Segoe UI");
        let ids: Vec<&str> = report
            .losses_by_severity()
            .iter()
            .map(|loss| loss.id)
            .collect();
        assert_eq!(ids, vec!["pdf.font.missing", "pdf.image.no_source"]);
        let text = report.to_string();
        assert!(
            text.find("pdf.font.missing") < text.find("pdf.image.no_source"),
            "the printed report must lead with the worst loss: {text}"
        );
        // The discovery order is still there for SC-1, which is what it is for.
        assert_eq!(report.losses()[0].id, "pdf.image.no_source");
    }

    #[test]
    fn a_stable_sort_keeps_discovery_order_within_a_severity() {
        let mut report = PdfReport::new();
        report.record_placeholder("first");
        report.record_placeholder("second");
        report.record_placeholder("third");
        let details: Vec<&str> = report
            .losses_by_severity()
            .iter()
            .map(|loss| loss.detail.as_str())
            .collect();
        assert_eq!(details.len(), 3);
        assert!(details[0].starts_with("picture 'first'"), "{details:?}");
        assert!(details[1].starts_with("picture 'second'"), "{details:?}");
        assert!(details[2].starts_with("picture 'third'"), "{details:?}");
    }

    #[test]
    fn a_clean_report_says_so_and_has_nothing_to_sort() {
        let report = PdfReport::new();
        assert!(report.losses_by_severity().is_empty());
        assert_eq!(report.to_string(), "nothing lost\n");
    }
}
