//! Embedded JSON Schema for the Feature Report (`STAGE-3-TASK.md` §6).
//!
//! `schema/support-report.schema.json` is the source of truth for the JSON
//! contract. It is embedded here so the CLI and consumers can expose it without
//! touching the filesystem; the independent `jsonschema` validator in the tests
//! validates reports against the same file (ADR-0005).

/// The Feature Report JSON Schema (schema v2), as shipped in the repository.
pub const SUPPORT_REPORT_SCHEMA: &str = include_str!("../schema/support-report.schema.json");

/// Returns the embedded JSON Schema document.
#[must_use]
pub const fn schema_json() -> &'static str {
    SUPPORT_REPORT_SCHEMA
}

#[cfg(test)]
mod tests {
    use super::{schema_json, SUPPORT_REPORT_SCHEMA};
    use crate::model::SCHEMA_VERSION;

    #[test]
    fn embedded_schema_is_valid_json_and_versioned() {
        assert_eq!(schema_json(), SUPPORT_REPORT_SCHEMA);
        let value: serde_json::Value =
            serde_json::from_str(schema_json()).expect("schema is valid JSON");
        assert_eq!(
            value["properties"]["schema_version"]["const"],
            SCHEMA_VERSION
        );
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }
}
