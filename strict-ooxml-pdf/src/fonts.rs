//! Font dictionaries: how a byte in a string becomes a character and an advance.
//!
//! Three things have to agree before a glyph can be placed, and each has its own
//! fallback chain in the specification:
//!
//! 1. **Which character a code stands for.** The `ToUnicode` map first, then the
//!    font's encoding (`/Differences` over a base encoding), then the glyph name.
//!    This is what makes a subset font readable instead of a row of boxes.
//! 2. **How wide it is.** `/Widths` for a simple font, `/W` for a CID font, both
//!    in 1000-unit text space; the standard-14 metrics are the last resort.
//! 3. **How many bytes a code takes.** Two for a `Type0` font with a
//!    two-byte CMap, one otherwise — and getting that wrong shifts every
//!    subsequent glyph on the line.
//!
//! The width fallbacks are approximate and say so: a PDF may omit `/Widths`
//! entirely for a non-embedded standard-14 font, and there is no table in this
//! crate for the AFM metrics of those faces. A missing width becomes
//! [`Width::Estimated`] so a caller can see which advances are guesses.

use std::collections::BTreeMap;

use encoding_rs::{Encoding, MACINTOSH, UTF_16LE, WINDOWS_1252};
use lopdf::Object;

use crate::error::{LimitKind, PdfLimits, Result};

/// A base encoding of a simple font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BaseEncoding {
    /// The font does not name one.
    #[default]
    Standard,
    /// `/WinAnsiEncoding` — Windows-1252.
    WinAnsi,
    /// `/MacRomanEncoding`.
    MacRoman,
    /// A named encoding this reader does not know; the caller falls back to
    /// Latin-1 and then to the glyph name.
    Other,
}

impl BaseEncoding {
    fn from_name(name: &[u8]) -> Self {
        match name {
            b"WinAnsiEncoding" => Self::WinAnsi,
            b"MacRomanEncoding" => Self::MacRoman,
            b"StandardEncoding" => Self::Standard,
            _ => Self::Other,
        }
    }

    /// Decodes one byte through this encoding.
    ///
    /// `StandardEncoding` has no single-byte table that agrees with what
    /// producers emit (it differs from Latin-1 in the upper half), so the
    /// pragmatic choice is Latin-1, which is right for the ASCII range and for
    /// the accented letters a real document mostly uses. A character this gets
    /// wrong is also reachable through `ToUnicode` in a well-formed file.
    pub fn decode_byte(self, byte: u8) -> Option<char> {
        match self {
            Self::WinAnsi => decode_windows_1252(byte),
            // `encoding_rs` carries MacRoman, so there is no hand-written table
            // here to get wrong.
            Self::MacRoman => decode_with(MACINTOSH, byte),
            // Latin-1 for `StandardEncoding` and for a named encoding this
            // reader does not know: the ASCII range and the accented letters a
            // real document mostly uses are right, and anything else is
            // reachable through `ToUnicode` in a well-formed file.
            Self::Standard | Self::Other => char::from_u32(u32::from(byte)),
        }
    }
}

fn decode_windows_1252(byte: u8) -> Option<char> {
    decode_with(WINDOWS_1252, byte)
}

/// Decodes one byte through a single-byte encoding.
fn decode_with(encoding: &'static Encoding, byte: u8) -> Option<char> {
    let bytes = [byte];
    let (text, _, _) = encoding.decode(&bytes);
    text.chars().next()
}

/// Where an advance width came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Width {
    /// The font stated it.
    Stated(f64),
    /// A default was assumed: 500/1000 em, the conventional average width.
    Estimated,
}

impl Width {
    /// The width in 1000-unit text space.
    #[must_use]
    pub fn value(self) -> f64 {
        match self {
            Self::Stated(value) => value,
            Self::Estimated => 500.0,
        }
    }

    /// Returns `true` when the width is a guess.
    #[must_use]
    pub const fn is_estimated(self) -> bool {
        matches!(self, Self::Estimated)
    }
}

