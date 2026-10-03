//! Field evaluation support (`STAGE-5` S5.6).
//!
//! `fldSimple`/`fldChar`/`instrText` are modelled flat in the DOM; this module
//! interprets a field instruction and identifies the fields the renderer can
//! compute (PAGE/NUMPAGES/SECTIONPAGES/SECTION). Other fields are rendered from
//! their cached result.

use strict_ooxml_wml::model::{Block, Inline, RunContent, Table};

use crate::notes::NumberFormat;

/// A computed field the renderer places itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// `PAGE` — the current page number.
    Page,
    /// `NUMPAGES` — the total number of pages.
    NumPages,
    /// `SECTIONPAGES` — the number of pages in the current section.
    SectionPages,
    /// `SECTION` — the current section index (1-based).
    Section,
}

/// A computed field placed in a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMarker {
    /// Which field.
    pub kind: FieldKind,
    /// Number format from the field's `\*` switch, when present.
    ///
    /// `None` means the section's `w:pgNumType/@w:fmt` (or decimal) applies.
    pub format: Option<NumberFormat>,
}

/// Page-dependent values used when laying out headers/footers (AUD-70).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldEnv {
    /// Display page number (`pgNumType/@start` already applied).
    pub page_number: usize,
    /// Document page count (`NUMPAGES`).
    pub page_count: usize,
    /// 1-based section index (`SECTION`).
    pub section_index: usize,
    /// Pages in the current section (`SECTIONPAGES`).
    pub section_pages: usize,
    /// Default number format from `w:pgNumType/@w:fmt`.
    pub page_format: NumberFormat,
}

impl FieldEnv {
    /// Resolves a marker to `(value, format)`.
    pub(crate) fn resolve(self, marker: FieldMarker) -> (u32, NumberFormat) {
        let format = marker.format.unwrap_or(self.page_format);
        let value = match marker.kind {
            FieldKind::Page => self.page_number,
            FieldKind::NumPages => self.page_count,
            FieldKind::SectionPages => self.section_pages,
            FieldKind::Section => self.section_index,
        };
        (usize_to_u32(value), format)
    }
}

/// Parses a field instruction, returning the computed kind (if any) and format.
///
/// Only `PAGE`, `NUMPAGES`, `SECTIONPAGES` and `SECTION` are computed; every
/// other instruction returns `None` (the caller uses the cached result).
pub(crate) fn parse_instruction(instruction: &str) -> (Option<FieldKind>, Option<NumberFormat>) {
    let mut tokens = instruction.split_whitespace();
    let name = tokens.next().unwrap_or("").to_ascii_uppercase();
    let kind = match name.as_str() {
        "PAGE" => Some(FieldKind::Page),
        "NUMPAGES" => Some(FieldKind::NumPages),
        "SECTIONPAGES" | "SECTIONPAGE" => Some(FieldKind::SectionPages),
        "SECTION" => Some(FieldKind::Section),
        _ => None,
    };
    (kind, parse_format_switch(instruction))
}

/// Parses the `\*` number-format switch of an instruction.
fn parse_format_switch(instruction: &str) -> Option<NumberFormat> {
    let mut tokens = instruction.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "\\*" {
            return match tokens.next() {
                Some("roman") => Some(NumberFormat::LowerRoman),
                Some("ROMAN") => Some(NumberFormat::UpperRoman),
                Some("alphabetic") => Some(NumberFormat::LowerLetter),
                Some("ALPHABETIC") => Some(NumberFormat::UpperLetter),
                // `MERGEFORMAT` and unknown switches do not override `pgNumType`.
                _ => None,
            };
        }
    }
    None
}

/// Whether `blocks` contain a PAGE/NUMPAGES/SECTIONPAGES/SECTION field (AUD-70).
pub(crate) fn blocks_have_dynamic_fields(blocks: &[Block]) -> bool {
    blocks.iter().any(block_has_dynamic_fields)
}

fn block_has_dynamic_fields(block: &Block) -> bool {
    match block {
        Block::Paragraph(para) => inlines_have_dynamic_fields(&para.inlines),
        Block::Table(table) => table_has_dynamic_fields(table),
        Block::SdtBlock(sdt) => blocks_have_dynamic_fields(&sdt.blocks),
        Block::AltChunk(_) | Block::Opaque(_) => false,
    }
}

fn table_has_dynamic_fields(table: &Table) -> bool {
    table.rows.iter().any(|row| {
        row.cells
            .iter()
            .any(|cell| blocks_have_dynamic_fields(&cell.blocks))
    })
}

fn inlines_have_dynamic_fields(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Field(simple) => {
            let instruction = simple.instruction.as_deref().unwrap_or("");
            parse_instruction(instruction).0.is_some()
                || inlines_have_dynamic_fields(&simple.inlines)
        }
        Inline::Run(run) => run.content.iter().any(|content| match content {
            RunContent::InstrText(text) => parse_instruction(text).0.is_some(),
            _ => false,
        }),
        Inline::Hyperlink(link) => inlines_have_dynamic_fields(&link.inlines),
        Inline::SdtInline(sdt) => {
            inlines_have_dynamic_fields(&sdt.inlines) || blocks_have_dynamic_fields(&sdt.blocks)
        }
        Inline::Directional(dir) => inlines_have_dynamic_fields(&dir.inlines),
        _ => false,
    })
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{parse_instruction, FieldKind};
    use crate::notes::NumberFormat;

    #[test]
    fn identifies_computed_fields() {
        assert_eq!(parse_instruction("PAGE").0, Some(FieldKind::Page));
        assert_eq!(parse_instruction(" NUMPAGES ").0, Some(FieldKind::NumPages));
        assert_eq!(
            parse_instruction("SECTIONPAGES").0,
            Some(FieldKind::SectionPages)
        );
        assert_eq!(parse_instruction("SECTION").0, Some(FieldKind::Section));
        assert_eq!(parse_instruction("AUTHOR").0, None);
        assert_eq!(parse_instruction("").0, None);
    }

    #[test]
    fn parses_format_switch() {
        assert_eq!(
            parse_instruction(r"PAGE \* roman").1,
            Some(NumberFormat::LowerRoman)
        );
        assert_eq!(
            parse_instruction(r"PAGE \* ROMAN").1,
            Some(NumberFormat::UpperRoman)
        );
        assert_eq!(
            parse_instruction(r"PAGE \* ALPHABETIC").1,
            Some(NumberFormat::UpperLetter)
        );
        assert_eq!(parse_instruction(r"PAGE \* MERGEFORMAT").1, None);
        assert_eq!(parse_instruction("PAGE").1, None);
    }
}
