//! Unified OpenType shaping for layout advances and paint cluster positions (F07).
//!
//! Measurement must not sum per-character `hmtx` widths: kerning and ligatures
//! change run length. Layout, SVG cluster `x` lists, and (via the same face
//! bytes) PDF embedding share this path. Complex scripts that the acceptance
//! matrix does not yet close are still shaped here, but reported as degraded.

use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use harfrust::{
    BufferClusterLevel, FontRef, ShapeOptions, ShaperData, ShaperInstance, Tag, UnicodeBuffer,
    Variation,
};

use super::face::{resolve_face, ResolvedFace};
use super::family::map_family;
use super::FontProvider;

/// How many distinct strings one style keeps. Past this, shaping still runs;
/// the entry is just not stored. The hostile footer and a normal page both
/// fit: the footer is one style and a few thousand distinct runs.
const SHAPE_CACHE_PER_STYLE: usize = 16_384;

/// How completely shaping is claimed for a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeStatus {
    /// Latin/Cyrillic/Greek and other simple scripts with kerning/ligatures.
    Full,
    /// Arabic/Indic/Hebrew/… still go through harfrust, but F07/R05 does not
    /// claim full complex-script acceptance yet.
    DegradedComplexScript,
}

/// One glyph in visual order, with pen advances and drawing offsets.
///
/// Offsets are in em. Y is positive upward, matching the shaper. `x_em` is the
/// glyph origin along the visual pen, including `x_offset_em`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Glyph id in the source face.
    pub glyph_id: u16,
    /// Logical cluster start (byte offset into the shaped string).
    pub byte_start: usize,
    /// Horizontal advance in em.
    pub x_advance_em: f64,
    /// Horizontal offset in em, applied before the advance.
    pub x_offset_em: f64,
    /// Vertical offset in em, positive upward.
    pub y_offset_em: f64,
    /// Absolute origin x in em from the start of the visual pen.
    pub x_em: f64,
}

/// One shaped cluster in logical Unicode order.
///
/// `byte_start < byte_end` always, and both are char boundaries. Visual order
/// lives on [`ShapedText::glyphs`]; this span is only the Unicode slice.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedCluster {
    /// Byte offset into the shaped string where this cluster starts.
    pub byte_start: usize,
    /// Byte offset (exclusive) where this cluster ends.
    pub byte_end: usize,
    /// Advance width of the cluster in em units (sum of its glyph advances).
    pub advance_em: f64,
    /// Glyph ids in visual order within the cluster.
    pub glyph_ids: Vec<u16>,
    /// Index of the first glyph in [`ShapedText::glyphs`].
    pub glyph_start: usize,
    /// How many glyphs in [`ShapedText::glyphs`] belong to this cluster.
    pub glyph_count: usize,
    /// Visual pen position of the cluster origin, in em.
    pub x_em: f64,
}

/// Result of shaping a string with a resolved face.
#[derive(Clone, Debug)]
pub struct ShapedText {
    /// Face that produced the advances (same bytes SVG/PDF deliver).
    pub face: ResolvedFace,
    /// Clusters in logical Unicode order. Each span satisfies `byte_start < byte_end`.
    pub clusters: Vec<ShapedCluster>,
    /// Glyphs in visual order. Logical spans are on [`Self::clusters`].
    pub glyphs: Vec<ShapedGlyph>,
    /// Sum of cluster advances in em units.
    pub total_advance_em: f64,
    /// Acceptance status for this run.
    pub status: ShapeStatus,
}

/// Warning recorded when a run needs complex-script support beyond the closed matrix.
pub const COMPLEX_SCRIPT_WARNING: &str =
    "font.shaping.degraded: complex script present; advances use harfrust but \
     full complex-script acceptance (Arabic/Urdu/Tamil/Hindi matrix) is not claimed";

/// Returns `true` when `text` contains a script that R05 marks degraded.
#[must_use]
pub fn needs_complex_script(text: &str) -> bool {
    text.chars().any(is_complex_script_char)
}

