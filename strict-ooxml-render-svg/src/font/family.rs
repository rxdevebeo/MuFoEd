//! Family-name mapping to the bundled metric-compatible faces (S4F.2).

/// RTF/Word script qualifiers appended to a base family name.
///
/// Word and WPS export one font-table entry per char-set under names such as
/// `Times New Roman Cyr`, `Arial CE`, `Calibri (Hebrew)`. They name the same
/// design, so the qualifier must be removed before the family is mapped — an
/// unnormalized `Times New Roman Cyr` otherwise falls through to the Calibri
/// (Carlito) wildcard and the whole document is measured with the wrong face,
/// shifting both line pitch and advances.
const SCRIPT_SUFFIXES: [&str; 15] = [
    "cyr",
    "cyrillic",
    "ce",
    "baltic",
    "greek",
    "tur",
    "turkish",
    "latin",
    "hebrew",
    "arabic",
    "vietnamese",
    "thai",
    "devanagari",
    "complex",
    "central european",
];

/// Strips a trailing `(...)` script tag and a bare script suffix word.
///
/// Only the final qualifier is removed, and only when it is a whole trailing
/// token, so a base name that merely contains those letters (`Courier`) is left
/// alone.
fn base_family(family: &str) -> String {
    let mut name = family.trim().to_ascii_lowercase();
    if name.ends_with(')') {
        if let Some(open) = name.rfind(" (") {
            name.truncate(open);
        }
    }
    for suffix in SCRIPT_SUFFIXES {
        if let Some(stripped) = name.strip_suffix(&format!(" {suffix}")) {
            return stripped.to_string();
        }
    }
    name
}

/// Maps a requested font family to its bundled metric-compatible equivalent.
///
/// Word documents request proprietary families (Calibri, Arial, …). The renderer
/// substitutes the bundled open fonts that share their metrics so that line
/// breaking matches the producer. Unknown families resolve to Carlito, and that
/// same name is what the SVG `@font-face` delivers.
#[must_use]
pub fn map_family(family: &str) -> &'static str {
    match base_family(family).as_str() {
        // Calibri is the wildcard below: it is the family Carlito metrics match.
        "cambria" | "caladea" => "Caladea",
        // LibreOffice writes its own metric-compatible Liberation families;
        // the bundled Arimo/Tinos/Cousine are the same designs under the OFL
        // names (STAGE-5C-REWORK-1 D1).
        // Regular PostScript names used by Cocoa RTF producers (name ID 6).
        // Font style still comes from the run's bold/italic properties.
        "arial" | "arialmt" | "helvetica" | "helvetica neue" | "helveticaneue" | "verdana"
        | "tahoma" | "liberation sans" | "arimo" => "Arimo",
        "times new roman" | "timesnewromanpsmt" | "liberation serif" | "tinos" => "Tinos",
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
    fn maps_the_common_sans_families_to_arimo() {
        // WPS renders Verdana at the Arial-metric line height (072 line pitch
        // 16.1pt = 1.15em x 1.1667); the Calibri-metric wildcard gave 17.1pt.
        assert_eq!(map_family("Verdana"), "Arimo");
        assert_eq!(map_family("Tahoma"), "Arimo");
        assert_eq!(map_family("Helvetica Neue"), "Arimo");
        assert_eq!(map_family("HelveticaNeue"), "Arimo");
    }

    #[test]
    fn strips_script_qualifiers_before_mapping() {
        // Word/WPS export one entry per char-set; these name the same design.
        assert_eq!(map_family("Times New Roman Cyr"), "Tinos");
        assert_eq!(map_family("Times New Roman CE"), "Tinos");
        assert_eq!(map_family("Times New Roman Baltic"), "Tinos");
        assert_eq!(map_family("Arial CYR"), "Arimo");
        assert_eq!(map_family("Arial Greek"), "Arimo");
        assert_eq!(map_family("Arial (Hebrew)"), "Arimo");
        assert_eq!(map_family("Cambria (Vietnamese)"), "Caladea");
        assert_eq!(map_family("Cambria Math Cyr"), "STIX Two Math");
        assert_eq!(map_family("Courier New Tur"), "Cousine");
        // The qualifier is only stripped from the end; a base name containing
        // the same letters is untouched.
        assert_eq!(map_family("Courier"), "Carlito");
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

    #[test]
    fn maps_regular_postscript_names_to_the_same_bundled_faces() {
        assert_eq!(map_family("ArialMT"), map_family("Arial"));
        assert_eq!(map_family("  arialmt "), map_family("Arial"));
        assert_eq!(
            map_family("TimesNewRomanPSMT"),
            map_family("Times New Roman")
        );
        // Styled PostScript names need their weight/slant resolved as well;
        // a family-only alias must not silently discard those properties.
        assert_eq!(map_family("Arial-BoldMT"), "Arial-BoldMT");
    }
}
