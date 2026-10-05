//! Soft stand-ins for symbol faces the bundled fonts cannot draw.
//!
//! Word stores Symbol and Wingdings glyphs as private-use characters. The
//! renderer then paints them with Carlito, which has no glyph there, so the
//! marker becomes `.notdef`. Each row below is a private-use source whose
//! Unicode stand-in Carlito does contain. A character Carlito already has is
//! left alone, and a private-use character with no verified stand-in (the
//! Wingdings clock, for example) is left alone too.

use std::borrow::Cow;

/// Symbol private-use → Unicode, restricted to glyphs Carlito contains.
///
/// Source: the single-codepoint rows of Apple's `SYMBOL.TXT`. Rows whose
/// target is absent from Carlito, or is itself private-use, are omitted.
const SYMBOL: &[(char, char)] = &[
    ('\u{F020}', '\u{0020}'),
    ('\u{F021}', '\u{0021}'),
    ('\u{F023}', '\u{0023}'),
    ('\u{F025}', '\u{0025}'),
    ('\u{F026}', '\u{0026}'),
    ('\u{F028}', '\u{0028}'),
    ('\u{F029}', '\u{0029}'),
    ('\u{F02B}', '\u{002B}'),
    ('\u{F02C}', '\u{002C}'),
    ('\u{F02D}', '\u{2212}'),
    ('\u{F02E}', '\u{002E}'),
    ('\u{F02F}', '\u{002F}'),
    ('\u{F030}', '\u{0030}'),
    ('\u{F031}', '\u{0031}'),
    ('\u{F032}', '\u{0032}'),
    ('\u{F033}', '\u{0033}'),
    ('\u{F034}', '\u{0034}'),
    ('\u{F035}', '\u{0035}'),
    ('\u{F036}', '\u{0036}'),
    ('\u{F037}', '\u{0037}'),
    ('\u{F038}', '\u{0038}'),
    ('\u{F039}', '\u{0039}'),
    ('\u{F03A}', '\u{003A}'),
    ('\u{F03B}', '\u{003B}'),
    ('\u{F03C}', '\u{003C}'),
    ('\u{F03D}', '\u{003D}'),
    ('\u{F03E}', '\u{003E}'),
    ('\u{F03F}', '\u{003F}'),
    ('\u{F041}', '\u{0391}'),
    ('\u{F042}', '\u{0392}'),
    ('\u{F043}', '\u{03A7}'),
    ('\u{F044}', '\u{0394}'),
    ('\u{F045}', '\u{0395}'),
    ('\u{F046}', '\u{03A6}'),
    ('\u{F047}', '\u{0393}'),
    ('\u{F048}', '\u{0397}'),
    ('\u{F049}', '\u{0399}'),
    ('\u{F04A}', '\u{03D1}'),
    ('\u{F04B}', '\u{039A}'),
    ('\u{F04C}', '\u{039B}'),
    ('\u{F04D}', '\u{039C}'),
    ('\u{F04E}', '\u{039D}'),
    ('\u{F04F}', '\u{039F}'),
    ('\u{F050}', '\u{03A0}'),
    ('\u{F051}', '\u{0398}'),
    ('\u{F052}', '\u{03A1}'),
    ('\u{F053}', '\u{03A3}'),
    ('\u{F054}', '\u{03A4}'),
    ('\u{F055}', '\u{03A5}'),
    ('\u{F056}', '\u{03C2}'),
    ('\u{F057}', '\u{03A9}'),
    ('\u{F058}', '\u{039E}'),
    ('\u{F059}', '\u{03A8}'),
    ('\u{F05A}', '\u{0396}'),
    ('\u{F05B}', '\u{005B}'),
    ('\u{F05D}', '\u{005D}'),
    ('\u{F05F}', '\u{005F}'),
    ('\u{F061}', '\u{03B1}'),
    ('\u{F062}', '\u{03B2}'),
    ('\u{F063}', '\u{03C7}'),
    ('\u{F064}', '\u{03B4}'),
    ('\u{F065}', '\u{03B5}'),
    ('\u{F066}', '\u{03C6}'),
    ('\u{F067}', '\u{03B3}'),
    ('\u{F068}', '\u{03B7}'),
    ('\u{F069}', '\u{03B9}'),
    ('\u{F06A}', '\u{03D5}'),
    ('\u{F06B}', '\u{03BA}'),
    ('\u{F06C}', '\u{03BB}'),
    ('\u{F06D}', '\u{03BC}'),
    ('\u{F06E}', '\u{03BD}'),
    ('\u{F06F}', '\u{03BF}'),
    ('\u{F070}', '\u{03C0}'),
    ('\u{F071}', '\u{03B8}'),
    ('\u{F072}', '\u{03C1}'),
    ('\u{F073}', '\u{03C3}'),
    ('\u{F074}', '\u{03C4}'),
    ('\u{F075}', '\u{03C5}'),
    ('\u{F076}', '\u{03D6}'),
    ('\u{F077}', '\u{03C9}'),
    ('\u{F078}', '\u{03BE}'),
    ('\u{F079}', '\u{03C8}'),
    ('\u{F07A}', '\u{03B6}'),
    ('\u{F07B}', '\u{007B}'),
    ('\u{F07C}', '\u{007C}'),
    ('\u{F07D}', '\u{007D}'),
    ('\u{F0A0}', '\u{20AC}'),
    ('\u{F0A1}', '\u{03D2}'),
    ('\u{F0A3}', '\u{2264}'),
    ('\u{F0A4}', '\u{2044}'),
    ('\u{F0A5}', '\u{221E}'),
    ('\u{F0A6}', '\u{0192}'),
    ('\u{F0AB}', '\u{2194}'),
    ('\u{F0AC}', '\u{2190}'),
    ('\u{F0AD}', '\u{2191}'),
    ('\u{F0AE}', '\u{2192}'),
    ('\u{F0AF}', '\u{2193}'),
    ('\u{F0B0}', '\u{00B0}'),
    ('\u{F0B1}', '\u{00B1}'),
    ('\u{F0B2}', '\u{2033}'),
    ('\u{F0B3}', '\u{2265}'),
    ('\u{F0B4}', '\u{00D7}'),
    ('\u{F0B6}', '\u{2202}'),
    ('\u{F0B7}', '\u{2022}'),
    ('\u{F0B8}', '\u{00F7}'),
    ('\u{F0B9}', '\u{2260}'),
    ('\u{F0BA}', '\u{2261}'),
    ('\u{F0BB}', '\u{2248}'),
    ('\u{F0BC}', '\u{2026}'),
    ('\u{F0C7}', '\u{2229}'),
    ('\u{F0D2}', '\u{00AE}'),
    ('\u{F0D3}', '\u{00A9}'),
    ('\u{F0D4}', '\u{2122}'),
    ('\u{F0D5}', '\u{220F}'),
    ('\u{F0D6}', '\u{221A}'),
    ('\u{F0D8}', '\u{00AC}'),
    ('\u{F0E0}', '\u{25CA}'),
    ('\u{F0E5}', '\u{2211}'),
    ('\u{F0F2}', '\u{222B}'),
    ('\u{F0F3}', '\u{2320}'),
    ('\u{F0F5}', '\u{2321}'),
];

