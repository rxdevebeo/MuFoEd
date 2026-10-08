//! The object-stream budget, checked on the raw bytes before `lopdf` loads them.
//!
//! `lopdf::Document::load_mem` inflates every object stream (`/Type /ObjStm`)
//! and every cross-reference stream (`/Type /XRef`) while it builds the object
//! graph, before this crate sees a single object. A 256 KiB file whose object
//! stream inflates to gigabytes used to exhaust memory right there (waiver
//! `PDF-OBJSTM-BOMB`). Two fences close that:
//!
//! 1. **This scan.** Every object or cross-reference stream dictionary in the
//!    file is found, and a `/FlateDecode` one is inflated into a fixed scratch
//!    buffer that is thrown away, counting bytes. Their **sum** over the whole
//!    file is held to [`object_stream_budget`]; past it the file is refused with
//!    [`LimitKind::ObjectStreamBytes`] before `lopdf` is called. Nothing the
//!    scan inflates is kept, so the scan itself costs one 32 KiB buffer and one
//!    inflater, whatever the file claims.
//! 2. **`LoadOptions::max_decompressed_size`.** The same budget is handed to
//!    `lopdf` as its per-stream ceiling, so a stream the scan could not read (an
//!    indirect `/Filter`, a chain such as `[/ASCII85Decode /FlateDecode]`, a
//!    dictionary the scan could not parse) is still bounded one stream at a
//!    time. `lopdf` drops such a stream rather than failing the load.
//!
//! The scan is bounded on every axis a hostile file controls: the number of
//! stream dictionaries examined ([`MAX_STREAM_DICTIONARIES`]), the bytes
//! examined around each ([`DICTIONARY_WINDOW`]), the bytes inflated (the
//! budget), and the compressed bytes consumed (the file's own length — honest
//! object streams do not overlap, so a file that makes the scan read the same
//! bytes twice is refused as malformed).

use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

use crate::error::{LimitKind, PdfError, PdfLimits, OBJECT_STREAM_BUDGET_FACTOR};

/// Object or cross-reference stream dictionaries examined per file.
///
/// A real file of 256 MiB at a hundred objects per stream has fewer. Past this
/// count the scan stops and the remaining streams are bounded by `lopdf`'s
/// per-stream ceiling alone.
pub(crate) const MAX_STREAM_DICTIONARIES: usize = 65_536;

/// Bytes looked at around one `/Type` key to find its dictionary.
///
/// An object stream dictionary is `/Type /ObjStm /N /First /Length /Filter`,
/// perhaps `/Extends` and `/DecodeParms`: well under a kilobyte.
pub(crate) const DICTIONARY_WINDOW: usize = 4096;

/// The scratch buffer inflated output is written to and discarded from.
const SCRATCH_BYTES: usize = 32 * 1024;

/// The decompressed bytes all object and cross-reference streams of one file
/// may reach (see [`OBJECT_STREAM_BUDGET_FACTOR`]).
#[must_use]
pub(crate) fn object_stream_budget(limits: &PdfLimits) -> usize {
    limits
        .max_content_bytes
        .saturating_mul(OBJECT_STREAM_BUDGET_FACTOR)
}

/// Refuses a file whose object and cross-reference streams inflate past the
/// budget, before `lopdf` inflates them.
///
/// # Errors
///
/// [`PdfError::LimitExceeded`] with [`LimitKind::ObjectStreamBytes`] past the
/// budget, and [`PdfError::Malformed`] for a file whose stream data overlaps.
pub(crate) fn check_object_streams(bytes: &[u8], limits: &PdfLimits) -> Result<(), PdfError> {
    let budget = object_stream_budget(limits);
    let mut meter = Meter::new(budget, bytes.len());
    let mut examined = 0usize;
    let mut cursor = 0usize;
    let mut last_charged = None;
    while let Some(at) = find(bytes, b"/Type", cursor) {
        cursor = at + b"/Type".len();
        let Some((name, _)) = name_after(bytes, cursor) else {
            continue;
        };
        if name != b"ObjStm" && name != b"XRef" {
            continue;
        }
        examined += 1;
        if examined > MAX_STREAM_DICTIONARIES {
            break;
        }
        let Some(stream) = locate_stream(bytes, at) else {
            continue;
        };
        // A dictionary that writes `/Type` twice is found twice; its data is
        // charged once.
        if last_charged == Some(stream.data_start) {
            continue;
        }
        last_charged = Some(stream.data_start);
        meter.charge(bytes, &stream)?;
        if meter.inflated > budget {
            return Err(limits.exceeded(LimitKind::ObjectStreamBytes, meter.inflated as u64));
        }
    }
    Ok(())
}

