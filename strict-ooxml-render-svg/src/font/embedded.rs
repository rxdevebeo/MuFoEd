//! Document font programs, owned by the placed text that uses them.
use super::{FaceSource, FontMetrics};
use skrifa::{
    instance::{LocationRef, Size},
    FontRef, MetadataProvider,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use strict_ooxml_wml::model::{fonts::EmbedKind, Document};

#[derive(Clone, Debug, PartialEq)]
/// Validated OpenType bytes and metrics from a document font part.
pub struct FontProgram {
    /// Collision-free SVG/PDF resource family.
    pub family: String,
    /// Deobfuscated font program.
    pub bytes: Arc<[u8]>,
    /// Vertical metrics of this program.
    pub metrics: FontMetrics,
    /// SHA-256 of [`Self::bytes`], computed once: shaping keys its caches by
    /// it, and hashing a megabyte font per measured word made a document with
    /// embedded fonts take a minute per page.
    pub hash: [u8; 32],
    /// What the font's `OS/2.fsType` lets an output carry of it.
    pub embedding: Embedding,
    /// The family the machine knows the program by, for a caller's own
    /// program ([`super::FontAsset`]); `None` for a document's embedded
    /// font, which only [`Self::family`] names.
    pub installed: Option<String>,
}

/// What a font's licence flags (`OS/2.fsType`) let an output carry of it.
/// Measuring with the font is no embedding; layout uses it whatever this says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Embedding {
    /// Installable, editable or preview & print: subsets and whole programs.
    Allowed,
    /// No subsetting (`0x0100`): only the whole program.
    WholeProgram,
    /// Restricted licence (`0x0002` alone) or bitmap only (`0x0200`): no
    /// outlines in the output.
    Forbidden,
}

impl Embedding {
    /// From `OS/2.fsType`.
    #[must_use]
    pub fn from_fs_type(fs_type: u16) -> Self {
        let usage = fs_type & 0x000E;
        // Version 0-2 fonts may set several usage bits; the least restrictive
        // applies (OpenType OS/2 fsType).
        if (usage & 0x0002 != 0 && usage & 0x000C == 0) || fs_type & 0x0200 != 0 {
            Self::Forbidden
        } else if fs_type & 0x0100 != 0 {
            Self::WholeProgram
        } else {
            Self::Allowed
        }
    }

