//! The font table of a converted document (W1, `PDF-IMPORT-COMPARISON-2026-10-19.md`).
//!
//! A converted document names the PDF's families and ships none of them. A
//! renderer that does not have `Montserrat` stands another face in for it, and
//! with an empty `word/fontTable.xml` it has nothing to choose by: WPS drew every
//! sans document of the comparison in a monospaced default, so every line grew
//! and every page overflowed.
//!
//! The PDF states what the substitute needs to know — fixed or proportional,
//! serif or not, script, and sometimes a PANOSE number — in the font descriptor.
//! This module carries those statements into `w:family`, `w:pitch` and
//! `w:panose1`, and nothing it would have to guess: a font that states no flags
//! gets hints only when its name is one of the standard fourteen, whose look
//! ISO 32000-1 fixes.
//!
//! It also names a concrete stand-in in `w:altName` — Arial, Times New Roman or
//! Courier New by that same class — because the classification alone changes
//! nothing in WPS 12.1: measured on `TERMS & CONDITIONS.pdf`, `family`, `pitch`
//! and `panose1` left Montserrat drawn in Liberation Mono (7 pages for 6), and
//! `altName` alone drew it in Arial on 6. A stand-in is chosen only from a
//! stated class, never from the name.
//!
//! Two refinements came from a run over the whole corpus
//! (`PDF-CORPUS-RUN-2026-10-19.md`), where a reflowed document that came out a
//! page shorter than its source moved every page after it:
//!
//! - **The family's own name.** `/BaseFont` spells it without spaces, and
//!   splitting `DejaVuSans` at its capitals named `Deja Vu Sans`, which no
//!   machine has, so a font that *was* installed was drawn in Arial. The reader
//!   now takes the family from the descriptor or the embedded program
//!   ([`FontFace::family`]), and the runs are renamed to it ([`rename`]).
//! - **A stand-in as wide as the face.** Arial is narrower than DejaVu Sans,
//!   Montserrat or Onest, and their text set in it ran short. The letters the
//!   PDF set are measured against Helvetica's (Arial's) and Times's widths for
//!   the same letters ([`width_ratios`]); a sans face a twelfth wider gets
//!   Verdana, a serif one Georgia.

use std::collections::BTreeMap;
use std::sync::Arc;

use strict_ooxml_pdf::FontFace;
use strict_ooxml_wml::model::fonts::{FontEntry, FontHints, FontTable};

use crate::semantic::FontName;

/// `w:family` and `w:pitch` for one face, when it says enough to choose them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Look {
    family: &'static str,
    fixed: bool,
}

