//! Field evaluation support (`STAGE-5` S5.6).
//!
//! `fldSimple`/`fldChar`/`instrText` are modelled flat in the DOM; this module
//! interprets a field instruction and identifies the fields the renderer can
//! compute (PAGE/NUMPAGES/SECTIONPAGES). Other fields are rendered from their
//! cached result.

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
}

/// A computed field placed in a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMarker {
    /// Which field.
    pub kind: FieldKind,
    /// Number format from the field's `*` switch.
    pub format: crate::notes::NumberFormat,
}

/// Parses a field instruction, returning the computed kind (if any) and format.
///
/// Only `PAGE`, `NUMPAGES` and `SECTIONPAGES` are computed; every other
/// instruction returns `None` (the caller uses the cached result).
pub(crate) fn parse_instruction(instruction: &str) -> (Option<FieldKind>, NumberFormat) {
    let mut tokens = instruction.split_whitespace();
    let name = tokens.next().unwrap_or("").to_ascii_uppercase();
    let kind = match name.as_str() {
        "PAGE" => Some(FieldKind::Page),
        "NUMPAGES" => Some(FieldKind::NumPages),
        "SECTIONPAGES" | "SECTIONPAGE" => Some(FieldKind::SectionPages),
        _ => None,
    };
    (kind, parse_format_switch(instruction))
}

/// Parses the `\*` number-format switch of an instruction.
fn parse_format_switch(instruction: &str) -> NumberFormat {
    let mut tokens = instruction.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "\\*" {
            return match tokens.next() {
                Some("roman") => NumberFormat::LowerRoman,
                Some("ROMAN") => NumberFormat::UpperRoman,
                Some("alphabetic") => NumberFormat::LowerLetter,
                Some("ALPHABETIC") => NumberFormat::UpperLetter,
                _ => NumberFormat::Decimal,
            };
        }
    }
    NumberFormat::Decimal
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
        assert_eq!(parse_instruction("AUTHOR").0, None);
        assert_eq!(parse_instruction("").0, None);
    }

    #[test]
    fn parses_format_switch() {
        assert_eq!(
            parse_instruction(r"PAGE \* roman").1,
            NumberFormat::LowerRoman
        );
        assert_eq!(
            parse_instruction(r"PAGE \* ROMAN").1,
            NumberFormat::UpperRoman
        );
        assert_eq!(
            parse_instruction(r"PAGE \* ALPHABETIC").1,
            NumberFormat::UpperLetter
        );
        assert_eq!(
            parse_instruction(r"PAGE \* MERGEFORMAT").1,
            NumberFormat::Decimal
        );
        assert_eq!(parse_instruction("PAGE").1, NumberFormat::Decimal);
    }
}
