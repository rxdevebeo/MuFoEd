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

/// Script qualifiers RTF and Word append to a family name (`Times New Roman
/// Cyr`, `Arial CE`, `Calibri (Hebrew)`): the same design, which otherwise
/// fell to the Carlito wildcard (RTF RC04, doc 054; restored after merge
/// 5274d07f dropped it).
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

/// A trimmed, lower-cased name without one trailing `(...)` tag or one
/// trailing script word.
fn base_family(name: &str) -> &str {
    if name.ends_with(')') {
        if let Some(base) = name.rfind(" (").and_then(|open| name.get(..open)) {
            return base;
        }
    }
    for suffix in SCRIPT_SUFFIXES {
        if let Some(stripped) = name
            .strip_suffix(suffix)
            .and_then(|rest| rest.strip_suffix(' '))
        {
            return stripped;
        }
    }
    name
}

/// Returns a bundled face only for a recognized family or alias.
#[must_use]
pub fn known_family(family: &str) -> Option<&'static str> {
    let name = family.trim().to_ascii_lowercase();
    // The exact name first: the script faces ("Arimo Fallback", "Noto Sans
    // Arabic") end in a qualifier word and map to themselves.
    exact_family(&name).or_else(|| {
        let base = base_family(&name);
        (base != name).then(|| exact_family(base)).flatten()
    })
}

fn exact_family(name: &str) -> Option<&'static str> {
    Some(match name {
        "calibri" | "carlito" => "Carlito",
        // Calibri is the wildcard below: it is the family Carlito metrics match.
        "cambria" | "caladea" => "Caladea",
        // LibreOffice writes its own metric-compatible Liberation families;
        // the bundled Arimo/Tinos/Cousine are the same designs under the OFL
        // names (STAGE-5C-REWORK-1 D1).
        // Regular PostScript names used by Cocoa RTF producers (name ID 6).
        // Font style still comes from the run's bold/italic properties.
        // Helvetica Neue: WPS sets it at Arial-metric line height (RTF 072:
        // 16.1 pt against Carlito's 17.1 pt). Verdana and Tahoma have metric
        // profiles of their own (`profiles`).
        "arial" | "arialmt" | "helvetica" | "helvetica neue" | "helveticaneue"
        | "liberation sans" | "arimo" => "Arimo",
        "times new roman" | "timesnewromanpsmt" | "liberation serif" | "tinos" => "Tinos",
        "courier new" | "liberation mono" | "cousine" => "Cousine",
        // Gelasio and Selawik are drawn to Georgia's and Segoe UI's metrics;
        // neither has Cyrillic or full Greek, which [`script_fallback`] takes
        // from Tinos and Arimo.
        "georgia" | "gelasio" => "Gelasio",
        "segoe ui" | "segoeui" | "selawik" => "Selawik",
        "segoe ui light" | "segoeui-light" | "selawik light" => "Selawik Light",
        "segoe ui semilight" | "segoeui-semilight" | "selawik semilight" => "Selawik Semilight",
        "segoe ui semibold" | "segoeui-semibold" | "selawik semibold" => "Selawik Semibold",
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
        "jomolhari" => "Jomolhari",
        "arimo fallback" => "Arimo Fallback",
        "tinos fallback" => "Tinos Fallback",
        // A family with a metric profile is drawn by a bundled face at the
        // family's own advances and line box (`profiles`). An unknown CSS
        // family is measured and delivered as Carlito, the bundled
        // Calibri-metric face. Leaving the request unchanged made layout use
        // the fallback model while the SVG named a face the viewer does not have.
        _ => return super::profiles::for_name(name).map(|profile| profile.family),
    })
}

