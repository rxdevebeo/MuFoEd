//! Family-name mapping to the bundled metric-compatible faces (S4F.2).

/// Maps a requested font family to its bundled metric-compatible equivalent.
///
/// Word documents request proprietary families (Calibri, Arial, …). The renderer
/// substitutes the bundled open fonts that share their metrics so that line
/// breaking matches the producer. Unknown families resolve to Carlito, and that
/// same name is what the SVG `@font-face` delivers.
#[must_use]
pub fn map_family(family: &str) -> &'static str {
    known_family(family).unwrap_or("Carlito")
}

/// Returns a bundled face only for a recognized family or alias.
#[must_use]
pub fn known_family(family: &str) -> Option<&'static str> {
    let lower = family.trim().to_ascii_lowercase();
    // Nastaliq is a different calligraphic style with its own, much taller
    // line box; an Urdu document that asks for one (Jameel Noori Nastaleeq,
    // AlQalam Taj Nastaleeq, …) gets the bundled Nastaliq face as such.
    if lower.contains("nastaleeq") || lower.contains("nastaliq") {
        return Some("Noto Nastaliq Urdu");
    }
    Some(match lower.as_str() {
        "calibri" | "carlito" => "Carlito",
        // Calibri is the wildcard below: it is the family Carlito metrics match.
        "cambria" | "caladea" => "Caladea",
        // LibreOffice writes its own metric-compatible Liberation families;
        // the bundled Arimo/Tinos/Cousine are the same designs under the OFL
        // names (STAGE-5C-REWORK-1 D1).
        "arial" | "helvetica" | "liberation sans" | "arimo" => "Arimo",
        "times new roman" | "liberation serif" | "tinos" => "Tinos",
        "courier new" | "liberation mono" | "cousine" => "Cousine",
        // The math family of Word documents (Stage 5C, §9.1): STIX Two Math
        // is the bundled OFL face with the full OMML glyph repertoire.
        "cambria math" | "strict math" => "Strict Math",
        "strict math script" => "Strict Math Script",
        "strict math fraktur" => "Strict Math Fraktur",
        "strict math doublestruck" => "Strict Math DoubleStruck",
        "stix two math" | "stix2 math" | "stixgeneral" => "STIX Two Math",
        // The script faces answer to their own names, so a family chosen by
        // [`script_fallback`] maps to itself on the next pass.
        "noto naskh arabic" => "Noto Naskh Arabic",
        "noto sans arabic" => "Noto Sans Arabic",
        "noto nastaliq urdu" => "Noto Nastaliq Urdu",
        "jomolhari" => "Jomolhari",
        // An unknown CSS family is measured and delivered as Carlito, the bundled
        // Calibri-metric face. Leaving the request unchanged made layout use the
        // fallback model while the SVG named a face the viewer does not have.
        _ => return None,
    })
}

/// Bundled face that draws `ch` when the `base` face has no glyph for it.
///
/// The metric-compatible faces cover Latin, Greek, Cyrillic and Hebrew only;
/// Word and WPS draw Arabic and Tibetan from a font that has them (WQ-004).
/// Arabic next to a sans face (Arial, Tahoma via Arimo) takes the sans
/// Arabic design, otherwise the Naskh one, which is the style of Times New
/// Roman's and Traditional Arabic's Arabic.
#[must_use]
pub fn script_fallback(base: &str, ch: char) -> Option<&'static str> {
    match ch {
        '\u{0600}'..='\u{06FF}'
        | '\u{0750}'..='\u{077F}'
        | '\u{0870}'..='\u{08FF}'
        | '\u{FB50}'..='\u{FDFF}'
        | '\u{FE70}'..='\u{FEFF}' => Some(if base == "Arimo" {
            "Noto Sans Arabic"
        } else {
            "Noto Naskh Arabic"
        }),
        '\u{0F00}'..='\u{0FFF}' => Some("Jomolhari"),
        _ => None,
    }
}

/// Whether `family` is a face [`script_fallback`] substitutes for glyphs only.
///
/// Such a face draws the text, but the line box stays that of the face the
/// run asked for: WPS, lacking Traditional Arabic, sets Arabic in Times New
/// Roman's line box, and the Naskh face's own box is almost twice as tall.
#[must_use]
pub fn is_script_fallback(family: &str) -> bool {
    matches!(
        family,
        "Noto Naskh Arabic" | "Noto Sans Arabic" | "Jomolhari"
    )
}

