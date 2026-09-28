//! Font metrics providers (ADR-0006).
//!
//! Layout depends on glyph advances, but the library must not depend on system
//! fonts: results would otherwise vary across machines and CI. The default
//! provider is the bundled deterministic [`BuiltinFontProvider`]. A system-font
//! provider is explicitly outside the Stage-4 acceptance criteria.

pub mod builtin;
pub mod metrics;

pub use builtin::BuiltinFontProvider;
pub use metrics::FontMetrics;

/// Provides metrics for a font face.
///
/// Implementations must be deterministic for a given `(family, bold, italic)`.
pub trait FontProvider: Send + Sync {
    /// Returns the metrics for a face.
    fn metrics(&self, family: &str, bold: bool, italic: bool) -> FontMetrics;

    /// Returns a stable provider name (used in diagnostics).
    fn name(&self) -> &'static str;
}

/// Which font provider the renderer uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontProviderKind {
    /// The bundled deterministic provider (default; required for determinism).
    #[default]
    Builtin,
    /// A system-font provider (feature `system-fonts`; not part of acceptance).
    #[cfg(feature = "system-fonts")]
    System,
}

impl FontProviderKind {
    /// Creates the provider for this kind.
    #[must_use]
    pub fn make(self) -> Box<dyn FontProvider> {
        match self {
            Self::Builtin => Box::new(BuiltinFontProvider::new()),
            #[cfg(feature = "system-fonts")]
            Self::System => Box::new(BuiltinFontProvider::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FontProviderKind;

    #[test]
    fn kinds_produce_providers() {
        let provider = FontProviderKind::Builtin.make();
        assert!(!provider.name().is_empty());
        assert!(provider.metrics("Calibri", false, false).units_per_em > 0.0);
    }
}
