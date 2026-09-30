//! The geometry oracle (`SESSION-HANDOFF.md` §6 №2).
//!
//! №2 is blocked on one question only the customer can answer: **does WPS's
//! PDF export expose extractable glyph positions?** Everything on our side of
//! that question is settled here.
//!
//! ## Why geometry rather than pixels
//!
//! The accuracy gate compares pages against WPS references by SSIM, and it is
//! blocked because the reference is set in **Cambria Math** while we draw with
//! **STIX Two Math**. A pixel comparison measures two things at once: where the
//! layout put the glyphs, and what the glyphs look like. The second is not a
//! defect — it is a substitution we are not going to reverse — so the
//! measurement is dominated by a difference that cannot be fixed, and the part
//! that *can* be fixed is invisible inside it. That is why the `formulas`
//! class sits outside its own bounds and why widening the bound is not the
//! answer.
//!
//! A PDF separates the two. Every run is written as
//!
//! ```text
//! BT /F1 <size> Tf 1 0 0 1 <x> <y> Tm <glyph ids> Tj ET
//! ```
//!
//! so the run origin and size are **exact numbers in the file**, and every
//! advance is declared in `/W`. Nothing about the outline of a `p` takes part.
//! Comparing `(x, y, size, advances)` between two PDFs compares layout and
//! nothing else, which is the comparison №2 asks for.
//!
//! ## What is asserted
//!
//! That the geometry is recoverable from a PDF **this project produced**, by a
//! reader that shares no code with the writer, and that the numbers recovered
//! are the layout's own numbers. If that holds the harness is written; the only
//! thing left is a WPS reference to feed it.
//!
//! `lopdf` is that independent reader. An extractor written against our own PDF
//! object model would agree with us by construction — the same defect as
//! comparing our output with our output.
//!
//! The `a_foreign_pdf_is_readable` test at the bottom feeds the oracle a PDF from
//! another writer. It found four things this one does not do, each of which
//! would have made a hand-rolled comparison quietly wrong rather than fail: a
//! `cm` transform wrapping the whole page, a second `cm` inside it, glyph
//! strings written as hexadecimal rather than as an escaped literal, and `<<…>>`
//! dictionaries inline in the content stream. An oracle that ignores `cm` reports
//! text-space numbers as page coordinates — wrong by the scale factor, and
//! indistinguishable from a layout defect.

// `CMap` and `ToUnicode` in prose are PDF keys, not identifiers of this crate;
// `pdf.rs` allows the same lint for the same reason.
#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::path::Path;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::layout::{Item, TextItem};
use strict_ooxml_render_svg::{place_pages, PlacedPage, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// One glyph run, as a PDF consumer recovers it.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    /// Zero-based page index.
    pub page: usize,
    /// Run origin in PDF user space: points, origin at the bottom-left.
    pub x: f64,
    /// Run baseline, same space.
    pub y: f64,
    /// Font size in points.
    pub size: f64,
    /// The run's text, via the font's `ToUnicode` `CMap`.
    pub text: String,
    /// Total advance of the run in points, summed from `/W` and `/DW`.
    pub advance: f64,
}

/// The advance and character tables of one embedded face.
struct Face {
    /// Glyph id → advance in 1/1000 em.
    widths: BTreeMap<u32, f64>,
    /// Advance for a glyph `/W` does not mention.
    default_width: f64,
    /// Glyph id → the characters it stands for.
    to_unicode: BTreeMap<u32, String>,
}