/// What the scan learned about one stream.
struct StreamSpan {
    /// Where the stream data starts (after `stream` and its end of line).
    data_start: usize,
    /// The data length, when `/Length` is a direct integer that lands on
    /// `endstream`.
    length: Option<usize>,
    /// Whether the filter chain is exactly one `/FlateDecode`.
    flate: bool,
}

/// Bytes inflated and consumed so far, against their ceilings.
struct Meter {
    budget: usize,
    inflated: usize,
    consumed: usize,
    consumed_ceiling: usize,
    state: Option<Box<InflateState>>,
    scratch: Vec<u8>,
}

impl Meter {
    fn new(budget: usize, file_len: usize) -> Self {
        Self {
            budget,
            inflated: 0,
            consumed: 0,
            consumed_ceiling: file_len,
            state: None,
            scratch: Vec::new(),
        }
    }

    fn charge(&mut self, bytes: &[u8], stream: &StreamSpan) -> Result<(), PdfError> {
        if !stream.flate {
            // Unfiltered data is already in the file: `lopdf` holds a copy of
            // it, not an expansion. Any other chain is left to `lopdf`'s
            // per-stream ceiling (see the module documentation).
            return Ok(());
        }
        let Some(rest) = bytes.get(stream.data_start..) else {
            return Ok(());
        };
        let data = match stream.length {
            Some(length) => rest.get(..length).unwrap_or(rest),
            None => rest,
        };
        // `lopdf` retries a stream whose zlib header is bad as raw deflate from
        // the third byte; the scan follows it there, or it would undercount.
        let (produced, consumed) = self.inflate(data, DataFormat::Zlib);
        let (produced, consumed) = if produced == 0 && data.len() > 2 {
            let raw = data.get(2..).unwrap_or_default();
            let (raw_produced, raw_consumed) = self.inflate(raw, DataFormat::Raw);
            (raw_produced, consumed.max(raw_consumed.saturating_add(2)))
        } else {
            (produced, consumed)
        };
        self.inflated = self.inflated.saturating_add(produced);
        self.consumed = self.consumed.saturating_add(consumed);
        if self.consumed > self.consumed_ceiling {
            return Err(PdfError::Malformed(
                "object stream data overlaps another stream's; the file was not loaded".to_owned(),
            ));
        }
        Ok(())
    }

    /// Inflates `data` into the scratch buffer, stopping once the remaining
    /// budget is passed. Returns `(bytes produced, bytes consumed)`.
    fn inflate(&mut self, data: &[u8], format: DataFormat) -> (usize, usize) {
        let remaining = self.budget.saturating_sub(self.inflated);
        if self.scratch.is_empty() {
            self.scratch = vec![0; SCRATCH_BYTES];
        }
        let state = self
            .state
            .get_or_insert_with(|| InflateState::new_boxed(format));
        state.reset(format);
        let mut produced = 0usize;
        let mut consumed = 0usize;
        loop {
            let input = data.get(consumed..).unwrap_or_default();
            let result = inflate(state, input, &mut self.scratch, MZFlush::None);
            consumed = consumed.saturating_add(result.bytes_consumed);
            produced = produced.saturating_add(result.bytes_written);
            if produced > remaining {
                break;
            }
            let finished = matches!(result.status, Ok(MZStatus::StreamEnd) | Err(_));
            let stalled = result.bytes_consumed == 0 && result.bytes_written == 0;
            if finished || stalled {
                break;
            }
        }
        (produced, consumed.min(data.len()))
    }
}

