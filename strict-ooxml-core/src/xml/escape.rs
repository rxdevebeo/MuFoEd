//! XML escaping for every serializer in the workspace (`REWORK-AUDIT-2026-10.md`
//! AUD-03, G-5).
//!
//! One module, because six hand-written copies had drifted apart and none of
//! them knew that XML 1.0 forbids most C0 controls outright: a `w:sym` of
//! `0001` or a run with `U+0001` in it came out as a part, a page or a document
//! that no XML parser would open. A character outside the `Char` production
//! cannot be written at all, not even as a character reference, so it is
//! **removed**, and the functions return how many were, for the caller to put
//! in its report.

/// Whether `c` is a `Char` of XML 1.0 (§2.2):
/// `#x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`.
///
/// Surrogates cannot occur in a `char`, so the middle range needs no hole.
#[must_use]
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{FFFD}' | '\u{10000}'..)
        && !matches!(c, '\u{FFFE}' | '\u{FFFF}')
}

/// Appends `value` escaped as character data and returns the number of
/// characters removed because XML cannot carry them.
///
/// `&`, `<` and `>` become entity references. `\r` becomes `&#13;`, because a
/// literal CR is normalized to LF by every conforming parser; `\t` and `\n` are
/// written as they are.
pub fn escape_text_into(out: &mut String, value: &str) -> usize {
    let mut removed = 0;
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            _ if is_xml_char(ch) => out.push(ch),
            _ => removed += 1,
        }
    }
    removed
}

/// Appends `value` escaped for an attribute delimited by `"` or `'` and
/// returns the number of characters removed.
///
/// Besides the five predefined entities, `\t`, `\n` and `\r` become character
/// references: attribute-value normalization would turn the literal characters
/// into spaces.
pub fn escape_attr_into(out: &mut String, value: &str) -> usize {
    let mut removed = 0;
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ if is_xml_char(ch) => out.push(ch),
            _ => removed += 1,
        }
    }
    removed
}

/// [`escape_text_into`] into a new string; the count is discarded.
#[must_use]
pub fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    escape_text_into(&mut out, value);
    out
}

/// [`escape_attr_into`] into a new string; the count is discarded.
#[must_use]
pub fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    escape_attr_into(&mut out, value);
    out
}

/// The number of characters in `value` that [`escape_text_into`] and
/// [`escape_attr_into`] would remove.
#[must_use]
pub fn count_invalid(value: &str) -> usize {
    value.chars().filter(|&c| !is_xml_char(c)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use quick_xml::events::Event;
    use quick_xml::Reader;

    fn parses(document: &str) -> bool {
        let mut reader = Reader::from_str(document);
        loop {
            match reader.read_event() {
                Ok(Event::Eof) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
    }

    #[test]
    fn malformed_markup_is_not_well_formed() {
        // Exercises the `Err(_)` arm of [`parses`] so the helper is fully covered.
        assert!(!parses("<a><b></a>"));
        assert!(!parses("<a>&</a>"));
    }

    #[test]
    fn every_forbidden_control_is_removed() {
        for code in (0u32..0x20).filter(|c| ![0x9, 0xA, 0xD].contains(c)) {
            let ch = char::from_u32(code).unwrap();
            assert!(!is_xml_char(ch), "U+{code:04X}");
            let mut out = String::new();
            assert_eq!(escape_text_into(&mut out, &format!("a{ch}b")), 1);
            assert_eq!(out, "ab");
            let mut out = String::new();
            assert_eq!(escape_attr_into(&mut out, &format!("a{ch}b")), 1);
            assert_eq!(out, "ab");
        }
        for ch in ['\u{FFFE}', '\u{FFFF}'] {
            assert!(!is_xml_char(ch));
            assert_eq!(count_invalid(&format!("x{ch}")), 1);
        }
    }

    #[test]
    fn the_edges_of_the_char_production_are_kept() {
        for ch in [
            '\t',
            '\n',
            '\r',
            ' ',
            '\u{D7FF}',
            '\u{E000}',
            '\u{FFFD}',
            '\u{10000}',
            '\u{10FFFF}',
        ] {
            assert!(is_xml_char(ch), "{ch:?}");
        }
        assert!(!is_xml_char('\u{7}'));
    }

    #[test]
    fn text_escaping() {
        let mut out = String::new();
        assert_eq!(escape_text_into(&mut out, "a<b>&c\r\n\td"), 0);
        assert_eq!(out, "a&lt;b&gt;&amp;c&#13;\n\td");
    }

    #[test]
    fn attribute_escaping() {
        let mut out = String::new();
        assert_eq!(escape_attr_into(&mut out, "q\"x'y\tz\n\r<&>"), 0);
        assert_eq!(out, "q&quot;x&apos;y&#9;z&#10;&#13;&lt;&amp;&gt;");
    }

    proptest! {
        #[test]
        fn escaped_text_is_well_formed_and_round_trips(value in any::<String>()) {
            let escaped = escape_text(&value);
            let document = format!("<a>{escaped}</a>");
            prop_assert!(parses(&document));
            let expected: String = value.chars().filter(|&c| is_xml_char(c)).collect();
            prop_assert_eq!(quick_xml::escape::unescape(&escaped).unwrap(), expected);
        }

        #[test]
        fn escaped_attributes_are_well_formed_and_round_trip(value in any::<String>()) {
            let escaped = escape_attr(&value);
            let double = format!("<a v=\"{escaped}\"/>");
            let single = format!("<a v='{escaped}'/>");
            prop_assert!(parses(&double));
            prop_assert!(parses(&single));
            let expected: String = value.chars().filter(|&c| is_xml_char(c)).collect();
            prop_assert_eq!(quick_xml::escape::unescape(&escaped).unwrap(), expected);
        }
    }
}