/// Wingdings private-use → Unicode, only for shapes checked against the font
/// and present in Carlito.
///
/// `U+F0B7` is a clock in Wingdings, so it is not in this table.
const WINGDINGS: &[(char, char)] = &[
    ('\u{F06C}', '\u{25CF}'),
    ('\u{F06F}', '\u{25A1}'),
    ('\u{F09F}', '\u{2022}'),
    ('\u{F0A7}', '\u{25AA}'),
];

/// Returns the text a bundled face can draw for a symbol font.
///
/// `family` is the requested face, before it is mapped onto a bundled family.
/// Measurement and painting must both use this string, so the marker width
/// matches the ink.
pub(crate) fn present_text<'a>(family: &str, text: &'a str) -> Cow<'a, str> {
    let Some(table) = symbol_table(family) else {
        return Cow::Borrowed(text);
    };
    if !text.chars().any(|ch| lookup(table, ch).is_some()) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .map(|ch| lookup(table, ch).unwrap_or(ch))
            .collect(),
    )
}

/// Selects the substitution table for a symbol face name.
fn symbol_table(family: &str) -> Option<&'static [(char, char)]> {
    match family.trim().to_ascii_lowercase().as_str() {
        "symbol" => Some(SYMBOL),
        "wingdings" => Some(WINGDINGS),
        _ => None,
    }
}

/// Looks up one private-use character in a sorted table.
fn lookup(table: &[(char, char)], ch: char) -> Option<char> {
    table
        .binary_search_by_key(&ch, |&(source, _)| source)
        .ok()
        .map(|index| table[index].1)
}

#[cfg(test)]
mod tests {
    use super::{present_text, SYMBOL, WINGDINGS};
    use crate::font::builtin::BuiltinFontProvider;
    use crate::font::FontProvider;

    #[test]
    fn symbol_bullet_becomes_a_bullet() {
        assert_eq!(present_text("Symbol", "\u{F0B7}"), "\u{2022}");
        assert_eq!(present_text(" symbol ", "\u{F0B7}"), "\u{2022}");
    }

    #[test]
    fn wingdings_square_becomes_a_square_and_the_clock_stays() {
        assert_eq!(present_text("Wingdings", "\u{F0A7}"), "\u{25AA}");
        assert_eq!(present_text("Wingdings", "\u{F0B7}"), "\u{F0B7}");
    }

    #[test]
    fn characters_the_paint_font_already_has_stay() {
        assert_eq!(present_text("Symbol", "o"), "o");
        assert_eq!(present_text("Symbol", "\u{2022}"), "\u{2022}");
        assert_eq!(present_text("Calibri", "\u{F0B7}"), "\u{F0B7}");
    }

    #[test]
    fn every_stand_in_is_in_carlito_and_every_source_is_not() {
        let font = BuiltinFontProvider::new();
        for &(source, target) in SYMBOL.iter().chain(WINGDINGS) {
            assert!(
                font.has_glyph("Carlito", target),
                "Carlito lacks the stand-in U+{:04X}",
                u32::from(target)
            );
            assert!(
                !font.has_glyph("Carlito", source),
                "Carlito already has U+{:04X}",
                u32::from(source)
            );
        }
    }
}