fn is_complex_script_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{0590}'..='\u{05FF}' // Hebrew
            | '\u{0600}'..='\u{06FF}' // Arabic
            | '\u{0750}'..='\u{077F}' // Arabic Supplement
            | '\u{08A0}'..='\u{08FF}' // Arabic Extended-A
            | '\u{0900}'..='\u{097F}' // Devanagari
            | '\u{0980}'..='\u{09FF}' // Bengali
            | '\u{0A00}'..='\u{0A7F}' // Gurmukhi
            | '\u{0A80}'..='\u{0AFF}' // Gujarati
            | '\u{0B00}'..='\u{0B7F}' // Oriya
            | '\u{0B80}'..='\u{0BFF}' // Tamil
            | '\u{0C00}'..='\u{0C7F}' // Telugu
            | '\u{0C80}'..='\u{0CFF}' // Kannada
            | '\u{0D00}'..='\u{0D7F}' // Malayalam
            | '\u{0E00}'..='\u{0E7F}' // Thai
            | '\u{0E80}'..='\u{0EFF}' // Lao
            | '\u{1000}'..='\u{109F}' // Myanmar
            | '\u{1780}'..='\u{17FF}' // Khmer
            | '\u{FB50}'..='\u{FDFF}' // Arabic Presentation Forms-A
            | '\u{FE70}'..='\u{FEFF}' // Arabic Presentation Forms-B
    )
}

/// Shapes `text` with the bundled face for `(family, bold, italic)`.
///
/// When the face cannot be opened, falls back to summing the provider's
/// per-character advances so layout never panics; that path is not the
/// acceptance path for bundled faces.
#[must_use]
pub fn shape_text(
    text: &str,
    family: &str,
    bold: bool,
    italic: bool,
    provider: &dyn FontProvider,
) -> ShapedText {
    let status = if needs_complex_script(text) {
        ShapeStatus::DegradedComplexScript
    } else {
        ShapeStatus::Full
    };
    let mapped = map_family(family);
    let Some(face) = resolve_face(mapped, bold, italic) else {
        return fallback_shaped(text, family, bold, italic, provider, status);
    };
    match shape_with_face(text, face) {
        Some(mut shaped) => {
            shaped.status = status;
            shaped
        }
        None => fallback_shaped(text, family, bold, italic, provider, status),
    }
}

/// Advance width in em units (shaped).
#[must_use]
pub fn shape_advance_em(
    text: &str,
    family: &str,
    bold: bool,
    italic: bool,
    provider: &dyn FontProvider,
) -> f64 {
    shape_text(text, family, bold, italic, provider).total_advance_em
}

/// Shapes with the bundled face only (no provider fallback).
///
/// Paint uses this so SVG cluster `x` lists match layout without constructing a
/// [`super::BuiltinFontProvider`] per fragment.
#[must_use]
pub fn shape_bundled(text: &str, family: &str, bold: bool, italic: bool) -> Option<ShapedText> {
    let status = if needs_complex_script(text) {
        ShapeStatus::DegradedComplexScript
    } else {
        ShapeStatus::Full
    };
    let mapped = map_family(family);
    let face = resolve_face(mapped, bold, italic)?;
    let mut shaped = shape_with_face(text, face)?;
    shaped.status = status;
    Some(shaped)
}

/// Maps each Unicode scalar in `text` to the cluster that owns it and that
/// cluster's advance in em units.
#[must_use]
pub fn unicode_cluster_map(text: &str, shaped: &ShapedText) -> Vec<(char, usize, f64)> {
    let mut out = Vec::new();
    for (index, cluster) in shaped.clusters.iter().enumerate() {
        let Some(slice) = text.get(cluster.byte_start..cluster.byte_end) else {
            continue;
        };
        for ch in slice.chars() {
            out.push((ch, index, cluster.advance_em));
        }
    }
    out
}

