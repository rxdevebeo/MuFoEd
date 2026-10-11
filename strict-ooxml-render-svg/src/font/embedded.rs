//! Document font programs, owned by the placed text that uses them.
use super::{FaceSource, FontMetrics};
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
}
impl FontProgram {
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
/// Programs indexed by requested family and exact style.
pub struct FontPrograms {
    /// Family/style registry.
    pub faces: BTreeMap<(String, bool, bool), Arc<FontProgram>>,
    /// Unavailable programs reported by layout.
    pub(crate) warnings: Vec<String>,
}
impl FontPrograms {
    pub(crate) fn load(document: &Document, media: Option<&dyn crate::MediaSource>) -> Self {
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
                    let hex: String = key.chars().filter(|ch| ch.is_ascii_hexdigit()).collect();
                    if hex.len() != 32 {
                        result
                            .warnings
                            .push(format!("font.embedded.key: {}", embedded.part));
                        continue;
                    }
                    let mut guid = [0u8; 16];
                    let mut valid = true;
                    for (index, byte) in guid.iter_mut().enumerate() {
                        match u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16) {
                            Ok(value) => *byte = value,
                            Err(_) => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    if !valid {
                        continue;
                    }
                    for (index, byte) in bytes[..32].iter_mut().enumerate() {
                        *byte ^= guid[15 - index % 16];
                    }
                }
                use skrifa::{
                    instance::{LocationRef, Size},
                    FontRef, MetadataProvider,
                };
                let Ok(font) = FontRef::new(&bytes) else {
                    result
                        .warnings
                        .push(format!("font.embedded.invalid: {}", embedded.part));
                    continue;
                };
                let metrics = font.metrics(Size::unscaled(), LocationRef::default());
                let units = f64::from(metrics.units_per_em);
                if units <= 0.0 {
                    continue;
                }
                let metrics = FontMetrics {
                    units_per_em: units,
                    ascender: f64::from(metrics.ascent) / units,
                    descender: f64::from(metrics.descent).abs() / units,
                    line_gap: f64::from(metrics.leading) / units,
                    width_scale: 1.0,
                };
                let family = format!("OOXML-{}", super::hash_face_bytes_hex(&bytes));
                let bold = matches!(kind, EmbedKind::Bold | EmbedKind::BoldItalic);
                let italic = matches!(kind, EmbedKind::Italic | EmbedKind::BoldItalic);
                total_bytes += bytes.len();
                result.faces.insert(
                    (entry.name.to_string(), bold, italic),
                    Arc::new(FontProgram {
                        family,
                        bytes: bytes.into(),
                        metrics,
                    }),
                );
            }
        }
        result
    }
    /// Looks up an exact embedded face request.
    pub fn for_request(&self, family: &str, bold: bool, italic: bool) -> Option<&Arc<FontProgram>> {
        self.faces.get(&(family.to_owned(), bold, italic))
    }
    /// Looks up its stable resource name.
    pub fn for_resource(&self, family: &str) -> Option<&Arc<FontProgram>> {
        self.faces.values().find(|program| program.family == family)
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
pub fn run_metrics(
    run: &crate::style::ComputedRun,
    family: &str,
    provider: &dyn super::FontProvider,
) -> FontMetrics {
    run.font_programs.for_resource(family).map_or_else(
        || provider.metrics(family, run.bold, run.italic),
        |program| program.metrics,
    )
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
