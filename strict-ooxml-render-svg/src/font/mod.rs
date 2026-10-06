//! Font metrics providers (ADR-0006).
//!
//! Layout depends on glyph advances, but the library must not depend on system
//! fonts: results would otherwise vary across machines and CI. The default
//! provider is the bundled deterministic [`BuiltinFontProvider`]. A system-font
//! provider is explicitly outside the Stage-4 acceptance criteria.

pub mod builtin;
pub mod face;
pub mod family;
pub mod metrics;
pub mod shape;
mod symbol;

pub use symbol::present_text;

pub use builtin::{bundled_families, face_source, BuiltinFontProvider, FaceSource};
pub use face::{hash_face_bytes, hash_face_bytes_hex, resolve_face, FaceId, ResolvedFace};
pub use family::{map_family, substitute_width_scale};
pub use metrics::FontMetrics;
pub use shape::{
    needs_complex_script, shape_advance_em, shape_bundled, shape_text, unicode_cluster_map,
    unicode_x_positions_px, ShapeStatus, ShapedCluster, ShapedGlyph, ShapedText,
    COMPLEX_SCRIPT_WARNING,
};

/// Provides metrics for a font face.
///
/// Implementations must be deterministic for a given `(family, bold, italic)`.
pub trait FontProvider: Send + Sync {
    /// Returns the vertical metrics for a face.
    fn metrics(&self, family: &str, bold: bool, italic: bool) -> FontMetrics;

    /// Returns the advance width of `ch` in em units.
    ///
    /// Providers over real fonts override this with true glyph advances; the
    /// default falls back to the deterministic character-class model.
    fn advance_em(&self, family: &str, ch: char, bold: bool, italic: bool) -> f64 {
        self.metrics(family, bold, italic).advance_em(ch, bold)
    }

    /// Returns a stable provider name (used in diagnostics).
    fn name(&self) -> &'static str;

    /// Returns `true` when the face for `family` actually covers `ch`.
    ///
    /// Math layout uses this to decide between a real glyph and the
    /// deterministic vector fallback (Stage 5C, §9.1/§12). The default is
    /// `false` — a provider that cannot answer must not silently claim coverage.
    fn has_glyph(&self, _family: &str, _ch: char) -> bool {
        false
    }
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
