//! Bundled, deterministic font provider (ADR-0006, S4F.1).
//!
//! Real glyph advances come from bundled *metric-compatible* open fonts
//! (Carlito/Caladea/Arimo/Tinos/Cousine), read at run time with `skrifa`. The
//! font binaries are compiled into the library, so layout is identical on every
//! machine and never consults system fonts. Unknown families fall back to the
//! deterministic character-class model in [`super::metrics`].

use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::prelude::{MetadataProvider, Tag};
use skrifa::FontRef;

use super::family::map_family;
use super::metrics::FontMetrics;
use super::FontProvider;

/// A bundled face: family, style, parsed font, variation location and metrics.
struct FaceEntry {
    family: &'static str,
    bold: bool,
    italic: bool,
    /// Whether this face serves every `(bold, italic)` combination.
    single: bool,
    font: FontRef<'static>,
    location: Vec<NormalizedCoord>,
    units_per_em: f64,
    vertical: FontMetrics,
}

/// One compiled-in font file and the style it provides.
struct FaceSpec {
    family: &'static str,
    bold: bool,
    italic: bool,
    /// Whether the single face serves every `(bold, italic)` combination.
    ///
    /// STIX Two Math ships one upright face only; the mathematical italic of
    /// variables is a *slant* of the same outlines, so the same advances apply.
    single: bool,
    /// Variable-font `wght` value, when the face is a variable font instance.
    weight: Option<f32>,
    data: &'static [u8],
}

/// The bundled metric-compatible faces (Calibri/Cambria/Arial/Times/Courier).
const FACE_SPECS: &[FaceSpec] = &[
    FaceSpec {
        family: "Carlito",
        bold: false,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/carlito/Carlito-Regular.ttf"),
    },
    FaceSpec {
        family: "Carlito",
        bold: true,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/carlito/Carlito-Bold.ttf"),
    },
    FaceSpec {
        family: "Carlito",
        bold: false,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/carlito/Carlito-Italic.ttf"),
    },
    FaceSpec {
        family: "Carlito",
        bold: true,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/carlito/Carlito-BoldItalic.ttf"),
    },
    FaceSpec {
        family: "Caladea",
        bold: false,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/caladea/Caladea-Regular.ttf"),
    },
    FaceSpec {
        family: "Caladea",
        bold: true,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/caladea/Caladea-Bold.ttf"),
    },
    FaceSpec {
        family: "Caladea",
        bold: false,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/caladea/Caladea-Italic.ttf"),
    },
    FaceSpec {
        family: "Caladea",
        bold: true,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/caladea/Caladea-BoldItalic.ttf"),
    },
    FaceSpec {
        family: "Arimo",
        bold: false,
        italic: false,
        single: false,
        weight: Some(400.0),
        data: include_bytes!("../../assets/fonts/arimo/Arimo[wght].ttf"),
    },
    FaceSpec {
        family: "Arimo",
        bold: true,
        italic: false,
        single: false,
        weight: Some(700.0),
        data: include_bytes!("../../assets/fonts/arimo/Arimo[wght].ttf"),
    },
    FaceSpec {
        family: "Arimo",
        bold: false,
        italic: true,
        single: false,
        weight: Some(400.0),
        data: include_bytes!("../../assets/fonts/arimo/Arimo-Italic[wght].ttf"),
    },
    FaceSpec {
        family: "Arimo",
        bold: true,
        italic: true,
        single: false,
        weight: Some(700.0),
        data: include_bytes!("../../assets/fonts/arimo/Arimo-Italic[wght].ttf"),
    },
    FaceSpec {
        family: "Tinos",
        bold: false,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/tinos/Tinos-Regular.ttf"),
    },
    FaceSpec {
        family: "Tinos",
        bold: true,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/tinos/Tinos-Bold.ttf"),
    },
    FaceSpec {
        family: "Tinos",
        bold: false,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/tinos/Tinos-Italic.ttf"),
    },
    FaceSpec {
        family: "Tinos",
        bold: true,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/tinos/Tinos-BoldItalic.ttf"),
    },
    FaceSpec {
        family: "Cousine",
        bold: false,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/cousine/Cousine-Regular.ttf"),
    },
    FaceSpec {
        family: "Cousine",
        bold: true,
        italic: false,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/cousine/Cousine-Bold.ttf"),
    },
    FaceSpec {
        family: "Cousine",
        bold: false,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/cousine/Cousine-Italic.ttf"),
    },
    FaceSpec {
        family: "Cousine",
        bold: true,
        italic: true,
        single: false,
        weight: None,
        data: include_bytes!("../../assets/fonts/cousine/Cousine-BoldItalic.ttf"),
    },
    // The OFL math face (Stage 5C, §9.1). One upright face covers the whole
    // OMML glyph repertoire: n-ary operators, radicals, stretchy delimiter
    // parts, accents, over/under-braces and the letterlike-alphanumeric block.
    FaceSpec {
        family: "STIX Two Math",
        bold: false,
        italic: false,
        single: true,
        weight: None,
        data: include_bytes!("../../assets/fonts/stix/STIXTwoMath-Regular.otf"),
    },
];

