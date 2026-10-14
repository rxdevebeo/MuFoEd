//! Font metrics providers (ADR-0006).
//!
//! Layout depends on glyph advances, but the library must not depend on system
//! fonts: results would otherwise vary across machines and CI. The default
//! provider is the bundled deterministic [`BuiltinFontProvider`]. A system-font
//! provider is explicitly outside the Stage-4 acceptance criteria.

pub mod assets;
pub mod builtin;
pub mod embedded;
pub mod face;
pub mod family;
pub mod metrics;
pub mod shape;
mod symbol;
#[cfg(feature = "system-fonts")]
pub mod system;

pub use symbol::present_text;

pub use assets::{FontAsset, FontAssetError, FontAssetLimits, FontAssets, ScriptFallback};
pub use builtin::{bundled_families, face_source, BuiltinFontProvider, FaceSource};
pub use face::{hash_face_bytes, hash_face_bytes_hex, resolve_face, FaceId, ResolvedFace};
pub use family::{map_family, substitute_width_scale};
pub use metrics::FontMetrics;
pub use shape::{
    needs_complex_script, shape_advance_em, shape_bundled, shape_bundled_run, shape_run,
    shape_text, unicode_cluster_map, unicode_x_positions_px, ShapeStatus, ShapedCluster,
    ShapedGlyph, ShapedText, COMPLEX_SCRIPT_WARNING,
};

/// Per-cluster origin adjustments shared by SVG and PDF. Accumulate spacing
/// in visual cluster order so right-to-left text receives the same advances.
#[must_use]
pub fn shaped_cluster_offsets_px(
    item: &crate::layout::TextItem,
    shown: &str,
    shaped: &ShapedText,
) -> Vec<f64> {
    let provider = BuiltinFontProvider::new();
    let family = crate::style::chosen_family(&item.run, shown);
    let extra = crate::style::spacing_px(&item.run, item.size_px);
    let chars: Vec<_> = shown.char_indices().collect();
    let mut deltas = vec![0.0; shaped.clusters.len()];
    let mut previous = None;
    let mut cluster_index = 0;
    for (index, &(byte, ch)) in chars.iter().enumerate() {
        while shaped
            .clusters
            .get(cluster_index)
            .is_some_and(|cluster| cluster.byte_end <= byte)
        {
            cluster_index += 1;
        }
        let ordinary = crate::font::embedded::run_advance_em(&item.run, &family, ch, &provider);
        let advance = crate::style::space_advance_em(&item.run, &family, ch, ordinary);
        let next = chars
            .get(index + 1)
            .map(|(_, ch)| *ch)
            .or(item.following_non_space);
        let factor = crate::style::compressed_char_factor(
            previous,
            ch,
            next,
            item.compress_punctuation,
            item.plain_space_factor,
        );
        if let Some(delta) = deltas.get_mut(cluster_index) {
            *delta += (advance * factor - ordinary) * item.size_px
                + if ch.is_whitespace() { 0.0 } else { extra };
        }
        if !ch.is_whitespace() || factor > 0.0 {
            previous = Some(ch);
        }
    }
    let mut order: Vec<(usize, f64)> = shaped
        .clusters
        .iter()
        .enumerate()
        .map(|(index, cluster)| (index, cluster.x_em))
        .collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut offsets = vec![0.0; order.len()];
    let mut cursor = 0.0;
    for (index, _) in order {
        if let Some(offset) = offsets.get_mut(index) {
            *offset = cursor;
        }
        cursor += deltas.get(index).copied().unwrap_or(0.0);
    }
    offsets
}

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

    /// Outline ascent and descent in em; absent when a provider has no outlines.
    fn glyph_vertical_em(
        &self,
        _family: &str,
        _ch: char,
        _bold: bool,
        _italic: bool,
    ) -> Option<(f64, f64)> {
        None
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
