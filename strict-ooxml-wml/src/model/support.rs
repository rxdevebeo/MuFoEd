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

/// Maximum number of distinct locations retained per mechanism (ADR-0005).
///
/// The [`FeatureUse::count`] keeps counting every occurrence; only the first
/// `MAX_LOCATIONS_PER_FEATURE` distinct locations are retained so large
/// documents cannot blow up the report (`STAGE-3-TASK.md` §9, question 1).
pub const MAX_LOCATIONS_PER_FEATURE: usize = 8;

/// One aggregated record of a mechanism usage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureUse {
    /// Feature identifier, for example `w:tbl` or `wp:anchor`.
    pub feature_id: Arc<str>,
    /// Aggregated status (the most severe status observed).
    pub status: SupportStatus,
    /// Optional human-readable message (first occurrence).
    pub message: Option<String>,
    /// Distinct locations, in first-seen order (at most
    /// [`MAX_LOCATIONS_PER_FEATURE`]). The first entry is the primary location.
    pub locations: Vec<SourceLocation>,
    /// Number of occurrences recorded (may exceed `locations.len()`).
    pub count: u32,
}

impl FeatureUse {
    fn new(feature_id: Arc<str>) -> Self {
        Self {
            feature_id,
            status: SupportStatus::Supported,
            message: None,
            locations: Vec::new(),
            count: 0,
        }
    }

    /// Returns the primary (first-seen) location, if any.
    #[must_use]
    pub fn first_location(&self) -> Option<&SourceLocation> {
        self.locations.first()
    }

    /// Returns all retained locations, in first-seen order.
    #[must_use]
    pub fn locations(&self) -> &[SourceLocation] {
        &self.locations
    }

    /// Retains one distinct location, respecting the cap and de-duplicating.
    fn push_location(&mut self, location: SourceLocation) {
        if self.locations.len() >= MAX_LOCATIONS_PER_FEATURE || self.locations.contains(&location) {
            return;
        }
        self.locations.push(location);
    }
}

/// Feature id that absorbs records past [`SupportModel::max_features`] (AUD-51).
pub const SUPPORT_OVERFLOW_ID: &str = "support.overflow";

/// Aggregated support information for a document.
#[derive(Clone, Debug)]
pub struct SupportModel {
    by_feature: BTreeMap<Arc<str>, FeatureUse>,
    /// Cap on distinct feature keys (from `ResourceLimits::max_support_features`).
    max_features: usize,
}

impl Default for SupportModel {
    fn default() -> Self {
        Self::with_limit(10_000)
    }
}

impl SupportModel {
    /// Creates an empty support model with the default feature-key budget.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty support model that admits at most `max_features`
    /// distinct keys before collapsing into [`SUPPORT_OVERFLOW_ID`] (AUD-51).
    #[must_use]
    pub fn with_limit(max_features: usize) -> Self {
        Self {
            by_feature: BTreeMap::new(),
            max_features,
        }
    }

    /// Records one use of a mechanism.
    ///
    /// If the feature is already present, its status is raised to the more
    /// severe of the two, its count is incremented, and the message of the first
    /// occurrence is kept. The location (if any) is added to the bounded,
    /// de-duplicated location list (ADR-0005).
    ///
    /// When the distinct-key budget is exhausted, the record is folded into
    /// [`SUPPORT_OVERFLOW_ID`] instead of growing the map (AUD-51).
    pub fn record(
        &mut self,
        feature_id: impl Into<Arc<str>>,
        status: SupportStatus,
        message: Option<String>,
        location: Option<SourceLocation>,
    ) {
        let feature_id = feature_id.into();
        let key = if self.by_feature.contains_key(&feature_id)
            || feature_id.as_ref() == SUPPORT_OVERFLOW_ID
            || self.by_feature.len() < self.max_features
        {
            feature_id
        } else {
            Arc::from(SUPPORT_OVERFLOW_ID)
        };
        let entry = self
            .by_feature
            .entry(key.clone())
            .or_insert_with(|| FeatureUse::new(key));
        entry.count = entry.count.saturating_add(1);
        if status > entry.status {
            entry.status = status;
        }
        if entry.message.is_none() {
            entry.message = message;
        }
        if let Some(location) = location {
            entry.push_location(location);
        }
    }

    /// Merges another support model into this one, aggregating counts.
    pub fn merge(&mut self, other: SupportModel) {
        for (feature_id, use_) in other.by_feature {
            let key = if self.by_feature.contains_key(&feature_id)
                || feature_id.as_ref() == SUPPORT_OVERFLOW_ID
                || self.by_feature.len() < self.max_features
            {
                feature_id
            } else {
                Arc::from(SUPPORT_OVERFLOW_ID)
            };
            let entry = self
                .by_feature
                .entry(key.clone())
                .or_insert_with(|| FeatureUse::new(key));
            entry.count = entry.count.saturating_add(use_.count);
            if use_.status > entry.status {
                entry.status = use_.status;
            }
            if entry.message.is_none() {
                entry.message = use_.message;
            }
            for location in use_.locations {
                entry.push_location(location);
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
                .first_location()
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

#[cfg(test)]
mod tests {
    use super::{SupportModel, SupportStatus, SUPPORT_OVERFLOW_ID};

    #[test]
    fn overflow_collapses_past_the_budget() {
        let mut model = SupportModel::with_limit(3);
        for index in 0..10 {
            model.record(
                format!("w:zzz{index}"),
                SupportStatus::Unsupported,
                None,
                None,
            );
        }
        assert!(model.len() <= 4); // 3 + support.overflow
        assert!(model.get(SUPPORT_OVERFLOW_ID).is_some());
        assert!(model.get(SUPPORT_OVERFLOW_ID).unwrap().count >= 7);
    }
}