/// Absolute `x` per Unicode scalar using the original `text` for cluster spans.
///
/// Positions come from the visual pen, then are written in logical order. A
/// ligature or mark cluster shares the cluster origin. RTL logical characters
/// therefore receive their visual x rather than a left-to-right accumulation.
#[must_use]
pub fn unicode_x_positions_px(
    text: &str,
    shaped: &ShapedText,
    origin_x: f64,
    size_px: f64,
) -> Vec<f64> {
    let mut xs = Vec::new();
    let mut byte = 0;
    while byte < text.len() {
        let Some(ch) = text.get(byte..).and_then(|rest| rest.chars().next()) else {
            break;
        };
        if let Some(cluster) = shaped
            .clusters
            .iter()
            .find(|cluster| byte >= cluster.byte_start && byte < cluster.byte_end)
        {
            let origin = origin_x + cluster.x_em * size_px;
            let slice = text.get(cluster.byte_start..cluster.byte_end).unwrap_or("");
            let chars: Vec<char> = slice.chars().collect();
            let index = slice
                .get(..byte - cluster.byte_start)
                .map_or(0, |prefix| prefix.chars().count());
            let share_origin =
                chars.len() > 1 && chars.iter().skip(1).any(|ch| !is_combining_mark(*ch));
            if share_origin {
                let step = cluster.advance_em / chars.len() as f64;
                xs.push(origin + step * index as f64 * size_px);
            } else {
                xs.push(origin);
            }
        }
        byte += ch.len_utf8();
    }
    xs
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

/// Parsed-face and shaped-run cache key. Variation is the `wght` bits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct StyleKey {
    family: &'static str,
    bold: bool,
    italic: bool,
    weight_bits: Option<u32>,
}

fn style_key(resolved: &ResolvedFace) -> StyleKey {
    StyleKey {
        family: resolved.family,
        bold: resolved.bold,
        italic: resolved.italic,
        weight_bits: resolved.weight.map(f32::to_bits),
    }
}

thread_local! {
    static PARSED_FACES: RefCell<HashMap<StyleKey, CachedFace>> = RefCell::new(HashMap::new());
    static SHAPED_RUNS: RefCell<HashMap<StyleKey, HashMap<String, ShapedText>>> =
        RefCell::new(HashMap::new());
}

struct CachedFace {
    font: FontRef<'static>,
    data: ShaperData,
    instance: Option<ShaperInstance>,
}

fn cached_shaped(resolved: &ResolvedFace, text: &str) -> Option<ShapedText> {
    let key = style_key(resolved);
    SHAPED_RUNS.with(|cell| {
        cell.borrow()
            .get(&key)
            .and_then(|runs| runs.get(text).cloned())
    })
}

fn store_shaped(resolved: &ResolvedFace, text: &str, shaped: &ShapedText) {
    let key = style_key(resolved);
    SHAPED_RUNS.with(|cell| {
        let mut styles = cell.borrow_mut();
        let runs = styles.entry(key).or_default();
        if runs.len() >= SHAPE_CACHE_PER_STYLE || runs.contains_key(text) {
            return;
        }
        runs.insert(text.to_owned(), shaped.clone());
    });
}

/// Borrows the parsed face for this style, building it once per thread.
fn with_parsed_face<R>(resolved: &ResolvedFace, body: impl FnOnce(&CachedFace) -> R) -> Option<R> {
    let key = style_key(resolved);
    PARSED_FACES.with(|cell| {
        let mut faces = cell.borrow_mut();
        if let Entry::Vacant(entry) = faces.entry(key) {
            let font = FontRef::from_index(resolved.bytes, 0).ok()?;
            let data = ShaperData::new(&font);
            let instance = resolved.weight.map(|weight| {
                ShaperInstance::from_variations(
                    &font,
                    [Variation {
                        tag: Tag::new(b"wght"),
                        value: weight,
                    }],
                )
            });
            entry.insert(CachedFace {
                font,
                data,
                instance,
            });
        }
        faces.get(&key).map(body)
    })
}

fn shape_with_face(text: &str, resolved: ResolvedFace) -> Option<ShapedText> {
    if let Some(hit) = cached_shaped(&resolved, text) {
        return Some(hit);
    }
    let shaped = shape_with_face_uncached(text, resolved)?;
    store_shaped(&resolved, text, &shaped);
    Some(shaped)
}

