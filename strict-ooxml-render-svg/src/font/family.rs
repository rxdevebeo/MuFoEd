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
        "arial" | "helvetica" => "Arimo",
        "times new roman" => "Tinos",
        "courier new" => "Cousine",
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
    fn leaves_unknown_families_unchanged() {
        assert_eq!(map_family("Segoe UI"), "Segoe UI");
        assert_eq!(map_family(""), "");
    }
}
