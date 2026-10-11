//! Footnote/endnote numbering and number formatting (`STAGE-5` S5.4/S5.5).
//!
//! Numbers are assigned in document order (continuous restart). The format is
//! taken from `w:footnotePr`/`w:endnotePr` (section overrides settings).

use std::collections::HashMap;

use strict_ooxml_wml::model::notes::NoteProperties;
use strict_ooxml_wml::model::{Block, Document, Inline, RunContent, Table};

/// A supported note number format (`ST_NumberFormat` subset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberFormat {
    /// `decimal` (`1, 2, 3`).
    #[default]
    Decimal,
    /// `decimalZero` (`01, 02`).
    DecimalZero,
    /// `lowerRoman` (`i, ii`).
    LowerRoman,
    /// `upperRoman` (`I, II`).
    UpperRoman,
    /// `lowerLetter` (`a, b`).
    LowerLetter,
    /// `upperLetter` (`A, B`).
    UpperLetter,
    /// `bullet`.
    Bullet,
    /// `none`.
    None,
}

impl NumberFormat {
    /// Parses a Strict `ST_NumberFormat` value, falling back to `default`.
    #[must_use]
    pub(crate) fn from_strict(value: Option<&str>, default: Self) -> Self {
        match value {
            None => default,
            Some("decimalZero") => Self::DecimalZero,
            Some("lowerRoman") => Self::LowerRoman,
            Some("upperRoman") => Self::UpperRoman,
            Some("lowerLetter") => Self::LowerLetter,
            Some("upperLetter") => Self::UpperLetter,
            Some("bullet") => Self::Bullet,
            Some("none") => Self::None,
            // Unsupported formats fall back to decimal (recorded at parse time).
            Some(_) => Self::Decimal,
        }
    }

    /// Formats `number` in this format.
    #[must_use]
    pub fn format(self, number: u32) -> String {
        match self {
            Self::Decimal => number.to_string(),
            Self::DecimalZero => format!("{number:02}"),
            Self::LowerRoman => roman(number).to_ascii_lowercase(),
            Self::UpperRoman => roman(number),
            Self::LowerLetter => letters(number).to_ascii_lowercase(),
            Self::UpperLetter => letters(number),
            Self::Bullet => "\u{2022}".to_owned(),
            Self::None => String::new(),
        }
    }
}

/// Formats `number` as a Roman numeral (decimal fallback outside `1..=3999`).
fn roman(number: u32) -> String {
    const TABLE: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    if number == 0 || number > 3999 {
        return number.to_string();
    }
    let mut remaining = number;
    let mut out = String::new();
    for (value, glyph) in TABLE {
        while remaining >= *value {
            out.push_str(glyph);
            remaining -= value;
        }
    }
    out
}