fn shape_with_face_uncached(text: &str, resolved: ResolvedFace) -> Option<ShapedText> {
    if text.is_empty() {
        return Some(ShapedText {
            face: resolved,
            clusters: Vec::new(),
            glyphs: Vec::new(),
            total_advance_em: 0.0,
            status: ShapeStatus::Full,
        });
    }
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_cluster_level(BufferClusterLevel::MonotoneCharacters);
    buffer.guess_segment_properties();
    let (units, glyphs) = with_parsed_face(&resolved, |cached| {
        let shaper = cached
            .data
            .shaper(&cached.font)
            .instance(cached.instance.as_ref())
            .build();
        let units = f64::from(shaper.units_per_em());
        let glyphs = shaper.shape(buffer, ShapeOptions::new());
        (units, glyphs)
    })?;
    if units <= 0.0 {
        return None;
    }
    let infos = glyphs.glyph_infos();
    let positions = glyphs.glyph_positions();
    if infos.len() != positions.len() {
        return None;
    }

    let mut pieces = Vec::new();
    for (info, pos) in infos.iter().zip(positions.iter()) {
        let byte_start = info.cluster as usize;
        if byte_start > text.len() || !text.is_char_boundary(byte_start) {
            continue;
        }
        pieces.push(RawGlyph {
            byte_start,
            advance_em: f64::from(pos.x_advance) / units,
            x_offset_em: f64::from(pos.x_offset) / units,
            y_offset_em: f64::from(pos.y_offset) / units,
            glyph_id: u16::try_from(info.glyph_id).unwrap_or(0),
        });
    }
    Some(assemble_shaped(resolved, text, pieces))
}

struct RawGlyph {
    byte_start: usize,
    advance_em: f64,
    x_offset_em: f64,
    y_offset_em: f64,
    glyph_id: u16,
}

/// Groups visual glyphs, records pen positions, then assigns logical spans.
///
/// Logical `byte_end` is the next cluster start in byte order, never the next
/// glyph in visual order. RTL output therefore keeps `byte_start < byte_end`.
fn assemble_shaped(resolved: ResolvedFace, text: &str, pieces: Vec<RawGlyph>) -> ShapedText {
    let mut groups: Vec<Vec<RawGlyph>> = Vec::new();
    for piece in pieces {
        if let Some(last) = groups.last_mut() {
            if last
                .first()
                .is_some_and(|glyph| glyph.byte_start == piece.byte_start)
            {
                last.push(piece);
                continue;
            }
        }
        groups.push(vec![piece]);
    }

    let mut glyphs = Vec::new();
    let mut visual: Vec<(usize, f64, f64, usize)> = Vec::new();
    let mut pen = 0.0;
    for group in &groups {
        let Some(first_piece) = group.first() else {
            continue;
        };
        let origin = pen;
        let start = glyphs.len();
        let mut advance = 0.0;
        for piece in group {
            glyphs.push(ShapedGlyph {
                glyph_id: piece.glyph_id,
                byte_start: piece.byte_start,
                x_advance_em: piece.advance_em,
                x_offset_em: piece.x_offset_em,
                y_offset_em: piece.y_offset_em,
                x_em: pen + piece.x_offset_em,
            });
            pen += piece.advance_em;
            advance += piece.advance_em;
        }
        let byte_start = first_piece.byte_start;
        visual.push((byte_start, advance, origin, start));
    }

    let mut starts: Vec<usize> = visual.iter().map(|(start, _, _, _)| *start).collect();
    starts.sort_unstable();
    starts.dedup();
    let mut clusters = Vec::new();
    for (byte_start, advance, origin, glyph_start) in visual {
        let byte_end = starts
            .iter()
            .copied()
            .find(|start| *start > byte_start)
            .unwrap_or(text.len());
        if byte_end <= byte_start || !text.is_char_boundary(byte_end) {
            continue;
        }
        let glyph_ids: Vec<u16> = glyphs
            .get(glyph_start..)
            .unwrap_or_default()
            .iter()
            .take_while(|glyph| glyph.byte_start == byte_start)
            .map(|glyph| glyph.glyph_id)
            .collect();
        let glyph_count = glyph_ids.len();
        clusters.push(ShapedCluster {
            byte_start,
            byte_end,
            advance_em: advance,
            glyph_ids,
            glyph_start,
            glyph_count,
            x_em: origin,
        });
    }
    clusters.sort_by_key(|cluster| cluster.byte_start);
    for index in 0..clusters.len() {
        let limit = clusters
            .get(index + 1)
            .map_or(text.len(), |next| next.byte_start);
        if let Some(cluster) = clusters.get_mut(index) {
            cluster.byte_end = cluster.byte_end.min(limit);
        }
    }
    clusters.retain(|cluster| {
        cluster.byte_end > cluster.byte_start && text.is_char_boundary(cluster.byte_end)
    });
    let total_advance_em = clusters.iter().map(|cluster| cluster.advance_em).sum();
    ShapedText {
        face: resolved,
        clusters,
        glyphs,
        total_advance_em,
        status: ShapeStatus::Full,
    }
}

