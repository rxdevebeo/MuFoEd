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
    footnotes: HashMap<u32, u32>,
    endnotes: HashMap<u32, u32>,
    /// Number format for footnotes.
    pub footnote_format: NumberFormat,
    /// Number format for endnotes.
    pub endnote_format: NumberFormat,
}

impl NoteNumbering {
    /// Builds the numbering for `document` from its settings and last section.
    #[must_use]
    pub(crate) fn build(document: &Document) -> Self {
        let section = document.sections.last().map(|section| &section.properties);
        let mut footnote_props = document.settings.footnote_properties.clone();
        let mut endnote_props = document.settings.endnote_properties.clone();
        if let Some(section) = section {
            footnote_props.merge_from(&section.footnote_properties);
            endnote_props.merge_from(&section.endnote_properties);
        }

        let mut refs = NoteRefs::default();
        collect_blocks(&document.body.blocks, &mut refs);
        let footnotes = number(&refs.footnotes, &footnote_props);
        let endnotes = number(&refs.endnotes, &endnote_props);

        Self {
            footnotes,
            endnotes,
            footnote_format: NumberFormat::from_strict(
                footnote_props.num_format.as_deref(),
                NumberFormat::Decimal,
            ),
            endnote_format: NumberFormat::from_strict(
                endnote_props.num_format.as_deref(),
                NumberFormat::LowerRoman,
            ),
        }
    }

    /// Returns the number and format for a footnote id.
    #[must_use]
    pub(crate) fn footnote_number(&self, id: u32) -> Option<(u32, NumberFormat)> {
        self.footnotes
            .get(&id)
            .map(|&number| (number, self.footnote_format))
    }

    /// Returns the number and format for an endnote id.
    #[must_use]
    pub(crate) fn endnote_number(&self, id: u32) -> Option<(u32, NumberFormat)> {
        self.endnotes
            .get(&id)
            .map(|&number| (number, self.endnote_format))
    }

    /// Returns all footnote ids in numbering order.
    #[must_use]
    pub(crate) fn footnotes_in_order(&self) -> Vec<u32> {
        ordered(&self.footnotes)
    }

    /// Returns all endnote ids in numbering order.
    #[must_use]
    pub(crate) fn endnotes_in_order(&self) -> Vec<u32> {
        ordered(&self.endnotes)
    }
}

/// Returns the keys of `map` sorted by their assigned number.
fn ordered(map: &HashMap<u32, u32>) -> Vec<u32> {
    let mut entries: Vec<(u32, u32)> = map.iter().map(|(&id, &n)| (n, id)).collect();
    entries.sort_unstable();
    entries.into_iter().map(|(_, id)| id).collect()
}

/// Assigns sequential numbers to `ids` (first occurrence wins).
fn number(ids: &[u32], props: &NoteProperties) -> HashMap<u32, u32> {
    let start = props.num_start.unwrap_or(1);
    let mut map = HashMap::new();
    let mut next = start;
    for &id in ids {
        if map.contains_key(&id) {
            continue;
        }
        map.insert(id, next);
        next = next.saturating_add(1);
    }
    map
}

/// Footnote/endnote reference ids collected in document order.
#[derive(Default)]
struct NoteRefs {
    footnotes: Vec<u32>,
    endnotes: Vec<u32>,
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
                        RunContent::FootnoteRef(id) => refs.footnotes.push(*id),
                        RunContent::EndnoteRef(id) => refs.endnotes.push(*id),
                        _ => {}
                    }
                }
            }
            Inline::Hyperlink(link) => collect_inlines(&link.inlines, refs),
            Inline::Field(field) => collect_inlines(&field.inlines, refs),
            Inline::SdtInline(sdt) => collect_inlines(&sdt.inlines, refs),
            Inline::FootnoteRef(id) => refs.footnotes.push(*id),
            Inline::EndnoteRef(id) => refs.endnotes.push(*id),
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
