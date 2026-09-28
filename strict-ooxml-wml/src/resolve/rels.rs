//! Relationship reference validation (STAGE-2 S2.13).
//!
//! Drawing `r:embed` targets are resolved during parsing (the media index is
//! built there); this pass validates the remaining references — header/footer
//! relationship ids declared by sections.

use strict_ooxml_core::opc::Package;

use crate::model::support::SupportStatus;
use crate::model::Document;

/// Validates section header/footer references against the package.
pub(crate) fn validate_references(document: &mut Document, package: &Package) {
    let main = document.source.main_document.clone();
    let mut issues = Vec::new();
    for section in &document.sections {
        let properties = &section.properties;
        for reference in properties.headers.iter().chain(properties.footers.iter()) {
            if package
                .resolve_relationship(&main, reference.rel_id.as_str())
                .is_err()
            {
                issues.push(format!(
                    "unresolved header/footer relationship '{}'",
                    reference.rel_id
                ));
            }
        }
    }
    for message in issues {
        document.support.record(
            "w:headerReference",
            SupportStatus::Partial,
            Some(message),
            None,
        );
    }
}