/// The stream whose dictionary holds the `/Type` key at `type_at`, if the
/// bytes around it parse as `N G obj << … >> stream`.
fn locate_stream(bytes: &[u8], type_at: usize) -> Option<StreamSpan> {
    // The dictionary opens after the nearest `obj` before the key. A string
    // containing `obj` in front of the key would stop the search early, so a
    // few earlier ones are tried before giving up on the candidate.
    let floor = type_at.saturating_sub(DICTIONARY_WINDOW);
    let mut before = type_at;
    for _ in 0..4 {
        let obj = rfind(bytes, b"obj", floor, before)?;
        before = obj;
        let open = skip_space(bytes, obj + b"obj".len());
        if !bytes.get(open..).unwrap_or_default().starts_with(b"<<") {
            continue;
        }
        let Some(dictionary) = parse_dictionary(bytes, open) else {
            continue;
        };
        if !dictionary.covers(type_at) {
            continue;
        }
        let keyword = skip_space(bytes, dictionary.end);
        if !bytes.get(keyword..).unwrap_or_default().starts_with(b"stream") {
            return None;
        }
        let mut data_start = keyword + b"stream".len();
        let after_keyword = bytes.get(data_start..).unwrap_or_default();
        if after_keyword.starts_with(b"\r\n") {
            data_start += 2;
        } else if after_keyword.starts_with(b"\n") || after_keyword.starts_with(b"\r") {
            data_start += 1;
        }
        // `lopdf` trusts `/Length` only when `endstream` follows it; otherwise
        // it searches for `endstream`, and so does the inflater here by reading
        // until the deflate data ends.
        let length = dictionary.length.filter(|length| {
            data_start
                .checked_add(*length)
                .filter(|end| *end <= bytes.len())
                .and_then(|end| bytes.get(skip_space(bytes, end)..))
                .is_some_and(|rest| rest.starts_with(b"endstream"))
        });
        return Some(StreamSpan {
            data_start,
            length,
            flate: dictionary.flate,
        });
    }
    None
}

/// The top-level facts of a dictionary that matter to the scan.
struct Dictionary {
    /// The index of the opening `<<`.
    start: usize,
    /// The index just past the closing `>>`.
    end: usize,
    /// A direct integer `/Length`.
    length: Option<usize>,
    /// Whether `/Filter` is `/FlateDecode` (or `/Fl`), alone.
    flate: bool,
}

impl Dictionary {
    fn covers(&self, at: usize) -> bool {
        self.start < at && at < self.end
    }
}

/// Parses the dictionary opening at `open`, within [`DICTIONARY_WINDOW`] bytes.
///
/// Strings, hex strings, comments and nested dictionaries are stepped over, so a
/// `>>` inside `(…)` does not end the dictionary early.
fn parse_dictionary(bytes: &[u8], open: usize) -> Option<Dictionary> {
    let limit = bytes.len().min(open.saturating_add(DICTIONARY_WINDOW));
    let window = bytes.get(..limit)?;
    let mut depth = 0usize;
    let mut index = open;
    let mut length = None;
    let mut flate = false;
    while let Some(&byte) = window.get(index) {
        match byte {
            b'<' if window.get(index + 1) == Some(&b'<') => {
                depth += 1;
                index += 2;
            }
            b'>' if window.get(index + 1) == Some(&b'>') => {
                depth = depth.checked_sub(1)?;
                index += 2;
                if depth == 0 {
                    return Some(Dictionary {
                        start: open,
                        end: index,
                        length,
                        flate,
                    });
                }
            }
            b'<' => index = skip_hex_string(window, index)?,
            b'(' => index = skip_literal_string(window, index)?,
            b'%' => index = skip_comment(window, index),
            b'/' => {
                let (name, after) = read_name(window, index)?;
                index = after;
                if depth != 1 {
                    continue;
                }
                match name {
                    b"Length" => {
                        let (value, after_value) = direct_integer(window, after);
                        length = value;
                        index = after_value;
                    }
                    b"Filter" => {
                        let (is_flate, after_value) = filter_is_flate(window, after)?;
                        flate = is_flate;
                        index = after_value;
                    }
                    _ => {}
                }
            }
            _ => index += 1,
        }
    }
    None
}