/// The default, platform-independent font provider.
pub struct BuiltinFontProvider {
    faces: Vec<FaceEntry>,
}

impl BuiltinFontProvider {
    /// Creates the provider, parsing the bundled fonts.
    #[must_use]
    pub fn new() -> Self {
        let faces = FACE_SPECS
            .iter()
            .filter_map(|spec| {
                let font = FontRef::new(spec.data).ok()?;
                let location = spec.weight.map_or_else(Vec::new, |weight| {
                    font.axes()
                        .get_by_tag(Tag::new(b"wght"))
                        .map(|axis| vec![axis.normalize(weight)])
                        .unwrap_or_default()
                });
                let metrics = font.metrics(Size::unscaled(), LocationRef::new(&location));
                let units_per_em = f64::from(metrics.units_per_em);
                let vertical = if units_per_em > 0.0 {
                    FontMetrics {
                        units_per_em,
                        ascender: f64::from(metrics.ascent) / units_per_em,
                        descender: f64::from(metrics.descent).abs() / units_per_em,
                        line_gap: f64::from(metrics.leading) / units_per_em,
                        width_scale: 1.0,
                    }
                } else {
                    FontMetrics::default()
                };
                Some(FaceEntry {
                    family: spec.family,
                    bold: spec.bold,
                    italic: spec.italic,
                    single: spec.single,
                    font,
                    location,
                    units_per_em,
                    vertical,
                })
            })
            .collect();
        Self { faces }
    }

    /// Returns the parsed face for a mapped family and style, if bundled.
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<&FaceEntry> {
        self.faces
            .iter()
            .find(|entry| entry.family == family && entry.bold == bold && entry.italic == italic)
            .or_else(|| {
                // A single-face family (STIX Two Math) serves every style.
                self.faces
                    .iter()
                    .find(|entry| entry.family == family && entry.single)
            })
    }
}

impl Default for BuiltinFontProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for BuiltinFontProvider {
    fn metrics(&self, family: &str, bold: bool, italic: bool) -> FontMetrics {
        let Some(entry) = self.face(map_family(family), bold, italic) else {
            return FontMetrics {
                width_scale: family_scale(family),
                ..FontMetrics::default()
            };
        };
        entry.vertical
    }

    fn advance_em(&self, family: &str, ch: char, bold: bool, italic: bool) -> f64 {
        if let Some(entry) = self.face(map_family(family), bold, italic) {
            if entry.units_per_em > 0.0 {
                if let Some(glyph) = entry.font.charmap().map(ch) {
                    let glyphs = GlyphMetrics::new(
                        &entry.font,
                        Size::unscaled(),
                        LocationRef::new(&entry.location),
                    );
                    if let Some(advance) = glyphs.advance_width(glyph) {
                        return f64::from(advance) / entry.units_per_em;
                    }
                }
            }
        }
        // Unknown family or a character without a glyph: deterministic fallback.
        self.metrics(family, bold, italic).advance_em(ch, bold)
    }

