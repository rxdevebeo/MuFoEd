//! XML safety guards.
//!
//! Enforces the forbidden-construct policy (no DOCTYPE, no entity expansion, no
//! external entities) and the depth / attribute-count / text-length limits from
//! [`ResourceLimits`] (stage task S1.9). The
//! DOCTYPE check lives in [`crate::xml`] where events are matched; the helpers
//! here cover the numeric limits.

use crate::error::{LimitKind, Result, StrictError};
use crate::limits::ResourceLimits;

/// Rejects an element nesting depth above `max_xml_depth`.
pub(crate) fn check_depth(depth: u32, limits: &ResourceLimits) -> Result<()> {
    if depth > limits.max_xml_depth {
        return Err(StrictError::LimitExceeded {
            kind: LimitKind::XmlDepth,
            limit: u64::from(limits.max_xml_depth),
            actual: u64::from(depth),
        });
    }
    Ok(())
}

/// Rejects more attributes on one element than `max_xml_attributes_per_elem`.
pub(crate) fn check_attributes(count: usize, limits: &ResourceLimits) -> Result<()> {
    if count as u64 > u64::from(limits.max_xml_attributes_per_elem) {
        return Err(StrictError::LimitExceeded {
            kind: LimitKind::XmlAttributesPerElement,
            limit: u64::from(limits.max_xml_attributes_per_elem),
            actual: count as u64,
        });
    }
    Ok(())
}

/// Rejects cumulative text longer than `max_text_len` within one part.
pub(crate) fn check_text(total: u64, limits: &ResourceLimits) -> Result<()> {
    if total > limits.max_text_len as u64 {
        return Err(StrictError::LimitExceeded {
            kind: LimitKind::TextLen,
            limit: limits.max_text_len as u64,
            actual: total,
        });
    }
    Ok(())
}
