//! What normalization changed and what it cost (`TZ-STRICT-OOXML-RUST.md`
//! §10.9, §10.12).
//!
//! The report exists for one reason: a document that is opened has to be
//! auditable. A normalizer that silently drops a VML picture, or rewrites
//! `w:jc` to a value the renderer does not understand, produces a page that
//! looks plausible and is wrong — and the operator has no way to tell. So
//! every change is counted, every removal is recorded with a location, and
//! [`NormalizationReport::verify_no_silent_loss`] exists to be asserted
//! against (criterion SC-4).

use std::collections::BTreeMap;
use std::fmt;

use crate::error::SourceLocation;
use crate::ns::Conformance;

/// How much a recorded change matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Rewritten with no change in meaning — a `left` alignment becoming a
    /// `start` alignment is the same instruction in Strict's vocabulary.
    Info,
    /// Removed, and the rendered page is unaffected: editor state, mail-merge
    /// settings, a style-pane filter. Removing these is what lets most real
    /// documents open at all, so they are reported, not hidden.
    Ignorable,
    /// Removed, and the rendered page changes: an OLE object, a VML drawing
    /// with no raster substitute, a compatibility flag that moves text.
    Lossy,
    /// Normalization cannot continue for this document.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Info => "info",
            Self::Ignorable => "ignorable",
            Self::Lossy => "lossy",
            Self::Error => "error",
        };
        f.write_str(text)
    }
}

/// One transformation the pipeline applied, counted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransformRecord {
    /// Stable stage id, for example `T1.namespace`.
    pub id: &'static str,
    /// How many times it fired.
    pub count: u32,
    /// The source form, when the transformation has one (a URI, a value).
    pub from: Option<String>,
    /// The target form.
    pub to: Option<String>,
}

impl TransformRecord {
    /// Creates a counter-only record.
    #[must_use]
    pub fn new(id: &'static str, count: u32) -> Self {
        Self {
            id,
            count,
            from: None,
            to: None,
        }
    }

    /// Returns the record with `from`/`to` set.
    #[must_use]
    pub fn with_mapping(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self.to = Some(to.into());
        self
    }
}

/// One removal or rewrite worth reporting, with where it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LossRecord {
    /// Stable stage id, for example `T5.removal`.
    pub transform_id: &'static str,
    /// The construct, for example `w:compat` or `v:shape`.
    pub feature_id: String,
    /// Why it was removed or rewritten.
    pub reason: String,
    /// How much it matters.
    pub severity: Severity,
    /// Where it was found. Cascading removals are one record with many.
    pub locations: Vec<SourceLocation>,
}

impl fmt::Display for LossRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} ({})",
            self.severity, self.feature_id, self.reason
        )?;
        if let Some(first) = self.locations.first() {
            write!(f, " at {first}")?;
            if self.locations.len() > 1 {
                write!(f, " (+{} more)", self.locations.len() - 1)?;
            }
        }
        Ok(())
    }
}

/// The outcome of normalizing a package.
///
/// Collected per part and merged in [`PartId`](crate::part::PartId) order
/// (ADR-0017 / AUD-30): the order parts happen to be read in is not part of
/// the contract, and re-reading the same part replaces its contribution rather
/// than adding to it. Two runs must produce the same report
/// (`TZ` §10.10.1 and §10.10.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NormalizationReport {
    /// What the package was detected as before anything ran.
    pub conformance_detected: Option<Conformance>,
    /// Applied transformations, keyed by stage id.
    applied: BTreeMap<&'static str, TransformRecord>,
    /// Removals and rewrites, in discovery order.
    losses: Vec<LossRecord>,
    /// Transformations whose count has not been fixed up yet, by stage id.
    pending: BTreeMap<&'static str, u32>,
    /// Number of nodes removed by the pipeline, whether or not reported.
    removed_nodes: u32,
    /// Number of removed nodes that produced a [`LossRecord`].
    reported_nodes: u32,
    /// Set when a stage hit an unrecoverable condition.
    pub fatal: Option<String>,
}