/// Horizontal advance scale for a mapped family versus WPS-embedded Times.
///
/// Bundled Tinos is metric-compatible with Times New Roman on paper, but Clio
/// WPS PDFs still drift: regular runs land short (~0.8%), bold runs long
/// (~1.7%). Scale advances so body keys on pages 54/104 stay within 0.25 px
/// without a global wrap-breaking multiplier.
#[must_use]
pub fn substitute_width_scale(mapped_family: &str, bold: bool) -> f64 {
    if mapped_family != "Tinos" {
        return 1.0;
    }
    // Regular Tinos runs short vs WPS Times (~0.8%). Bold runs long (~1.7%);
    // callers gate bold on justified body lines so right-aligned captions stay put.
    if bold {
        0.9833
    } else {
        1.008
    }
}

#[cfg(test)]
mod tests {
    use super::map_family;

    #[test]
    fn maps_known_families_case_insensitively() {
        assert_eq!(map_family("Calibri"), "Carlito");
        assert_eq!(map_family("  calibri "), "Carlito");
        assert_eq!(map_family("ARIAL"), "Arimo");
        assert_eq!(map_family("Helvetica"), "Arimo");
        assert_eq!(map_family("Cambria"), "Caladea");
        assert_eq!(map_family("Times New Roman"), "Tinos");
        assert_eq!(map_family("Courier New"), "Cousine");
    }

    #[test]
    fn maps_the_math_family_to_the_bundled_ofl_face() {
        assert_eq!(map_family("Cambria Math"), "Strict Math");
        assert_eq!(map_family("cambria math"), "Strict Math");
        assert_eq!(map_family("STIX Two Math"), "STIX Two Math");
    }

    #[test]
    fn maps_the_libreoffice_families_to_their_bundled_twins() {
        // STAGE-5C-REWORK-1 D1: an unmapped family is resolved by the
        // rasterizer's fallback and comes out grey, so `04-libreoffice-…`
        // rendered no ink at all.
        assert_eq!(map_family("Liberation Sans"), "Arimo");
        assert_eq!(map_family("Liberation Serif"), "Tinos");
        assert_eq!(map_family("Liberation Mono"), "Cousine");
        assert_eq!(map_family("  liberation mono "), "Cousine");
    }

    #[test]
    fn script_faces_cover_what_the_metric_faces_lack() {
        use super::{is_script_fallback, script_fallback};
        assert_eq!(script_fallback("Tinos", 'ب'), Some("Noto Naskh Arabic"));
        assert_eq!(script_fallback("Carlito", 'ﻻ'), Some("Noto Naskh Arabic"));
        assert_eq!(script_fallback("Arimo", 'ب'), Some("Noto Sans Arabic"));
        assert_eq!(script_fallback("Carlito", 'ཀ'), Some("Jomolhari"));
        assert_eq!(script_fallback("Carlito", 'a'), None);
        assert_eq!(script_fallback("Tinos", 'א'), None);
        assert!(is_script_fallback("Jomolhari"));
        assert!(!is_script_fallback("Noto Nastaliq Urdu"));
        for family in ["Noto Naskh Arabic", "Noto Sans Arabic", "Jomolhari"] {
            assert_eq!(map_family(family), family);
        }
        assert_eq!(map_family("Jameel Noori Nastaleeq"), "Noto Nastaliq Urdu");
        assert_eq!(map_family("AlQalam Taj Nastaleeq"), "Noto Nastaliq Urdu");
    }

    #[test]
    fn unknown_families_use_the_bundled_fallback() {
        assert_eq!(map_family("Segoe UI"), "Carlito");
        assert_eq!(map_family(""), "Carlito");
        assert_eq!(map_family("Not A Real Font"), "Carlito");
        // Mapping is idempotent: a second pass must not send Arimo through the
        // unknown-family wildcard.
        assert_eq!(map_family("Arimo"), "Arimo");
        assert_eq!(map_family("Tinos"), "Tinos");
        assert_eq!(map_family("Cousine"), "Cousine");
        assert_eq!(map_family("Caladea"), "Caladea");
        assert_eq!(map_family("Carlito"), "Carlito");
    }
}