/// Bundled face that draws `ch` when the `base` face has no glyph for it.
///
/// Carlito, Arimo, Tinos and Cousine cover Latin, Greek and Cyrillic; Arimo,
/// Tinos and Cousine cover Hebrew too, Carlito and Caladea do not, though the
/// Calibri and Cambria they stand for do. Caladea covers Latin only, so the
/// Greek and Cyrillic of Cambria - every Russian heading in Word's default
/// theme - come from Tinos, as the Hebrew does. Word and WPS draw Arabic and Tibetan
/// from a font that has them (WQ-004), and Hebrew in Calibri from Calibri:
/// here it comes from Arimo's or Tinos' Hebrew. Gelasio and Selawik (Georgia,
/// Segoe UI) cover Latin only: the rest of the Latin-Greek-Cyrillic range
/// comes from Tinos and Arimo, under their fallback names.
/// Arabic next to a sans face (Arial, Tahoma via Arimo) takes the sans
/// Arabic design, otherwise the Naskh one, which is the style of Times New
/// Roman's and Traditional Arabic's Arabic. Urdu set in a Nastaliq family
/// that is not installed comes out in Naskh too, as WPS draws it: a Nastaliq
/// face needs a line box twice as tall, and the requested font's box is unknown.
#[must_use]
pub fn script_fallback(base: &str, ch: char) -> Option<&'static str> {
    // A profiled family falls back the way the face that draws it does.
    let base = super::profiles::for_family(base).map_or(base, |profile| profile.substitute);
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
        '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}' => Some(if serif(base) {
            "Tinos Fallback"
        } else {
            "Arimo Fallback"
        }),
        '\u{0000}'..='\u{052F}' | '\u{1E00}'..='\u{1FFF}' | '\u{2000}'..='\u{2BFF}'
            if base == "Gelasio" || base == "Caladea" =>
        {
            Some("Tinos Fallback")
        }
        '\u{0000}'..='\u{052F}' | '\u{1E00}'..='\u{1FFF}' | '\u{2000}'..='\u{2BFF}'
            if base.starts_with("Selawik") =>
        {
            Some("Arimo Fallback")
        }
        _ => None,
    }
}