impl NormalizationReport {
    /// Returns an empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that a transformation fired `count` times.
    ///
    /// Repeated calls for the same stage accumulate, and the first mapping
    /// seen is kept so the report can show what the stage maps.
    pub fn record(&mut self, id: &'static str, count: u32) {
        if count == 0 {
            return;
        }
        *self.pending.entry(id).or_default() += count;
        self.applied
            .entry(id)
            .and_modify(|record| record.count += count)
            .or_insert_with(|| TransformRecord::new(id, count));
    }

    /// Records that a transformation fired once, with the form it mapped.
    pub fn record_mapping(&mut self, id: &'static str, from: &str, to: &str) {
        self.record(id, 1);
        if let Some(record) = self.applied.get_mut(id) {
            if record.from.is_none() {
                record.from = Some(from.to_owned());
                record.to = Some(to.to_owned());
            }
        }
    }

    /// Records a removal or rewrite.
    pub fn record_loss(&mut self, loss: LossRecord) {
        if loss.severity == Severity::Error {
            self.fatal = Some(format!("{}: {}", loss.feature_id, loss.reason));
        }
        self.losses.push(loss);
    }

    /// Counts a removed node that is expected to appear in the report.
    pub fn count_reported_removal(&mut self, nodes: u32) {
        self.removed_nodes += nodes;
        self.reported_nodes += nodes;
    }

    /// Counts a removed node that produced no report entry.
    pub fn count_unreported_removal(&mut self, nodes: u32) {
        self.removed_nodes += nodes;
    }

    /// The applied transformations, ordered by stage id.
    #[must_use]
    pub fn applied(&self) -> Vec<TransformRecord> {
        self.applied.values().cloned().collect()
    }

    /// The recorded losses, ordered by part then position.
    ///
    /// [`SourceLocation`] deliberately has no `Ord` — two locations in the
    /// same part are not meaningfully ordered by a derived key — so the
    /// comparison is written out.
    #[must_use]
    pub fn losses(&self) -> Vec<LossRecord> {
        let order = |loss: &LossRecord| {
            loss.locations.first().map_or_else(
                || (String::new(), 0, 0, 0),
                |location| {
                    (
                        location.part.as_str().to_owned(),
                        location.byte_offset,
                        location.line,
                        location.column,
                    )
                },
            )
        };
        let mut losses = self.losses.clone();
        losses.sort_by(|left, right| {
            order(left)
                .cmp(&order(right))
                .then_with(|| left.feature_id.cmp(&right.feature_id))
        });
        losses
    }