/// `/Length`'s value: `Some(n)` for a direct integer, `None` for a reference
/// (`12 0 R`) or anything else. Returns where parsing stopped.
fn direct_integer(bytes: &[u8], from: usize) -> (Option<usize>, usize) {
    let start = skip_space(bytes, from);
    let digits = bytes
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return (None, start);
    }
    let end = start + digits;
    let value = bytes
        .get(start..end)
        .and_then(|digits| std::str::from_utf8(digits).ok())
        .and_then(|text| text.parse::<usize>().ok());
    // `12 0 R` is a reference, not a length.
    let next = skip_space(bytes, end);
    let generation = bytes
        .get(next..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if generation > 0 {
        let after = skip_space(bytes, next + generation);
        if bytes.get(after) == Some(&b'R') {
            return (None, after + 1);
        }
    }
    (value, end)
}

/// Whether `/Filter`'s value is exactly one Flate filter. Returns where parsing
/// stopped.
fn filter_is_flate(bytes: &[u8], from: usize) -> Option<(bool, usize)> {
    let start = skip_space(bytes, from);
    match bytes.get(start) {
        Some(b'/') => {
            let (name, after) = read_name(bytes, start)?;
            Some((is_flate_name(name), after))
        }
        Some(b'[') => {
            let mut index = start + 1;
            let mut names = 0usize;
            let mut all_flate = true;
            loop {
                index = skip_space(bytes, index);
                match bytes.get(index) {
                    Some(b'/') => {
                        let (name, after) = read_name(bytes, index)?;
                        names += 1;
                        all_flate &= is_flate_name(name);
                        index = after;
                    }
                    Some(b']') => return Some((names == 1 && all_flate, index + 1)),
                    // A reference or anything else inside the array: not a
                    // chain the scan reads; `lopdf`'s ceiling applies.
                    Some(_) => return Some((false, index)),
                    None => return None,
                }
            }
        }
        _ => Some((false, start)),
    }
}

fn is_flate_name(name: &[u8]) -> bool {
    name == b"FlateDecode" || name == b"Fl"
}

/// The name after `/Type` (or any key) at `from`: `(name without the slash,
/// index after it)`.
fn name_after(bytes: &[u8], from: usize) -> Option<(&[u8], usize)> {
    let start = skip_space(bytes, from);
    if bytes.get(start) != Some(&b'/') {
        return None;
    }
    read_name(bytes, start)
}

/// Reads the name whose `/` is at `slash`.
fn read_name(bytes: &[u8], slash: usize) -> Option<(&[u8], usize)> {
    let start = slash.checked_add(1)?;
    let rest = bytes.get(start..)?;
    let len = rest.iter().take_while(|byte| is_regular(**byte)).count();
    Some((rest.get(..len)?, start + len))
}

/// A PDF regular character: neither white space nor a delimiter.
fn is_regular(byte: u8) -> bool {
    !is_space(byte)
        && !matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}

fn skip_space(bytes: &[u8], from: usize) -> usize {
    let mut index = from.min(bytes.len());
    while bytes.get(index).is_some_and(|&byte| is_space(byte)) {
        index += 1;
    }
    index
}

fn skip_comment(bytes: &[u8], from: usize) -> usize {
    let mut index = from;
    while bytes.get(index).is_some_and(|&byte| byte != b'\n' && byte != b'\r') {
        index += 1;
    }
    index
}

fn skip_hex_string(bytes: &[u8], from: usize) -> Option<usize> {
    let close = bytes.get(from..)?.iter().position(|byte| *byte == b'>')?;
    Some(from + close + 1)
}