    fn name(&self) -> &'static str {
        "bundled"
    }

    fn has_glyph(&self, family: &str, ch: char) -> bool {
        use skrifa::raw::types::GlyphId;
        self.face(map_family(family), false, false)
            .is_some_and(|entry| {
                entry
                    .font
                    .charmap()
                    .map(ch)
                    .is_some_and(|glyph| glyph != GlyphId::NOTDEF)
            })
    }
}

/// Returns the deterministic width scale for a family name (fallback only).
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::{family_scale, BuiltinFontProvider};
    use crate::font::FontProvider;

    #[test]
    fn builtin_metrics_are_stable() {
        let provider = BuiltinFontProvider::new();
        let a = provider.metrics("Calibri", false, false);
        let b = provider.metrics("Calibri", false, false);
        assert_eq!(a, b);
        assert_eq!(provider.name(), "bundled");
        assert!(family_scale("Courier New") > family_scale("Calibri"));
        assert!((family_scale("Unknown Family") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn math_face_covers_the_omml_repertoire() {
        let provider = BuiltinFontProvider::new();
        // Letters, digits and the OMML operators/decorations the renderer draws
        // as text. Values are read independently from the bundled OTF.
        for ch in [
            'a', 'Z', '0', '9', '+', '−', '∑', '∏', '∫', '∞', '≠', '≤', '√', '̂', '⃗', '↻', '→', 'ℝ',
            '⟨', '⟩',
        ] {
            assert!(
                provider.has_glyph("Cambria Math", ch),
                "missing glyph {ch:?}"
            );
        }
        // A single face serves every style: mathematical italic is a slant.
        assert!(provider.advance_em("Cambria Math", 'x', true, true) > 0.0);
        assert_eq!(
            provider.advance_em("Cambria Math", 'x', true, true),
            provider.advance_em("Cambria Math", 'x', false, false)
        );
        // Characters outside the repertoire report no coverage so the math
        // layout falls back to its deterministic vector model.
        assert!(!provider.has_glyph("Cambria Math", '\u{10FFFD}'));
        assert!(!provider.has_glyph("Totally Unknown", 'a'));
    }

    #[test]
    fn bundled_advances_match_known_metrics() {
        let provider = BuiltinFontProvider::new();
        // Each expected value is read independently from the bundled TTF (via
        // fontTools) and differs from the deterministic fallback model by more
        // than the tolerance, so this proves the real bundled face is used.
        let check = |family: &str, ch: char, expected: f64| {
            let got = provider.advance_em(family, ch, false, false);
            assert!(
                (got - expected).abs() < 0.002,
                "{family} '{ch}': {got} != {expected}"
            );
        };
        check("Calibri", ' ', 0.2261); // Carlito (fallback 0.25)
        check("Cambria", 'M', 0.888); // Caladea (fallback 0.85)
        check("Arial", ' ', 0.2778); // Arimo (fallback 0.25)
        check("Times New Roman", 'M', 0.8892); // Tinos (fallback 0.85)
        check("Courier New", 'i', 0.6001); // Cousine (fallback 0.28)
                                           // A wide 'M' is wider than a narrow 'l'.
        assert!(
            provider.advance_em("Calibri", 'M', false, false)
                > provider.advance_em("Calibri", 'l', false, false)
        );
        // Bold differs from regular in a real bold face.
        assert!(
            provider.advance_em("Calibri", 'a', true, false)
                > provider.advance_em("Calibri", 'a', false, false)
        );
        // Unknown family falls back to the deterministic model.
        assert!(provider.advance_em("Totally Unknown", 'a', false, false) > 0.0);
    }
}