    /// Whether an output that embeds the whole program (`true`, SVG) or a
    /// subset (`false`, PDF) may carry the font.
    #[must_use]
    pub fn allows(self, whole_program: bool) -> bool {
        match self {
            Self::Allowed => true,
            Self::WholeProgram => whole_program,
            Self::Forbidden => false,
        }
    }
}
impl FontProgram {
    /// A program from a single font's bytes, named `<prefix>-<sha256>` so
    /// that no two programs share a resource family; `None` when the bytes are
    /// no usable font.
    pub(crate) fn from_bytes(bytes: Vec<u8>, prefix: &str) -> Option<Self> {
        let font = FontRef::new(&bytes).ok()?;
        let raw = font.metrics(Size::unscaled(), LocationRef::default());
        let units = f64::from(raw.units_per_em);
        if units <= 0.0 {
            return None;
        }
        let metrics = word_line_metrics(&font, units).unwrap_or(FontMetrics {
            units_per_em: units,
            ascender: f64::from(raw.ascent) / units,
            descender: f64::from(raw.descent).abs() / units,
            line_gap: f64::from(raw.leading) / units,
            width_scale: 1.0,
        });
        let embedding = {
            use skrifa::raw::TableProvider as _;
            font.os2().map_or(Embedding::Allowed, |os2| {
                Embedding::from_fs_type(os2.fs_type())
            })
        };
        let hash = super::hash_face_bytes(&bytes);
        let family = format!(
            "{prefix}-{}",
            hash.iter()
                .fold(String::with_capacity(64), |mut hex, byte| {
                    use std::fmt::Write as _;
                    let _ = write!(hex, "{byte:02x}");
                    hex
                })
        );
        Some(Self {
            family,
            bytes: bytes.into(),
            metrics,
            hash,
            embedding,
            installed: None,
        })
    }
    /// Checks that a subset supplies every character in this text fragment.
    pub fn supports(&self, text: &str) -> bool {
        use skrifa::{FontRef, MetadataProvider};
        let Ok(font) = FontRef::new(&self.bytes) else {
            return false;
        };
        text.chars()
            .all(|ch| ch.is_control() || font.charmap().map(ch).is_some())
    }
    /// Real advance of a scalar in em units.
    pub fn advance_em(&self, ch: char) -> Option<f64> {
        use skrifa::{
            instance::{LocationRef, Size},
            FontRef, MetadataProvider,
        };
        let font = FontRef::new(&self.bytes).ok()?;
        let glyph = font.charmap().map(ch)?;
        let advance = font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(glyph)?;
        Some(f64::from(advance) / self.metrics.units_per_em)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
/// Programs indexed by requested family (lower-cased) and exact style.
pub struct FontPrograms {
    /// Family/style registry.
    pub faces: BTreeMap<(String, bool, bool), Arc<FontProgram>>,
    /// Unavailable programs reported by layout.
    pub(crate) warnings: Vec<String>,
    /// Caller programs that draw what a run's face lacks
    /// ([`super::assets::FontAsset::fallback`]), in the caller's order.
    pub fallbacks: Vec<super::assets::ScriptFallback>,
}
impl FontPrograms {
    /// The document's embedded fonts, then the caller's `assets` for every
    /// family and style the document does not embed.
    pub(crate) fn load(
        document: &Document,
        media: Option<&dyn crate::MediaSource>,
        assets: Option<&super::assets::FontAssets>,
    ) -> Self {
        let mut result = Self::load_embedded(document, media);
        if let Some(assets) = assets {
            for (key, program) in assets.programs() {
                result
                    .faces
                    .entry(key.clone())
                    .or_insert_with(|| Arc::clone(program));
            }
            result.fallbacks = assets.fallbacks().to_vec();
        }
        result
    }
    #[allow(clippy::too_many_lines)]
    fn load_embedded(document: &Document, media: Option<&dyn crate::MediaSource>) -> Self {
        let mut result = Self::default();
        let Some(table) = &document.font_table else {
            return result;
        };
        let Some(media) = media else {
            if table.fonts.iter().any(|entry| !entry.embeds.is_empty()) {
                result
                    .warnings
                    .push("font.embedded.unavailable: no package media source".to_owned());
            }
            return result;
        };
        let mut total_bytes = 0usize;
        for entry in &table.fonts {
            for (kind, embedded) in &entry.embeds {
                let Ok(mut bytes) = media.read_media(&embedded.part) else {
                    result
                        .warnings
                        .push(format!("font.embedded.unavailable: {}", embedded.part));
                    continue;
                };
                if bytes.len() < 32
                    || bytes.len() > 64 * 1024 * 1024
                    || total_bytes.saturating_add(bytes.len()) > 128 * 1024 * 1024
                    || result.faces.len() >= 256
                {
                    result
                        .warnings
                        .push(format!("font.embedded.limit: {}", embedded.part));
                    continue;
                }
                if let Some(key) = embedded.font_key.as_deref() {
                    let hex: String = key.chars().filter(char::is_ascii_hexdigit).collect();
                    if hex.len() != 32 {
                        result
                            .warnings
                            .push(format!("font.embedded.key: {}", embedded.part));
                        continue;
                    }
                    let mut guid = [0u8; 16];
                    let mut valid = true;
                    for (byte, pair) in guid.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
                        if let Some(value) = std::str::from_utf8(pair)
                            .ok()
                            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                        {
                            *byte = value;
                        } else {
                            valid = false;
                            break;
                        }
                    }
                    if !valid {
                        continue;
                    }
                    // The key is applied with its bytes reversed, repeated over
                    // the first 32 bytes of the part (ECMA-376 Part 1 §17.8.1).
                    for (byte, key) in bytes.iter_mut().take(32).zip(guid.iter().rev().cycle()) {
                        *byte ^= key;
                    }
                }
                let size = bytes.len();
                let Some(program) = FontProgram::from_bytes(bytes, "OOXML") else {
                    result
                        .warnings
                        .push(format!("font.embedded.invalid: {}", embedded.part));
                    continue;
                };
                let bold = matches!(kind, EmbedKind::Bold | EmbedKind::BoldItalic);
                let italic = matches!(kind, EmbedKind::Italic | EmbedKind::BoldItalic);
                total_bytes += size;
                result.faces.insert(
                    (entry.name.trim().to_lowercase(), bold, italic),
                    Arc::new(program),
                );
            }
        }
        result
    }
    /// Looks up an exact face request (family case-insensitive).
    pub fn for_request(&self, family: &str, bold: bool, italic: bool) -> Option<&Arc<FontProgram>> {
        self.faces
            .get(&(family.trim().to_lowercase(), bold, italic))
    }
    /// Looks up its stable resource name.
    pub fn for_resource(&self, family: &str) -> Option<&Arc<FontProgram>> {
        self.faces
            .values()
            .chain(self.fallbacks.iter().map(|fallback| &fallback.program))
            .find(|program| program.family == family)
    }

    /// Whether the resource `family` is drawing only as a fallback: it keeps
    /// the line box of the face the run asked for.
    #[must_use]
    pub fn is_fallback(&self, family: &str) -> bool {
        self.fallbacks
            .iter()
            .any(|fallback| fallback.program.family == family)
    }

    /// The fallback program that draws `ch` for a run in `language` (BCP 47,
    /// any case) and style. Preferred, in order: the longest language tag
    /// that is a prefix of the run's (kana is `ja` and hangul `ko` whatever
    /// the run says), then the run's style, then the caller's order.
    #[must_use]
    pub fn fallback_for(
        &self,
        ch: char,
        language: Option<&str>,
        bold: bool,
        italic: bool,
    ) -> Option<&Arc<FontProgram>> {
        let script = match ch {
            '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => {
                Some("ja")
            }
            '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}' => {
                Some("ko")
            }
            _ => None,
        };
        let language = language.map(str::to_ascii_lowercase);
        let wanted = script.or(language.as_deref()).unwrap_or("");
        let mut best: Option<((usize, u8), &Arc<FontProgram>)> = None;
        for fallback in &self.fallbacks {
            let tag = fallback.language.as_str();
            let matches = tag.is_empty()
                || wanted == tag
                || wanted
                    .strip_prefix(tag)
                    .is_some_and(|rest| rest.starts_with('-'));
            if !matches || fallback.program.advance_em(ch).is_none() {
                continue;
            }
            let style = u8::from(fallback.bold == bold) * 2 + u8::from(fallback.italic == italic);
            let score = (tag.len(), style);
            if best.is_none_or(|(top, _)| score > top) {
                best = Some((score, &fallback.program));
            }
        }
        best.map(|(_, program)| program).or_else(|| {
            // No program for the language: any that has the glyph beats none.
            self.fallbacks
                .iter()
                .find(|fallback| fallback.program.advance_em(ch).is_some())
                .map(|fallback| &fallback.program)
        })
    }
}
/// Returns the font bytes this run measures and paints.
pub fn run_face_source<'a>(
    run: &'a crate::style::ComputedRun,
    family: &str,
) -> Option<FaceSource<'a>> {
    if let Some(program) = run.font_programs.for_resource(family) {
        return Some(FaceSource {
            family: &program.family,
            data: &program.bytes,
            weight: None,
            single: false,
        });
    }
    super::face_source(family, run.bold, run.italic)
}

