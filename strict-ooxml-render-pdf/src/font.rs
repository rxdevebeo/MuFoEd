//! Font subsetting and embedding as a PDF CID font.
//!
//! The text in the PDF has to be *real text* (SC-7): selectable, searchable and
//! extractable. That needs an embedded font, an `Identity-H` encoding, a width
//! array in 1000-unit text space and a `ToUnicode` CMap.
//!
//! [`subsetter`] does the hard part: it rewrites the face to CID-keyed outlines,
//! and `subset_with_variations` instances a variable face so the bundled Arimo
//! can be embedded at the weight the layout used.
//!
//! CID and GID are **not** the same (AUD-82). Two characters that share a glyph
//! (U+0020 and U+00A0) each get their own CID; `/CIDToGIDMap` points both at the
//! one outline, and `ToUnicode` recovers each character from its CID.
//!
//! The faces come from [`strict_ooxml_render_svg::font`], so the program that
//! gets embedded is the same one that produced the advances. That is what makes
//! the PDF agree with the SVG rather than merely resemble it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use skrifa::instance::{LocationRef, Size};
use skrifa::raw::types::GlyphId;
use skrifa::{FontRef, MetadataProvider};
use strict_ooxml_render_svg::font::{
    FaceInstanceKey, FaceSource, FontResource, ResolvedFaceHandle,
};
use subsetter::{subset, subset_with_variations, Error as SubsetError, GlyphRemapper, Tag};

/// A face that some text uses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceKey {
    /// The mapped family (the bundled substitute).
    pub family: String,
    /// Exact external program and instance, absent for bundled legacy faces.
    pub instance: Option<FaceInstanceKey>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
}

impl FaceKey {
    /// Identifies an explicitly registered face without merging distinct programs.
    #[must_use]
    pub fn from_resource(face: &ResolvedFaceHandle) -> Self {
        Self {
            family: strict_ooxml_render_svg::style::resource_family(face),
            instance: Some(face.resource.key().clone()),
            bold: face.bold,
            italic: face.italic,
        }
    }

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
#[derive(Clone, Debug, PartialEq, Eq)]
struct UsedGlyph {
    /// Glyph id in the *source* face.
    source: GlyphId,
    /// The character, when the glyph came from a single scalar.
    character: Option<char>,
    /// Unicode this CID extracts as. Empty for `.notdef` and continuation glyphs.
    cluster_text: String,
}

/// Collects the glyphs a document uses, per face.
///
/// The two passes are deliberate: the set has to be complete before a subset can
/// be built, and the subset decides the new glyph ids, so the text cannot be
/// encoded until every page has been walked.
///
/// Keys are `(glyph, character)` pairs (AUD-82): space and NBSP share a glyph
/// but must keep distinct CIDs, so a map keyed only by glyph would drop one.
#[derive(Debug, Default)]
pub struct FaceCollector {
    faces: BTreeMap<FaceKey, BTreeSet<(GlyphId, Option<char>)>>,
    resources: BTreeMap<FaceKey, FontResource>,
    /// Shaped clusters whose glyph is not a single cmap character: `(source gid, unicode)`.
    shaped: BTreeMap<FaceKey, BTreeSet<(u16, String)>>,
    /// Characters the selected bundled face does not contain.
    missing_glyphs: BTreeMap<FaceKey, BTreeSet<char>>,
}

impl FaceCollector {
    /// Returns an empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one character for a face.
    ///
    /// Missing characters remain attached to their known face so the PDF can
    /// distinguish a missing glyph from a family that is not bundled.
    pub fn add(&mut self, source: &FaceSource, bold: bool, italic: bool, ch: char) {
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold,
            italic,
        };
        let Some(glyph) = glyph_for(source, ch) else {
            // Keep the face in the collection. `add_notdef` gives it a valid
            // subset, while the drawing pass reports this character as a
            // missing glyph instead of misclassifying the whole face.
            self.faces.entry(key.clone()).or_default();
            self.missing_glyphs.entry(key).or_default().insert(ch);
            return;
        };
        self.faces.entry(key).or_default().insert((glyph, Some(ch)));
    }