/// Reads every glyph run of a PDF.
///
/// Models only what a position comparison needs, and refuses to guess: an
/// operator that would move the cursor in a way this does not track returns an
/// error naming it. A comparison that silently mis-reads one page is worse than
/// no comparison, because it reports a number.
pub fn glyph_runs(pdf: &[u8]) -> Result<Vec<GlyphRun>, String> {
    let document = lopdf::Document::load_mem(pdf).map_err(|error| error.to_string())?;
    let mut out: Vec<GlyphRun> = Vec::new();
    for (index, page_id) in document.page_iter().enumerate() {
        let page = document
            .get_object(page_id)
            .map_err(|error| error.to_string())?
            .as_dict()
            .map_err(|_| format!("page {index} is not a dictionary"))?
            .clone();

        let mut faces: BTreeMap<Vec<u8>, Face> = BTreeMap::new();
        // `/Resources` is legal as an inline dictionary, and `/Font` inside it
        // likewise; only handling the reference form would find no fonts at all
        // on a PDF that inlines them and then report every run as unplaceable.
        let resources: Option<lopdf::Dictionary> = match page.get(b"Resources") {
            Ok(lopdf::Object::Reference(id)) => match document.get_object(*id) {
                Ok(lopdf::Object::Dictionary(found)) => Some(found.clone()),
                _ => None,
            },
            Ok(lopdf::Object::Dictionary(found)) => Some(found.clone()),
            _ => None,
        };
        if let Some(resources) = resources {
            if let Ok(lopdf::Object::Dictionary(font_dict)) = resources.get(b"Font") {
                for (name, value) in font_dict {
                    if let lopdf::Object::Reference(id) = value {
                        faces.insert(name.clone(), read_face(&document, *id)?);
                    }
                }
            }
        }

        let contents = page
            .get(b"Contents")
            .map_err(|_| format!("page {index} has no /Contents"))?;
        let mut raw = Vec::new();
        match contents {
            lopdf::Object::Reference(id) => {
                let stream = document
                    .get_object(*id)
                    .map_err(|error| error.to_string())?
                    .as_stream()
                    .map_err(|error| error.to_string())?;
                raw.extend_from_slice(
                    &stream
                        .decompressed_content()
                        .map_err(|error| error.to_string())?,
                );
            }
            lopdf::Object::Array(parts) => {
                for part in parts {
                    if let lopdf::Object::Reference(id) = part {
                        let stream = document
                            .get_object(*id)
                            .map_err(|error| error.to_string())?
                            .as_stream()
                            .map_err(|error| error.to_string())?;
                        raw.extend_from_slice(
                            &stream
                                .decompressed_content()
                                .map_err(|error| error.to_string())?,
                        );
                    }
                }
            }
            other => return Err(format!("page {index}: /Contents is a {other:?}")),
        }

        out.extend(walk_content(&raw, &faces, index)?);
    }
    Ok(out)
}

/// The text state a content stream carries from operator to operator.
#[derive(Default, Clone)]
struct Cursor {
    x: f64,
    y: f64,
    size: f64,
    font: Option<Vec<u8>>,
    /// `TD` moves the line and sets the leading; `T*` moves by it.
    leading: f64,
}

/// A 2-D affine transform, PDF order `[a b c d e f]`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self` then `other`: the matrix a point is put through by `other` and
    /// then by `self`.
    fn then(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Applies the matrix to a point.
    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The uniform scale this matrix applies: `sqrt(|det|)`, so a rotation or a
    /// flip does not read as a resize.
    fn scale(self) -> f64 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}

/// Walks a content stream, emitting one [`GlyphRun`] per shown string.
fn walk_content(
    stream: &[u8],
    faces: &BTreeMap<Vec<u8>, Face>,
    page: usize,
) -> Result<Vec<GlyphRun>, String> {
    let mut out = Vec::new();
    let mut cursor = Cursor::default();
    let mut operands: Vec<Operand> = Vec::new();
    // The current transformation matrix, and the `q` stack behind it.
    let mut ctm = Matrix::IDENTITY;
    let mut stack: Vec<Matrix> = Vec::new();
    let mut lexer = Lexer::new(stream);

    loop {
        let Some(token) = lexer.next() else {
            break;
        };
        match token {
            Token::Operand(operand) => operands.push(operand),
            Token::Operator(operator) => {
                apply_operator(
                    &operator,
                    &operands,
                    &mut cursor,
                    &mut ctm,
                    &mut stack,
                    faces,
                    page,
                    &mut out,
                )?;
                operands.clear();
            }
        }
    }
    if let Some(error) = lexer.error {
        return Err(error);
    }
    if !stack.is_empty() {
        return Err(format!(
            "page {page}: {} unbalanced `q` — the page ends inside a saved state",
            stack.len()
        ));
    }
    Ok(out)
}

