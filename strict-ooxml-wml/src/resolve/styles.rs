//! Style cascade resolution: `basedOn` chains (STAGE-2 S2.13).

use crate::model::support::SupportStatus;
use crate::model::Document;

/// Computes the `basedOn` chain of every style and records cycles.
pub(crate) fn resolve_styles(document: &mut Document) {
    let Document {
        styles, support, ..
    } = document;

    for id in styles.rechain() {
        support.record(
            "w:basedOn",
            SupportStatus::Partial,
            Some(format!(
                "cycle in basedOn chain for style '{}'",
                id.as_str()
            )),
            None,
        );
    }
}
