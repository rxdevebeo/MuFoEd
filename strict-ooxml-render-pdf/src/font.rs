//! Font subsetting and embedding as a PDF CID font.
//!
//! The text in the PDF has to be *real text* (SC-7): selectable, searchable and
//! extractable. That needs an embedded font, an `Identity-H` encoding, a width
//! array in 1000-unit text space and a `ToUnicode` CMap.
//!
//! [`subsetter`] does the hard part: it rewrites the face to CID-keyed outlines
//! with an identity GID→CID mapping, which is precisely what a PDF CID font
//! expects, and `subset_with_variations` instances a variable face so the
//! bundled Arimo can be embedded at the weight the layout used.
//!
//! The faces come from [`strict_ooxml_render_svg::font`], so the program that
//! gets embedded is the same one that produced the advances. That is what makes
//! the PDF agree with the SVG rather than merely resemble it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use skrifa::instance::{LocationRef, Size};
use skrifa::raw::types::GlyphId;
use skrifa::{FontRef, MetadataProvider};
use strict_ooxml_render_svg::font::FaceSource;
use subsetter::{subset, subset_with_variations, Error as SubsetError, GlyphRemapper, Tag};

/// A face that some text uses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceKey {
    /// The mapped family (the bundled substitute).
    pub family: &'static str,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
}

impl FaceKey {
    /// A stable name for the PDF resource dictionary, e.g. `F1`.
    #[must_use]
    pub fn resource_name(&self, index: usize) -> String {
        format!("F{}", index + 1)
    }

    /// A stable PostScript-ish base name for the font dictionary.
    #[must_use]
    pub fn base_font(&self) -> String {
        let mut name = String::with_capacity(24);
        name.push_str(&self.family.replace(' ', ""));
        if self.bold {
            name.push_str("-Bold");
        }
        if self.italic {
            name.push_str("-Italic");
        }
        name
    }
}

/// One glyph the document uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UsedGlyph {
    /// Glyph id in the *source* face.
    source: GlyphId,
    /// The character, when the glyph came from text.
    character: Option<char>,
}

/// Collects the glyphs a document uses, per face.
///
/// The two passes are deliberate: the set has to be complete before a subset can
/// be built, and the subset decides the new glyph ids, so the text cannot be
/// encoded until every page has been walked.
#[derive(Debug, Default)]
pub struct FaceCollector {
    faces: BTreeMap<FaceKey, BTreeMap<GlyphId, Option<char>>>,
    /// Faces requested that the bundle does not carry.
    unknown: BTreeSet<FaceKey>,
}

impl FaceCollector {
    /// Returns an empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one character for a face.
    ///
    /// A family the bundle does not carry is recorded and reported rather than
    /// silently drawn with a substitute: the text would keep its metrics and
    /// lose its glyphs, which is exactly the kind of quiet loss the project
    /// refuses.
    pub fn add(&mut self, source: &FaceSource, bold: bool, italic: bool, ch: char) {
        let key = FaceKey {
            family: source.family,
            bold,
            italic,
        };
        let Some(glyph) = glyph_for(source, ch) else {
            self.unknown.insert(key);
            return;
        };
        self.faces
            .entry(key)
            .or_default()
            .entry(glyph)
            .or_insert(Some(ch));
    }

    /// The faces that carry at least one glyph, in a deterministic order.
    #[must_use]
    pub fn faces(&self) -> Vec<FaceKey> {
        self.faces.keys().cloned().collect()
    }

    /// The faces that were requested but are not bundled.
    #[must_use]
    pub fn unknown_faces(&self) -> Vec<FaceKey> {
        self.unknown.iter().cloned().collect()
    }

    /// Records glyph 0 (`.notdef`) for every face, which a font must keep.
    pub fn add_notdef(&mut self) {
        for glyphs in self.faces.values_mut() {
            glyphs.entry(GlyphId::new(0)).or_insert(None);
        }
    }

