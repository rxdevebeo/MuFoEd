//! XML safety guards.
//!
//! Enforces the forbidden-construct policy (no DOCTYPE, no entity expansion, no
//! external entities) and the depth / element-count / attribute-count /
//! text-length / attribute-byte limits from
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

/// Rejects more elements in one part than `max_xml_elements`.
pub(crate) fn check_elements(count: u64, limits: &ResourceLimits) -> Result<()> {
    if count > limits.max_xml_elements {
        return Err(StrictError::LimitExceeded {
            kind: LimitKind::XmlElements,
            limit: limits.max_xml_elements,
            actual: count,
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

/// Rejects cumulative attribute bytes (names plus raw values) above
/// `max_text_len` within one part.
///
/// A separate budget from [`check_text`] with the same bound: attributes
/// were counted but their bytes were not, so 1024 attributes of 100 KiB each on
/// one element allocated ~100 MiB of `String`s past every limit.
pub(crate) fn check_attribute_bytes(total: u64, limits: &ResourceLimits) -> Result<()> {
    check_text(total, limits)
}

#[cfg(test)]
mod tests {
    use super::{check_attribute_bytes, check_elements};
    use crate::error::{LimitKind, StrictError};
    use crate::limits::ResourceLimits;

    #[test]
    fn element_count_at_the_limit_passes_and_one_more_fails() {
        let limits = ResourceLimits {
            max_xml_elements: 3,
            ..ResourceLimits::default()
        };
        assert!(check_elements(3, &limits).is_ok());
        assert!(matches!(
            check_elements(4, &limits),
            Err(StrictError::LimitExceeded {
                kind: LimitKind::XmlElements,
                limit: 3,
                actual: 4,
            })
        ));
    }

    #[test]
    fn attribute_bytes_use_the_text_bound() {
        let limits = ResourceLimits {
            max_text_len: 10,
            ..ResourceLimits::default()
        };
        assert!(check_attribute_bytes(10, &limits).is_ok());
        assert!(matches!(
            check_attribute_bytes(11, &limits),
            Err(StrictError::LimitExceeded {
                kind: LimitKind::TextLen,
                ..
            })
        ));
    }
}
