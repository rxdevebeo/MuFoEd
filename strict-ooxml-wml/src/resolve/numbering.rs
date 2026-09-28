//! Numbering validation: `numId → abstractNumId` (STAGE-2 S2.13).

use crate::model::support::SupportStatus;
use crate::model::Document;

/// Validates that every numbering instance points at an existing abstract
/// definition and records the problems.
pub(crate) fn resolve_numbering(document: &mut Document) {
    let Document {
        numbering, support, ..
    } = document;

    let missing: Vec<(u32, u32, strict_ooxml_core::error::SourceLocation)> = numbering
        .nums()
        .filter(|num| numbering.abstract_num(num.abstract_num_id).is_none())
        .map(|num| (num.num_id.0, num.abstract_num_id.0, num.location.clone()))
        .collect();

    for (num_id, abstract_id, location) in missing {
        support.record(
            "w:numId",
            SupportStatus::Partial,
            Some(format!(
                "numId {num_id} references missing abstractNumId {abstract_id}"
            )),
            Some(location),
        );
    }
}