/// Formats `number` as a bijective base-26 letter sequence (`a`, `z`, `aa`).
fn letters(number: u32) -> String {
    if number == 0 {
        return number.to_string();
    }
    let mut remaining = number;
    let mut out = Vec::new();
    while remaining > 0 {
        // `(x % 26)` is 0..=25 by construction, so the `u8` is the value's
        // own bound rather than a narrowing of a wider one (AUD-09).
        let rem = u8::try_from((remaining - 1) % 26).unwrap_or(0);
        out.push(b'A' + rem);
        remaining = (remaining - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Numbering of the notes referenced by a document, in document order.
#[derive(Clone, Debug, Default)]
pub(crate) struct NoteNumbering {
    footnotes: HashMap<u32, (u32, NumberFormat)>,
    endnotes: HashMap<u32, (u32, NumberFormat)>,
    /// Reference order of the footnotes (numbers alone do not give it once a
    /// section restarts them).
    footnote_order: Vec<u32>,
    /// Reference order of the endnotes.
    endnote_order: Vec<u32>,
}

impl NoteNumbering {
    /// Builds the numbering for `document`.
    ///
    /// Each note takes the `w:footnotePr`/`w:endnotePr` of the section its
    /// reference is in, over the document settings: the number format, the
    /// start value, and whether numbering restarts with the section
    /// (`w:numRestart="eachSect"`).
    #[must_use]
    pub(crate) fn build(document: &Document) -> Self {
        let mut refs = NoteRefs::default();
        for block in &document.body.blocks {
            collect_blocks(std::slice::from_ref(block), &mut refs);
            if ends_section(block) {
                refs.section += 1;
            }
        }
        let props = |section: usize, endnote: bool| {
            let mut props = if endnote {
                document.settings.endnote_properties.clone()
            } else {
                document.settings.footnote_properties.clone()
            };
            let section = document
                .sections
                .get(section)
                .or_else(|| document.sections.last())
                .map(|section| &section.properties);
            if let Some(section) = section {
                props.merge_from(if endnote {
                    &section.endnote_properties
                } else {
                    &section.footnote_properties
                });
            }
            props
        };
        let (footnotes, footnote_order) = number(
            &refs.footnotes,
            |section| props(section, false),
            NumberFormat::Decimal,
        );
        let (endnotes, endnote_order) = number(
            &refs.endnotes,
            |section| props(section, true),
            NumberFormat::LowerRoman,
        );
        Self {
            footnotes,
            endnotes,
            footnote_order,
            endnote_order,
        }
    }

    /// Returns the number and format for a footnote id.
    #[must_use]
    pub(crate) fn footnote_number(&self, id: u32) -> Option<(u32, NumberFormat)> {
        self.footnotes.get(&id).copied()
    }

    /// Returns the number and format for an endnote id.
    #[must_use]
    pub(crate) fn endnote_number(&self, id: u32) -> Option<(u32, NumberFormat)> {
        self.endnotes.get(&id).copied()
    }

    /// Returns all footnote ids in numbering order.
    #[must_use]
    pub(crate) fn footnotes_in_order(&self) -> Vec<u32> {
        self.footnote_order.clone()
    }

    /// Returns all endnote ids in numbering order.
    #[must_use]
    pub(crate) fn endnotes_in_order(&self) -> Vec<u32> {
        self.endnote_order.clone()
    }
}

/// Assigns numbers to `ids` (first occurrence wins), restarting at a section
/// whose properties say `eachSect`. Returns the numbers and the order of the
/// first references.
fn number(
    ids: &[(u32, usize)],
    props: impl Fn(usize) -> NoteProperties,
    default_format: NumberFormat,
) -> (HashMap<u32, (u32, NumberFormat)>, Vec<u32>) {
    let mut map = HashMap::new();
    let mut order = Vec::new();
    let mut next: u32 = 1;
    let mut current_section: Option<usize> = None;
    for &(id, section) in ids {
        if map.contains_key(&id) {
            continue;
        }
        let props = props(section);
        let restart = match current_section {
            None => true,
            Some(previous) => {
                previous != section && props.num_restart.as_deref() == Some("eachSect")
            }
        };
        current_section = Some(section);
        let value = if restart {
            props.num_start.unwrap_or(1)
        } else {
            next
        };
        let format = NumberFormat::from_strict(props.num_format.as_deref(), default_format);
        map.insert(id, (value, format));
        order.push(id);
        next = value.saturating_add(1);
    }
    (map, order)
}

/// Whether a top-level body block ends a section (it carries `w:sectPr`).
fn ends_section(block: &Block) -> bool {
    match block {
        Block::Paragraph(para) => para.props.section.is_some(),
        Block::SdtBlock(sdt) => sdt.blocks.iter().any(ends_section),
        _ => false,
    }
}

/// Footnote/endnote reference ids collected in document order, with the index
/// of the section each reference is in.
#[derive(Default)]
struct NoteRefs {
    footnotes: Vec<(u32, usize)>,
    endnotes: Vec<(u32, usize)>,
    section: usize,
}

/// Collects note references from a block sequence.
fn collect_blocks(blocks: &[Block], refs: &mut NoteRefs) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => collect_inlines(&paragraph.inlines, refs),
            Block::Table(table) => collect_table(table, refs),
            Block::SdtBlock(sdt) => collect_blocks(&sdt.blocks, refs),
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
}

/// Collects note references from a table.
fn collect_table(table: &Table, refs: &mut NoteRefs) {
    for row in &table.rows {
        for cell in &row.cells {
            collect_blocks(&cell.blocks, refs);
        }
    }
}

/// Collects note references from inline content.
fn collect_inlines(inlines: &[Inline], refs: &mut NoteRefs) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                for content in &run.content {
                    match content {
                        RunContent::FootnoteRef(id) => {
                            refs.footnotes.push((*id, refs.section));
                        }
                        RunContent::EndnoteRef(id) => refs.endnotes.push((*id, refs.section)),
                        _ => {}
                    }
                }
            }
            Inline::Hyperlink(link) => collect_inlines(&link.inlines, refs),
            Inline::Field(field) => collect_inlines(&field.inlines, refs),
            Inline::SdtInline(sdt) => collect_inlines(&sdt.inlines, refs),
            Inline::Directional(dir) => collect_inlines(&dir.inlines, refs),
            Inline::FootnoteRef(id) => refs.footnotes.push((*id, refs.section)),
            Inline::EndnoteRef(id) => refs.endnotes.push((*id, refs.section)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{letters, roman, NumberFormat};

    #[test]
    fn formats_cover_all_variants() {
        assert_eq!(NumberFormat::Decimal.format(7), "7");
        assert_eq!(NumberFormat::DecimalZero.format(7), "07");
        assert_eq!(NumberFormat::LowerRoman.format(4), "iv");
        assert_eq!(NumberFormat::UpperRoman.format(1994), "MCMXCIV");
        assert_eq!(NumberFormat::LowerLetter.format(1), "a");
        assert_eq!(NumberFormat::UpperLetter.format(27), "AA");
        assert_eq!(NumberFormat::Bullet.format(1), "\u{2022}");
        assert_eq!(NumberFormat::None.format(1), "");
    }

    #[test]
    fn roman_and_letters_fall_back_outside_range() {
        assert_eq!(roman(0), "0");
        assert_eq!(roman(4000), "4000");
        assert_eq!(letters(0), "0");
    }

    #[test]
    fn parses_number_formats_with_defaults() {
        assert_eq!(
            NumberFormat::from_strict(None, NumberFormat::LowerRoman),
            NumberFormat::LowerRoman
        );
        assert_eq!(
            NumberFormat::from_strict(Some("decimalZero"), NumberFormat::Decimal),
            NumberFormat::DecimalZero
        );
        assert_eq!(
            NumberFormat::from_strict(Some("upperLetter"), NumberFormat::Decimal),
            NumberFormat::UpperLetter
        );
        assert_eq!(
            NumberFormat::from_strict(Some("unknownFormat"), NumberFormat::Decimal),
            NumberFormat::Decimal
        );
    }
}