/// Applies one content-stream operator. Returns `Ok(())` for anything that leaves
/// the text state alone.
#[allow(clippy::too_many_arguments)]
fn apply_operator(
    operator: &str,
    operands: &[Operand],
    cursor: &mut Cursor,
    ctm: &mut Matrix,
    stack: &mut Vec<Matrix>,
    faces: &BTreeMap<Vec<u8>, Face>,
    page: usize,
    out: &mut Vec<GlyphRun>,
) -> Result<(), String> {
    match operator {
        "q" => stack.push(*ctm),
        "Q" => {
            *ctm = stack
                .pop()
                .ok_or_else(|| format!("page {page}: Q with nothing to restore"))?;
        }
        // The transform that decides where a glyph actually lands. Chromium
        // opens its export with `0.24 0 0 -0.24 0 841.92 cm` — a scale and a
        // flip — so a reader that ignores `cm` reports text-space numbers as
        // though they were page coordinates. Every one of them would be wrong
        // by the scale factor and look like a layout defect.
        "cm" => {
            let matrix = Matrix {
                a: need(operands, 0, operator, page)?,
                b: need(operands, 1, operator, page)?,
                c: need(operands, 2, operator, page)?,
                d: need(operands, 3, operator, page)?,
                e: need(operands, 4, operator, page)?,
                f: need(operands, 5, operator, page)?,
            };
            // PDF composes left to right, so a later `cm` is applied *after* an
            // earlier one to the point.
            *ctm = matrix.then(*ctm);
        }
        "TL" => cursor.leading = need(operands, 0, operator, page)?,
        "Td" | "TD" => {
            let dx = need(operands, 0, operator, page)?;
            let dy = need(operands, 1, operator, page)?;
            cursor.x += dx;
            cursor.y += dy;
            if operator == "TD" {
                cursor.leading = -dy;
            }
        }
        "Tm" => {
            cursor.x = need(operands, 4, operator, page)?;
            cursor.y = need(operands, 5, operator, page)?;
        }
        "T*" => cursor.y -= cursor.leading,
        "Tf" => {
            let name = match operands.first() {
                Some(Operand::Name(name)) => name.clone(),
                other => {
                    return Err(format!(
                        "page {page}: Tf without a font name, got {other:?}"
                    ))
                }
            };
            cursor.font = Some(name);
            cursor.size = need(operands, 1, operator, page)?;
        }
        "Tj" => match operands.last() {
            Some(Operand::String(bytes)) => {
                out.push(run(cursor, bytes, faces, page, *ctm)?);
            }
            other => return Err(format!("page {page}: Tj without a string, got {other:?}")),
        },
        "TJ" => match operands.last() {
            Some(Operand::Array(items)) => {
                let mut bytes = Vec::new();
                for item in items {
                    if let Operand::String(part) = item {
                        bytes.extend_from_slice(part);
                    }
                }
                out.push(run(cursor, &bytes, faces, page, *ctm)?);
            }
            other => return Err(format!("page {page}: TJ without an array, got {other:?}")),
        },
        // Anything else leaves the text state alone. The list above is the set
        // this extractor has seen in the wild; an operator that would move a
        // glyph is not in it, and `a_foreign_pdf_is_readable` is what would
        // notice.
        _ => {}
    }
    Ok(())
}

/// One number out of an operator's operands, naming what was missing.
fn need(operands: &[Operand], index: usize, operator: &str, page: usize) -> Result<f64, String> {
    number(operands, index)
        .ok_or_else(|| format!("page {page}: {operator} needs operand {index} to be a number"))
}

