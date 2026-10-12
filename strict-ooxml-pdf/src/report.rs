//! What reading a PDF could not carry out.
//!
//! The same rule as the rest of the toolkit: nothing is dropped silently. A
//! glyph whose character the font did not claim, a width the font did not
//! state, an image filter this crate does not carry, an operator with no
//! meaning here — each is counted, and the counts are the report.
//!
//! The distinction the report keeps is between *lost* and *approximated*:
//! a lost glyph is a character that is not in the output at all, while an
//! approximated width is a number the reader guessed. A converter can act on
//! the first and must not act on the second.

use std::collections::BTreeMap;
use std::fmt;

/// A thing the reader did not carry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Loss {
    /// A stable id, for a report line.
    pub id: String,
    /// Detail, for a report line.
    pub detail: String,
    /// How many times it happened.
    pub count: u32,
}

impl fmt::Display for Loss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.id, self.detail)?;
        if self.count > 1 {
            write!(f, " x{}", self.count)?;
        }
        Ok(())
    }
}

/// The losses of one read, deduplicated by id and detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadReport {
    losses: BTreeMap<(String, String), Loss>,
    unmapped_glyphs: usize,
    estimated_widths: usize,
}

impl ReadReport {
    /// Returns an empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records something the reader skipped.
    pub fn record_ignored(&mut self, id: &str, detail: &str) {
        let key = (id.to_owned(), detail.to_owned());
        let entry = self.losses.entry(key).or_insert_with(|| Loss {
            id: id.to_owned(),
            detail: detail.to_owned(),
            count: 0,
        });
        entry.count = entry.count.saturating_add(1);
    }

    /// Records an image that could not be carried.
    pub fn record_image_missing(&mut self, detail: &str) {
        self.record_ignored("pdf.image.missing", detail);
    }

    /// Sets the glyphs that had no character mapping.
    pub fn set_unmapped(&mut self, count: usize) {
        self.unmapped_glyphs = count;
    }

    /// Sets the advances that were estimated rather than stated.
    pub fn set_estimated_widths(&mut self, count: usize) {
        self.estimated_widths = count;
    }

    /// Folds another report into this one.
    pub fn merge(&mut self, other: &Self) {
        for loss in other.losses.values() {
            let entry = self
                .losses
                .entry((loss.id.clone(), loss.detail.clone()))
                .or_insert_with(|| Loss {
                    id: loss.id.clone(),
                    detail: loss.detail.clone(),
                    count: 0,
                });
            entry.count = entry.count.saturating_add(loss.count);
        }
        self.unmapped_glyphs = self.unmapped_glyphs.saturating_add(other.unmapped_glyphs);
        self.estimated_widths = self.estimated_widths.saturating_add(other.estimated_widths);
    }

    /// The losses, ordered by id then detail.
    #[must_use]
    pub fn losses(&self) -> Vec<&Loss> {
        self.losses.values().collect()
    }

    /// Glyphs whose character came from a fallback rather than a mapping.
    #[must_use]
    pub fn unmapped_glyphs(&self) -> usize {
        self.unmapped_glyphs
    }

    /// Advances that were estimated rather than stated.
    #[must_use]
    pub fn estimated_widths(&self) -> usize {
        self.estimated_widths
    }

    /// Whether anything was lost.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.losses.is_empty() && self.unmapped_glyphs == 0 && self.estimated_widths == 0
    }
}

impl fmt::Display for ReadReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_clean() {
            return f.write_str("nothing lost\n");
        }
        for loss in self.losses.values() {
            writeln!(f, "  {loss}")?;
        }
        if self.unmapped_glyphs > 0 {
            writeln!(
                f,
                "  glyphs with no character mapping: {}",
                self.unmapped_glyphs
            )?;
        }
        if self.estimated_widths > 0 {
            writeln!(
                f,
                "  advances estimated rather than stated: {}",
                self.estimated_widths
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ReadReport;

    #[test]
    fn a_clean_report_says_so() {
        let report = ReadReport::new();
        assert!(report.is_clean());
        assert_eq!(report.to_string(), "nothing lost\n");
    }

    #[test]
    fn the_same_fact_is_counted_once() {
        let mut report = ReadReport::new();
        for _ in 0..4 {
            report.record_ignored("pdf.shading", "shading pattern is not carried");
        }
        report.record_ignored("pdf.dash", "dash pattern is not carried");
        assert_eq!(report.losses().len(), 2);

        // By detail, not by position: the losses are ordered by (id, detail),
        // and a caller that has to know the order has taken on a contract the
        // report never promised.
        let count_of = |needle: &str| -> u32 {
            report
                .losses()
                .iter()
                .find(|loss| loss.id == needle)
                .map_or(0, |loss| loss.count)
        };
        assert_eq!(count_of("pdf.shading"), 4);
        assert_eq!(count_of("pdf.dash"), 1);
    }

    #[test]
    fn merging_adds_counts_and_keeps_approximations_apart_from_losses() {
        let mut first = ReadReport::new();
        first.record_ignored("pdf.dash", "dash");
        first.set_unmapped(2);
        first.set_estimated_widths(7);
        let mut second = ReadReport::new();
        second.record_ignored("pdf.dash", "dash");
        second.set_unmapped(1);
        first.merge(&second);
        assert_eq!(first.losses().len(), 1);
        assert_eq!(first.losses()[0].count, 2);
        assert_eq!(first.unmapped_glyphs(), 3);
        assert_eq!(first.estimated_widths(), 7);
        assert!(!first.is_clean());
        assert!(first.to_string().contains("estimated rather than stated"));
    }
}
