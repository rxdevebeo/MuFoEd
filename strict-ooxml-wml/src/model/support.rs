//! Support model: aggregated usage of mechanisms, the Stage-3 input.
//!
//! Every mechanism the parser encounters is recorded as a [`FeatureUse`]
//! (STAGE-2 §9). Unknown or partially supported constructs are recorded with
//! their [`SourceLocation`] so nothing is lost silently.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

/// How well a mechanism is supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SupportStatus {
    /// Fully parsed into the model.
    Supported,
    /// Recognised, but only partially represented.
    Partial,
    /// Not represented; recorded for reporting.
    Unsupported,
    /// Deliberately ignored (for example comments in Stage 2).
    Ignored,
}

impl SupportStatus {
    /// Returns a stable, machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
            Self::Ignored => "ignored",
        }
    }
}

/// One aggregated record of a mechanism usage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureUse {
    /// Feature identifier, for example `w:tbl` or `wp:anchor`.
    pub feature_id: Arc<str>,
    /// Aggregated status (the most severe status observed).
    pub status: SupportStatus,
    /// Optional human-readable message (first occurrence).
    pub message: Option<String>,
    /// Location of the first occurrence.
    pub location: Option<SourceLocation>,
    /// Number of occurrences recorded.
    pub count: u32,
}

impl FeatureUse {
    fn new(feature_id: Arc<str>) -> Self {
        Self {
            feature_id,
            status: SupportStatus::Supported,
            message: None,
            location: None,
            count: 0,
        }
    }
}

/// Aggregated support information for a document.
#[derive(Clone, Debug, Default)]
pub struct SupportModel {
    by_feature: BTreeMap<Arc<str>, FeatureUse>,
}

impl SupportModel {
    /// Creates an empty support model.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one use of a mechanism.
    ///
    /// If the feature is already present, its status is raised to the more
    /// severe of the two, its count is incremented, and the message/location of
    /// the first occurrence are kept.
    pub fn record(
        &mut self,
        feature_id: impl Into<Arc<str>>,
        status: SupportStatus,
        message: Option<String>,
        location: Option<SourceLocation>,
    ) {
        let feature_id = feature_id.into();
        let entry = self
            .by_feature
            .entry(feature_id.clone())
            .or_insert_with(|| FeatureUse::new(feature_id));
        entry.count = entry.count.saturating_add(1);
        if status > entry.status {
            entry.status = status;
        }
        if entry.message.is_none() {
            entry.message = message;
        }
        if entry.location.is_none() {
            entry.location = location;
        }
    }

    /// Merges another support model into this one, aggregating counts.
    pub fn merge(&mut self, other: SupportModel) {
        for (feature_id, use_) in other.by_feature {
            let entry = self
                .by_feature
                .entry(feature_id.clone())
                .or_insert_with(|| FeatureUse::new(feature_id));
            entry.count = entry.count.saturating_add(use_.count);
            if use_.status > entry.status {
                entry.status = use_.status;
            }
            if entry.message.is_none() {
                entry.message = use_.message;
            }
            if entry.location.is_none() {
                entry.location = use_.location;
            }
        }
    }

    /// Returns the record for one feature.
    #[must_use]
    pub fn get(&self, feature_id: &str) -> Option<&FeatureUse> {
        self.by_feature.get(feature_id)
    }

    /// Iterates over feature records in stable (lexicographic) order.
    pub fn iter(&self) -> impl Iterator<Item = &FeatureUse> {
        self.by_feature.values()
    }

    /// Returns the number of distinct features recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_feature.len()
    }

    /// Returns `true` if no feature was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_feature.is_empty()
    }

    /// Returns how many features have the given status.
    #[must_use]
    pub fn count_with_status(&self, status: SupportStatus) -> usize {
        self.by_feature
            .values()
            .filter(|feature| feature.status == status)
            .count()
    }

    /// Renders a human-readable summary (the Stage-2 stand-in for the Stage-3
    /// JSON Feature Report).
    #[must_use]
    pub fn debug_summary(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "support: {} feature(s), {} partial, {} unsupported, {} ignored",
            self.len(),
            self.count_with_status(SupportStatus::Partial),
            self.count_with_status(SupportStatus::Unsupported),
            self.count_with_status(SupportStatus::Ignored),
        );
        for feature in self.iter() {
            let location = feature
                .location
                .as_ref()
                .map_or_else(String::new, |loc| format!(" @ {loc}"));
            let message = feature
                .message
                .as_ref()
                .map_or_else(String::new, |msg| format!(" — {msg}"));
            let _ = writeln!(
                out,
                "  [{}] {} x{}{}{}",
                feature.status.as_str(),
                feature.feature_id,
                feature.count,
                location,
                message,
            );
        }
        out
    }
}