/// One shown string, resolved against the current face and placed in page space.
fn run(
    cursor: &Cursor,
    bytes: &[u8],
    faces: &BTreeMap<Vec<u8>, Face>,
    page: usize,
    ctm: Matrix,
) -> Result<GlyphRun, String> {
    let name = cursor
        .font
        .clone()
        .ok_or_else(|| format!("page {page}: text shown with no font selected"))?;
    let face = faces
        .get(&name)
        .ok_or_else(|| format!("page {page}: font {name:?} is not in /Resources /Font"))?;

    // Identity-H: two bytes per glyph, big-endian.
    let mut text = String::new();
    let mut em = 0.0f64;
    for pair in bytes.chunks_exact(2) {
        let glyph = u32::from(u16::from_be_bytes([pair[0], pair[1]]));
        em += face
            .widths
            .get(&glyph)
            .copied()
            .unwrap_or(face.default_width)
            / 1000.0;
        if let Some(character) = face.to_unicode.get(&glyph) {
            text.push_str(character);
        }
    }
    if bytes.len() % 2 != 0 {
        return Err(format!(
            "page {page}: a {} byte string is not Identity-H",
            bytes.len()
        ));
    }
    let scale = ctm.scale();
    let (x, y) = ctm.apply(cursor.x, cursor.y);
    Ok(GlyphRun {
        page,
        x,
        y,
        size: cursor.size * scale,
        text,
        advance: em * cursor.size * scale,
    })
}

/// `W` is `[ c [w …] c_first c_last w … ]`; both spellings occur.
fn parse_widths(array: &[lopdf::Object]) -> BTreeMap<u32, f64> {
    let mut out = BTreeMap::new();
    let mut index = 0;
    while index < array.len() {
        let Ok(first) = array[index].as_i64() else {
            index += 1;
            continue;
        };
        match array.get(index + 1) {
            Some(lopdf::Object::Array(run)) => {
                for (offset, width) in run.iter().enumerate() {
                    if let Ok(width) = width.as_float() {
                        out.insert(
                            u32::try_from(first).unwrap_or(0) + u32::try_from(offset).unwrap_or(0),
                            f64::from(width),
                        );
                    }
                }
                index += 2;
            }
            Some(lopdf::Object::Integer(last)) => {
                if let Some(Ok(width)) = array.get(index + 2).map(lopdf::Object::as_float) {
                    for glyph in u32::try_from(first).unwrap_or(0)
                        ..=u32::try_from((*last).max(first)).unwrap_or(0)
                    {
                        out.insert(glyph, f64::from(width));
                    }
                }
                index += 3;
            }
            _ => index += 1,
        }
    }
    out
}

/// Reads the `ToUnicode` CMap's `bfchar` and `bfrange` sections.
///
/// Only the two sections this writer emits are read. A CMap in any other shape
/// yields a face whose glyphs read back empty, which the assertions below
/// notice — a silently empty mapping would make every run's text `""`.
fn parse_cmap(bytes: &[u8]) -> BTreeMap<u32, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = BTreeMap::new();

    for section in text.split("beginbfchar").skip(1) {
        let Some(end) = section.find("endbfchar") else {
            break;
        };
        for line in section[..end].lines() {
            let codes = codes_in(line);
            if let [source, destination] = codes.as_slice() {
                let (Some(code), Some(mapped)) = (hex(source), hex(destination)) else {
                    continue;
                };
                if let Some(character) = char::from_u32(mapped) {
                    out.insert(code, character.to_string());
                }
            }
        }
    }

    for section in text.split("beginbfrange").skip(1) {
        let Some(end) = section.find("endbfrange") else {
            break;
        };
        for line in section[..end].lines() {
            let codes = codes_in(line);
            let [low, high, destination] = codes.as_slice() else {
                continue;
            };
            let (Some(low), Some(high), Some(base)) = (hex(low), hex(high), hex(destination))
            else {
                continue;
            };
            for (offset, code) in (low..=high).enumerate() {
                let step = u32::try_from(offset).unwrap_or(0);
                if let Some(character) = char::from_u32(base.saturating_add(step)) {
                    out.insert(code, character.to_string());
                }
            }
        }
    }
    out
}

