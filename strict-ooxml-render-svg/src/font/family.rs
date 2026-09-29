//! Family-name mapping to the bundled metric-compatible faces (S4F.2).

/// Maps a requested font family to its bundled metric-compatible equivalent.
///
/// Word documents request proprietary families (Calibri, Arial, …). The renderer
/// substitutes the bundled open fonts that share their metrics so that line
/// breaking matches the producer; the same name is emitted in the SVG
/// `font-family` so the rasterizer resolves the bundled face rather than a
/// system fallback. Unknown families are returned unchanged and resolved by the
/// deterministic fallback model.
#[must_use]
pub fn map_family(family: &str) -> &str {
    match family.trim().to_ascii_lowercase().as_str() {
        "calibri" => "Carlito",
        "cambria" => "Caladea",
        // LibreOffice writes its own metric-compatible Liberation families;
        // the bundled Arimo/Tinos/Cousine are the same designs under the OFL
        // names (STAGE-5C-REWORK-1 D1).
        "arial" | "helvetica" | "liberation sans" => "Arimo",
        "times new roman" | "liberation serif" => "Tinos",
        "courier new" | "liberation mono" => "Cousine",
        // The math family of Word documents (Stage 5C, §9.1): STIX Two Math
        // is the bundled OFL face with the full OMML glyph repertoire.
        "cambria math" | "stix two math" | "stix2 math" | "stixgeneral" => "STIX Two Math",
        _ => family,
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
    fn leaves_unknown_families_unchanged() {
        assert_eq!(map_family("Segoe UI"), "Segoe UI");
        assert_eq!(map_family(""), "");
    }
}