impl Look {
    /// The face every renderer has for this class and width, when there is
    /// one. `widths` is the face's letters against Arial's and against
    /// Times's ([`Widths`]); without it the class's usual face stands in.
    fn stand_in(self, widths: Option<Widths>) -> Option<&'static str> {
        match self.family {
            "modern" => Some("Courier New"),
            "roman"
                if widths
                    .and_then(|w| w.times)
                    .is_some_and(|w| w >= WIDE_ROMAN) =>
            {
                Some("Georgia")
            }
            "roman" => Some("Times New Roman"),
            "swiss"
                if widths
                    .and_then(|w| w.arial)
                    .is_some_and(|w| w >= WIDE_SWISS) =>
            {
                Some("Verdana")
            }
            "swiss" => Some("Arial"),
            _ => None,
        }
    }

    fn of(face: &FontFace, family: &str, measured: Option<Widths>) -> Option<Self> {
        let look = Self::stated(face, family);
        // The letters the PDF set overrule the descriptor's pitch, either way:
        // Palatino and Trebuchet MS under `/Flags 33` claimed a fixed pitch,
        // Roboto Mono under its flags a proportional serif. Where too few
        // letters were set to measure, the name decides the same question.
        let fixed = measured
            .and_then(|widths| widths.fixed)
            .or_else(|| look.map(|_| is_monospace_name(family)));
        match (look, fixed) {
            (Some(look), Some(false)) if look.fixed => Some(Self {
                family: "swiss",
                fixed: false,
            }),
            (Some(look), Some(true)) if !look.fixed && look.family != "decorative" => Some(Self {
                family: "modern",
                fixed: true,
            }),
            (None, Some(true)) => Some(Self {
                family: "modern",
                fixed: true,
            }),
            (look, _) => look,
        }
    }

    fn stated(face: &FontFace, family: &str) -> Option<Self> {
        // A pi font by name, before the flags: subset producers set Symbolic on
        // ordinary text faces too, so the flag cannot say this.
        if is_pi_font(family) {
            return Some(Self {
                family: "decorative",
                fixed: false,
            });
        }
        if let (Some(fixed), Some(serif), Some(script)) =
            (face.fixed_pitch(), face.serif(), face.script())
        {
            let family = if fixed {
                "modern"
            } else if script {
                "script"
            } else if serif {
                "roman"
            } else {
                "swiss"
            };
            return Some(Self { family, fixed });
        }
        Self::standard(family)
    }

    /// The standard fourteen, whose look the specification fixes; a PDF may use
    /// them without a descriptor.
    fn standard(family: &str) -> Option<Self> {
        let (family, fixed) = match family {
            "Courier" => ("modern", true),
            "Times" | "Times New Roman" => ("roman", false),
            "Helvetica" | "Arial" => ("swiss", false),
            _ => return None,
        };
        Some(Self { family, fixed })
    }
}