/// Effective vertical metrics, including a document-supplied font.
///
/// A script fallback face keeps the line box of the face the run asked for
/// ([`super::family::is_script_fallback`], [`FontPrograms::is_fallback`]).
pub fn run_metrics(
    run: &crate::style::ComputedRun,
    family: &str,
    provider: &dyn super::FontProvider,
) -> FontMetrics {
    if let Some(program) = run.font_programs.for_resource(family) {
        if !run.font_programs.is_fallback(family) {
            return program.metrics;
        }
        return provider.metrics(crate::style::script_line_family(run), run.bold, run.italic);
    }
    let line_family = if super::family::is_script_fallback(family) {
        crate::style::script_line_family(run)
    } else {
        family
    };
    provider.metrics(line_family, run.bold, run.italic)
}
/// The vertical metrics Word lays a document font out with (GDI): the OS/2
/// Windows ascent and descent, plus the external leading GDI derives from the
/// `hhea` line gap. A subset whose `hhea` box is tighter than its Windows box
/// (doc 091's embedded Helvetica: 1.0 em against 1.175) otherwise lays every
/// line out too close, and the document loses a tenth of its pages.
fn word_line_metrics(font: &FontRef<'_>, units: f64) -> Option<FontMetrics> {
    use skrifa::raw::TableProvider;
    let os2 = font.os2().ok()?;
    let hhea = font.hhea().ok()?;
    let win_ascent = f64::from(os2.us_win_ascent());
    let win_descent = f64::from(os2.us_win_descent());
    if win_ascent + win_descent <= 0.0 {
        return None;
    }
    let hhea_height = f64::from(hhea.ascender().to_i16()) - f64::from(hhea.descender().to_i16());
    let external =
        (f64::from(hhea.line_gap().to_i16()) - (win_ascent + win_descent - hhea_height)).max(0.0);
    Some(FontMetrics {
        units_per_em: units,
        ascender: win_ascent / units,
        descender: win_descent / units,
        line_gap: external / units,
        width_scale: 1.0,
    })
}