/// A font, resolved far enough to decode and place text.
///
/// The four booleans are four different statements the PDF makes about a font —
/// is it composite, did the producer name the encoding, did it state a default
/// width, is the program embedded — and folding them into flags would make the
/// reader's questions unanswerable rather than the struct tidier.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct PdfFont {
    /// The resource name, for diagnostics.
    pub name: String,
    /// Whether a code is two bytes wide.
    pub two_byte: bool,
    /// The base encoding of a simple font.
    pub base: BaseEncoding,
    /// Whether the producer **named** the base encoding (`/Encoding
    /// /WinAnsiEncoding`).
    ///
    /// This is the difference between a mapping the document states and one this
    /// reader supplies. A character that came out of `/ToUnicode` or
    /// `/Differences` is stated; a character that came out of a *named* encoding is
    /// stated too, because the producer said which table to use. A character that
    /// came out of the fallback — no `/Encoding` at all, or `StandardEncoding`,
    /// whose upper half this reader approximates with Latin-1 — is a guess, and
    /// `Glyph::mapped` says so.
    pub encoding_stated: bool,
    /// `/Differences`: code → glyph name.
    pub differences: BTreeMap<u8, String>,
    /// `ToUnicode`: code → character.
    pub to_unicode: BTreeMap<u32, char>,
    /// `/Widths` or `/W`, code → 1000-unit width.
    pub widths: BTreeMap<u32, f64>,
    /// The default width from `/DW` for a CID font.
    ///
    /// The specification's fallback when `/DW` is absent is one em, which is a
    /// guess: `Width::Estimated` says so, and a caller that positions text by
    /// advance can then treat the number as provisional rather than declared.
    pub default_width: f64,
    /// Whether the font actually stated `/DW`.
    pub default_width_stated: bool,
    /// Whether the font is embedded.
    pub embedded: bool,
    /// The base font name, for diagnostics.
    pub base_font: String,
    /// `/Ascent` in 1000-unit text space, when the font states one.
    pub ascent: Option<f64>,
    /// `/Descent` in 1000-unit text space, when the font states one.
    pub descent: Option<f64>,
}

impl PdfFont {
    /// Builds a font from a resource dictionary.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::LimitExceeded`](crate::error::PdfError::LimitExceeded)
    /// when a font records more codes than the budget allows.
    pub fn build(
        name: &str,
        dictionary: &lopdf::Dictionary,
        document: &lopdf::Document,
        limits: &PdfLimits,
    ) -> Result<Self> {
        let resolve = resolver(document);
        let subtype = name_of(dictionary, b"Subtype", resolve);
        let descendant: Option<lopdf::Dictionary> = dictionary
            .get(b"DescendantFonts")
            .ok()
            .and_then(|value| array_items(value, resolve).into_iter().next())
            .and_then(|item| resolve.get(&item).cloned())
            .and_then(|item| item.as_dict().ok().cloned());
        let two_byte = subtype.as_deref() == Some(b"Type0");

        let mut font = Self {
            name: name.to_owned(),
            two_byte,
            base: BaseEncoding::Standard,
            encoding_stated: false,
            differences: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
            widths: BTreeMap::new(),
            default_width: 1000.0,
            default_width_stated: false,
            embedded: false,
            base_font: String::new(),
            ascent: None,
            descent: None,
        };

        if two_byte {
            // A composite font's encoding is a CMap; only the two-byte case is
            // read here, which is what every real document uses.
            if let Ok(encoding) = dictionary.get(b"Encoding") {
                if let Some(bytes) = stream_bytes(encoding, resolve) {
                    if bytes
                        .windows(2)
                        .any(|pair| pair == b"H\x00" || pair == b"V\x00")
                    {
                        font.two_byte = true;
                    } else if bytes
                        .windows(2)
                        .any(|pair| pair == b"h\x00" || pair == b"v\x00")
                    {
                        // `Identity-V`: vertical writing with two-byte codes.
                        font.two_byte = true;
                    }
                }
            }
            // A composite font normally has exactly one descendant; when the
            // entry is missing or empty, the font dictionary itself is the
            // closest thing to a CID font there is.
            let owned = dictionary.clone();
            let descendant: &lopdf::Dictionary = descendant.as_ref().unwrap_or(&owned);
            match number_entry(descendant, b"DW", resolve) {
                Some(width) if width > 0.0 => {
                    font.default_width = width;
                    font.default_width_stated = true;
                }
                _ => {
                    // No `/DW`, or a nonsensical one: the fallback is one em
                    // and it is a *guess*, not a number the document committed
                    // to. Reporting it as declared is how a wrong advance
                    // becomes indistinguishable from a right one.
                    font.default_width = 1000.0;
                    font.default_width_stated = false;
                }
            }
            read_cid_widths(descendant, resolve, &mut font.widths);
            font.base_font =
                string_of(descendant.get(b"BaseFont").ok(), resolve).unwrap_or_default();
            font.embedded = descendant.get(b"FontDescriptor").is_ok();
        } else {
            read_simple_font(dictionary, resolve, &mut font);
        }

        // The descriptor of a composite font hangs off the CID font, not off the
        // `Type0` wrapper.
        let owned = dictionary.clone();
        let wrapper = dictionary.clone();
        let descriptor_source: &lopdf::Dictionary = if two_byte {
            descendant.as_ref().unwrap_or(&owned)
        } else {
            &wrapper
        };
        if let Ok(descriptor) = descriptor_source.get(b"FontDescriptor") {
            let inner = resolve
                .get(descriptor)
                .and_then(|value| value.as_dict().ok())
                .cloned()
                .unwrap_or_else(|| descriptor_source.clone());
            font.ascent = number_entry(&inner, b"Ascent", resolve);
            font.descent = number_entry(&inner, b"Descent", resolve);
        }

        if let Ok(to_unicode) = dictionary.get(b"ToUnicode") {
            if let Some(bytes) = stream_bytes(to_unicode, resolve) {
                read_to_unicode(&bytes, &mut font.to_unicode, limits)?;
            }
        }
        if font.to_unicode.len() > limits.max_font_glyphs {
            return Err(limits.exceeded(LimitKind::FontGlyphs, font.to_unicode.len() as u64));
        }
        Ok(font)
    }