/// A family whose name says it is monospaced.
fn is_monospace_name(family: &str) -> bool {
    let lower = family.to_ascii_lowercase();
    [
        "mono",
        "courier",
        "consol",
        "code",
        "typewriter",
        "fixedsys",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

/// Families of pictograms rather than letters.
fn is_pi_font(family: &str) -> bool {
    let lower = family.to_ascii_lowercase();
    [
        "wingdings",
        "webdings",
        "symbol",
        "zapf dingbats",
        "zapfdingbats",
        "dingbats",
    ]
    .iter()
    .any(|pi| lower.starts_with(pi))
}

/// Verdana is about a sixth wider than Arial; a face a twelfth wider is
/// nearer it.
const WIDE_SWISS: f64 = 1.08;
/// Georgia is about an eighth wider than Times; a face a sixteenth wider is
/// nearer it.
const WIDE_ROMAN: f64 = 1.06;
/// Fewer measured letters than this say nothing about a face's width.
const MIN_MEASURED: usize = 50;

/// How wide a face's letters run, against the same letters in Arial and in
/// Times (1.0 = as wide).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Widths {
    /// Against Arial; `None` with too few letters set to say.
    pub(crate) arial: Option<f64>,
    /// Against Times; `None` with too few letters set to say.
    pub(crate) times: Option<f64>,
    /// Whether the face set every measured lowercase Latin letter at one
    /// advance; `None` with too few distinct ones to say.
    pub(crate) fixed: Option<bool>,
}

/// Helvetica's (Arial's) and Times-Roman's advances, in thousandths of an em,
/// for `a`-`z`, `A`-`Z` and `0`-`9` (the standard fourteen's AFM metrics,
/// which Arial and Times New Roman match).
const HELVETICA_LOWER: [u16; 26] = [
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500,
];
const HELVETICA_UPPER: [u16; 26] = [
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667,
    611, 722, 667, 944, 667, 667, 611,
];
const TIMES_LOWER: [u16; 26] = [
    444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, 500, 500, 333, 389,
    278, 500, 500, 722, 500, 500, 444,
];
const TIMES_UPPER: [u16; 26] = [
    722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556,
    611, 722, 722, 944, 722, 722, 611,
];

/// A character's advance in Helvetica and in Times, in ems. A letter of
/// another alphabet (Cyrillic, Greek, accented Latin) counts as an average
/// letter of its case; anything else is not measured.
fn reference(ch: char) -> Option<(f64, f64)> {
    let mean =
        |table: &[u16; 26]| f64::from(table.iter().map(|&w| u32::from(w)).sum::<u32>()) / 26_000.0;
    let at = |table: &[u16; 26], base: char| {
        let index = usize::try_from(u32::from(ch).checked_sub(u32::from(base))?).ok()?;
        table.get(index).map(|&w| f64::from(w) / 1000.0)
    };
    match ch {
        'a'..='z' => Some((at(&HELVETICA_LOWER, 'a')?, at(&TIMES_LOWER, 'a')?)),
        'A'..='Z' => Some((at(&HELVETICA_UPPER, 'A')?, at(&TIMES_UPPER, 'A')?)),
        '0'..='9' => Some((0.556, 0.5)),
        _ if ch.is_lowercase() => Some((mean(&HELVETICA_LOWER), mean(&TIMES_LOWER))),
        _ if ch.is_uppercase() => Some((mean(&HELVETICA_UPPER), mean(&TIMES_UPPER))),
        _ => None,
    }
}

/// The width of each family's letters, by the family name its runs carry
/// (`FontName::of`), measured on the glyphs the pages set: the advance the PDF
/// gave each letter, in ems of its size, against the same letters in Arial and
/// in Times. Upright regular faces are measured when they set enough letters,
/// since bold and italic run wider and narrower; a family with too few is
/// left out.
pub(crate) fn width_ratios(pages: &[strict_ooxml_pdf::PdfPage]) -> BTreeMap<String, Widths> {
    // (regular, all): (count, set, arial, times)
    let mut sums: BTreeMap<String, [(usize, f64, f64, f64); 2]> = BTreeMap::new();
    // The narrowest and widest advance of each lowercase Latin letter set.
    let mut letters: BTreeMap<String, BTreeMap<char, (f64, f64)>> = BTreeMap::new();
    for page in pages {
        for item in page.items() {
            let strict_ooxml_pdf::Item::Glyph(glyph) = item else {
                continue;
            };
            let mut chars = glyph.text.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                continue;
            };
            let Some((arial, times)) = reference(ch) else {
                continue;
            };
            if !(glyph.size > 0.0 && glyph.width.is_finite() && glyph.width > 0.0) {
                continue;
            }
            let name = FontName::of(&glyph.font);
            let set = glyph.width / glyph.size;
            if ch.is_ascii_lowercase() {
                let seen = letters
                    .entry(name.family.clone())
                    .or_default()
                    .entry(ch)
                    .or_insert((set, set));
                seen.0 = seen.0.min(set);
                seen.1 = seen.1.max(set);
            }
            let entry = sums.entry(name.family).or_default();
            for (slot, counts) in entry.iter_mut().enumerate() {
                if slot == 0 && (name.bold || name.italic) {
                    continue;
                }
                counts.0 = counts.0.saturating_add(1);
                counts.1 += set;
                counts.2 += arial;
                counts.3 += times;
            }
        }
    }
    let mut out = BTreeMap::new();
    for (family, [regular, all]) in sums {
        let (count, set, arial, times) = if regular.0 >= MIN_MEASURED {
            regular
        } else {
            all
        };
        let enough = count >= MIN_MEASURED && arial > 0.0 && times > 0.0;
        // Fixed pitch, measured: `i` and `m` as wide as each other. Five
        // letters say it, where the widths need fifty.
        let fixed = letters.get(&family).and_then(|seen| {
            let narrowest = seen
                .values()
                .map(|(low, _)| *low)
                .fold(f64::INFINITY, f64::min);
            let widest = seen.values().map(|(_, high)| *high).fold(0.0_f64, f64::max);
            (seen.len() >= 5 && narrowest > 0.0).then_some(widest / narrowest < 1.05)
        });
        if enough || fixed.is_some() {
            out.insert(
                family,
                Widths {
                    arial: enough.then_some(set / arial),
                    times: enough.then_some(set / times),
                    fixed,
                },
            );
        }
    }
    out
}

