//! Inline images (`BI`…`ID`…`EI`) in content streams (AUD-84 / CORE-QUEUE L-9).
//!
//! `lopdf` tokenises a content stream before this crate sees it. An inline
//! image's sample bytes therefore become operators (`m`, `l`, `re`, `f`, …),
//! which both lose the picture and paint garbage. This module pulls every
//! `BI…ID…EI` out of the **raw** stream (finding `EI` by the delimiter rule in
//! ISO 32000-1 §8.9.7), replaces it with a marker the interpreter understands,
//! and keeps the samples for placement under the CTM at that marker.

use std::sync::Arc;

use crate::image::Encoded;

/// Marker operator written in place of a consumed inline image.
/// Private content-stream operator that stands in for one extracted inline image.
/// Must be a plain PDF name token: lopdf's content decoder drops tokens that
/// contain underscores (AUD-84).
pub const INLINE_OP: &str = "MuFoEdInline";

/// One inline image extracted from a content stream.
#[derive(Clone, Debug)]
pub struct InlineImage {
    /// Decoded samples, when the layout is one this reader carries.
    pub encoded: Option<Encoded>,
    /// Why decoding failed, when it did.
    pub missing: Option<String>,
}

/// Strips every `BI…ID…EI` from `bytes`, replacing each with
/// `N __MuFoEd_Inline` where `N` indexes the returned vector.
#[must_use]
pub fn extract(bytes: &[u8]) -> (Vec<u8>, Vec<InlineImage>) {
    let mut out = Vec::with_capacity(bytes.len());
    let mut images = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(start) = find_bi(bytes, index) {
            out.extend_from_slice(bytes.get(index..start).unwrap_or_default());
            if let Some((after, image)) = take_inline(bytes, start) {
                let n = images.len();
                images.push(image);
                out.extend_from_slice(format!("{n} {INLINE_OP} ").as_bytes());
                index = after;
            } else {
                // A BI we cannot finish: copy the BI token and continue so
                // the rest of the stream still tokenises.
                let end = start.saturating_add(2);
                out.extend_from_slice(bytes.get(start..end).unwrap_or_default());
                index = end;
            }
        } else {
            out.extend_from_slice(bytes.get(index..).unwrap_or_default());
            break;
        }
    }
    (out, images)
}

/// Finds `BI` as a free-standing operator at or after `from`.
fn find_bi(bytes: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while let Some(pair) = bytes.get(index..index.saturating_add(2)) {
        if pair == b"BI"
            && is_delim_before(bytes, index)
            && is_delim_after(bytes, index.saturating_add(2))
        {
            return Some(index);
        }
        index = index.saturating_add(1);
    }
    None
}

fn is_delim_before(bytes: &[u8], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|before| bytes.get(before))
        .is_none_or(|&byte| is_whitespace(byte) || is_delimiter(byte))
}

fn is_delim_after(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index)
        .is_none_or(|&byte| is_whitespace(byte) || is_delimiter(byte))
}

