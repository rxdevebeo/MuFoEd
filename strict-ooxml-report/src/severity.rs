//! Severity, summary and `overall_status` rules (ADR-0005; `TZ` §11.2).
//!
//! The default severity table is duplicated by construction in the JSON Schema
//! (`if`/`then` constraints), so the code and the external contract cannot drift
//! without a test failing.

use crate::model::{Feature, FeatureStatus, OverallStatus, Severity, Summary};

/// Maps a feature status to its default severity (ADR-0005).
#[must_use]
pub const fn severity_for(status: FeatureStatus) -> Severity {
    match status {
        FeatureStatus::Supported | FeatureStatus::Ignored => Severity::Info,
        FeatureStatus::Partial | FeatureStatus::Unsupported => Severity::Warning,
        FeatureStatus::Error => Severity::Error,
    }
}

/// Counts features per status.
#[must_use]
pub fn summarize(features: &[Feature]) -> Summary {
    let mut summary = Summary::default();
    for feature in features {
        match feature.status {
            FeatureStatus::Supported => summary.supported += 1,
            FeatureStatus::Partial => summary.partial += 1,
            FeatureStatus::Unsupported => summary.unsupported += 1,
            FeatureStatus::Ignored => summary.ignored += 1,
            FeatureStatus::Error => summary.error += 1,
        }
    }
    summary
}

/// Aggregates the document status as “worst of”
/// (`error`/`unsupported` → `unsupported`, else `partial`, else `supported`).
#[must_use]
pub fn overall_status(features: &[Feature]) -> OverallStatus {
    let mut has_partial = false;
    for feature in features {
        match feature.status {
            FeatureStatus::Unsupported | FeatureStatus::Error => {
                return OverallStatus::Unsupported;
            }
            FeatureStatus::Partial => has_partial = true,
            FeatureStatus::Supported | FeatureStatus::Ignored => {}
        }
    }
    if has_partial {
        OverallStatus::Partial
    } else {
        OverallStatus::Supported
    }
}

#[cfg(test)]
mod tests {
    use super::{overall_status, severity_for, summarize};
    use crate::model::{Feature, FeatureStatus, Location, OverallStatus, Severity};

    fn feature(status: FeatureStatus) -> Feature {
        Feature {
            feature_id: format!("w:{status}"),
            status,
            severity: severity_for(status),
            message: None,
            locations: vec![Location::new("/word/document.xml:1:1")],
            count: 1,
        }
    }

    #[test]
    fn default_severity_table() {
        assert_eq!(severity_for(FeatureStatus::Supported), Severity::Info);
        assert_eq!(severity_for(FeatureStatus::Ignored), Severity::Info);
        assert_eq!(severity_for(FeatureStatus::Partial), Severity::Warning);
        assert_eq!(severity_for(FeatureStatus::Unsupported), Severity::Warning);
        assert_eq!(severity_for(FeatureStatus::Error), Severity::Error);
    }

    #[test]
    fn summarize_counts_every_status() {
        let features = vec![
            feature(FeatureStatus::Supported),
            feature(FeatureStatus::Supported),
            feature(FeatureStatus::Partial),
            feature(FeatureStatus::Unsupported),
            feature(FeatureStatus::Ignored),
            feature(FeatureStatus::Error),
        ];
        let summary = summarize(&features);
        assert_eq!(summary.supported, 2);
        assert_eq!(summary.partial, 1);
        assert_eq!(summary.unsupported, 1);
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.error, 1);
    }

    #[test]
    fn overall_is_worst_of() {
        assert_eq!(overall_status(&[]), OverallStatus::Supported);
        assert_eq!(
            overall_status(&[feature(FeatureStatus::Supported)]),
            OverallStatus::Supported
        );
        assert_eq!(
            overall_status(&[feature(FeatureStatus::Ignored)]),
            OverallStatus::Supported
        );
        assert_eq!(
            overall_status(&[
                feature(FeatureStatus::Supported),
                feature(FeatureStatus::Partial),
            ]),
            OverallStatus::Partial
        );
        assert_eq!(
            overall_status(&[
                feature(FeatureStatus::Partial),
                feature(FeatureStatus::Unsupported),
            ]),
            OverallStatus::Unsupported
        );
        assert_eq!(
            overall_status(&[feature(FeatureStatus::Error)]),
            OverallStatus::Unsupported
        );
    }
}
