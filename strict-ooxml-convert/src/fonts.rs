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
    /// The face every renderer has for this class, when there is one.
    fn stand_in(self) -> Option<&'static str> {
        match self.family {
            "modern" => Some("Courier New"),
            "roman" => Some("Times New Roman"),
            "swiss" => Some("Arial"),
            _ => None,
        }
    }

    fn of(face: &FontFace, family: &str) -> Option<Self> {
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

/// One `w:font` per family the PDF's fonts name, with the hints its faces state.
///
/// A family's faces (regular, bold, italic) usually agree; when they do not,
/// the regular face's statement wins, then the first face that states one.
/// `None` when no font was read.
pub(crate) fn table(faces: &[FontFace]) -> Option<FontTable> {
    let mut families: BTreeMap<String, Vec<&FontFace>> = BTreeMap::new();
    for face in faces {
        let name = FontName::of(&face.base_font);
        if name.family.trim().is_empty() {
            continue;
        }
        let regular = !name.bold && !name.italic;
        let entry = families.entry(name.family).or_default();
        if regular {
            entry.insert(0, face);
        } else {
            entry.push(face);
        }
    }
    if families.is_empty() {
        return None;
    }
    let fonts = families
        .into_iter()
        .map(|(family, faces)| {
            let look = faces.iter().find_map(|face| Look::of(face, &family));
            let panose = faces.iter().find_map(|face| face.panose);
            let alt_name = look
                .and_then(Look::stand_in)
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
    Some(FontTable { fonts })
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
        }
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
        let table = table(&[face("AAAAAB+Montserrat-Regular", Some(32))]).unwrap();
        let hints = hints(&table, "Montserrat");
        assert_eq!(hints.family.as_deref(), Some("swiss"));
        assert_eq!(hints.pitch.as_deref(), Some("variable"));
        assert_eq!(hints.alt_name.as_deref(), Some("Arial"));
    }

    #[test]
    fn a_face_is_not_its_own_stand_in_and_a_pi_font_has_none() {
        let table = table(&[
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
        let table = table(&[face("Consolas", Some(1 | 2 | 32))]).unwrap();
        let hints = hints(&table, "Consolas");
        assert_eq!(hints.family.as_deref(), Some("modern"));
        assert_eq!(hints.pitch.as_deref(), Some("fixed"));
        assert_eq!(hints.alt_name.as_deref(), Some("Courier New"));
    }

    #[test]
    fn faces_of_one_family_make_one_entry_and_the_regular_face_speaks() {
        let table = table(&[
            face("XYZABC+Onest-Bold", Some(2 | 32)),
            face("XYZABD+Onest-Regular", Some(32)),
        ])
        .unwrap();
        assert_eq!(table.fonts.len(), 1);
        assert_eq!(hints(&table, "Onest").family.as_deref(), Some("swiss"));
    }

    #[test]
    fn a_face_without_flags_gets_no_guess_unless_it_is_standard() {
        let table = table(&[face("Mystery", None), face("Times-Roman", None)]).unwrap();
        assert_eq!(hints(&table, "Mystery"), &FontHints::default());
        assert_eq!(hints(&table, "Times").family.as_deref(), Some("roman"));
    }

    #[test]
    fn a_pi_font_is_decorative_whatever_its_flags_say() {
        let table = table(&[face("Wingdings-Regular", Some(4))]).unwrap();
        assert_eq!(
            hints(&table, "Wingdings").family.as_deref(),
            Some("decorative")
        );
    }

    #[test]
    fn panose_is_written_as_twenty_hex_digits() {
        let mut stated = face("ArialMT", Some(32));
        stated.panose = Some([2, 11, 6, 4, 2, 2, 2, 2, 2, 4]);
        let table = table(&[stated]).unwrap();
        assert_eq!(
            hints(&table, "Arial").panose1.as_deref(),
            Some("020B0604020202020204")
        );
    }

    #[test]
    fn no_fonts_is_no_table() {
        assert!(table(&[]).is_none());
    }
}