    /// Records one shaped glyph and the Unicode it should extract as.
    ///
    /// A single scalar is stored with the character path so space and NBSP keep
    /// distinct CIDs. A ligature or a continuation glyph (empty Unicode) is
    /// stored separately and still subsets the source glyph id.
    pub fn add_cluster(
        &mut self,
        source: &FaceSource,
        bold: bool,
        italic: bool,
        glyph_id: u16,
        unicode: &str,
    ) {
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold,
            italic,
        };
        self.insert_cluster(key, glyph_id, unicode);
    }

    /// Records a glyph using an owned resource, retaining it through subsetting.
    pub fn add_resource(&mut self, face: &ResolvedFaceHandle, ch: char) {
        self.add_cluster_resource(
            face,
            face.resource.glyph_id(ch).unwrap_or(0),
            &ch.to_string(),
        );
    }

    /// Records a shaped cluster using the exact owned resource.
    pub fn add_cluster_resource(
        &mut self,
        face: &ResolvedFaceHandle,
        glyph_id: u16,
        unicode: &str,
    ) {
        let key = FaceKey::from_resource(face);
        self.resources
            .entry(key.clone())
            .or_insert_with(|| face.resource.clone());
        self.insert_cluster(key, glyph_id, unicode);
    }

    fn insert_cluster(&mut self, key: FaceKey, glyph_id: u16, unicode: &str) {
        if glyph_id == 0 {
            // Keep a valid face subset for the paint pass, which will report
            // the cluster as missing text instead of treating the face as absent.
            self.faces.entry(key.clone()).or_default();
            self.missing_glyphs
                .entry(key)
                .or_default()
                .extend(unicode.chars());
            return;
        }
        if unicode.chars().count() == 1 {
            if let Some(ch) = unicode.chars().next() {
                self.faces
                    .entry(key)
                    .or_default()
                    .insert((GlyphId::new(u32::from(glyph_id)), Some(ch)));
                return;
            }
        }
        self.faces.entry(key.clone()).or_default();
        self.shaped
            .entry(key)
            .or_default()
            .insert((glyph_id, unicode.to_owned()));
    }

    /// The faces that carry at least one glyph, in a deterministic order.
    #[must_use]
    pub fn faces(&self) -> Vec<FaceKey> {
        self.faces.keys().cloned().collect()
    }

    /// Characters absent from a known bundled face, in deterministic order.
    #[must_use]
    pub fn missing_glyphs(&self) -> Vec<(FaceKey, char)> {
        self.missing_glyphs
            .iter()
            .flat_map(|(key, chars)| chars.iter().map(|ch| (key.clone(), *ch)))
            .collect()
    }

    /// Records glyph 0 (`.notdef`) for every face, which a font must keep.
    pub fn add_notdef(&mut self) {
        for glyphs in self.faces.values_mut() {
            glyphs.insert((GlyphId::new(0), None));
        }
    }

    /// Records two characters against the same source glyph (AUD-82 tests).
    #[cfg(test)]
    fn add_shared_glyph(
        &mut self,
        source: &FaceSource,
        bold: bool,
        italic: bool,
        glyph: GlyphId,
        first: char,
        second: char,
    ) {
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold,
            italic,
        };
        let entry = self.faces.entry(key).or_default();
        entry.insert((glyph, Some(first)));
        entry.insert((glyph, Some(second)));
    }

    /// Builds the subset of one face.
    ///
    /// Returns the subset program, a CID→GID map, widths keyed by CID and the
    /// character→CID table the drawing pass needs.
    ///
    /// # Errors
    ///
    /// Returns a [`FontError`] when the face cannot be parsed or subsetted.
    pub fn build(&self, key: &FaceKey, source: &FaceSource) -> Result<EmbeddedFont, FontError> {
        let variations = source
            .weight
            .map(|weight| vec![(Tag::new(b"wght"), weight)])
            .unwrap_or_default();
        self.build_program(key, source.data, 0, &variations)
    }

    /// Builds a subset from the exact retained program and variation instance.
    ///
    /// # Errors
    /// Returns a font error when the resource is absent or cannot be subsetted.
    pub fn build_resource(&self, key: &FaceKey) -> Result<EmbeddedFont, FontError> {
        let resource = self
            .resources
            .get(key)
            .ok_or_else(|| FontError::Parse("missing owned font resource".into()))?;
        let variations: Vec<_> = resource
            .key()
            .variations
            .iter()
            .map(|axis| (Tag::new(&axis.tag()), axis.value()))
            .collect();
        self.build_program(
            key,
            resource.bytes(),
            resource.key().face_index,
            &variations,
        )
    }

    fn build_program(
        &self,
        key: &FaceKey,
        data: &[u8],
        index: u32,
        variations: &[(Tag, f32)],
    ) -> Result<EmbeddedFont, FontError> {
        let used: Vec<UsedGlyph> = self
            .faces
            .get(key)
            .map(|glyphs| {
                glyphs
                    .iter()
                    .map(|(source, character)| UsedGlyph {
                        source: *source,
                        character: *character,
                        cluster_text: character.map(|ch| ch.to_string()).unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Subset once per distinct source glyph; several CIDs may share it.
        let mut remapper = GlyphRemapper::new();
        let mut seen_gids = BTreeSet::new();
        let mut used = used;
        if let Some(extra) = self.shaped.get(key) {
            for (glyph_id, unicode) in extra {
                used.push(UsedGlyph {
                    source: GlyphId::new(u32::from(*glyph_id)),
                    character: None,
                    cluster_text: unicode.clone(),
                });
            }
        }
        for glyph in &used {
            let old = glyph.source.to_u32() as u16;
            if seen_gids.insert(old) {
                remapper.remap(old);
            }
        }
        let subset_bytes = if variations.is_empty() {
            subset(data, index, &remapper)
        } else {
            subset_with_variations(data, index, variations, &remapper)
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
        let bounds = metrics
            .bounds
            .ok_or_else(|| FontError::Parse("missing font bounds".into()))?;
        let gids = subset_font.glyph_metrics(Size::unscaled(), LocationRef::default());
        // The width is a whole number of 1000-unit steps, which is what the PDF
        // `/W` array is specified in. Keeping it an integer rather than an `f32`
        // is what lets the run-detection in the writer compare widths exactly
        // instead of with a margin that would have to be justified.
        let mut gid_widths: BTreeMap<u16, u16> = BTreeMap::new();
        for &old in &seen_gids {
            let Some(new_gid) = remapper.get(old) else {
                continue;
            };
            let advance = gids
                .advance_width(GlyphId::new(u32::from(new_gid)))
                .unwrap_or(0.0);
            let width = (f64::from(advance) * scale).round();
            gid_widths.insert(new_gid, width.clamp(0.0, f64::from(u16::MAX)) as u16);
        }

        // AUD-82: one CID per (glyph, character) pair, in sorted order so the
        // assignment is deterministic. The content stream writes CIDs; the map
        // stream turns them back into subset GIDs for painting.
        let mut chars: BTreeMap<char, u16> = BTreeMap::new();
        let mut to_unicode: BTreeMap<u16, char> = BTreeMap::new();
        let mut cluster_unicode: BTreeMap<u16, String> = BTreeMap::new();
        let mut cluster_cids: BTreeMap<(u16, String), u16> = BTreeMap::new();
        let mut widths: BTreeMap<u16, u16> = BTreeMap::new();
        let mut cid_to_gid: Vec<u16> = Vec::with_capacity(used.len());
        for glyph in &used {
            let source_gid = glyph.source.to_u32() as u16;
            let Some(new_gid) = remapper.get(source_gid) else {
                continue;
            };
            let cid = u16::try_from(cid_to_gid.len()).unwrap_or(u16::MAX);
            cid_to_gid.push(new_gid);
            widths.insert(cid, *gid_widths.get(&new_gid).unwrap_or(&0));
            cluster_cids.insert((source_gid, glyph.cluster_text.clone()), cid);
            if !glyph.cluster_text.is_empty() {
                cluster_unicode.insert(cid, glyph.cluster_text.clone());
            }
            if let Some(ch) = glyph.character {
                chars.insert(ch, cid);
                to_unicode.insert(cid, ch);
            }
        }

        Ok(EmbeddedFont {
            key: key.clone(),
            data: subset_bytes,
            widths,
            chars,
            to_unicode,
            cluster_cids,
            cluster_unicode,
            cid_to_gid,
            is_cff,
            bounds: [bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max]
                .map(|value| f64::from(value) * scale),
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
    /// Advance width per **CID**, in 1000-unit text space.
    pub widths: BTreeMap<u16, u16>,
    /// Character → CID.
    ///
    /// This is the table the drawing pass needs: a text run arrives as
    /// characters, and encoding it means asking "which CID is this character".
    /// Two characters that share a glyph get two CIDs (AUD-82).
    pub chars: BTreeMap<char, u16>,
    /// CID → the character it stands for, for the `ToUnicode` CMap.
    pub to_unicode: BTreeMap<u16, char>,
    /// `(source glyph id, cluster Unicode)` → CID. Empty Unicode is a continuation glyph.
    pub cluster_cids: BTreeMap<(u16, String), u16>,
    /// CID → the Unicode string a shaped cluster extracts as.
    pub cluster_unicode: BTreeMap<u16, String>,
    /// CID → subset GID, written as `/CIDToGIDMap` (AUD-82).
    pub cid_to_gid: Vec<u16>,
    /// Whether the program carries CFF (OpenType/PostScript) outlines.
    pub is_cff: bool,
    /// Font bounding box in 1000-unit text space, from the embedded program.
    pub bounds: [f64; 4],
    /// Ascender in 1000-unit text space.
    pub ascent: f64,
    /// Descender (negative) in 1000-unit text space.
    pub descent: f64,
    /// Cap height in 1000-unit text space, when the face declares one.
    pub cap_height: Option<f64>,
}

/// Builds the `/CIDToGIDMap` stream body: two big-endian bytes per CID.
#[must_use]
pub fn cid_to_gid_bytes(font: &EmbeddedFont) -> Vec<u8> {
    let mut out = Vec::with_capacity(font.cid_to_gid.len() * 2);
    for &gid in &font.cid_to_gid {
        out.extend_from_slice(&gid.to_be_bytes());
    }
    out
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
    let Some(&[high, low]) = data.get(4..6) else {
        return false;
    };
    let num_tables = usize::from(u16::from_be_bytes([high, low]));
    (0..num_tables).any(|index| {
        let offset = 12 + index * 16 + 4;
        data.get(offset..offset + 4)
            .is_some_and(|tag| tag == b"CFF ")
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
    let mut entries: Vec<(u16, String)> = font
        .to_unicode
        .iter()
        .map(|(gid, ch)| (*gid, ch.to_string()))
        .collect();
    for (cid, text) in &font.cluster_unicode {
        if font.to_unicode.contains_key(cid) || text.is_empty() {
            continue;
        }
        entries.push((*cid, text.clone()));
    }
    entries.sort_by_key(|(cid, _)| *cid);
    // The number before `beginbfchar` is the entry count of that section
    // (PDF 32000-1 §9.10.3, Adobe TN 5099); readers that trust it read exactly
    // that many pairs.
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for (gid, text) in chunk {
            let _ = write!(out, "<{gid:04X}> <");
            for unit in text.encode_utf16() {
                let _ = write!(out, "{unit:04X}");
            }
            out.push_str(">\n");
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
    out.push_str("/CMapName /Identity-H def\n/CMapType 1 def\n");
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
        assert!(collector.missing_glyphs().is_empty());

        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
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
                    family: source.family.to_owned(),
                    instance: None,
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
            family: source.family.to_owned(),
            instance: None,
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
    fn a_character_without_a_glyph_keeps_its_face_and_is_recorded() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let mut collector = FaceCollector::new();
        collector.add(&source, false, false, '\u{10FFFD}');
        assert_eq!(collector.faces().len(), 1);
        assert_eq!(collector.missing_glyphs().len(), 1);
        collector.add_notdef();
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold: false,
            italic: false,
        };
        let font = collector.build(&key, &source).expect("subset .notdef");
        assert!(font.chars.is_empty());
    }

    #[test]
    fn cmaps_are_well_formed_enough_to_be_found() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let mut collector = FaceCollector::new();
        collector.add(&source, false, false, 'A');
        collector.add_notdef();
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold: false,
            italic: false,
        };
        let mut font = collector.build(&key, &source).expect("subset");
        let cmap = String::from_utf8(super::to_unicode_cmap(&font)).expect("utf-8");
        assert!(cmap.contains("beginbfchar"), "{cmap}");
        assert!(cmap.contains("<0041>"), "A must map to U+0041:\n{cmap}");
        let identity = String::from_utf8(super::identity_h_cmap()).expect("utf-8");
        assert!(identity.contains("begincidrange"), "{identity}");
        for count in [0_u16, 1, 100, 101, 201] {
            font.cluster_unicode.clear();
            font.to_unicode = (0..count).map(|cid| (cid, 'A')).collect();
            let cmap = String::from_utf8(super::to_unicode_cmap(&font)).expect("utf-8");
            let counts: Vec<usize> = cmap
                .lines()
                .filter_map(|line| line.strip_suffix(" beginbfchar"))
                .map(|value| value.parse().expect("mapping count"))
                .collect();
            let expected: Vec<usize> = (0..usize::from(count))
                .collect::<Vec<_>>()
                .chunks(100)
                .map(<[usize]>::len)
                .collect();
            assert_eq!(counts, expected, "{count} mappings");
        }
    }

    #[test]
    fn cmap_encodes_non_bmp_destinations_as_utf16_surrogates() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let mut collector = FaceCollector::new();
        collector.add(&source, false, false, 'A');
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold: false,
            italic: false,
        };
        let mut font = collector.build(&key, &source).expect("subset");
        font.to_unicode.clear();
        font.to_unicode.insert(1, '\u{1F600}');
        let cmap = String::from_utf8(super::to_unicode_cmap(&font)).expect("utf-8");
        assert!(cmap.contains("<0001> <D83DDE00>"), "{cmap}");
    }

    /// Every `bfchar` section announces its own entry count, including the
    /// short last one and documents with more than one section.
    #[test]
    fn bfchar_sections_announce_their_entry_count() {
        let source = face_source("Calibri", false, false).expect("bundled");
        let mut collector = FaceCollector::new();
        for ch in (' '..='~').chain('\u{C0}'..='\u{FF}') {
            collector.add(&source, false, false, ch);
        }
        collector.add_notdef();
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold: false,
            italic: false,
        };
        let font = collector.build(&key, &source).expect("subset");
        let cmap = String::from_utf8(super::to_unicode_cmap(&font)).expect("utf-8");
        let mut sections = 0;
        let mut lines = cmap.lines();
        while let Some(line) = lines.next() {
            let Some(count) = line.strip_suffix(" beginbfchar") else {
                continue;
            };
            let declared: usize = count.parse().expect("numeric count");
            let actual = lines.by_ref().take_while(|l| *l != "endbfchar").count();
            assert_eq!(declared, actual, "section {sections}:\n{cmap}");
            assert!((1..=100).contains(&declared), "{declared}");
            sections += 1;
        }
        assert!(
            sections >= 2,
            "the test needs more than 100 mappings:\n{cmap}"
        );
    }

    /// AUD-82: two characters that share a source glyph get distinct CIDs.
    #[test]
    fn two_characters_that_share_a_glyph_get_distinct_cids() {
        use skrifa::{FontRef, MetadataProvider};

        let source = face_source("Calibri", false, false).expect("bundled");
        let face = FontRef::new(source.data).expect("face");
        let space_gid = face.charmap().map(' ').expect("space glyph");
        let mut collector = FaceCollector::new();
        // Carlito gives NBSP its own id; force the collision the bug is about.
        collector.add_shared_glyph(&source, false, false, space_gid, ' ', '\u{A0}');
        collector.add_notdef();
        let key = FaceKey {
            family: source.family.to_owned(),
            instance: None,
            bold: false,
            italic: false,
        };
        let font = collector.build(&key, &source).expect("subset");
        let space = *font.chars.get(&' ').expect("space");
        let nbsp = *font.chars.get(&'\u{A0}').expect("nbsp");
        assert_ne!(
            space, nbsp,
            "shared-outline characters must not share a CID"
        );
        assert_eq!(
            font.cid_to_gid[usize::from(space)],
            font.cid_to_gid[usize::from(nbsp)],
            "both CIDs must paint the same outline"
        );
        assert_eq!(font.to_unicode.get(&space), Some(&' '));
        assert_eq!(font.to_unicode.get(&nbsp), Some(&'\u{A0}'));
    }
}