/// Whether a fallback for `base` takes the serif design.
fn serif(base: &str) -> bool {
    matches!(base, "Caladea" | "Gelasio")
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
        "Noto Naskh Arabic"
            | "Noto Sans Arabic"
            | "Jomolhari"
            | "Arimo Fallback"
            | "Tinos Fallback"
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
        // Cambria's Cyrillic and Greek: Caladea has neither.
        assert_eq!(script_fallback("Caladea", 'Ж'), Some("Tinos Fallback"));
        assert_eq!(script_fallback("Caladea", 'Ω'), Some("Tinos Fallback"));
        assert!(!crate::font::builtin::bundled_has_glyph("Caladea", 'Ж'));
        assert_eq!(script_fallback("Carlito", 'a'), None);
        // Hebrew: offered for any face, taken only where the face lacks it
        // (`chosen_family` checks the glyph): Carlito and Caladea, not Tinos.
        assert_eq!(script_fallback("Carlito", 'א'), Some("Arimo Fallback"));
        assert_eq!(script_fallback("Caladea", 'א'), Some("Tinos Fallback"));
        assert!(crate::font::builtin::bundled_has_glyph("Tinos", 'א'));
        assert!(!crate::font::builtin::bundled_has_glyph("Carlito", 'א'));
        assert!(is_script_fallback("Jomolhari"));
        assert!(!is_script_fallback("Tinos"));
        for family in [
            "Noto Naskh Arabic",
            "Noto Sans Arabic",
            "Jomolhari",
            "Arimo Fallback",
            "Tinos Fallback",
        ] {
            assert_eq!(map_family(family), family);
            assert!(is_script_fallback(family));
        }
        assert_eq!(super::known_family("Jameel Noori Nastaleeq"), None);
    }

    #[test]
    fn unknown_families_use_the_bundled_fallback() {
        assert_eq!(map_family("Segoe UI Black"), "Carlito");
        // A metric profile answers to its own delivered name too.
        assert_eq!(map_family("Verdana"), "Arimo for Verdana");
        assert_eq!(map_family("Arimo for Verdana"), "Arimo for Verdana");
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
        assert_eq!(super::known_family("Arial-BoldMT"), None);
    }

    #[test]
    fn maps_the_common_sans_families_to_arimo() {
        for family in ["Helvetica Neue", "HelveticaNeue"] {
            assert_eq!(map_family(family), "Arimo", "{family}");
        }
    }

    #[test]
    fn maps_georgia_and_segoe_ui_to_their_metric_twins() {
        assert_eq!(map_family("Georgia"), "Gelasio");
        assert_eq!(map_family("Georgia Cyr"), "Gelasio");
        assert_eq!(map_family("Segoe UI"), "Selawik");
        assert_eq!(map_family("Segoe UI Semibold"), "Selawik Semibold");
        assert_eq!(map_family("Segoe UI Light"), "Selawik Light");
        assert_eq!(map_family("Segoe UI Semilight"), "Selawik Semilight");
        for family in [
            "Gelasio",
            "Selawik",
            "Selawik Light",
            "Selawik Semilight",
            "Selawik Semibold",
        ] {
            assert_eq!(map_family(family), family);
            assert!(crate::font::builtin::bundled_has_glyph(family, 'a'));
        }
    }

    #[test]
    fn georgia_and_segoe_ui_keep_their_own_line_box() {
        use crate::font::FontProvider as _;
        let provider = crate::font::builtin::BuiltinFontProvider::new();
        let georgia = provider.metrics("Georgia", false, false);
        assert!((georgia.line_height_em() - 2327.0 / 2048.0).abs() < 1e-9);
        let segoe = provider.metrics("Segoe UI", true, false);
        assert!((segoe.line_height_em() - 2724.0 / 2048.0).abs() < 1e-9);
    }

    #[test]
    fn latin_only_faces_take_cyrillic_and_greek_from_tinos_and_arimo() {
        use super::script_fallback;
        use crate::font::builtin::bundled_has_glyph;
        for (base, fallback) in [
            ("Gelasio", "Tinos Fallback"),
            ("Selawik", "Arimo Fallback"),
            ("Selawik Semibold", "Arimo Fallback"),
        ] {
            assert!(!bundled_has_glyph(base, 'Ж'), "{base}");
            assert_eq!(script_fallback(base, 'Ж'), Some(fallback));
            assert_eq!(script_fallback(base, 'א'), Some(fallback));
            assert!(bundled_has_glyph(fallback, 'Ж'));
        }
        // Offered for Latin too, but taken only where the glyph is missing.
        assert!(bundled_has_glyph("Gelasio", 'é'));
        // Tinos has no CJK either: no point in offering it.
        assert_eq!(script_fallback("Gelasio", '中'), None);
        assert_eq!(script_fallback("Tinos", 'Ж'), None);
    }

    #[test]
    fn strips_script_qualifiers_before_mapping() {
        assert_eq!(map_family("Times New Roman Cyr"), "Tinos");
        assert_eq!(map_family("Times New Roman CE"), "Tinos");
        assert_eq!(map_family("Times New Roman Baltic"), "Tinos");
        assert_eq!(map_family("Arial CYR"), "Arimo");
        assert_eq!(map_family("Arial Greek"), "Arimo");
        assert_eq!(map_family("Arial (Hebrew)"), "Arimo");
        assert_eq!(map_family("Cambria (Vietnamese)"), "Caladea");
        assert_eq!(map_family("Courier New Tur"), "Cousine");
        // A qualifier is not a family: an unknown base stays unknown.
        assert_eq!(super::known_family("Courier"), None);
        // The script faces keep their own names.
        for family in [
            "Arimo Fallback",
            "Tinos Fallback",
            "Noto Sans Arabic",
            "Noto Naskh Arabic",
        ] {
            assert_eq!(map_family(family), family);
        }
    }
}
