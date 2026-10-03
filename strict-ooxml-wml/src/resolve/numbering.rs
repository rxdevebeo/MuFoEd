//! Numbering validation and `numStyleLink` resolution (STAGE-2 S2.13, AUD-47).

use crate::model::ids::{AbstractNumId, StyleId};
use crate::model::support::SupportStatus;
use crate::model::values::StyleType;
use crate::model::Document;

/// Maximum `numStyleLink` hops before a cycle / runaway chain is assumed.
const MAX_LINK_HOPS: usize = 8;

/// Validates numbering instances and resolves `numStyleLink` chains.
pub(crate) fn resolve_numbering(document: &mut Document) {
    resolve_num_style_links(document);

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

/// Follows each `numStyleLink` to the abstract that carries the levels.
fn resolve_num_style_links(document: &mut Document) {
    let starters: Vec<(
        AbstractNumId,
        StyleId,
        strict_ooxml_core::error::SourceLocation,
    )> = document
        .numbering
        .abstracts()
        .filter_map(|abstract_num| {
            abstract_num.num_style_link.as_ref().map(|style_id| {
                (
                    abstract_num.id,
                    style_id.clone(),
                    abstract_num.location.clone(),
                )
            })
        })
        .collect();

    for (start_id, _style_id, location) in starters {
        match follow_num_style_link(document, start_id) {
            LinkOutcome::Resolved(target) if target != start_id => {
                document.numbering.set_link_target(start_id, target);
            }
            LinkOutcome::Resolved(_) => {}
            LinkOutcome::Cycle => {
                document.support.record(
                    "w:numStyleLink",
                    SupportStatus::Partial,
                    Some("cycle".to_owned()),
                    Some(location),
                );
            }
            LinkOutcome::Broken(reason) => {
                document.support.record(
                    "w:numStyleLink",
                    SupportStatus::Partial,
                    Some(reason),
                    Some(location),
                );
            }
        }
    }
}

enum LinkOutcome {
    Resolved(AbstractNumId),
    Cycle,
    Broken(String),
}

/// Walks `numStyleLink` → numbering style → `numPr/numId` → `num` → abstract.
fn follow_num_style_link(document: &Document, start: AbstractNumId) -> LinkOutcome {
    let mut current = start;
    let mut seen_styles: Vec<StyleId> = Vec::new();

    for _ in 0..MAX_LINK_HOPS {
        let Some(abstract_num) = document.numbering.abstract_num(current) else {
            return LinkOutcome::Broken(format!(
                "numStyleLink chain reached missing abstractNumId {}",
                current.0
            ));
        };
        let Some(style_id) = abstract_num.num_style_link.clone() else {
            return LinkOutcome::Resolved(current);
        };
        if seen_styles.iter().any(|seen| seen == &style_id) {
            return LinkOutcome::Cycle;
        }
        seen_styles.push(style_id.clone());

        let Some(style) = document.styles.get(&style_id) else {
            return LinkOutcome::Broken(format!(
                "numStyleLink references missing style '{}'",
                style_id.as_str()
            ));
        };
        if style.style_type != StyleType::Numbering {
            return LinkOutcome::Broken(format!(
                "numStyleLink style '{}' is not a numbering style",
                style_id.as_str()
            ));
        }
        let Some(num_id) = style
            .paragraph
            .numbering
            .as_ref()
            .and_then(|num_pr| num_pr.num_id)
        else {
            return LinkOutcome::Broken(format!(
                "numbering style '{}' has no pPr/numPr/numId",
                style_id.as_str()
            ));
        };
        let Some(num) = document.numbering.num(num_id) else {
            return LinkOutcome::Broken(format!(
                "numbering style '{}' references missing numId {}",
                style_id.as_str(),
                num_id.0
            ));
        };
        current = num.abstract_num_id;
    }

    LinkOutcome::Cycle
}