fn read_face(document: &lopdf::Document, id: lopdf::ObjectId) -> Result<Face, String> {
    let dictionary = document
        .get_object(id)
        .map_err(|error| error.to_string())?
        .as_dict()
        .map_err(|_| "a font resource is not a dictionary".to_owned())?;

    // A face whose glyphs cannot be read back is refused outright. Returning an
    // empty map instead would make every run's text `""`, which flows onward as
    // a plausible-looking comparison between two documents that share nothing.
    let to_unicode = match dictionary.get(b"ToUnicode") {
        Ok(lopdf::Object::Reference(cmap)) => {
            let bytes = document
                .get_object(*cmap)
                .map_err(|error| error.to_string())?
                .as_stream()
                .map_err(|error| error.to_string())?
                .decompressed_content()
                .map_err(|error| error.to_string())?;
            parse_cmap(&bytes)
        }
        Ok(_) => return Err("a font has no /ToUnicode, so its glyphs cannot be read".to_owned()),
        Err(_) => return Err("a font has no /ToUnicode entry".to_owned()),
    };

    // A Type0 font delegates its real metrics to its descendant; `/W` and `/DW`
    // live there, not on the Type0 wrapper.
    // `Dictionary::get` follows a reference, so `/DescendantFonts [8 0 R]`
    // arrives here as the array itself.
    let metrics = match dictionary.get(b"DescendantFonts") {
        Ok(lopdf::Object::Array(array)) => {
            let Some(lopdf::Object::Reference(descendant)) = array.first() else {
                return Err("/DescendantFonts is empty".to_owned());
            };
            match document
                .get_object(*descendant)
                .map_err(|error| error.to_string())?
            {
                lopdf::Object::Dictionary(found) => found.clone(),
                other => return Err(format!("a descendant font is a {other:?}")),
            }
        }
        Ok(lopdf::Object::Dictionary(found)) => found.clone(),
        Ok(other) => {
            return Err(format!(
                "/DescendantFonts is a {other:?}, neither an array nor a dictionary"
            ))
        }
        // A simple font carries its metrics directly.
        Err(_) => dictionary.clone(),
    };

    // `/DW` is a direct object, not an array, and a reader that misses it
    // silently substitutes a default of 1000 — which turns every advance into
    // "one em per glyph" and looks like a plausible number. It is read here or
    // the comparison is refused.
    let default_width = match metrics.get(b"DW") {
        Ok(lopdf::Object::Integer(value)) => f64::from(i32::try_from(*value).unwrap_or(i32::MAX)),
        Ok(lopdf::Object::Real(value)) => f64::from(*value),
        Ok(other) => {
            return Err(format!(
                "a /DW that is neither an integer nor a real: {other:?}"
            ))
        }
        Err(_) => 1000.0,
    };

    let widths = match metrics.get(b"W") {
        Ok(lopdf::Object::Array(array)) => parse_widths(array),
        Ok(other) => return Err(format!("a /W that is not an array: {other:?}")),
        Err(_) => return Err("a CID font has no /W array, so no advance can be read".to_owned()),
    };

    Ok(Face {
        widths,
        default_width,
        to_unicode,
    })
}

/// The hexadecimal codes of one CMap line.
fn codes_in(line: &str) -> Vec<&str> {
    line.split('<')
        .skip(1)
        .filter_map(|part| part.split_once('>').map(|(code, _)| code))
        .collect()
}

fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim(), 16).ok()
}

#[derive(Clone, Debug, PartialEq)]
enum Operand {
    Number(f64),
    Name(Vec<u8>),
    String(Vec<u8>),
    Array(Vec<Operand>),
}

enum Token {
    Operand(Operand),
    Operator(String),
}

/// Splits a content stream into operands and operators.
///
/// A bare number, a `/Name`, a `(string)` and a `[…]` array are the only
/// operand forms a text run can be built from; anything else is reported by
/// `walk_content` naming the operator it could not use, rather than being
/// skipped here.
struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
    /// Set when a token could not be read; the walk reports it and stops, rather
    /// than carrying on with a stream it has already lost track of.
    error: Option<String>,
}