    /// Total number of `Lossy` removals — the number that actually matters,
    /// since an `Ignorable` removal costs nothing that reaches the page.
    #[must_use]
    pub fn lossy_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|loss| loss.severity == Severity::Lossy)
            .count()
    }

    /// Counts of each severity, for a one-line summary.
    #[must_use]
    pub fn severity_counts(&self) -> BTreeMap<Severity, usize> {
        let mut counts = BTreeMap::new();
        for loss in &self.losses {
            *counts.entry(loss.severity).or_insert(0) += 1;
        }
        counts
    }

    /// Whether every removed node reached the report (criterion SC-4).
    ///
    /// This is the check that makes "no silent loss" a property rather than
    /// an intention: the pipeline counts what it drops, and this asserts the
    /// counts agree.
    pub fn verify_no_silent_loss(&self) -> Result<(), String> {
        if self.removed_nodes == self.reported_nodes {
            return Ok(());
        }
        Err(format!(
            "{} removed node(s) reached no LossRecord: normalization must not drop \
             content silently (SC-4)",
            self.removed_nodes - self.reported_nodes
        ))
    }

    /// Whether normalization changed nothing at all.
    ///
    /// The property criterion SC-1 asserts for already-Strict packages.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.losses.is_empty()
            && self.applied.values().all(|record| record.count == 0)
            && self.removed_nodes == 0
    }

    /// Merges another per-part report into this one (ADR-0017 / AUD-30).
    ///
    /// Counts and node tallies use saturating addition. The first `from`/`to`
    /// mapping for a stage id wins. The first `fatal` wins.
    /// [`Self::conformance_detected`] is left untouched — package-level
    /// detection is filled by the caller (AUD-31).
    pub fn merge_part(&mut self, other: &Self) {
        for record in other.applied.values() {
            if record.count == 0 {
                continue;
            }
            let pending = self.pending.entry(record.id).or_default();
            *pending = pending.saturating_add(record.count);
            match self.applied.get_mut(record.id) {
                Some(existing) => {
                    existing.count = existing.count.saturating_add(record.count);
                }
                None => {
                    self.applied.insert(record.id, record.clone());
                }
            }
        }
        self.losses.extend(other.losses.iter().cloned());
        self.removed_nodes = self.removed_nodes.saturating_add(other.removed_nodes);
        self.reported_nodes = self.reported_nodes.saturating_add(other.reported_nodes);
        if self.fatal.is_none() {
            self.fatal.clone_from(&other.fatal);
        }
    }
}