/// Effective scalar advance from the program used for shaping and paint.
pub fn run_advance_em(
    run: &crate::style::ComputedRun,
    family: &str,
    ch: char,
    provider: &dyn super::FontProvider,
) -> f64 {
    run.font_programs
        .for_resource(family)
        .and_then(|program| program.advance_em(ch))
        .unwrap_or_else(|| provider.advance_em(family, ch, run.bold, run.italic))
}

#[cfg(test)]
mod tests {
    use super::word_line_metrics;
    use skrifa::FontRef;

    /// Kana goes to the Japanese program, hangul to the Korean one, Han by
    /// the run's language; a style match only breaks ties within a language.
    #[test]
    fn a_fallback_is_chosen_by_script_then_language_then_style() {
        use super::super::assets::ScriptFallback;
        use super::{FontProgram, FontPrograms};
        use std::sync::Arc;
        // STIX stands in for a CJK face: the choice is checked on a glyph it
        // has, with the script read from the character the tests pass.
        let source = super::super::face_source("STIX Two Math", false, false).expect("STIX");
        let program = Arc::new(FontProgram::from_bytes(source.data.to_vec(), "T").expect("font"));
        let entry = |language: &str, bold: bool| ScriptFallback {
            language: language.to_owned(),
            bold,
            italic: false,
            program: Arc::new((*program).clone()),
        };
        let programs = FontPrograms {
            fallbacks: vec![
                entry("zh-cn", false),
                entry("zh-tw", false),
                entry("ja", false),
                entry("ja", true),
            ],
            ..FontPrograms::default()
        };
        let pick = |language: Option<&str>, bold: bool| {
            let chosen = programs
                .fallback_for('A', language, bold, false)
                .expect("a program");
            programs
                .fallbacks
                .iter()
                .position(|fallback| Arc::ptr_eq(&fallback.program, chosen))
        };
        assert_eq!(
            pick(None, false),
            Some(0),
            "no language: the caller's order"
        );
        assert_eq!(pick(Some("zh-TW"), false), Some(1));
        assert_eq!(pick(Some("zh-HK"), false), Some(0), "no zh-hk program");
        assert_eq!(pick(Some("ja-JP"), false), Some(2));
        assert_eq!(pick(Some("ja-JP"), true), Some(3));
        assert_eq!(programs.fallback_for('\u{AC00}', None, false, false), None);
    }

    /// Carlito carries Calibri's metrics: Word's single line is 1.2207 em,
    /// the Windows box with no external leading.
    #[test]
    fn a_document_font_takes_the_windows_line_box() {
        let source = super::super::face_source("Calibri", false, false).expect("Carlito");
        let font = FontRef::new(source.data).expect("font");
        let metrics = word_line_metrics(&font, 2048.0).expect("metrics");
        assert!(
            (metrics.line_height_em() - 2500.0 / 2048.0).abs() < 0.002,
            "{}",
            metrics.line_height_em()
        );
    }
}
