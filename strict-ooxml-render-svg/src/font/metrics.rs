//! Font metrics abstraction and the deterministic character-advance fallback.
//!
//! Vertical metrics and real glyph advances come from the bundled
//! metric-compatible fonts via [`super::builtin::BuiltinFontProvider`]
//! (`S4F.1`). The per-character-class model here is only a **fallback**, used
//! for unknown families and characters without a glyph in the referenced face;
//! [`FontMetrics::advance_em`] implements it and [`char_width_ratio`] is its
//! pure ratio table.

/// Vertical and horizontal metrics of one font face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// Font design units per em.
    pub units_per_em: f64,
    /// Ascender as a fraction of the em.
    pub ascender: f64,
    /// Descender (positive value) as a fraction of the em.
    pub descender: f64,
    /// Line gap as a fraction of the em.
    pub line_gap: f64,
    /// Family scaling factor applied to advances.
    pub width_scale: f64,
}

impl Default for FontMetrics {
    fn default() -> Self {
        Self {
            units_per_em: 2048.0,
            ascender: 0.75,
            descender: 0.25,
            line_gap: 0.0,
            width_scale: 1.0,
        }
    }
}

impl FontMetrics {
    /// Returns the advance width of `ch` in em units (deterministic).
    #[must_use]
    pub fn advance_em(&self, ch: char, bold: bool) -> f64 {
        let base = char_width_ratio(ch);
        let weight = if bold { 0.02 } else { 0.0 };
        (base + weight) * self.width_scale
    }

    /// Returns the default line height in em units.
    #[must_use]
    pub fn line_height_em(&self) -> f64 {
        self.ascender + self.descender + self.line_gap
    }

    /// Returns the ascent in em units.
    #[must_use]
    pub fn ascent_em(&self) -> f64 {
        self.ascender
    }
}

/// Returns `true` for CJK ideographs and full-width forms (advance 1 em).
#[must_use]
pub fn is_wide_character(ch: char) -> bool {
    // `char as u32` is the definition of a code point, not a narrowing of a
    // number this crate computed from input (AUD-09''s G-2 audit).
    matches!(ch as u32,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFD)
}

/// Pure, deterministic character-advance ratio (fraction of the em).
#[must_use]
pub fn char_width_ratio(ch: char) -> f64 {
    match ch {
        ' ' | '\u{00A0}' => 0.25,
        'i' | 'l' | 'j' | 'I' | '!' | '.' | ',' | ';' | ':' | '\'' | '|' => 0.28,
        'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '"' => 0.33,
        'm' | 'M' | 'w' | 'W' => 0.85,
        'A'..='Z' => 0.62,
        '0'..='9' => 0.5,
        ch if is_wide_character(ch) => 1.0,
        ch if ch.is_ascii() => 0.5,
        _ => 0.52,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{char_width_ratio, is_wide_character, FontMetrics};

    #[test]
    fn advance_is_deterministic_and_positive() {
        let metrics = FontMetrics::default();
        assert!(metrics.advance_em('a', false) > 0.0);
        assert!(metrics.advance_em('a', true) > metrics.advance_em('a', false));
        assert!(metrics.line_height_em() > 0.0);
        assert!(metrics.ascent_em() > 0.0);
    }

    #[test]
    fn character_classes() {
        assert!(char_width_ratio('i') < char_width_ratio('a'));
        assert!(char_width_ratio('M') > char_width_ratio('a'));
        assert!(is_wide_character('\u{4E00}'));
        assert!(!is_wide_character('a'));
        assert_eq!(char_width_ratio(' '), 0.25);
    }
}