/// Neither white space nor a delimiter.
fn is_regular(byte: u8) -> bool {
    !is_whitespace(byte) && !is_delimiter(byte)
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Consumes one inline image starting at `BI`. Returns the index after `EI`.
fn take_inline(bytes: &[u8], bi: usize) -> Option<(usize, InlineImage)> {
    // Skip "BI" and whitespace.
    let mut index = bi.saturating_add(2);
    while bytes.get(index).is_some_and(|&byte| is_whitespace(byte)) {
        index = index.saturating_add(1);
    }
    let (id_at, dict) = parse_inline_dict(bytes, index)?;
    // Skip "ID" and the single whitespace that follows (ISO 32000-1 §8.9.7).
    let mut data_start = id_at.saturating_add(2);
    if bytes
        .get(data_start)
        .is_some_and(|&byte| is_whitespace(byte))
    {
        data_start = data_start.saturating_add(1);
    }
    let (ei_at, data_end) = find_ei(bytes, data_start, &dict)?;
    let data = bytes.get(data_start..data_end)?.to_vec();
    let image = decode_inline(&dict, &data);
    let after = {
        let mut end = ei_at.saturating_add(2);
        while bytes.get(end).is_some_and(|&byte| is_whitespace(byte)) {
            end = end.saturating_add(1);
        }
        end
    };
    Some((after, image))
}

/// Key/value pairs of an inline-image dictionary (abbreviated names).
struct InlineDict {
    width: Option<u32>,
    height: Option<u32>,
    bits: Option<u32>,
    components: Option<u8>,
    /// The filter chain in decoding order, as full names (`/A85` →
    /// `ASCII85Decode`); a single name and an array both land here.
    filters: Vec<Vec<u8>>,
    /// `/IM true`: a 1-bit stencil painted in the fill colour.
    image_mask: bool,
    /// `/DP` was given (predictors and the like, which are not applied here).
    decode_parms: bool,
}

impl InlineDict {
    /// The sample bytes the dictionary promises, when the data between `ID`
    /// and `EI` is the samples themselves. A filtered image's data is as long
    /// as its encoding happens to be, so it promises nothing.
    fn expected_bytes(&self) -> Option<usize> {
        if !self.filters.is_empty() {
            return None;
        }
        let width = usize::try_from(self.width?).ok()?;
        let height = usize::try_from(self.height?).ok()?;
        // ISO 32000-1 §8.9.6.2: a stencil is one component of one bit.
        let (bits, components) = if self.image_mask {
            (1, 1)
        } else {
            (
                usize::try_from(self.bits.unwrap_or(8)).ok()?,
                usize::from(self.components.unwrap_or(3)),
            )
        };
        let bits_total = width
            .checked_mul(height)?
            .checked_mul(components)?
            .checked_mul(bits)?;
        Some(bits_total.div_ceil(8))
    }
}

/// Parses `/Key value …` until the `ID` operator.
fn parse_inline_dict(bytes: &[u8], mut index: usize) -> Option<(usize, InlineDict)> {
    let mut dict = InlineDict {
        width: None,
        height: None,
        bits: None,
        components: None,
        filters: Vec::new(),
        image_mask: false,
        decode_parms: false,
    };
    while index < bytes.len() {
        while bytes.get(index).is_some_and(|&byte| is_whitespace(byte)) {
            index = index.saturating_add(1);
        }
        if bytes
            .get(index..index.saturating_add(2))
            .is_some_and(|pair| pair == b"ID")
            && is_delim_before(bytes, index)
            && is_delim_after(bytes, index.saturating_add(2))
        {
            return Some((index, dict));
        }
        if bytes.get(index) != Some(&b'/') {
            return None;
        }
        index = index.saturating_add(1);
        let key_start = index;
        while bytes.get(index).is_some_and(|&byte| is_regular(byte)) {
            index = index.saturating_add(1);
        }
        let key = bytes.get(key_start..index)?;
        while bytes.get(index).is_some_and(|&byte| is_whitespace(byte)) {
            index = index.saturating_add(1);
        }
        let (value, next) = parse_value(bytes, index)?;
        index = next;
        match key {
            b"W" | b"Width" => dict.width = value.as_number(),
            b"H" | b"Height" => dict.height = value.as_number(),
            b"BPC" | b"BitsPerComponent" => dict.bits = value.as_number(),
            b"CS" | b"ColorSpace" => dict.components = value.components(),
            b"F" | b"Filter" => dict.filters = value.filter_chain(),
            b"IM" | b"ImageMask" => dict.image_mask = matches!(value, Value::Bool(true)),
            b"DP" | b"DecodeParms" => dict.decode_parms = !matches!(value, Value::Null),
            _ => {}
        }
    }
    None
}

enum Value {
    Number(u32),
    Name(Vec<u8>),
    Bool(bool),
    Null,
    /// The names in an array, in order; other members are dropped.
    Names(Vec<Vec<u8>>),
    Other,
}

impl Value {
    fn as_number(&self) -> Option<u32> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn filter_chain(&self) -> Vec<Vec<u8>> {
        match self {
            Self::Name(name) => vec![full_filter_name(name)],
            Self::Names(names) => names.iter().map(|name| full_filter_name(name)).collect(),
            _ => Vec::new(),
        }
    }

    fn components(&self) -> Option<u8> {
        match self {
            Self::Name(n) => match n.as_slice() {
                b"G" | b"DeviceGray" => Some(1),
                b"RGB" | b"DeviceRGB" => Some(3),
                b"CMYK" | b"DeviceCMYK" => Some(4),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The full name of an inline-image filter (ISO 32000-1 Table 94).
fn full_filter_name(name: &[u8]) -> Vec<u8> {
    let full: &[u8] = match name {
        b"AHx" => b"ASCIIHexDecode",
        b"A85" => b"ASCII85Decode",
        b"LZW" => b"LZWDecode",
        b"Fl" => b"FlateDecode",
        b"RL" => b"RunLengthDecode",
        b"CCF" => b"CCITTFaxDecode",
        b"DCT" => b"DCTDecode",
        b"JPX" => b"JPXDecode",
        b"JBIG2" => b"JBIG2Decode",
        other => other,
    };
    full.to_vec()
}

/// How deep an inline-image value may nest arrays before it is refused.
const MAX_VALUE_DEPTH: u32 = 8;

fn parse_value(bytes: &[u8], index: usize) -> Option<(Value, usize)> {
    parse_value_at(bytes, index, 0)
}

fn parse_value_at(bytes: &[u8], index: usize, depth: u32) -> Option<(Value, usize)> {
    let first = *bytes.get(index)?;
    if first == b'/' {
        let mut end = index.saturating_add(1);
        while bytes.get(end).is_some_and(|&byte| is_regular(byte)) {
            end = end.saturating_add(1);
        }
        return Some((
            Value::Name(bytes.get(index.saturating_add(1)..end)?.to_vec()),
            end,
        ));
    }
    if first == b'[' {
        // A filter chain (`[/A85 /Fl]`) is the array that matters; a `/Decode`
        // array of numbers parses and is dropped.
        if depth >= MAX_VALUE_DEPTH {
            return None;
        }
        let mut names = Vec::new();
        let mut end = index.saturating_add(1);
        loop {
            while bytes.get(end).is_some_and(|&byte| is_whitespace(byte)) {
                end = end.saturating_add(1);
            }
            match bytes.get(end) {
                None => return None,
                Some(b']') => return Some((Value::Names(names), end.saturating_add(1))),
                Some(_) => {
                    let (item, next) = parse_value_at(bytes, end, depth.saturating_add(1))?;
                    if let Value::Name(name) = item {
                        names.push(name);
                    }
                    end = next;
                }
            }
        }
    }
    if bytes
        .get(index..index.saturating_add(2))
        .is_some_and(|pair| pair == b"<<")
    {
        // A `/DP` dictionary, skipped whole so that its keys are not read as
        // the image's own.
        let mut end = index.saturating_add(2);
        let mut open: u32 = 1;
        while open > 0 {
            let pair = bytes.get(end..end.saturating_add(2))?;
            if pair == b"<<" {
                open = open.saturating_add(1);
                end = end.saturating_add(2);
            } else if pair == b">>" {
                open = open.saturating_sub(1);
                end = end.saturating_add(2);
            } else {
                end = end.saturating_add(1);
            }
        }
        return Some((Value::Other, end));
    }
    if first == b'<' || first == b'(' {
        // A hex or literal string (an indexed palette, say): skipped.
        let literal = first == b'(';
        let mut end = index.saturating_add(1);
        let mut open: u32 = 1;
        while open > 0 {
            let byte = *bytes.get(end)?;
            if literal && byte == b'\\' {
                end = end.saturating_add(2);
                continue;
            }
            if literal && byte == b'(' {
                open = open.saturating_add(1);
            } else if byte == if literal { b')' } else { b'>' } {
                open = open.saturating_sub(1);
            }
            end = end.saturating_add(1);
        }
        return Some((Value::Other, end));
    }
    if first.is_ascii_digit() || matches!(first, b'-' | b'+' | b'.') {
        let mut end = index;
        while bytes
            .get(end)
            .is_some_and(|&byte| byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.'))
        {
            end = end.saturating_add(1);
        }
        let text = std::str::from_utf8(bytes.get(index..end)?).ok()?;
        let number = text.parse::<f64>().ok()?;
        return Some((Value::Number(number as u32), end));
    }
    // A keyword (`true`, `false`, `null`), a bare name, or an unknown token.
    let mut end = index;
    while bytes.get(end).is_some_and(|&byte| is_regular(byte)) {
        end = end.saturating_add(1);
    }
    let value = match bytes.get(index..end) {
        Some(b"true") => Value::Bool(true),
        Some(b"false") => Value::Bool(false),
        Some(b"null") => Value::Null,
        _ => Value::Other,
    };
    Some((value, end.max(index.saturating_add(1))))
}

/// Finds `EI` after `data_start`. Prefer the length implied by the dictionary;
/// fall back to the delimiter rule (ISO 32000-1 §8.9.7).
fn find_ei(bytes: &[u8], data_start: usize, dict: &InlineDict) -> Option<(usize, usize)> {
    if let Some(expected) = dict.expected_bytes() {
        let data_end = data_start.checked_add(expected)?;
        if data_end.saturating_add(2) <= bytes.len() {
            let mut ei = data_end;
            // Optional whitespace between samples and EI.
            while bytes.get(ei).is_some_and(|&byte| is_whitespace(byte)) {
                ei = ei.saturating_add(1);
            }
            if bytes
                .get(ei..ei.saturating_add(2))
                .is_some_and(|pair| pair == b"EI")
                && is_delim_after(bytes, ei.saturating_add(2))
            {
                return Some((ei, data_end.min(ei)));
            }
        }
    }
    // Delimiter search: whitespace (or start-of-data) + EI + whitespace/delimiter/end.
    let mut index = data_start;
    while let Some(pair) = bytes.get(index..index.saturating_add(2)) {
        let space_before = index > data_start
            && index
                .checked_sub(1)
                .and_then(|before| bytes.get(before))
                .is_some_and(|&byte| is_whitespace(byte));
        if pair == b"EI"
            && (index == data_start || space_before)
            && is_delim_after(bytes, index.saturating_add(2))
        {
            // data_end excludes the whitespace before EI when present.
            let data_end = if space_before {
                index.saturating_sub(1)
            } else {
                index
            };
            return Some((index, data_end));
        }
        index = index.saturating_add(1);
    }
    None
}

fn missing(reason: impl Into<String>) -> InlineImage {
    InlineImage {
        encoded: None,
        missing: Some(reason.into()),
    }
}

/// Filters that only carry bytes, which lopdf's stream decoder undoes.
fn is_transport_filter(name: &[u8]) -> bool {
    matches!(
        name,
        b"ASCIIHexDecode" | b"ASCII85Decode" | b"LZWDecode" | b"FlateDecode" | b"RunLengthDecode"
    )
}

fn decode_inline(dict: &InlineDict, data: &[u8]) -> InlineImage {
    let width = match dict.width {
        Some(w) if w > 0 => w,
        _ => return missing("inline image has no Width"),
    };
    let height = match dict.height {
        Some(h) if h > 0 => h,
        _ => return missing("inline image has no Height"),
    };
    // A stencil is the fill colour through a 1-bit mask, not a picture: the
    // same answer as for an XObject `/ImageMask`.
    if dict.image_mask {
        return missing("inline image mask is not carried");
    }
    // The chain is transport filters, then at most one image codec.
    let (codec, transport) = match dict.filters.split_last() {
        Some((last, rest)) if !is_transport_filter(last) => (Some(last.as_slice()), rest),
        _ => (None, dict.filters.as_slice()),
    };
    if let Some(other) = transport.iter().find(|name| !is_transport_filter(name)) {
        return missing(format!(
            "inline image filter /{} is not carried",
            String::from_utf8_lossy(other)
        ));
    }
    let codec_carries_depth = matches!(
        codec,
        Some(b"JPXDecode" | b"CCITTFaxDecode" | b"JBIG2Decode")
    );
    let bits = dict.bits.unwrap_or(8);
    let components = dict.components.unwrap_or(3);
    let expand_one_bit = !codec_carries_depth && bits == 1 && components == 1;
    if !codec_carries_depth && !expand_one_bit && bits != 8 {
        return missing("inline image bits per component is not 8");
    }
    let limits = crate::PdfLimits::default();
    // The same pixel budget as an XObject's, for every filter: an inline JPEG
    // goes on as its bytes, and `/W 65535 /H 65535` in front of a few bytes of
    // `ID` data is a decode nobody downstream can afford (default limits, as
    // for `/JPX`).
    if let Err(reject) = crate::image::check_pixels(width, height, &limits) {
        return missing(reject.to_string());
    }
    let bytes = if transport.is_empty() {
        data.to_vec()
    } else {
        if dict.decode_parms {
            // A predictor would have to be undone on the inflated rows.
            return missing("inline image DecodeParms is not carried");
        }
        let chain = transport
            .iter()
            .map(|name| lopdf::Object::Name(name.clone()))
            .collect();
        let mut stream_dict = lopdf::Dictionary::new();
        stream_dict.set("Filter", lopdf::Object::Array(chain));
        let stream = lopdf::Stream::new(stream_dict, data.to_vec());
        match crate::document::bounded_decompress(&stream, limits.max_image_bytes) {
            Ok(bytes) => bytes,
            Err(_) => return missing("inline image filters could not be decoded"),
        }
    };
    match codec {
        Some(b"DCTDecode") => InlineImage {
            encoded: Some(Encoded::Jpeg {
                width,
                height,
                data: Arc::new(bytes),
                alpha: None,
            }),
            missing: None,
        },
        Some(b"JPXDecode") => match crate::image::decode_jpx_bytes(&bytes, &limits) {
            // Same decoder as XObject `/JPXDecode`; default limits — a large
            // inline image has already paid for the content-stream budget.
            Ok(encoded) => InlineImage {
                encoded: Some(encoded),
                missing: None,
            },
            Err(reject) => missing(reject.to_string()),
        },
        None => decode_inline_samples(bytes, expand_one_bit, width, height, components),
        Some(other) => missing(format!(
            "inline image filter /{} is not carried",
            String::from_utf8_lossy(other)
        )),
    }
}

fn decode_inline_samples(
    samples: Vec<u8>,
    expand_one_bit: bool,
    width: u32,
    height: u32,
    components: u8,
) -> InlineImage {
    let samples = if expand_one_bit {
        match crate::image::expand_gray1_to_eight(
            &samples,
            width,
            height,
            &crate::PdfLimits::default(),
        ) {
            Ok(expanded) => expanded,
            Err(reject) => return missing(reject.to_string()),
        }
    } else {
        samples
    };
    InlineImage {
        encoded: Some(Encoded::Raw {
            width,
            height,
            samples: Arc::new(samples),
            components,
            alpha: None,
        }),
        missing: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{extract, INLINE_OP};

    #[test]
    fn an_inline_image_with_garbage_bytes_is_not_tokenised_as_ops() {
        // Samples deliberately contain operator-like bytes (`re f cm BT ET Tj`).
        // 5×1 RGB = 15 sample bytes.
        let garbage = b"re f cm BT ETTj";
        assert_eq!(garbage.len(), 15);
        let mut stream = Vec::new();
        stream.extend_from_slice(b"q 10 0 0 10 0 0 cm BI /W 5 /H 1 /BPC 8 /CS /RGB ID ");
        stream.extend_from_slice(garbage);
        stream.extend_from_slice(b" EI Q");
        let (cleaned, images) = extract(&stream);
        assert_eq!(images.len(), 1);
        assert!(images[0].encoded.is_some());
        let text = String::from_utf8_lossy(&cleaned);
        assert!(text.contains(INLINE_OP), "{text}");
        assert!(
            !text.as_bytes().windows(2).any(|w| w == b"BI"),
            "BI must be gone: {text}"
        );
        // The garbage operators must not remain as free text for lopdf.
        assert!(!text.contains(" BT "), "{text}");
        assert!(!text.contains(" Tj"), "{text}");
    }

    #[test]
    fn an_inline_jpeg_over_the_pixel_budget_is_reported_not_carried() {
        let mut stream = Vec::new();
        stream.extend_from_slice(b"q BI /W 65535 /H 65535 /BPC 8 /CS /RGB /F /DCT ID ");
        stream.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
        stream.extend_from_slice(b" EI Q");
        let (_, images) = extract(&stream);
        assert_eq!(images.len(), 1);
        assert!(images[0].encoded.is_none());
        let missing = images[0].missing.as_deref().unwrap_or_default();
        assert!(missing.contains("budget"), "{missing}");
    }

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for byte in bytes {
            let _ = write!(out, "{byte:02X}");
        }
        out.push('>');
        out
    }

    #[test]
    fn a_stencil_behind_a_filter_array_is_a_named_loss_between_intact_text() {
        // R1: `/IM true` with no `/CS`, `/F [/AHx /Fl]`; the text either side
        // must reach the tokeniser untouched.
        let data = miniz_oxide::deflate::compress_to_vec_zlib(&[0xAA, 0x55], 6);
        let mut stream = Vec::new();
        stream.extend_from_slice(
            b"BT (before) Tj ET q BI /W 8 /H 2 /BPC 1 /IM true /F [/AHx /Fl] ID ",
        );
        stream.extend_from_slice(hex(&data).as_bytes());
        stream.extend_from_slice(b" EI Q BT (after) Tj ET");
        let (cleaned, images) = extract(&stream);
        let text = String::from_utf8_lossy(&cleaned);
        assert_eq!(
            text,
            format!("BT (before) Tj ET q 0 {INLINE_OP} Q BT (after) Tj ET")
        );
        assert_eq!(images.len(), 1);
        assert!(images[0].encoded.is_none());
        let missing = images[0].missing.as_deref().unwrap_or_default();
        assert_eq!(missing, "inline image mask is not carried");
    }

    #[test]
    fn a_gray_image_behind_a_filter_array_decodes() {
        let data = miniz_oxide::deflate::compress_to_vec_zlib(&[0, 64, 128, 255], 6);
        let mut stream = Vec::new();
        stream.extend_from_slice(b"BI /W 2 /H 2 /BPC 8 /CS /G /F [/AHx /Fl] ID ");
        stream.extend_from_slice(hex(&data).as_bytes());
        stream.extend_from_slice(b" EI");
        let (_, images) = extract(&stream);
        assert_eq!(images.len(), 1);
        let Some(crate::image::Encoded::Raw {
            samples,
            components,
            ..
        }) = &images[0].encoded
        else {
            panic!("not decoded: {:?}", images[0].missing);
        };
        assert_eq!(*components, 1);
        assert_eq!(samples.as_slice(), &[0, 64, 128, 255]);
    }

    #[test]
    fn decode_parms_are_skipped_whole_and_reported() {
        let mut stream = Vec::new();
        stream.extend_from_slice(
            b"BI /W 1 /H 1 /CS /G /DP << /Predictor 15 /Columns 1 >> /F /Fl ID x EI Q",
        );
        let (cleaned, images) = extract(&stream);
        assert_eq!(
            String::from_utf8_lossy(&cleaned),
            format!("0 {INLINE_OP} Q")
        );
        let missing = images[0].missing.as_deref().unwrap_or_default();
        assert_eq!(missing, "inline image DecodeParms is not carried");
    }

    #[test]
    fn a_small_inline_jpeg_is_still_carried() {
        let mut stream = Vec::new();
        stream.extend_from_slice(b"q BI /W 4 /H 4 /BPC 8 /CS /RGB /F /DCT ID ");
        stream.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
        stream.extend_from_slice(b" EI Q");
        let (_, images) = extract(&stream);
        assert_eq!(images.len(), 1);
        assert!(images[0].encoded.is_some());
    }
}
