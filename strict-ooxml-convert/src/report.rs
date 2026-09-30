//! What a conversion inferred, and what it gave up.
//!
//! A PDF says where the ink is. A document says what the ink *means*. The
//! difference is a set of judgements, and a judgement that is not written down
//! cannot be reviewed - so every inference the converter makes is recorded
//! here with the rule that produced it, and a page whose structure could not be
//! recovered says so instead of arriving as an empty paragraph.

use std::fmt;

/// How much a recorded judgement costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A structural feature was reconstructed from geometry and may not match
    /// the author's intent: a table guessed from its ruling lines, a heading
    /// guessed from its size.
    Inferred,
    /// Something present in the PDF is absent from the document.
    Lost,
    /// The page has a shape the converter does not model at all.
    Unsupported,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Inferred => "inferred",
            Self::Lost => "lost",
            Self::Unsupported => "unsupported",
        })
    }
}

/// One recorded judgement.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConversionLoss {
    /// A stable id, for a report line.
    pub id: String,
    /// What happened, in prose.
    pub detail: String,
    /// How much it costs.
    pub severity: Severity,
    /// How many times it happened.
    pub count: u32,
}

impl fmt::Display for ConversionLoss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.id, self.detail)?;
        if self.count > 1 {
            write!(f, " x{}", self.count)?;
        }
        Ok(())
    }
}

/// Everything the conversion inferred or gave up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConversionReport {
    mode: Option<&'static str>,
    /// Every judgement the conversion made, ordered by id then detail.
    pub losses: Vec<ConversionLoss>,
    pub(crate) paragraphs: usize,
    pub(crate) lines: usize,
    pub(crate) tables: usize,
    pub(crate) images: usize,
    pub(crate) unmapped_glyphs: usize,
    pub(crate) estimated_advances: usize,
}

impl ConversionReport {
    /// An empty report for a mode.
    #[must_use]
    pub fn new(mode: crate::Mode) -> Self {
        Self {
            mode: Some(mode.as_str()),
            ..Self::default()
        }
    }

    /// The losses, ordered by id then detail.
    #[must_use]
    pub fn losses(&self) -> &[ConversionLoss] {
        &self.losses
    }

    /// Paragraphs the converter built.
    #[must_use]
    pub fn paragraphs(&self) -> usize {
        self.paragraphs
    }

    /// Lines the reader found.
    #[must_use]
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// Tables the converter recognised.
    #[must_use]
    pub fn tables(&self) -> usize {
        self.tables
    }

    /// Images embedded.
    #[must_use]
    pub fn images(&self) -> usize {
        self.images
    }

    /// Glyphs whose character the reader could not map, and which are therefore
    /// absent from the document.
    #[must_use]
    pub fn unmapped_glyphs(&self) -> usize {
        self.unmapped_glyphs
    }

    /// Advances the font did not state, so the text's spacing is a guess.
    #[must_use]
    pub fn estimated_advances(&self) -> usize {
        self.estimated_advances
    }

    /// Records a judgement; the same fact twice is one entry with a count.
    pub fn record(&mut self, id: &str, severity: Severity, detail: impl Into<String>) {
        let detail = detail.into();
        if let Some(existing) = self
            .losses
            .iter_mut()
            .find(|loss| loss.id == id && loss.detail == detail)
        {
            existing.count += 1;
            return;
        }
        self.losses.push(ConversionLoss {
            id: id.to_owned(),
            detail,
            severity,
            count: 1,
        });
        self.losses.sort();
    }

    /// Folds the reader's own report in, so a conversion carries both.
    pub fn merge(&mut self, other: &strict_ooxml_pdf::ReadReport) {
        for loss in other.losses() {
            self.record(&loss.id, Severity::Lost, loss.detail.clone());
        }
        self.unmapped_glyphs += other.unmapped_glyphs();
        self.estimated_advances += other.estimated_widths();
    }

    /// Whether anything was lost or unsupported, as opposed to merely inferred.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        !self
            .losses
            .iter()
            .any(|loss| matches!(loss.severity, Severity::Lost | Severity::Unsupported))
    }
}

impl fmt::Display for ConversionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "mode={} paragraphs={} lines={} tables={} images={}",
            self.mode.unwrap_or("?"),
            self.paragraphs,
            self.lines,
            self.tables,
            self.images
        )?;
        if self.unmapped_glyphs > 0 {
            writeln!(
                f,
                "  glyphs with no character mapping: {}",
                self.unmapped_glyphs
            )?;
        }
        if self.estimated_advances > 0 {
            writeln!(
                f,
                "  advances estimated rather than stated: {}",
                self.estimated_advances
            )?;
        }
        if self.losses.is_empty() {
            return f.write_str("  nothing inferred or lost\n");
        }
        for loss in &self.losses {
            writeln!(f, "  [{}] {}", loss.severity, loss)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ConversionReport, Severity};
    use crate::Mode;

    #[test]
    fn the_same_fact_is_one_entry_with_a_count() {
        let mut report = ConversionReport::new(Mode::Semantic);
        for _ in 0..3 {
            report.record(
                "table.grid",
                Severity::Inferred,
                "a grid was read from rules",
            );
        }
        report.record(
            "image.unsupported",
            Severity::Lost,
            "a JBIG2 image was dropped",
        );
        assert_eq!(report.losses().len(), 2);
        // By id: the losses are ordered by (id, detail), and a test that indexes
        // them is asserting an order the report does not promise.
        let grid = report
            .losses()
            .iter()
            .find(|loss| loss.id == "table.grid")
            .expect("the counted entry");
        assert_eq!(grid.count, 3);
        // The losses are ordered by (id, detail), so the *first* one is
        // `image.unsupported` - a test that indexed by position would be
        // asserting an order the report does not promise.
        assert_eq!(report.losses()[0].id, "image.unsupported");
    }

    #[test]
    fn inference_is_not_loss() {
        let mut report = ConversionReport::new(Mode::Semantic);
        report.record("heading", Severity::Inferred, "size above the body size");
        assert!(report.is_lossless(), "{report}");
        report.record("image", Severity::Lost, "dropped");
        assert!(!report.is_lossless());
    }

    #[test]
    fn the_report_counts_what_it_built() {
        let mut report = ConversionReport::new(Mode::Visual);
        report.paragraphs = 12;
        report.lines = 40;
        report.tables = 1;
        report.images = 2;
        let text = report.to_string();
        assert!(
            text.contains("mode=visual paragraphs=12 lines=40 tables=1 images=2"),
            "{text}"
        );
    }
}