    /// Builds the subset of one face.
    ///
    /// Returns the subset program, the new id of each source glyph and the width
    /// of each new glyph in 1000-unit text space.
    ///
    /// # Errors
    ///
    /// Returns a [`FontError`] when the face cannot be parsed or subsetted.
    pub fn build(&self, key: &FaceKey, source: &FaceSource) -> Result<EmbeddedFont, FontError> {
        let used: Vec<UsedGlyph> = self
            .faces
            .get(key)
            .map(|glyphs| {
                glyphs
                    .iter()
                    .map(|(source, character)| UsedGlyph {
                        source: *source,
                        character: *character,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut remapper = GlyphRemapper::new();
        for glyph in &used {
            remapper.remap(glyph.source.to_u32() as u16);
        }
        let subset_bytes = match source.weight {
            Some(weight) => {
                subset_with_variations(source.data, 0, &[(Tag::new(b"wght"), weight)], &remapper)
            }
            None => subset(source.data, 0, &remapper),
        }
        .map_err(|error: SubsetError| FontError::Subset(error.to_string()))?;

        // Widths come from the *subset* program, so they describe exactly the
        // outlines that were embedded - reading them from the source would be
        // wrong for a variable face, whose advances move with the instance.
        let subset_font =
            FontRef::new(&subset_bytes).map_err(|error| FontError::Parse(error.to_string()))?;
        let is_cff = is_cff_font(&subset_bytes);
        let metrics = subset_font.metrics(Size::unscaled(), LocationRef::default());
        let units_per_em = f64::from(metrics.units_per_em);
        if units_per_em <= 0.0 {
            return Err(FontError::NoUnitsPerEm);
        }
        let scale = 1000.0 / units_per_em;
        let gids = subset_font.glyph_metrics(Size::unscaled(), LocationRef::default());
        // The width is a whole number of 1000-unit steps, which is what the PDF
        // `/W` array is specified in. Keeping it an integer rather than an `f32`
        // is what lets the run-detection in the writer compare widths exactly
        // instead of with a margin that would have to be justified.
        let mut widths: BTreeMap<u16, u16> = BTreeMap::new();
        for glyph in &used {
            let old = glyph.source.to_u32() as u16;
            let Some(new_gid) = remapper.get(old) else {
                continue;
            };
            let advance = gids
                .advance_width(GlyphId::new(u32::from(new_gid)))
                .unwrap_or(0.0);
            let width = (f64::from(advance) * scale).round();
            widths.insert(new_gid, width.clamp(0.0, f64::from(u16::MAX)) as u16);
        }

        let mut chars: BTreeMap<char, u16> = BTreeMap::new();
        let mut to_unicode: BTreeMap<u16, char> = BTreeMap::new();
        for glyph in &used {
            let Some(new_gid) = remapper.get(glyph.source.to_u32() as u16) else {
                continue;
            };
            let Some(ch) = glyph.character else {
                continue;
            };
            chars.insert(ch, new_gid);
            to_unicode.insert(new_gid, ch);
        }

        Ok(EmbeddedFont {
            key: key.clone(),
            data: subset_bytes,
            widths,
            chars,
            to_unicode,
            is_cff,
            ascent: f64::from(metrics.ascent) * scale,
            descent: f64::from(metrics.descent) * scale,
            cap_height: metrics.cap_height.map(|value| f64::from(value) * scale),
        })
    }
}

/// A subsetted face, ready to be written as a PDF font.
#[derive(Clone, Debug)]
pub struct EmbeddedFont {
    /// Which face this is.
    pub key: FaceKey,
    /// The subsetted font program.
    pub data: Vec<u8>,
    /// Advance width per subset glyph id, in 1000-unit text space.
    pub widths: BTreeMap<u16, u16>,
    /// The character a subset glyph id stands for.
    ///
    /// This is the table the drawing pass needs: a text run arrives as
    /// characters, and encoding it means asking "which glyph is this character
    /// *in this subset*". Re-deriving that from the source face would be wrong
    /// for a variable face, whose glyph ids move with the instance.
    pub chars: BTreeMap<char, u16>,
    /// Subset glyph id → the character it stands for, for the `ToUnicode` CMap.
    pub to_unicode: BTreeMap<u16, char>,
    /// Whether the program carries CFF (OpenType/PostScript) outlines.
    pub is_cff: bool,
    /// Ascender in 1000-unit text space.
    pub ascent: f64,
    /// Descender (negative) in 1000-unit text space.
    pub descent: f64,
    /// Cap height in 1000-unit text space, when the face declares one.
    pub cap_height: Option<f64>,
}

/// Failure while preparing a face.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontError {
    /// The bundled program could not be parsed.
    Parse(String),
    /// The subsetter refused the face.
    Subset(String),
    /// The face reports no units per em, so nothing can be scaled.
    NoUnitsPerEm,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(detail) => write!(f, "cannot parse the bundled face: {detail}"),
            Self::Subset(detail) => write!(f, "cannot subset the face: {detail}"),
            Self::NoUnitsPerEm => f.write_str("the face reports no units per em"),
        }
    }
}

impl std::error::Error for FontError {}

/// Returns the glyph id a face uses for `ch`.
fn glyph_for(source: &FaceSource, ch: char) -> Option<GlyphId> {
    let font = FontRef::new(source.data).ok()?;
    let glyph = font.charmap().map(ch)?;
    (glyph != GlyphId::NOTDEF).then_some(glyph)
}

/// Detects CFF outlines from the sfnt table directory.
///
/// A CFF face must be embedded as `FontFile3`/`CIDFontType0`; a `glyf` one as
/// `FontFile2`/`CIDFontType2`. Getting this wrong produces a PDF that opens and
/// shows nothing, so it is read from the tables rather than assumed.
fn is_cff_font(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    let num_tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
    (0..num_tables).any(|index| {
        let offset = 12 + index * 16 + 4;
        let end = offset + 4;
        if end > data.len() {
            return false;
        }
        &data[offset..end] == b"CFF "
    })
}

/// Renders a `ToUnicode` CMap for a subset.
///
/// The CMap is what a reader uses to get characters back out of the glyph ids,
/// so it is the difference between "selectable text" and "selectable boxes".
#[must_use]
pub fn to_unicode_cmap(font: &EmbeddedFont) -> Vec<u8> {
    let mut out = String::with_capacity(256 + 24 * font.to_unicode.len());
    out.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
    out.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    out.push_str("/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
    out.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");

    // `bfchar` sections hold at most 100 entries each.
    let entries: Vec<(u16, char)> = font
        .to_unicode
        .iter()
        .map(|(gid, ch)| (*gid, *ch))
        .collect();
    for (index, chunk) in entries.chunks(100).enumerate() {
        let _ = writeln!(out, "{index} beginbfchar");
        for (gid, ch) in chunk {
            let _ = writeln!(out, "<{gid:04X}> <{:04X}>", *ch as u32);
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.into_bytes()
}

/// The `Identity-H` encoding, as a CMap stream.
///
/// Without it a CID font has no way to turn a code into a CID, and the text
/// renders as nothing at all.
#[must_use]
pub fn identity_h_cmap() -> Vec<u8> {
    let mut out = String::with_capacity(512);
    out.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
    out.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def\n");
    out.push_str("/CMapName /Identity-H def\n/CMapType 2 def\n");
    out.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    out.push_str("1 begincidrange\n<0000> <FFFF> 0\nendcidrange\n");
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use strict_ooxml_render_svg::font::face_source;

    use super::{FaceCollector, FaceKey};

    #[test]
    fn collects_and_subsets_a_bundled_face() {
        let source = face_source("Calibri", false, false).expect("Calibri is bundled");
        let mut collector = FaceCollector::new();
        for ch in "Hello, world".chars() {
            collector.add(&source, false, false, ch);
        }
        collector.add_notdef();
        assert_eq!(collector.faces().len(), 1);
        assert!(collector.unknown_faces().is_empty());

        let key = FaceKey {
            family: source.family,
            bold: false,
            italic: false,
        };
        let font = collector.build(&key, &source).expect("subset");
        assert!(!font.data.is_empty());
        // Nine distinct glyphs in "Hello, world" (the letters repeat) plus
        // `.notdef`, and a subset far smaller than the 600 KB source face.
        assert_eq!(font.widths.len(), 10, "{:?}", font.widths);
        assert!(font
            .to_unicode
            .values()
            .all(|ch| "Hello, world".contains(*ch)));
        assert!(font.data.len() < 20_000, "{}", font.data.len());
        assert!(!font.is_cff, "Carlito carries glyf outlines");
        assert!(font.ascent > 0.0 && font.descent < 0.0);
    }

    #[test]
    fn every_bundled_face_subsets() {
        for family in strict_ooxml_render_svg::font::bundled_families() {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let Some(source) = face_source(family, bold, italic) else {
                    continue;
                };
                let mut collector = FaceCollector::new();
                collector.add(&source, bold, italic, 'A');
                collector.add(&source, bold, italic, 'g');
                collector.add_notdef();
                let key = FaceKey {
                    family: source.family,
                    bold,
                    italic,
                };
                let font = collector
                    .build(&key, &source)
                    .unwrap_or_else(|error| panic!("{family} {bold}/{italic}: {error}"));
                assert!(!font.data.is_empty(), "{family} produced no data");
                assert!(font.widths.len() >= 2, "{family} lost its widths");
            }
        }
    }

    #[test]
    fn the_variable_face_is_instanced() {
        let Some(source) = face_source("Arial", true, false) else {
            return;
        };
        let mut collector = FaceCollector::new();
        collector.add(&source, true, false, 'B');
        collector.add_notdef();
        let key = FaceKey {
            family: source.family,
            bold: true,
            italic: false,
        };
        let font = collector.build(&key, &source).expect("subset Arimo");
        // An instanced static face has no `fvar` table left, so it embeds as
        // `FontFile2` rather than a variable program.
        assert!(!font.is_cff);
        assert!(font.data.len() < 20_000, "{}", font.data.len());
    }

    #[test]
    fn a_character_without_a_glyph_is_recorded_not_substituted() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let mut collector = FaceCollector::new();
        collector.add(&source, false, false, '\u{10FFFD}');
        assert_eq!(collector.faces().len(), 0);
        assert_eq!(collector.unknown_faces().len(), 1);
    }

    #[test]
    fn cmaps_are_well_formed_enough_to_be_found() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let mut collector = FaceCollector::new();
        collector.add(&source, false, false, 'A');
        collector.add_notdef();
        let key = FaceKey {
            family: source.family,
            bold: false,
            italic: false,
        };
        let font = collector.build(&key, &source).expect("subset");
        let cmap = String::from_utf8(super::to_unicode_cmap(&font)).expect("utf-8");
        assert!(cmap.contains("beginbfchar"), "{cmap}");
        assert!(cmap.contains("<0041>"), "A must map to U+0041:\n{cmap}");
        let identity = String::from_utf8(super::identity_h_cmap()).expect("utf-8");
        assert!(identity.contains("begincidrange"), "{identity}");
    }
}
