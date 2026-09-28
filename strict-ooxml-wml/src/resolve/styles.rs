//! Style cascade resolution: `basedOn` chains (STAGE-2 S2.13).

use crate::model::ids::StyleId;
use crate::model::support::SupportStatus;
use crate::model::Document;

/// Maximum `basedOn` chain length before a cycle is assumed.
const MAX_CHAIN: usize = 64;

/// Computes the `basedOn` chain of every style and records cycles.
pub(crate) fn resolve_styles(document: &mut Document) {
    let Document {
        styles, support, ..
    } = document;

    let ids: Vec<StyleId> = styles.ids().cloned().collect();
    for id in ids {
        let mut chain = Vec::new();
        let mut cycle = false;
        let mut current = styles.get(&id).and_then(|style| style.based_on.clone());
        while let Some(parent) = current {
            if parent == id || chain.contains(&parent) || chain.len() >= MAX_CHAIN {
                cycle = true;
                break;
            }
            current = styles.get(&parent).and_then(|style| style.based_on.clone());
            chain.push(parent);
        }
        if let Some(style) = styles.get_mut(&id) {
            style.based_on_chain = chain;
        }
        if cycle {
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
}
