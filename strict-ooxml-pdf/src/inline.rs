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
            out.extend_from_slice(&bytes[index..start]);
            if let Some((after, image)) = take_inline(bytes, start) {
                let n = images.len();
                images.push(image);
                out.extend_from_slice(format!("{n} {INLINE_OP} ").as_bytes());
                index = after;
            } else {
                // A BI we cannot finish: copy the BI token and continue so
                // the rest of the stream still tokenises.
                out.extend_from_slice(&bytes[start..start + 2]);
                index = start + 2;
            }
        } else {
            out.extend_from_slice(&bytes[index..]);
            break;
        }
    }
    (out, images)
}

/// Finds `BI` as a free-standing operator at or after `from`.
fn find_bi(bytes: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index + 2 <= bytes.len() {
        if bytes[index] == b'B'
            && bytes[index + 1] == b'I'
            && is_delim_before(bytes, index)
            && is_delim_after(bytes, index + 2)
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn is_delim_before(bytes: &[u8], index: usize) -> bool {
    index == 0 || is_whitespace(bytes[index - 1]) || is_delimiter(bytes[index - 1])
}

fn is_delim_after(bytes: &[u8], index: usize) -> bool {
    index >= bytes.len() || is_whitespace(bytes[index]) || is_delimiter(bytes[index])
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
    let mut index = bi + 2;
    while index < bytes.len() && is_whitespace(bytes[index]) {
        index += 1;
    }
    let (id_at, dict) = parse_inline_dict(bytes, index)?;
    // Skip "ID" and the single whitespace that follows (ISO 32000-1 §8.9.7).
    let mut data_start = id_at + 2;
    if data_start < bytes.len() && is_whitespace(bytes[data_start]) {
        data_start += 1;
    }
    let (ei_at, data_end) = find_ei(bytes, data_start, &dict)?;
    let data = bytes[data_start..data_end].to_vec();
    let image = decode_inline(&dict, &data);
    let after = {
        let mut end = ei_at + 2;
        while end < bytes.len() && is_whitespace(bytes[end]) {
            end += 1;
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
    filter: Option<Vec<u8>>,
}

impl InlineDict {
    fn expected_bytes(&self) -> Option<usize> {
        let width = usize::try_from(self.width?).ok()?;
        let height = usize::try_from(self.height?).ok()?;
        let bits = usize::try_from(self.bits.unwrap_or(8)).ok()?;
        let components = usize::from(self.components.unwrap_or(3));
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
        filter: None,
    };
    while index < bytes.len() {
        while index < bytes.len() && is_whitespace(bytes[index]) {
            index += 1;
        }
        if index + 2 <= bytes.len()
            && &bytes[index..index + 2] == b"ID"
            && is_delim_before(bytes, index)
            && is_delim_after(bytes, index + 2)
        {
            return Some((index, dict));
        }
        if index >= bytes.len() || bytes[index] != b'/' {
            return None;
        }
        index += 1;
        let key_start = index;
        while index < bytes.len() && !is_whitespace(bytes[index]) && !is_delimiter(bytes[index]) {
            index += 1;
        }
        let key = &bytes[key_start..index];
        while index < bytes.len() && is_whitespace(bytes[index]) {
            index += 1;
        }
        let (value, next) = parse_value(bytes, index)?;
        index = next;
        match key {
            b"W" | b"Width" => dict.width = value.as_number(),
            b"H" | b"Height" => dict.height = value.as_number(),
            b"BPC" | b"BitsPerComponent" => dict.bits = value.as_number(),
            b"CS" | b"ColorSpace" => dict.components = value.components(),
            b"F" | b"Filter" => dict.filter = value.as_name(),
            _ => {}
        }
    }
    None
}

enum Value {
    Number(u32),
    Name(Vec<u8>),
    Other,
}

impl Value {
    fn as_number(&self) -> Option<u32> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn as_name(&self) -> Option<Vec<u8>> {
        match self {
            Self::Name(n) => Some(n.clone()),
            _ => None,
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

fn parse_value(bytes: &[u8], index: usize) -> Option<(Value, usize)> {
    if index >= bytes.len() {
        return None;
    }
    if bytes[index] == b'/' {
        let mut end = index + 1;
        while end < bytes.len() && !is_whitespace(bytes[end]) && !is_delimiter(bytes[end]) {
            end += 1;
        }
        return Some((Value::Name(bytes[index + 1..end].to_vec()), end));
    }
    if bytes[index] == b'[' {
        // Skip array values we do not need (e.g. Decode).
        let mut end = index + 1;
        let mut depth = 1;
        while end < bytes.len() && depth > 0 {
            match bytes[end] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        return Some((Value::Other, end));
    }
    if bytes[index].is_ascii_digit() || bytes[index] == b'-' || bytes[index] == b'+' {
        let mut end = index;
        while end < bytes.len()
            && (bytes[end].is_ascii_digit() || matches!(bytes[end], b'-' | b'+' | b'.'))
        {
            end += 1;
        }
        let text = std::str::from_utf8(&bytes[index..end]).ok()?;
        let number = text.parse::<f64>().ok()?;
        return Some((Value::Number(number as u32), end));
    }
    // Bare name without slash (rare) or unknown token: skip one token.
    let mut end = index;
    while end < bytes.len() && !is_whitespace(bytes[end]) && !is_delimiter(bytes[end]) {
        end += 1;
    }
    Some((Value::Other, end.max(index + 1)))
}

/// Finds `EI` after `data_start`. Prefer the length implied by the dictionary;
/// fall back to the delimiter rule (ISO 32000-1 §8.9.7).
fn find_ei(bytes: &[u8], data_start: usize, dict: &InlineDict) -> Option<(usize, usize)> {
    if let Some(expected) = dict.expected_bytes() {
        let data_end = data_start.checked_add(expected)?;
        if data_end + 2 <= bytes.len() {
            let mut ei = data_end;
            // Optional whitespace between samples and EI.
            while ei < bytes.len() && is_whitespace(bytes[ei]) {
                ei += 1;
            }
            if ei + 2 <= bytes.len() && &bytes[ei..ei + 2] == b"EI" && is_delim_after(bytes, ei + 2)
            {
                return Some((ei, data_end.min(ei)));
            }
        }
    }
    // Delimiter search: whitespace (or start-of-data) + EI + whitespace/delimiter/end.
    let mut index = data_start;
    while index + 2 <= bytes.len() {
        if &bytes[index..index + 2] == b"EI"
            && (index == data_start || is_whitespace(bytes[index - 1]))
            && is_delim_after(bytes, index + 2)
        {
            // data_end excludes the whitespace before EI when present.
            let data_end = if index > data_start && is_whitespace(bytes[index - 1]) {
                index - 1
            } else {
                index
            };
            return Some((index, data_end));
        }
        index += 1;
    }
    None
}

fn decode_inline(dict: &InlineDict, data: &[u8]) -> InlineImage {
    let width = match dict.width {
        Some(w) if w > 0 => w,
        _ => {
            return InlineImage {
                encoded: None,
                missing: Some("inline image has no Width".into()),
            };
        }
    };
    let height = match dict.height {
        Some(h) if h > 0 => h,
        _ => {
            return InlineImage {
                encoded: None,
                missing: Some("inline image has no Height".into()),
            };
        }
    };
    let is_jpx = matches!(dict.filter.as_deref(), Some(b"JPX" | b"JPXDecode"));
    // JPX carries bit depth in the codestream; other filters still need 8-bit.
    if !is_jpx {
        let bits = dict.bits.unwrap_or(8);
        if bits != 8 {
            return InlineImage {
                encoded: None,
                missing: Some("inline image bits per component is not 8".into()),
            };
        }
    }
    let components = dict.components.unwrap_or(3);
    match dict.filter.as_deref() {
        Some(b"DCT" | b"DCTDecode") => InlineImage {
            encoded: Some(Encoded::Jpeg {
                width,
                height,
                data: Arc::new(data.to_vec()),
                alpha: None,
            }),
            missing: None,
        },
        Some(b"JPX" | b"JPXDecode") => {
            // Inline JPX uses the same decoder as XObject `/JPXDecode`. Limits
            // are the defaults: an inline image large enough to matter has
            // already paid for the content-stream budget.
            match crate::image::decode_jpx_bytes(data, &crate::PdfLimits::default()) {
                Ok(encoded) => InlineImage {
                    encoded: Some(encoded),
                    missing: None,
                },
                Err(reject) => InlineImage {
                    encoded: None,
                    missing: Some(reject.to_string()),
                },
            }
        }
        None | Some(b"Fl" | b"FlateDecode") => {
            let samples = if matches!(dict.filter.as_deref(), Some(b"Fl" | b"FlateDecode")) {
                match miniz_oxide::inflate::decompress_to_vec_zlib(data) {
                    Ok(raw) => raw,
                    Err(_) => {
                        return InlineImage {
                            encoded: None,
                            missing: Some("inline image FlateDecode failed".into()),
                        };
                    }
                }
            } else {
                data.to_vec()
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
        Some(other) => InlineImage {
            encoded: None,
            missing: Some(format!(
                "inline image filter /{} is not carried",
                String::from_utf8_lossy(other)
            )),
        },
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
}