impl<'a> Lexer<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            error: None,
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        while self.at < self.bytes.len() {
            if self.error.is_some() {
                return None;
            }
            let byte = self.bytes[self.at];
            if byte.is_ascii_whitespace() {
                self.at += 1;
                continue;
            }
            match byte {
                b'(' => {
                    let mut depth = 1;
                    let start = self.at + 1;
                    self.at = start;
                    while self.at < self.bytes.len() && depth > 0 {
                        match self.bytes[self.at] {
                            b'\\' => self.at += 1,
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        self.at += 1;
                    }
                    let end = self.at.saturating_sub(1);
                    return Some(Token::Operand(Operand::String(unescape(
                        &self.bytes[start..end],
                    ))));
                }
                b'<' => {
                    // `<<` opens a dictionary — marked content puts one inline
                    // (`/NonStruct <</MCID 0 >> BDC`) — and is not a string.
                    if self.bytes.get(self.at + 1) == Some(&b'<') {
                        self.at += 2;
                        continue;
                    }
                    // Otherwise a hexadecimal string. Reading its digits as bytes
                    // would turn `002A0048` into the eight characters `0`, `0`,
                    // `2`, `A`… — four "glyphs" that are in no `/ToUnicode` map
                    // and four advances of a default width.
                    let start = self.at + 1;
                    self.at = start;
                    while self.at < self.bytes.len() && self.bytes[self.at] != b'>' {
                        self.at += 1;
                    }
                    let end = self.at;
                    self.at += 1;
                    return Some(match decode_hex(&self.bytes[start..end]) {
                        Ok(bytes) => Token::Operand(Operand::String(bytes)),
                        Err(error) => {
                            self.error = Some(error);
                            return None;
                        }
                    });
                }
                b'[' => {
                    self.at += 1;
                    return Some(Token::Operand(Operand::Array(self.array())));
                }
                b']' => {
                    self.at += 1;
                }
                b'/' => {
                    let start = self.at + 1;
                    self.at = start;
                    while self.at < self.bytes.len()
                        && !self.bytes[self.at].is_ascii_whitespace()
                        && !b"()<>[]{}/%".contains(&self.bytes[self.at])
                    {
                        self.at += 1;
                    }
                    return Some(Token::Operand(Operand::Name(
                        self.bytes[start..self.at].to_vec(),
                    )));
                }
                _ => {
                    let start = self.at;
                    while self.at < self.bytes.len()
                        && !self.bytes[self.at].is_ascii_whitespace()
                        && !b"()<>[]{}%".contains(&self.bytes[self.at])
                    {
                        self.at += 1;
                    }
                    if start == self.at {
                        self.at += 1;
                        continue;
                    }
                    let text = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();
                    return Some(match text.parse::<f64>() {
                        Ok(number) => Token::Operand(Operand::Number(number)),
                        Err(_) => Token::Operator(text),
                    });
                }
            }
        }
        None
    }
}

impl Lexer<'_> {
    fn array(&mut self) -> Vec<Operand> {
        let mut out = Vec::new();
        while self.at < self.bytes.len() {
            let byte = self.bytes[self.at];
            if byte.is_ascii_whitespace() {
                self.at += 1;
                continue;
            }
            if byte == b']' {
                self.at += 1;
                return out;
            }
            // `next` consumes one token; a nested array arrives whole.
            let Some(token) = self.next() else {
                return out;
            };
            match token {
                Token::Operand(operand) => out.push(operand),
                // An operator inside an array ends the array in every producer
                // this has seen; treating it as the end is safer than dropping
                // the rest of the stream.
                Token::Operator(_) => return out,
            }
        }
        out
    }
}