impl fmt::Display for NormalizationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let conformance = match self.conformance_detected {
            Some(crate::ns::Conformance::Strict) => "strict",
            Some(crate::ns::Conformance::Transitional) => "transitional",
            Some(crate::ns::Conformance::Mixed) => "mixed",
            Some(crate::ns::Conformance::Unknown) | None => "unknown",
        };
        writeln!(f, "normalization: conformance={conformance}")?;
        if self.applied.is_empty() {
            writeln!(f, "  no transformations applied")?;
        }
        for record in self.applied() {
            match (&record.from, &record.to) {
                (Some(from), Some(to)) => {
                    writeln!(f, "  {} x{}: {from} -> {to}", record.id, record.count)?;
                }
                _ => writeln!(f, "  {} x{}", record.id, record.count)?,
            }
        }
        for (severity, count) in self.severity_counts() {
            writeln!(f, "  {severity}: {count}")?;
        }
        for loss in self.losses() {
            writeln!(f, "  {loss}")?;
        }
        if let Some(fatal) = &self.fatal {
            writeln!(f, "  FAILED: {fatal}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{LossRecord, NormalizationReport, Severity, TransformRecord};
    use crate::error::SourceLocation;
    use crate::part::PartId;

    fn location(part: &str, line: u32) -> SourceLocation {
        SourceLocation::new(PartId::new(part), line, 1, 0)
    }

    #[test]
    fn an_empty_report_is_a_noop() {
        let report = NormalizationReport::new();
        assert!(report.is_noop());
        assert_eq!(report.lossy_count(), 0);
        assert!(report.verify_no_silent_loss().is_ok());
    }

    #[test]
    fn repeated_records_accumulate_under_one_stage() {
        let mut report = NormalizationReport::new();
        report.record("T1.namespace", 3);
        report.record("T1.namespace", 2);
        let applied = report.applied();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].count, 5);
        assert!(!report.is_noop());
    }

    #[test]
    fn a_zero_count_is_not_recorded() {
        let mut report = NormalizationReport::new();
        report.record("T1.namespace", 0);
        assert!(
            report.is_noop(),
            "a stage that did not fire is not a change"
        );
    }

    #[test]
    fn the_first_mapping_is_kept_and_later_ones_do_not_overwrite_it() {
        let mut report = NormalizationReport::new();
        report.record_mapping(
            "T1.namespace",
            "schemas.openxmlformats.org/a",
            "purl.oclc.org/a",
        );
        report.record_mapping(
            "T1.namespace",
            "schemas.openxmlformats.org/b",
            "purl.oclc.org/b",
        );
        let applied = report.applied();
        assert_eq!(applied[0].count, 2);
        assert_eq!(
            applied[0].from.as_deref(),
            Some("schemas.openxmlformats.org/a")
        );
    }

    #[test]
    fn a_silent_removal_is_reported_as_such() {
        let mut report = NormalizationReport::new();
        report.count_unreported_removal(2);
        let error = report
            .verify_no_silent_loss()
            .expect_err("an unreported removal must fail SC-4");
        assert!(error.contains('2'), "{error}");
    }

    #[test]
    fn a_reported_removal_passes() {
        let mut report = NormalizationReport::new();
        report.count_reported_removal(3);
        assert!(report.verify_no_silent_loss().is_ok());
    }

    #[test]
    fn only_lossy_removals_count_as_a_cost() {
        let mut report = NormalizationReport::new();
        for severity in [Severity::Info, Severity::Ignorable, Severity::Lossy] {
            report.record_loss(LossRecord {
                transform_id: "T5.removal",
                feature_id: format!("w:{severity}"),
                reason: "test".to_owned(),
                severity,
                locations: vec![location("/word/document.xml", 1)],
            });
        }
        assert_eq!(report.lossy_count(), 1);
        let counts = report.severity_counts();
        assert_eq!(counts[&Severity::Ignorable], 1);
        assert_eq!(counts[&Severity::Lossy], 1);
    }

    #[test]
    fn an_error_severity_loss_marks_the_report_fatal() {
        let mut report = NormalizationReport::new();
        report.record_loss(LossRecord {
            transform_id: "T8.invariant",
            feature_id: "w:jc".to_owned(),
            reason: "value outside the Strict domain".to_owned(),
            severity: Severity::Error,
            locations: vec![location("/word/document.xml", 7)],
        });
        assert!(report.fatal.is_some());
    }

    #[test]
    fn losses_come_back_ordered_by_part() {
        let mut report = NormalizationReport::new();
        for (part, line) in [("/word/styles.xml", 3), ("/word/document.xml", 9)] {
            report.record_loss(LossRecord {
                transform_id: "T5.removal",
                feature_id: "w:compat".to_owned(),
                reason: "test".to_owned(),
                severity: Severity::Ignorable,
                locations: vec![location(part, line)],
            });
        }
        let losses = report.losses();
        assert_eq!(losses[0].locations[0].part.as_str(), "/word/document.xml");
        assert_eq!(losses[1].locations[0].part.as_str(), "/word/styles.xml");
    }

    #[test]
    fn transform_records_display_their_mapping() {
        let record = TransformRecord::new("T1.namespace", 4)
            .with_mapping("schemas.openxmlformats.org/a", "purl.oclc.org/a");
        assert_eq!(record.from.as_deref(), Some("schemas.openxmlformats.org/a"));
        assert_eq!(record.to.as_deref(), Some("purl.oclc.org/a"));
    }

    #[test]
    fn merge_part_sums_counts_and_keeps_the_first_mapping() {
        let mut left = NormalizationReport::new();
        left.record_mapping("T2.reltype", "from-a", "to-a");
        left.count_reported_removal(1);
        let mut right = NormalizationReport::new();
        right.record_mapping("T2.reltype", "from-b", "to-b");
        right.record("T1.namespace", 2);
        right.count_reported_removal(3);
        left.merge_part(&right);
        let applied = left.applied();
        assert_eq!(applied.len(), 2);
        let rel = applied.iter().find(|r| r.id == "T2.reltype").unwrap();
        assert_eq!(rel.count, 2);
        assert_eq!(rel.from.as_deref(), Some("from-a"));
        assert!(left.verify_no_silent_loss().is_ok());
    }

    #[test]
    fn equal_reports_compare_equal() {
        let mut a = NormalizationReport::new();
        a.record("T1.namespace", 1);
        let mut b = NormalizationReport::new();
        b.record("T1.namespace", 1);
        assert_eq!(a, b);
    }
}