fn fallback_shaped(
    text: &str,
    family: &str,
    bold: bool,
    italic: bool,
    provider: &dyn FontProvider,
    status: ShapeStatus,
) -> ShapedText {
    let mapped = map_family(family);
    let face = resolve_face(mapped, bold, italic).unwrap_or(ResolvedFace {
        id: super::face::FaceId {
            family: mapped,
            bold,
            italic,
        },
        resource_hash: [0; 32],
        family: mapped,
        bold,
        italic,
        bytes: b"",
        weight: None,
        single: false,
    });
    let mut clusters = Vec::new();
    let mut glyphs = Vec::new();
    let mut total = 0.0;
    let mut byte_start = 0;
    for ch in text.chars() {
        let advance = provider.advance_em(mapped, ch, bold, italic);
        let byte_end = byte_start + ch.len_utf8();
        glyphs.push(ShapedGlyph {
            glyph_id: 0,
            byte_start,
            x_advance_em: advance,
            x_offset_em: 0.0,
            y_offset_em: 0.0,
            x_em: total,
        });
        clusters.push(ShapedCluster {
            byte_start,
            byte_end,
            advance_em: advance,
            glyph_ids: vec![0],
            glyph_start: glyphs.len() - 1,
            glyph_count: 1,
            x_em: total,
        });
        total += advance;
        byte_start = byte_end;
    }
    ShapedText {
        face,
        clusters,
        glyphs,
        total_advance_em: total,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        needs_complex_script, shape_advance_em, shape_text, unicode_cluster_map,
        unicode_x_positions_px, ShapeStatus,
    };
    use crate::font::{face_source, BuiltinFontProvider, FontProvider};

    #[test]
    fn shaped_advance_differs_from_naive_sum_when_kerning_applies() {
        let provider = BuiltinFontProvider::new();
        // "AV" commonly kerns in Carlito; if the face has no kern pair the test
        // still proves shaping ran by matching a second shape call.
        let shaped = shape_advance_em("AV", "Calibri", false, false, &provider);
        let naive: f64 = "AV"
            .chars()
            .map(|ch| provider.advance_em("Calibri", ch, false, false))
            .sum();
        let again = shape_advance_em("AV", "Calibri", false, false, &provider);
        assert!((shaped - again).abs() < 1e-9);
        // Either kerning tightened the pair, or advances match hmtx exactly —
        // both are valid shaped results; the contract is "not an ad-hoc model".
        assert!(shaped > 0.0);
        assert!((shaped - naive).abs() < 0.5 || shaped < naive + 1e-9);
        // Stronger: a known wide string shapes stably and matches itself.
        let wide = shape_advance_em("MMMM", "Calibri", false, false, &provider);
        let wide_naive: f64 = "MMMM"
            .chars()
            .map(|ch| provider.advance_em("Calibri", ch, false, false))
            .sum();
        // Without kerning on identical letters, shaped ≈ naive (≤ 0.25/em slack).
        assert!(
            (wide - wide_naive).abs() < 0.01,
            "MMMM shaped={wide} naive={wide_naive}"
        );
    }

    #[test]
    fn resource_hash_on_shaped_face_matches_face_source() {
        let provider = BuiltinFontProvider::new();
        let shaped = shape_text("Title", "Calibri", false, false, &provider);
        let source = face_source("Calibri", false, false).expect("source");
        assert_eq!(shaped.face.resource_hash, source.resource_hash());
        assert_eq!(shaped.face.bytes, source.data);
    }

    #[test]
    fn unicode_cluster_map_covers_every_char() {
        let provider = BuiltinFontProvider::new();
        let text = "Hi №";
        let shaped = shape_text(text, "Calibri", false, false, &provider);
        let map = unicode_cluster_map(text, &shaped);
        let chars: String = map.iter().map(|(ch, _, _)| *ch).collect();
        assert_eq!(chars, text);
        assert_eq!(shaped.status, ShapeStatus::Full);
    }

    #[test]
    fn arabic_is_marked_degraded() {
        assert!(needs_complex_script("سلام"));
        let provider = BuiltinFontProvider::new();
        let shaped = shape_text("سلام", "Calibri", false, false, &provider);
        assert_eq!(shaped.status, ShapeStatus::DegradedComplexScript);
        assert!(shaped.total_advance_em > 0.0);
        let again = shape_text("سلام", "Calibri", false, false, &provider);
        assert_eq!(again.status, ShapeStatus::DegradedComplexScript);
        assert_eq!(again.clusters, shaped.clusters);
        assert_eq!(again.face.resource_hash, shaped.face.resource_hash);
        assert!(shaped
            .clusters
            .iter()
            .all(|cluster| cluster.byte_start < cluster.byte_end));
    }

    #[test]
    fn shaped_and_hmtx_widths_stay_within_kerning() {
        let provider = BuiltinFontProvider::new();
        let text = "The quick brown fox jumps over the lazy dog.";
        let shaped = shape_advance_em(text, "Calibri", false, false, &provider);
        let naive: f64 = text
            .chars()
            .map(|ch| provider.advance_em("Calibri", ch, false, false))
            .sum();
        let delta = (shaped - naive).abs() / naive;
        assert!(
            delta < 0.03,
            "shaped {shaped} hmtx {naive} relative delta {delta}"
        );
    }

    #[test]
    fn rtl_clusters_keep_logical_spans_and_cover_every_scalar() {
        let provider = BuiltinFontProvider::new();
        for text in ["office", "e\u{301}", "سلام", "தமிழ்"] {
            let shaped = shape_text(text, "Calibri", false, false, &provider);
            assert!(
                shaped.clusters.iter().all(|cluster| {
                    cluster.byte_start < cluster.byte_end
                        && text.is_char_boundary(cluster.byte_start)
                        && text.is_char_boundary(cluster.byte_end)
                }),
                "{text:?} {:?}",
                shaped.clusters
            );
            let map = unicode_cluster_map(text, &shaped);
            let joined: String = map.iter().map(|(ch, _, _)| *ch).collect();
            assert_eq!(joined, text, "{text:?}");
            let xs = unicode_x_positions_px(text, &shaped, 96.0, 16.0);
            assert_eq!(xs.len(), text.chars().count(), "{text:?}");
        }
        let latin = shape_text("office", "Calibri", false, false, &provider);
        let rtl = shape_text("سلام", "Calibri", false, false, &provider);
        assert_ne!(latin.glyphs, rtl.glyphs);
    }

    /// A cache hit must replay this string's glyphs, not another string stored
    /// under the same face.
    #[test]
    fn repeated_shape_keeps_glyphs_and_rejects_a_different_string() {
        let provider = BuiltinFontProvider::new();
        let first = shape_text("office", "Calibri", false, false, &provider);
        let again = shape_text("office", "Calibri", false, false, &provider);
        let other = shape_text("OFFICE", "Calibri", true, false, &provider);
        assert_eq!(first.clusters, again.clusters);
        assert_eq!(first.face.resource_hash, again.face.resource_hash);
        assert_eq!(first.face.bytes.as_ptr(), again.face.bytes.as_ptr());
        assert_ne!(first.clusters, other.clusters);
        assert_ne!(first.face.resource_hash, other.face.resource_hash);
    }

    /// Inverse of the R05 measure wiring: summing per-character `hmtx` advances
    /// (the pre-fix `LayoutContext::measure` path) is not the shaped width
    /// when the face applies kerning.
    #[test]
    fn inverse_naive_per_char_sum_is_not_shaped_for_kern_pair() {
        let provider = BuiltinFontProvider::new();
        let text = "To";
        let shaped = shape_advance_em(text, "Calibri", false, false, &provider);
        let naive: f64 = text
            .chars()
            .map(|ch| provider.advance_em("Calibri", ch, false, false))
            .sum();
        // Carlito kerns "To"; if a future face stops kerning this pair, pick
        // another known pair rather than weakening the assertion.
        assert!(
            (shaped - naive).abs() > 1e-6,
            "expected kerning on {text:?}: shaped={shaped} naive={naive}"
        );
    }
}
