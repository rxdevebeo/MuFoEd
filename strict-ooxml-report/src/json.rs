//! Deterministic JSON serialization of a [`SupportReport`].
//!
//! The output is pretty-printed with a single trailing newline and is a pure
//! function of the report (all collections are ordered; no hash map is
//! involved). Two runs always produce identical bytes (ADR-0005).

use crate::model::SupportReport;

/// Serializes a report to pretty JSON, terminated by `\n`.
///
/// Serialization cannot fail for the report model (it contains no fallible
/// custom serializers); should that ever change, the error itself is emitted
/// as a JSON object instead of a panic.
#[must_use]
pub fn to_json(report: &SupportReport) -> String {
    let mut out = serde_json::to_string_pretty(report)
        .unwrap_or_else(|error| serde_json::json!({ "error": error.to_string() }).to_string());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::to_json;
    use crate::build::{build, ReportInput};
    use strict_ooxml_wml::model::support::SupportModel;

    #[test]
    fn json_is_pretty_newline_terminated_and_stable() {
        let support = SupportModel::new();
        let report = build(ReportInput::new("f.docx", &support));
        let first = to_json(&report);
        let second = to_json(&report);
        assert_eq!(first, second);
        assert!(first.ends_with("}\n"), "{first}");
        assert!(first.contains("\n  \"schema_version\""), "{first}");
        let _ = serde_json::from_str::<serde_json::Value>(&first).expect("valid json");
    }
}