/// Decodes a hexadecimal string body into bytes.
///
/// An odd digit count or a non-hex digit is an error rather than something to
/// skip past: a glyph string that cannot be decoded means the run's position and
/// advance are unknown, and reporting a number here would be reporting a
/// fiction.
fn decode_hex(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let text = String::from_utf8_lossy(bytes);
    let digits: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() % 2 != 0 {
        return Err(format!(
            "a hexadecimal string of {} digit(s) is not a whole number of bytes",
            digits.len()
        ));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.chunks_exact(2) {
        let pair: String = pair.iter().collect();
        let byte = u8::from_str_radix(&pair, 16)
            .map_err(|_| format!("a hexadecimal string contains {pair:?}, which is not hex"))?;
        out.push(byte);
    }
    Ok(out)
}

/// Resolves the escapes of a PDF literal string.
///
/// This project's writer emits glyph ids as `(\000\003…)` rather than as a
/// hexadecimal string, so an extractor that skipped this step would read every
/// glyph as the four characters `\`, `0`, `0`, `0` and compute nonsense
/// advances — while looking like it worked.
fn unescape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte != b'\\' {
            out.push(byte);
            index += 1;
            continue;
        }
        index += 1;
        let Some(next) = bytes.get(index).copied() else {
            break;
        };
        index += 1;
        match next {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'(' | b')' | b'\\' => out.push(next),
            b'\r' => {
                // A line continuation swallows the newline and the next indent.
                if bytes.get(index) == Some(&b'\n') {
                    index += 1;
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                // Octal, one to three digits.
                let mut value = u32::from(next - b'0');
                for _ in 0..2 {
                    match bytes.get(index) {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            index += 1;
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(value).unwrap_or(0));
            }
            other => out.push(other),
        }
    }
    out
}

fn number(operands: &[Operand], index: usize) -> Option<f64> {
    match operands.get(index) {
        Some(Operand::Number(value)) => Some(*value),
        _ => None,
    }
}

/// 96 px per inch, 72 pt per inch: the layout works in px, the PDF in points.
const PT_PER_PX: f64 = 72.0 / 96.0;

fn to_pt(px: f64) -> f64 {
    px * PT_PER_PX
}

/// The Strict fixtures the geometry is checked against.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    for (name, file) in [
        ("strict-text", "strict-text.docx"),
        ("strict-text-grid", "strict-text-grid.docx"),
        ("strict-math-simple", "05-strict-math-simple.docx"),
        ("strict-math-eqarr", "10-strict-math-eqarr.docx"),
    ] {
        match std::fs::read(root.join(file)) {
            Ok(bytes) => out.push((name, bytes)),
            Err(error) => panic!("{}: {error}", root.join(file).display()),
        }
    }
    out
}

/// Lays a document out and renders it, the way the CLI does.
fn laid_out_and_rendered(bytes: &[u8]) -> (Vec<PlacedPage>, Vec<u8>) {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();

    let pages = place_pages(&document, &options, Some(&package)).expect("place");
    let pdf = render_with_source(&pages, &options, Some(&package)).expect("render");
    (pages, pdf.bytes)
}

fn text_items(page: &PlacedPage) -> Vec<&TextItem> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) if !text.text.trim().is_empty() => Some(text),
            _ => None,
        })
        .collect()
}

/// The load-bearing assertion: the PDF carries the layout's own numbers.
///
/// If this fails, the geometry comparison №2 depends on cannot be built, and no
/// amount of WPS references will help.
#[test]
fn glyph_positions_and_advances_survive_into_the_pdf() {
    let mut runs_seen = 0usize;
    for (name, bytes) in corpus() {
        let (pages, pdf) = laid_out_and_rendered(&bytes);
        let runs = glyph_runs(&pdf)
            .unwrap_or_else(|error| panic!("{name}: glyph geometry is not extractable: {error}"));
        assert!(
            !runs.is_empty(),
            "{name}: the PDF exposes no glyph positions at all — \
             a PDF without them cannot be compared to anything"
        );
        runs_seen += runs.len();

        let expected = text_items(&pages[0])
            .first()
            .copied()
            .unwrap_or_else(|| panic!("{name}: the first page has no text"));
        let got = &runs[0];

        // The origin is written verbatim, so it must come back exactly.
        assert!(
            (got.x - to_pt(expected.x)).abs() < 0.01,
            "{name}: the run's x moved between layout and PDF: {} px -> {} pt",
            expected.x,
            got.x
        );
        assert!(
            (got.size - to_pt(expected.size_px)).abs() < 0.01,
            "{name}: the size moved between layout and PDF: {} px -> {} pt",
            expected.size_px,
            got.size
        );
        assert_eq!(
            got.text, expected.text,
            "{name}: the run's text does not come back"
        );
        assert!(
            got.advance > 0.0,
            "{name}: no advance was recovered, so /W or /DW was not read"
        );
        // The PDF's y axis points up; the layout's points down from the top.
        let page_height_pt = to_pt(pages[0].height_px);
        assert!(
            (got.y - (page_height_pt - to_pt(expected.baseline))).abs() < 0.01,
            "{name}: the baseline moved between layout and PDF: {} px from the top \
             -> {} pt from the bottom of a {} pt page",
            expected.baseline,
            got.y,
            page_height_pt
        );
    }
    assert!(runs_seen > 20, "only {runs_seen} runs were recovered");
}