fn skip_literal_string(bytes: &[u8], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = from;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'(' => {
                depth += 1;
                index += 1;
            }
            b')' => {
                depth = depth.checked_sub(1)?;
                index += 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    None
}

/// The first `needle` at or after `from`.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

/// The last `needle` that starts in `floor..before`.
fn rfind(haystack: &[u8], needle: &[u8], floor: usize, before: usize) -> Option<usize> {
    haystack
        .get(floor..before.min(haystack.len()))?
        .windows(needle.len())
        .rposition(|window| window == needle)
        .map(|offset| floor + offset)
}

#[cfg(test)]
mod tests {
    use super::{check_object_streams, parse_dictionary};
    use crate::error::{LimitKind, PdfError, PdfLimits};

    fn zlib(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(data, 9)
    }

    fn object_stream_file(payload: &[u8], dictionary: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n5 0 obj\n".to_vec();
        out.extend_from_slice(
            dictionary
                .replace("{len}", &payload.len().to_string())
                .as_bytes(),
        );
        out.extend_from_slice(b"\nstream\n");
        out.extend_from_slice(payload);
        out.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
        out
    }

    fn small_limits() -> PdfLimits {
        PdfLimits {
            max_content_bytes: 64 * 1024,
            ..PdfLimits::default()
        }
    }

    #[test]
    fn a_dictionary_with_strings_and_nesting_parses_to_its_end() {
        let bytes = b"<< /Type /ObjStm /S (a >> b \\) c) /D << /K 1 >> /H <3e3e> /Length 7 /Filter [/FlateDecode] >> stream";
        let dictionary = parse_dictionary(bytes, 0).expect("parses");
        assert_eq!(dictionary.length, Some(7));
        assert!(dictionary.flate);
        assert_eq!(&bytes[dictionary.end..], b" stream");
    }

    #[test]
    fn an_indirect_length_is_not_a_length() {
        let bytes = b"<< /Length 12 0 R /Filter /FlateDecode >>";
        let dictionary = parse_dictionary(bytes, 0).expect("parses");
        assert_eq!(dictionary.length, None);
        assert!(dictionary.flate);
    }

    #[test]
    fn an_object_stream_past_the_budget_is_refused() {
        let payload = zlib(&vec![b' '; 1024 * 1024]);
        let file = object_stream_file(
            &payload,
            "<< /Type /ObjStm /N 1 /First 4 /Length {len} /Filter /FlateDecode >>",
        );
        let error = check_object_streams(&file, &small_limits()).expect_err("bomb");
        assert!(
            matches!(
                error,
                PdfError::LimitExceeded {
                    kind: LimitKind::ObjectStreamBytes,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn an_indirect_length_does_not_hide_the_bomb() {
        let payload = zlib(&vec![b' '; 1024 * 1024]);
        let file = object_stream_file(
            &payload,
            "<< /Type/XRef /Length 9 0 R /Filter/FlateDecode /W [1 2 1] >>",
        );
        assert!(check_object_streams(&file, &small_limits()).is_err());
    }

    #[test]
    fn a_small_object_stream_passes() {
        let payload = zlib(b"1 0 << /A 1 >>");
        let file = object_stream_file(
            &payload,
            "<< /Type /ObjStm /N 1 /First 4 /Length {len} /Filter /FlateDecode >>",
        );
        assert!(check_object_streams(&file, &small_limits()).is_ok());
    }

    #[test]
    fn garbage_around_type_keys_is_survived() {
        let mut file = Vec::new();
        let chunks: [&[u8]; 7] = [
            b"/Type /ObjStm",
            b"obj << /Type /ObjStm",
            b"obj << /Type /XRef >> stream",
            b"obj << /Type /XRef /Filter /FlateDecode >> stream\r\n\x78",
            b"obj << ( /Type /ObjStm",
            b"/Type",
            b"/Type /",
        ];
        for chunk in chunks {
            file.extend_from_slice(chunk);
            assert!(check_object_streams(&file, &small_limits()).is_ok());
            assert!(check_object_streams(chunk, &small_limits()).is_ok());
        }
    }
}
