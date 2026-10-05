//! Bounded definitions for RTF list tables; body references use WML numbering.
use strict_ooxml_wml::model::{Justification, Level};

pub(super) struct ListDraft {
    pub depth: usize,
    pub id: Option<u32>,
    pub levels: Vec<Level>,
}
pub(super) struct LevelDraft {
    pub depth: usize,
    pub start: u32,
    pub format: &'static str,
    pub text: String,
    pub justification: Justification,
    pub suffix: &'static str,
}
pub(super) struct OverrideDraft {
    pub depth: usize,
    pub list: Option<u32>,
    pub instance: Option<u32>,
}
pub(super) fn number_format(n: i32) -> Option<&'static str> {
    Some(match n {
        0 => "decimal",
        1 => "upperRoman",
        2 => "lowerRoman",
        3 => "upperLetter",
        4 => "lowerLetter",
        5 => "ordinal",
        22 => "decimalZero",
        23 => "bullet",
        255 => "none",
        _ => return None,
    })
}