    /// Splits a PDF string into its character codes.
    #[must_use]
    pub fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        let mut out = Vec::with_capacity(bytes.len() / if self.two_byte { 2 } else { 1 });
        if self.two_byte {
            // `as_chunks` rather than `chunks_exact(2)`: same semantics (a
            // trailing odd byte is ignored) and it says the chunk size once.
            for pair in bytes.as_chunks::<2>().0 {
                out.push(u32::from(u16::from_be_bytes([pair[0], pair[1]])));
            }
        } else {
            out.extend(bytes.iter().map(|byte| u32::from(*byte)));
        }
        out
    }

    /// The character a code stands for.
    #[must_use]
    pub fn character(&self, code: u32) -> Option<char> {
        if let Some(ch) = self.to_unicode.get(&code) {
            return Some(*ch);
        }
        let byte = u8::try_from(code).ok()?;
        if let Some(glyph) = self.differences.get(&byte) {
            if let Some(ch) = glyph_name_to_char(glyph) {
                return Some(ch);
            }
        }
        self.base.decode_byte(byte)
    }

    /// The advance of a code in 1000-unit text space.
    #[must_use]
    pub fn width(&self, code: u32) -> Width {
        match self.widths.get(&code) {
            Some(width) => Width::Stated(*width),
            // A CID font's `/DW` is a real default *when the font states it*;
            // the one-em fallback is the specification's suggestion, not a
            // number the document committed to.
            None if self.two_byte && self.default_width_stated => Width::Stated(self.default_width),
            None => Width::Estimated,
        }
    }

    /// The ascender and descender in 1000-unit text space, and where they came from.
    ///
    /// A PDF that embeds a font usually states /Ascent and /Descent; a
    /// standard-14 font often does not, and the fallback is the conventional
    /// 750/-250 rather than a silent zero, because a zero-height line is
    /// indistinguishable from a broken conversion.
    #[must_use]
    pub fn vertical_metrics(&self) -> (f64, f64) {
        let (ascent, descent) = match (self.ascent, self.descent) {
            (Some(ascent), Some(descent)) => (ascent, descent),
            (Some(ascent), None) => (ascent, -250.0),
            (None, Some(descent)) => (750.0, descent),
            (None, None) => (750.0, -250.0),
        };
        (ascent, descent)
    }

    /// The codes this reader can turn into characters, for diagnostics.
    #[must_use]
    pub fn mapped_codes(&self) -> usize {
        self.to_unicode.len() + self.differences.len()
    }
}