/// The font table, and the names to give the runs: `FontName::of`'s family →
/// the family the font itself names, where they differ.
pub(crate) struct Fonts {
    pub(crate) table: Option<FontTable>,
    pub(crate) renames: BTreeMap<String, String>,
    /// `(family, widths, stand-in)` for each family measured, for the report.
    pub(crate) stand_ins: Vec<(String, Widths, &'static str)>,
}

/// One `w:font` per family the PDF's fonts name, with the hints its faces state.
///
/// A family's faces (regular, bold, italic) usually agree; when they do not,
/// the regular face's statement wins, then the first face that states one.
/// The table is `None` when no font was read.
pub(crate) fn table(faces: &[FontFace], widths: &BTreeMap<String, Widths>) -> Fonts {
    // Faces that `/BaseFont` puts in one family may name themselves apart:
    // `Google Sans Text` and, without a typographic family, `Google Sans Text
    // Medium`. The runs carry one name, so the family is the shortest.
    let mut own: BTreeMap<String, String> = BTreeMap::new();
    for face in faces {
        let split = FontName::of(&face.base_font).family;
        let Some(named) = face
            .family
            .clone()
            .filter(|family| !family.trim().is_empty())
        else {
            continue;
        };
        let entry = own.entry(split).or_insert_with(|| named.clone());
        if named.len() < entry.len() {
            *entry = named;
        }
    }
    let mut families: BTreeMap<String, Vec<&FontFace>> = BTreeMap::new();
    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    // The width measured under the run's name, by the family's own name.
    let mut measured: BTreeMap<String, Widths> = BTreeMap::new();
    for face in faces {
        let name = FontName::of(&face.base_font);
        if name.family.trim().is_empty() {
            continue;
        }
        let family = own
            .get(&name.family)
            .cloned()
            .unwrap_or_else(|| name.family.clone());
        if family != name.family {
            renames.insert(name.family.clone(), family.clone());
        }
        // A face that states no widths advanced every glyph by an estimate:
        // `Times` without `/Widths` set every letter at half an em, measured
        // as a fixed pitch, and stood in as Courier New.
        if let Some(width) = widths.get(&name.family).filter(|_| face.widths_stated) {
            measured.entry(family.clone()).or_insert(*width);
        }
        let regular = !name.bold && !name.italic;
        let entry = families.entry(family).or_default();
        if regular {
            entry.insert(0, face);
        } else {
            entry.push(face);
        }
    }
    let mut stand_ins = Vec::new();
    if families.is_empty() {
        return Fonts {
            table: None,
            renames,
            stand_ins,
        };
    }
    let fonts = families
        .into_iter()
        .map(|(family, faces)| {
            let width = measured.get(&family).copied();
            let look = faces.iter().find_map(|face| Look::of(face, &family, width));
            let panose = faces.iter().find_map(|face| face.panose);
            let stand_in = look.and_then(|look| look.stand_in(width));
            if let (Some(width), Some(stand_in)) = (width.filter(|w| w.arial.is_some()), stand_in) {
                stand_ins.push((family.clone(), width, stand_in));
            }
            let alt_name = stand_in
                .filter(|stand_in| *stand_in != family)
                .map(Arc::from);
            FontEntry {
                hints: FontHints {
                    alt_name,
                    panose1: panose.map(|bytes| Arc::from(hex(&bytes))),
                    family: look.map(|look| Arc::from(look.family)),
                    pitch: look
                        .map(|look| Arc::from(if look.fixed { "fixed" } else { "variable" })),
                    ..FontHints::default()
                },
                name: Arc::from(family),
                embeds: BTreeMap::new(),
            }
        })
        .collect();
    Fonts {
        table: Some(FontTable { fonts }),
        renames,
        stand_ins,
    }
}

/// Renames the runs' fonts from `FontName::of`'s spelling to the family the
/// font names: the body, its tables and text boxes, and the headers and
/// footers.
pub(crate) fn rename(
    document: &mut strict_ooxml_wml::model::document::Document,
    renames: &BTreeMap<String, String>,
) {
    if renames.is_empty() {
        return;
    }
    rename_blocks(&mut document.body.blocks, renames, 0);
    for part in &mut document.headers_footers {
        rename_blocks(&mut part.blocks, renames, 0);
    }
}

fn rename_props(
    props: &mut strict_ooxml_wml::model::props::RunProperties,
    renames: &BTreeMap<String, String>,
) {
    let Some(fonts) = props.fonts.as_mut() else {
        return;
    };
    for slot in [
        &mut fonts.ascii,
        &mut fonts.h_ansi,
        &mut fonts.complex_script,
        &mut fonts.east_asia,
    ] {
        if let Some(to) = slot.as_deref().and_then(|name| renames.get(name)) {
            *slot = Some(Arc::from(to.as_str()));
        }
    }
}

fn rename_blocks(
    blocks: &mut [strict_ooxml_wml::model::block::Block],
    renames: &BTreeMap<String, String>,
    depth: usize,
) {
    use strict_ooxml_wml::model::block::Block;
    use strict_ooxml_wml::model::drawing::{DrawingKind, Graphic};
    use strict_ooxml_wml::model::inline::Inline;

    if depth > 16 {
        return;
    }
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                if let Some(props) = paragraph.props.run_props.as_mut() {
                    rename_props(props, renames);
                }
                for inline in &mut paragraph.inlines {
                    match inline {
                        Inline::Run(run) => rename_props(&mut run.props, renames),
                        Inline::Drawing(drawing) => {
                            let graphic = match &mut drawing.kind {
                                DrawingKind::Anchor(anchor) => Some(anchor.graphic.as_mut()),
                                DrawingKind::Inline(inline) => Some(inline.graphic.as_mut()),
                                DrawingKind::Opaque(_) => None,
                            };
                            if let Some(Graphic::Shape(shape)) = graphic {
                                if let Some(text) = shape.text.as_mut() {
                                    rename_blocks(&mut text.blocks, renames, depth + 1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        rename_blocks(&mut cell.blocks, renames, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(20), |mut out, byte| {
            let _ = write!(out, "{byte:02X}");
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(base_font: &str, flags: Option<u32>) -> FontFace {
        FontFace {
            base_font: base_font.to_owned(),
            flags,
            panose: None,
            embedded: true,
            family: None,
            widths_stated: true,
        }
    }

    fn table_of(faces: &[FontFace]) -> Option<FontTable> {
        table(faces, &BTreeMap::new()).table
    }

    fn hints<'a>(table: &'a FontTable, name: &str) -> &'a FontHints {
        &table
            .fonts
            .iter()
            .find(|entry| entry.name.as_ref() == name)
            .unwrap_or_else(|| panic!("no entry for {name}"))
            .hints
    }

    #[test]
    fn a_sans_face_is_swiss_and_proportional() {
        // Montserrat states Nonsymbolic (32) and nothing else.
        let table = table_of(&[face("AAAAAB+Montserrat-Regular", Some(32))]).unwrap();
        let hints = hints(&table, "Montserrat");
        assert_eq!(hints.family.as_deref(), Some("swiss"));
        assert_eq!(hints.pitch.as_deref(), Some("variable"));
        assert_eq!(hints.alt_name.as_deref(), Some("Arial"));
    }

    #[test]
    fn a_face_is_not_its_own_stand_in_and_a_pi_font_has_none() {
        let table = table_of(&[
            face("ArialMT", Some(32)),
            face("Wingdings-Regular", Some(4)),
        ])
        .unwrap();
        assert_eq!(hints(&table, "Arial").alt_name, None);
        assert_eq!(hints(&table, "Wingdings").alt_name, None);
    }

    #[test]
    fn fixed_pitch_wins_over_serif() {
        // A monospaced face that also states Serif: FixedPitch | Serif | Nonsymbolic.
        let table = table_of(&[face("Consolas", Some(1 | 2 | 32))]).unwrap();
        let hints = hints(&table, "Consolas");
        assert_eq!(hints.family.as_deref(), Some("modern"));
        assert_eq!(hints.pitch.as_deref(), Some("fixed"));
        assert_eq!(hints.alt_name.as_deref(), Some("Courier New"));
    }

    #[test]
    fn faces_of_one_family_make_one_entry_and_the_regular_face_speaks() {
        let table = table_of(&[
            face("XYZABC+Onest-Bold", Some(2 | 32)),
            face("XYZABD+Onest-Regular", Some(32)),
        ])
        .unwrap();
        assert_eq!(table.fonts.len(), 1);
        assert_eq!(hints(&table, "Onest").family.as_deref(), Some("swiss"));
    }

    #[test]
    fn a_face_without_flags_gets_no_guess_unless_it_is_standard() {
        let table = table_of(&[face("Mystery", None), face("Times-Roman", None)]).unwrap();
        assert_eq!(hints(&table, "Mystery"), &FontHints::default());
        assert_eq!(hints(&table, "Times").family.as_deref(), Some("roman"));
    }

    #[test]
    fn a_pi_font_is_decorative_whatever_its_flags_say() {
        let table = table_of(&[face("Wingdings-Regular", Some(4))]).unwrap();
        assert_eq!(
            hints(&table, "Wingdings").family.as_deref(),
            Some("decorative")
        );
    }

    #[test]
    fn panose_is_written_as_twenty_hex_digits() {
        let mut stated = face("ArialMT", Some(32));
        stated.panose = Some([2, 11, 6, 4, 2, 2, 2, 2, 2, 4]);
        let table = table_of(&[stated]).unwrap();
        assert_eq!(
            hints(&table, "Arial").panose1.as_deref(),
            Some("020B0604020202020204")
        );
    }

    /// The family the font names wins over `/BaseFont` split at its capitals,
    /// and the runs are told.
    #[test]
    fn a_family_the_font_names_is_the_entry_and_the_runs_rename() {
        // `/BaseFont` cut at its first hyphen names `Liberation`; the program
        // names `Liberation Serif`.
        let mut liberation = face("PFRHQO+Liberation-Serif-Bold", Some(2 | 32));
        liberation.family = Some("Liberation Serif".to_owned());
        let fonts = table(&[liberation], &BTreeMap::new());
        let table = fonts.table.expect("a table");
        assert_eq!(table.fonts[0].name.as_ref(), "Liberation Serif");
        assert_eq!(
            fonts.renames.get("Liberation").map(String::as_str),
            Some("Liberation Serif")
        );
    }

    /// A sans face a twelfth wider than Arial stands in as Verdana; one as
    /// wide as Arial as Arial; a wide serif face as Georgia.
    #[test]
    fn a_wide_face_gets_a_wide_stand_in() {
        let wide = Widths {
            arial: Some(1.15),
            times: Some(1.3),
            fixed: Some(false),
        };
        let plain = Widths {
            arial: Some(1.0),
            times: Some(1.1),
            fixed: Some(false),
        };
        let mut widths = BTreeMap::new();
        widths.insert("Montserrat".to_owned(), wide);
        widths.insert("Roboto".to_owned(), plain);
        widths.insert(
            "Lora".to_owned(),
            Widths {
                arial: Some(1.0),
                times: Some(1.12),
                fixed: Some(false),
            },
        );
        let fonts = table(
            &[
                face("AAAAAB+Montserrat-Regular", Some(32)),
                face("Roboto-Regular", Some(32)),
                face("Lora-Regular", Some(2 | 32)),
            ],
            &widths,
        );
        let table = fonts.table.expect("a table");
        assert_eq!(
            hints(&table, "Montserrat").alt_name.as_deref(),
            Some("Verdana")
        );
        assert_eq!(hints(&table, "Roboto").alt_name.as_deref(), Some("Arial"));
        assert_eq!(hints(&table, "Lora").alt_name.as_deref(), Some("Georgia"));
    }

    /// A descriptor that claims fixed pitch for a face whose letters differ
    /// in width is overruled; a real monospace keeps Courier New.
    #[test]
    fn measured_letters_overrule_a_false_fixed_pitch() {
        let mut widths = BTreeMap::new();
        widths.insert(
            "Trebuchet MS".to_owned(),
            Widths {
                arial: Some(0.93),
                times: Some(1.01),
                fixed: Some(false),
            },
        );
        widths.insert(
            "Consolas".to_owned(),
            Widths {
                arial: Some(0.98),
                times: Some(1.07),
                fixed: Some(true),
            },
        );
        let fonts = table(
            &[
                face("EAAAEF+TrebuchetMS", Some(33)),
                face("Consolas", Some(33)),
            ],
            &widths,
        );
        let table = fonts.table.expect("a table");
        assert_eq!(
            hints(&table, "Trebuchet MS").alt_name.as_deref(),
            Some("Arial")
        );
        assert_eq!(
            hints(&table, "Consolas").alt_name.as_deref(),
            Some("Courier New")
        );
    }

    /// Faces of one `/BaseFont` family that name themselves apart become
    /// one entry under the shortest name.
    #[test]
    fn a_weight_in_a_faces_own_name_does_not_split_the_family() {
        let mut medium = face("ZZZZZZ+GoogleSansText-Medium", Some(32));
        medium.family = Some("Google Sans Text Medium".to_owned());
        let mut regular = face("ZZZZZY+GoogleSansText-Regular", Some(32));
        regular.family = Some("Google Sans Text".to_owned());
        let fonts = table(&[medium, regular], &BTreeMap::new());
        let table = fonts.table.expect("a table");
        assert_eq!(table.fonts.len(), 1);
        assert_eq!(table.fonts[0].name.as_ref(), "Google Sans Text");
    }

    /// Unmeasured, the name settles a pitch the flags got wrong.
    #[test]
    fn unmeasured_the_name_settles_the_pitch() {
        let fonts = table(
            &[
                face("EAAAEI+DejaVuSans-Bold", Some(33)),
                face("RobotoMono-Regular", Some(2 | 32)),
            ],
            &BTreeMap::new(),
        );
        let table = fonts.table.expect("a table");
        assert_eq!(
            hints(&table, "DejaVu Sans").alt_name.as_deref(),
            Some("Arial")
        );
        assert_eq!(
            hints(&table, "Roboto Mono").alt_name.as_deref(),
            Some("Courier New")
        );
    }

    /// Advances a font did not state are estimates, not a measurement.
    #[test]
    fn estimated_widths_are_not_measured() {
        let mut times = face("Times-Roman", None);
        times.widths_stated = false;
        let mut widths = BTreeMap::new();
        widths.insert(
            "Times".to_owned(),
            Widths {
                arial: Some(0.9),
                times: Some(1.0),
                fixed: Some(true),
            },
        );
        let fonts = table(&[times], &widths);
        let table = fonts.table.expect("a table");
        assert_eq!(
            hints(&table, "Times").alt_name.as_deref(),
            Some("Times New Roman")
        );
        assert!(fonts.stand_ins.is_empty());
    }

    #[test]
    fn letters_are_measured_against_helvetica_and_times() {
        assert_eq!(reference('a'), Some((0.556, 0.444)));
        assert_eq!(reference('W'), Some((0.944, 0.944)));
        assert!(reference('ж').is_some());
        assert_eq!(reference('.'), None);
    }

    #[test]
    fn no_fonts_is_no_table() {
        assert!(table_of(&[]).is_none());
    }
}
