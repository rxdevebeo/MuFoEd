//! Builtin (bundled, deterministic) font provider (ADR-0006).
//!
//! Only metrics *data* is bundled — no font binaries. Advances come from the
//! deterministic character-class model in [`super::metrics`]; families only
//! adjust the width scale.

use super::metrics::FontMetrics;
use super::FontProvider;

/// The default, platform-independent font provider.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinFontProvider;

impl BuiltinFontProvider {
    /// Creates the provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl FontProvider for BuiltinFontProvider {
    fn metrics(&self, family: &str, _bold: bool, _italic: bool) -> FontMetrics {
        FontMetrics {
            width_scale: family_scale(family),
            ..FontMetrics::default()
        }
    }

    fn name(&self) -> &'static str {
        "builtin"
    }
}

/// Returns the deterministic width scale for a family name.
#[must_use]
pub fn family_scale(family: &str) -> f64 {
    let lower = family.to_ascii_lowercase();
    if lower.contains("courier") || lower.contains("mono") || lower.contains("consolas") {
        1.2
    } else if lower.contains("arial") || lower.contains("helvetica") || lower.contains("verdana") {
        1.05
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::{family_scale, BuiltinFontProvider};
    use crate::font::FontProvider;

    #[test]
    fn builtin_metrics_are_stable() {
        let provider = BuiltinFontProvider::new();
        let a = provider.metrics("Calibri", false, false);
        let b = provider.metrics("Calibri", false, false);
        assert_eq!(a, b);
        assert_eq!(provider.name(), "builtin");
        assert!(family_scale("Courier New") > family_scale("Calibri"));
        assert!((family_scale("Unknown Family") - 1.0).abs() < 1e-9);
    }
}
