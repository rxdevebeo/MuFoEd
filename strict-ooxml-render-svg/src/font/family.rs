//! Family-name mapping to the bundled metric-compatible faces (S4F.2).

/// Maps a requested font family to its bundled metric-compatible equivalent.
///
/// Word documents request proprietary families (Calibri, Arial, …). The renderer
/// substitutes the bundled open fonts that share their metrics so that line
/// breaking matches the producer. Unknown families resolve to Carlito, and that
/// same name is what the SVG `@font-face` delivers.
#[must_use]
pub fn map_family(family: &str) -> &str {
    match family.trim().to_ascii_lowercase().as_str() {
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
        "cambria math" | "stix two math" | "stix2 math" | "stixgeneral" => "STIX Two Math",
        // An unknown CSS family is measured and delivered as Carlito, the bundled
        // Calibri-metric face. Leaving the request unchanged made layout use the
        // fallback model while the SVG named a face the viewer does not have.
        _ => "Carlito",
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
        assert_eq!(map_family("Cambria Math"), "STIX Two Math");
        assert_eq!(map_family("cambria math"), "STIX Two Math");
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