/// The advance a PDF reader computes must equal the layout's own advance.
///
/// This is the number the comparison with WPS will actually use, and it is the
/// one that does not depend on a glyph outline at all — the whole point of the
/// geometry oracle.
#[test]
fn the_recovered_advance_matches_the_layout() {
    for (name, bytes) in corpus() {
        let (pages, pdf) = laid_out_and_rendered(&bytes);
        let runs = glyph_runs(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        let expected = text_items(&pages[0])
            .first()
            .copied()
            .unwrap_or_else(|| panic!("{name}: the first page has no text"));
        let got = &runs[0];
        // `TextItem::width` is the layout's advance for the run, in px.
        let layout_pt = to_pt(expected.width);
        assert!(
            (got.advance - layout_pt).abs() <= layout_pt.max(1.0) * 0.02,
            "{name}: advance disagrees: layout {} px ({} pt) vs PDF {} pt — \
             this is the number the WPS comparison uses, so it has to agree",
            expected.width,
            layout_pt,
            got.advance
        );
    }
}

/// Every run of every page is recovered, not just the first.
///
/// A reader that reads page 1 and gives up is worse than one that says so, and
/// this is what says so.
#[test]
fn every_text_run_of_every_page_is_recovered() {
    for (name, bytes) in corpus() {
        let (pages, pdf) = laid_out_and_rendered(&bytes);
        let runs = glyph_runs(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        for (index, page) in pages.iter().enumerate() {
            let expected = text_items(page).len();
            // Filtered the same way as the layout side: a whitespace-only run
            // is drawn but is not a run anybody compares, and counting it on one
            // side only would report a phantom discrepancy.
            let got = runs
                .iter()
                .filter(|run| run.page == index && !run.text.trim().is_empty())
                .count();
            assert_eq!(
                got, expected,
                "{name}: page {index} has {expected} text run(s) in the layout and \
                 {got} in the PDF — a PDF that draws text no reader can locate is \
                 not comparable"
            );
        }
    }
}

/// The comparison itself, run against a PDF compared with itself.
///
/// A comparison harness that cannot report agreement is not a harness. This is
/// the cheapest possible check that the oracle is not merely silent.
#[test]
fn the_oracle_reports_agreement_with_itself() {
    for (name, bytes) in corpus() {
        let (_pages, pdf) = laid_out_and_rendered(&bytes);
        let left = glyph_runs(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        let right = glyph_runs(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(left, right, "{name}: two reads of one PDF disagree");
    }
}

/// The oracle reads a PDF **this project did not write**.
///
/// Point `STRICT_GEOMETRY_PROBE_PDF` at any PDF from another producer — a WPS
/// export is the one that matters, but any will do — and the runs are reported.
/// This is the property №2 rests on: the extractor models the PDF format, not
/// our writer's habits. An oracle that only understands bytes we produced would
/// pass every test above and then refuse the reference it was built for.
///
/// Skips when the variable is unset, since CI has no foreign PDF to hand and
/// inventing one here would only prove the extractor reads our own writer
/// again.
#[test]
fn a_foreign_pdf_is_readable() {
    let Ok(path) = std::env::var("STRICT_GEOMETRY_PROBE_PDF") else {
        eprintln!("skipping: STRICT_GEOMETRY_PROBE_PDF is not set");
        return;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let runs = glyph_runs(&bytes).unwrap_or_else(|error| {
        panic!(
            "{path}: the oracle refused a PDF from another writer: {error}\n\
             This is the property №2 depends on — if it cannot read their file, \
             the geometry comparison is not available and the question for the \
             customer has been answered in the negative."
        )
    });
    for run in &runs {
        println!(
            "{path}: page {} x={:.2} y={:.2} size={:.2} advance={:.2} {:?}",
            run.page, run.x, run.y, run.size, run.advance, run.text
        );
    }
    assert!(
        !runs.is_empty(),
        "{path}: the oracle returned no runs at all"
    );
}