/// Reads everything a simple (one-byte) font states about itself.
///
/// Split out of [`PdfFont::build`] because that function is at its line budget
/// and because the two font kinds are genuinely different: a simple font states
/// an *encoding* and a width per code, a composite one states a descendant font
/// and a CID default width.
fn read_simple_font(dictionary: &lopdf::Dictionary, resolve: Resolver<'_>, font: &mut PdfFont) {
    font.base_font = string_of(dictionary.get(b"BaseFont").ok(), resolve).unwrap_or_default();
    if let Ok(encoding) = dictionary.get(b"Encoding") {
        read_encoding(
            encoding,
            resolve,
            &mut font.base,
            &mut font.differences,
            &mut font.encoding_stated,
        );
    }
    read_simple_widths(dictionary, resolve, &mut font.widths);
    font.embedded = dictionary.get(b"FontDescriptor").is_ok_and(|descriptor| {
        resolve
            .get(descriptor)
            .and_then(|value| value.as_dict().ok())
            .is_some_and(|descriptor| {
                [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                    .iter()
                    .any(|key| descriptor.get(key).is_ok())
            })
    });
}

/// Reads `/Encoding`: a name, or a dictionary with a base plus `/Differences`.
fn read_encoding(
    encoding: &Object,
    resolve: Resolver<'_>,
    base: &mut BaseEncoding,
    differences: &mut BTreeMap<u8, String>,
    stated: &mut bool,
) {
    let Some(object) = resolve.get(encoding) else {
        return;
    };
    match object {
        Object::Name(name) => {
            *base = BaseEncoding::from_name(name.as_slice());
            // The producer named the table, so a character that comes out of it
            // is a mapping the document states rather than one this reader
            // guessed. Without this, ordinary Latin text — which is most real
            // documents — would be reported as unmapped and sent to a model.
            *stated = true;
        }
        Object::Dictionary(dictionary) => {
            if let Ok(name) = dictionary.get(b"BaseEncoding") {
                if let Some(name) = resolve.get(name).and_then(|value| value.as_name().ok()) {
                    *base = BaseEncoding::from_name(name);
                    *stated = true;
                }
            }
            if let Ok(items) = dictionary.get(b"Differences") {
                let mut code: u32 = 0;
                for item in array_items(items, resolve) {
                    match &item {
                        Object::Integer(value) => code = u32::try_from(*value).unwrap_or(0),
                        // A `/Differences` array writes its codes as integers;
                        // a real one that writes a real is a producer quirk, and
                        // rounding it here is better than dropping the entry.
                        Object::Real(value) => {
                            code = to_code(f64::from(*value));
                        }
                        Object::Name(name) => {
                            if let Ok(byte) = u8::try_from(code) {
                                let name = String::from_utf8_lossy(name).into_owned();
                                differences.insert(byte, name);
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
}

/// Reads `/FirstChar`…`/Widths` for a simple font.
fn read_simple_widths(
    dictionary: &lopdf::Dictionary,
    resolve: Resolver<'_>,
    out: &mut BTreeMap<u32, f64>,
) {
    let Some(first) = number_entry(dictionary, b"FirstChar", resolve) else {
        return;
    };
    let Some(widths) = dictionary.get(b"Widths").ok() else {
        return;
    };
    for (index, item) in array_items(widths, resolve).into_iter().enumerate() {
        if let Some(width) = number_of(&item, resolve) {
            out.insert(
                to_code(first).saturating_add(u32::try_from(index).unwrap_or(0)),
                width,
            );
        }
    }
}

/// Reads `/W` for a CID font, whose two shapes are a run of the same width and a
/// `c_first c_last w` group.
fn read_cid_widths(
    dictionary: &lopdf::Dictionary,
    resolve: Resolver<'_>,
    out: &mut BTreeMap<u32, f64>,
) {
    let Ok(w) = dictionary.get(b"W") else {
        return;
    };
    let items = array_items(w, resolve);
    let mut index = 0;
    while index < items.len() {
        let Some(first) = number_of(&items[index], resolve) else {
            break;
        };
        let Some(second) = items
            .get(index + 1)
            .and_then(|item| number_of(item, resolve))
        else {
            break;
        };
        match items.get(index + 2) {
            Some(array @ Object::Array(_)) => {
                for (offset, width) in array_items(array, resolve).into_iter().enumerate() {
                    if let Some(width) = number_of(&width, resolve) {
                        out.insert(
                            to_code(first).saturating_add(u32::try_from(offset).unwrap_or(0)),
                            width,
                        );
                    }
                }
                index += 3;
            }
            Some(item) => {
                let Some(width) = number_of(item, resolve) else {
                    break;
                };
                for code in to_code(first)..=to_code(second) {
                    out.insert(code, width);
                }
                index += 3;
            }
            None => break,
        }
    }
}

/// Parses a `ToUnicode` CMap: `begincodespacerange`, `beginbfchar` and
/// `beginbfrange`.
///
/// Only these three sections matter; everything else in a CMap is layout. An
/// unparseable CMap yields an empty map, which makes the reader fall back to the
/// encoding instead of failing — a document with a broken CMap is still readable.
fn read_to_unicode(bytes: &[u8], out: &mut BTreeMap<u32, char>, limits: &PdfLimits) -> Result<()> {
    let text = String::from_utf8_lossy(bytes);
    // The scan walks absolute offsets rather than reslicing the buffer: a
    // relative `find` plus a relative advance silently skips a section, and the
    // symptom is a font whose first few characters are right and whose rest are
    // missing — which reads as a bad font rather than a bad scan.
    let mut cursor = 0usize;
    while let Some(found) = text[cursor..].find("begin") {
        let tag_start = cursor + found + "begin".len();
        let (tag, section_start) = if text[tag_start..].starts_with("bfchar") {
            ("bfchar", tag_start + "bfchar".len())
        } else if text[tag_start..].starts_with("bfrange") {
            ("bfrange", tag_start + "bfrange".len())
        } else {
            cursor = tag_start;
            continue;
        };
        // The section ends at its own `end…`, which is the first "end" after
        // its start: nothing inside a section body contains the word.
        let Some(offset) = text[section_start..].find("end") else {
            break;
        };
        let body = &text[section_start..section_start + offset];
        if tag == "bfchar" {
            for (code, ch) in hex_pairs(body) {
                let (Some(code), Some(ch)) = (code, ch) else {
                    continue;
                };
                if out.len() >= limits.max_font_glyphs {
                    return Err(limits.exceeded(LimitKind::FontGlyphs, out.len() as u64 + 1));
                }
                out.insert(code, ch);
            }
        } else {
            read_bfrange(body, out, limits)?;
        }
        cursor = section_start + offset;
    }
    Ok(())
}

/// Reads `beginbfrange` entries, which are `<lo> <hi> <dst>` or
/// `<lo> <hi> [<d1> <d2> …]`.
fn read_bfrange(body: &str, out: &mut BTreeMap<u32, char>, limits: &PdfLimits) -> Result<()> {
    for entry in body.split('\n') {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        // The array form is `<lo> <hi> [<d1> <d2> …]`, so the bracket is *after*
        // the two bounds: looking for a leading `[` misses every real one.
        let bracket = trimmed.find('[');
        let bounds = hex_tokens(&trimmed[..bracket.unwrap_or(trimmed.len())]);
        let (Some(low), Some(high)) = (
            bounds.first().and_then(|token| parse_hex(token)),
            bounds.get(1).and_then(|token| parse_hex(token)),
        ) else {
            continue;
        };

        if let Some(bracket) = bracket {
            let Some(close) = trimmed[bracket..].find(']') else {
                continue;
            };
            for (offset, ch) in utf16_values(&trimmed[bracket + 1..bracket + 1 + close]) {
                if out.len() >= limits.max_font_glyphs {
                    return Err(limits.exceeded(LimitKind::FontGlyphs, out.len() as u64 + 1));
                }
                out.insert(low.wrapping_add(offset), ch);
            }
            continue;
        }

        let Some(first) = bounds.get(2).and_then(|token| parse_hex(token)) else {
            continue;
        };
        // A `bfrange` destination is a *character* the range increments, so the
        // run is `first`, `first + 1`, ...; a `bfrange` whose span is absurd is a
        // malformed CMap, and the budget is what stops it allocating.
        let destination = char::from_u32(first).unwrap_or('\u{fffd}');
        let span = high.saturating_sub(low).min(u32::from(u16::MAX));
        for offset in 0..=span {
            if out.len() >= limits.max_font_glyphs {
                return Err(limits.exceeded(LimitKind::FontGlyphs, out.len() as u64 + 1));
            }
            let ch =
                char::from_u32(u32::from(destination).wrapping_add(offset)).unwrap_or('\u{fffd}');
            out.insert(low + offset, ch);
        }
    }
    Ok(())
}

/// The `<hex>` tokens of a CMap section, paired with the characters they mean.
///
/// Only `bfchar` has this shape: alternating code and character. A `bfrange`
/// line is three tokens in a row and uses [`hex_tokens`] instead — reading it
/// with this function silently shifts every character by one position, which
/// looks like a font problem and is not one.
fn hex_pairs(body: &str) -> Vec<(Option<u32>, Option<char>)> {
    let tokens = hex_tokens(body);
    let mut out = Vec::with_capacity(tokens.len() / 2);
    for pair in tokens.as_chunks::<2>().0 {
        out.push((
            parse_hex(&pair[0]),
            utf16_value(&pair[1]).or_else(|| char::from_u32(parse_hex(&pair[1]).unwrap_or(0))),
        ));
    }
    out
}

/// The numeric value of every `<hex>` token, in order.
///
/// The tokens of a `bfrange` array form are wrapped in `[` … `]`, so a token is
/// read as the text between `<` and `>` wherever those are — a `split_whitespace`
/// that then filters on `ends_with('>')` silently drops the last destination,
/// which is a missing character rather than an error.
fn hex_tokens(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            break;
        };
        out.push(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    out
}

fn parse_hex(token: &str) -> Option<u32> {
    u32::from_str_radix(token.trim(), 16).ok()
}

/// Decodes a UTF-16BE hex token to one character.
fn utf16_value(token: &str) -> Option<char> {
    let token = token.trim();
    if token.len() < 4 {
        return u32::from_str_radix(token, 16).ok().and_then(char::from_u32);
    }
    let units: Vec<u16> = (0..token.len() / 4)
        .filter_map(|index| u16::from_str_radix(&token[index * 4..index * 4 + 4], 16).ok())
        .collect();
    String::from_utf16(&units).ok()?.chars().next()
}

/// Decodes the `<d1> <d2> …` destinations of a `bfrange` array form.
fn utf16_values(body: &str) -> Vec<(u32, char)> {
    hex_tokens(body)
        .iter()
        .filter_map(|token| utf16_value(token))
        .enumerate()
        .map(|(index, ch)| (index as u32, ch))
        .collect()
}

/// A code from a number a producer wrote, clamped into the range a glyph code
/// can occupy.
///
/// A /Differences array and a /Widths array both index glyphs, and a
/// negative or absurd index is a producer bug rather than an attack — clamping
/// keeps the rest of the array readable instead of abandoning it.
fn to_code(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// The handful of Adobe glyph names a `/Differences` array actually uses.
///
/// A full Adobe Glyph List is a data file this crate does not carry; the names
/// below cover the accented Latin letters and the punctuation a real document
/// uses with a simple font, and anything else falls back to the base encoding.
fn glyph_name_to_char(name: &str) -> Option<char> {
    let stripped = name.strip_prefix("uni").and_then(|rest| {
        if rest.len() == 4 {
            u32::from_str_radix(rest, 16).ok()
        } else {
            None
        }
    });
    if let Some(code) = stripped {
        return char::from_u32(code);
    }
    let suffix = name.strip_prefix('u').and_then(|rest| {
        if (4..=6).contains(&rest.len()) {
            u32::from_str_radix(rest, 16).ok()
        } else {
            None
        }
    });
    if let Some(code) = suffix {
        return char::from_u32(code);
    }
    Some(match name {
        "space" => ' ',
        "exclam" => '!',
        "quotedbl" => '"',
        "numbersign" => '#',
        "dollar" => '$',
        "percent" => '%',
        "ampersand" => '&',
        "quotesingle" => '\'',
        "parenleft" => '(',
        "parenright" => ')',
        "asterisk" => '*',
        "plus" => '+',
        "comma" => ',',
        "hyphen" | "minus" => '-',
        "period" => '.',
        "slash" => '/',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        "colon" => ':',
        "semicolon" => ';',
        "less" => '<',
        "equal" => '=',
        "greater" => '>',
        "question" => '?',
        "at" => '@',
        "bracketleft" => '[',
        "backslash" => '\\',
        "bracketright" => ']',
        "asciicircum" => '^',
        "underscore" => '_',
        "grave" => '`',
        "braceleft" => '{',
        "bar" => '|',
        "braceright" => '}',
        "asciitilde" => '~',
        "quoteleft" => '\u{2018}',
        "quoteright" => '\u{2019}',
        "quotedblleft" => '\u{201c}',
        "quotedblright" => '\u{201d}',
        "endash" => '\u{2013}',
        "emdash" => '\u{2014}',
        "bullet" => '\u{2022}',
        "ellipsis" => '\u{2026}',
        "sterling" => '\u{a3}',
        "Euro" => '\u{20ac}',
        "trademark" => '\u{2122}',
        "degree" => '\u{b0}',
        "plusminus" => '\u{b1}',
        "multiply" => '\u{d7}',
        "divide" => '\u{f7}',
        _ => return None,
    })
}

/// Resolves one level of indirection.
///
/// A named type rather than an `impl Fn`: a closure over a document cannot
/// satisfy a `&dyn Fn` parameter, because the closure fixes one lifetime while
/// the trait object is higher-ranked, and the compiler rejects every call site
/// with a message that points at the closure instead of at the mismatch.
#[derive(Clone, Copy)]
pub struct Resolver<'a> {
    document: &'a lopdf::Document,
}

impl<'a> Resolver<'a> {
    /// Wraps a document.
    #[must_use]
    pub fn new(document: &'a lopdf::Document) -> Self {
        Self { document }
    }

    /// Resolves one object.
    #[must_use]
    pub fn get<'o>(&'o self, object: &'o Object) -> Option<&'o Object> {
        match object {
            Object::Reference(id) => self.document.get_object(*id).ok(),
            other => Some(other),
        }
    }

    /// Resolves an object that is expected to be a dictionary.
    #[must_use]
    pub fn dictionary<'o>(&'o self, object: &'o Object) -> Option<&'o lopdf::Dictionary> {
        self.get(object).and_then(|value| value.as_dict().ok())
    }

    /// The document behind the resolver.
    #[must_use]
    pub fn document(&self) -> &'a lopdf::Document {
        self.document
    }
}

/// Resolves one level of indirection.
#[must_use]
pub fn resolver(document: &lopdf::Document) -> Resolver<'_> {
    Resolver::new(document)
}

/// The name of a dictionary entry, dereferenced.
pub fn name_of(
    dictionary: &lopdf::Dictionary,
    key: &[u8],
    resolve: Resolver<'_>,
) -> Option<Vec<u8>> {
    dictionary
        .get(key)
        .ok()
        .and_then(|value| resolve.get(value))
        .and_then(|value| value.as_name().ok())
        .map(<[u8]>::to_vec)
}

/// A number, whether it is stored as an integer or a real.
pub fn number_of(object: &Object, resolve: Resolver<'_>) -> Option<f64> {
    resolve
        .get(object)
        .and_then(|value| value.as_float().ok())
        .map(f64::from)
}

/// A number from a dictionary entry, dereferenced through `resolve`.
pub fn number_entry(
    dictionary: &lopdf::Dictionary,
    key: &[u8],
    resolve: Resolver<'_>,
) -> Option<f64> {
    dictionary
        .get(key)
        .ok()
        .and_then(|value| number_of(value, resolve))
}

/// A number from a dictionary entry.
pub fn number(dictionary: &lopdf::Dictionary, key: &[u8]) -> Option<f64> {
    dictionary
        .get(key)
        .ok()
        .and_then(|value| value.as_float().ok())
        .map(f64::from)
}

/// A text string from a dictionary entry.
pub fn string_of(object: Option<&Object>, resolve: Resolver<'_>) -> Option<String> {
    let object = object?;
    let object = resolve.get(object)?;
    match object {
        Object::String(bytes, _) => Some(String::from_utf8_lossy(bytes).into_owned()),
        Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
        _ => None,
    }
}

/// The items of an array, dereferenced.
pub fn array_items(object: &Object, resolve: Resolver<'_>) -> Vec<Object> {
    match resolve.get(object) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| resolve.get(item).cloned().unwrap_or_else(|| item.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The decompressed bytes of a stream object, dereferenced.
pub fn stream_bytes(object: &Object, resolve: Resolver<'_>) -> Option<Vec<u8>> {
    let object = resolve.get(object)?;
    let stream = object.as_stream().ok()?;
    stream.decompressed_content().ok()
}

/// Decodes UTF-16BE bytes of a text string, which is how a PDF writes
/// non-ASCII text in a `Tj` outside a font's encoding.
#[must_use]
pub fn decode_text_string(bytes: &[u8], format: lopdf::StringFormat) -> String {
    if format == lopdf::StringFormat::Hexadecimal || !bytes.contains(&0) {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let le: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    let (text, _, _) = UTF_16LE.decode(&le);
    text.into_owned()
}

/// Parses a `ToUnicode` CMap into a code → character map.
///
/// The scanner is a state machine over a text format with no schema, so a
/// document's `ToUnicode` is a public operation rather than an internal step:
/// "which characters does this font claim to cover" is a question worth asking
/// out loud when a conversion produces mojibake.
///
/// # Errors
///
/// Returns [`PdfError::LimitExceeded`](crate::error::PdfError::LimitExceeded)
/// when the CMap records more codes than `limits.max_font_glyphs`.
pub fn parse_to_unicode(bytes: &[u8], limits: &PdfLimits) -> Result<BTreeMap<u32, char>> {
    let mut out = BTreeMap::new();
    read_to_unicode(bytes, &mut out, limits)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        glyph_name_to_char, hex_pairs, read_encoding, read_to_unicode, resolver, BaseEncoding,
        PdfFont, PdfLimits,
    };

    fn limits() -> PdfLimits {
        PdfLimits::default()
    }

    #[test]
    fn win_ansi_and_mac_roman_decode_the_latin_range() {
        assert_eq!(BaseEncoding::WinAnsi.decode_byte(0x41), Some('A'));
        assert_eq!(BaseEncoding::WinAnsi.decode_byte(0xE9), Some('\u{e9}'));
        // The two single-byte encodings agree below 0x80 and diverge above it:
        // Windows-1252 puts a euro at 0x80, MacRoman puts `Ä` there and puts
        // `é` at 0x8E where Windows-1252 has `Ž`. Confusing the two produces
        // mojibake that looks like a font problem.
        assert_eq!(BaseEncoding::WinAnsi.decode_byte(0x80), Some('\u{20ac}'));
        assert_eq!(BaseEncoding::MacRoman.decode_byte(0x80), Some('\u{c4}'));
        assert_eq!(BaseEncoding::MacRoman.decode_byte(0x8E), Some('\u{e9}'));
        assert_eq!(BaseEncoding::WinAnsi.decode_byte(0x8E), Some('\u{17d}'));
    }

    #[test]
    fn to_unicode_bfchar_and_bfrange_are_read() {
        let cmap = b"\
/CIDInit /ProcSet findresource begin
1 begincodespacerange <0000> <FFFF> endcodespacerange
2 beginbfchar
<0003> <0020>
<0004> <0041>
endbfchar
1 beginbfrange
<0010> <0012> <0061>
<0020> <0021> [<0041> <0042>]
endbfrange
endcmap";
        let mut out = std::collections::BTreeMap::new();
        read_to_unicode(cmap, &mut out, &limits()).expect("parsed");
        assert_eq!(out.get(&0x0003), Some(&' '));
        assert_eq!(out.get(&0x0004), Some(&'A'));
        assert_eq!(out.get(&0x0010), Some(&'a'));
        assert_eq!(out.get(&0x0012), Some(&'c'));
        assert_eq!(out.get(&0x0020), Some(&'A'));
        assert_eq!(out.get(&0x0021), Some(&'B'));
    }

    #[test]
    fn a_malformed_cmap_yields_nothing_rather_than_failing() {
        let mut out = std::collections::BTreeMap::new();
        read_to_unicode(b"not a cmap at all", &mut out, &limits()).expect("no failure");
        assert!(out.is_empty());
    }

    #[test]
    fn the_font_glyph_budget_is_enforced() {
        let mut out = std::collections::BTreeMap::new();
        let tight = PdfLimits {
            max_font_glyphs: 1,
            ..PdfLimits::default()
        };
        let cmap = b"1 beginbfchar <0001> <0041> <0002> <0042> endbfchar";
        let error = read_to_unicode(cmap, &mut out, &tight).expect_err("budget");
        assert!(error.to_string().contains("font_glyphs"), "{error}");
    }

    #[test]
    fn hex_tokens_are_paired() {
        let pairs = hex_pairs("<0001> <0041> <0002> <0042> ");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], (Some(1), Some('A')));
        assert_eq!(pairs[1], (Some(2), Some('B')));
    }

    #[test]
    fn glyph_names_resolve_the_common_latin_set() {
        assert_eq!(glyph_name_to_char("space"), Some(' '));
        assert_eq!(glyph_name_to_char("uni00E9"), Some('é'));
        assert_eq!(glyph_name_to_char("u1F600"), Some('\u{1F600}'));
        assert_eq!(glyph_name_to_char("bullet"), Some('\u{2022}'));
        assert_eq!(glyph_name_to_char("nonesuch"), None);
    }

    #[test]
    fn a_width_the_font_did_not_state_is_marked_as_a_guess() {
        let font = PdfFont {
            name: "F1".to_owned(),
            two_byte: false,
            base: BaseEncoding::Standard,
            encoding_stated: false,
            differences: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
            widths: BTreeMap::new(),
            default_width: 1000.0,
            default_width_stated: false,
            embedded: false,
            base_font: "Helvetica".to_owned(),
            ascent: None,
            descent: None,
        };
        assert!(font.width(65).is_estimated());
        assert!((font.width(65).value() - 500.0).abs() < 1e-9);
        assert_eq!(font.character(65), Some('A'));
    }

    /// A font that names its encoding has *stated* how to read its codes, and a
    /// character that comes out of that table is not a guess. The distinction is
    /// what `Glyph::mapped` reports, and getting it wrong makes a page of plain
    /// Latin text look unreadable.
    #[test]
    fn a_named_encoding_is_a_stated_mapping() {
        let font = PdfFont {
            name: "F1".to_owned(),
            two_byte: false,
            base: BaseEncoding::Standard,
            encoding_stated: false,
            differences: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
            widths: BTreeMap::new(),
            default_width: 1000.0,
            default_width_stated: false,
            embedded: false,
            base_font: "Helvetica".to_owned(),
            ascent: None,
            descent: None,
        };
        assert!(
            !font.encoding_stated,
            "a font with no `/Encoding` states nothing"
        );
        let named = lopdf::Object::Name(b"WinAnsiEncoding".to_vec());
        let mut base = font.base;
        let mut stated = font.encoding_stated;
        let mut differences = BTreeMap::new();
        read_encoding(
            &named,
            resolver(&lopdf::Document::new()),
            &mut base,
            &mut differences,
            &mut stated,
        );
        assert!(stated);
        assert_eq!(base, BaseEncoding::WinAnsi);
    }
}
