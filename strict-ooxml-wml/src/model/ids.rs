//! Small identifier newtypes used across the model.
//!
//! Identifiers are stored as `Arc<str>` (interned by the parser) so the DOM is
//! self-contained and cheap to clone (ADR-0004).

use std::fmt;
use std::sync::Arc;

/// Identifier of a style (`w:styleId`, `w:pStyle/@w:val`, `w:rStyle/@w:val`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StyleId(Arc<str>);

impl StyleId {
    /// Creates a style id from its text.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StyleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Paragraph identifier (`w:p/@w14:paraId`).
///
/// Eight hexadecimal digits: the attribute is typed `w:ST_LongHexNumber`
/// ([MS-DOCX] §2.6.2.3), which is `xsd:hexBinary` with `xsd:length="4"` — four
/// octets, eight digits (ISO/IEC 29500-1 §17.18.50, *ST_LongHexNumber (Eight
/// Digit Hexadecimal Value)*). This constructor does not check that; `validate`
/// is where the check lives.
///
/// [MS-DOCX] further requires the value to be greater than 0 and less than
/// `0x80000000`, and unique within the document part — but not across the
/// choices or fallback of an `mc:AlternateContent` block (ISO/IEC 29500-1
/// §17.17.3).
///
/// The attribute is **not part of ECMA-376 at all** — neither Part 1 (Strict)
/// nor Part 4 (Transitional). It is a Microsoft extension in
/// `http://schemas.microsoft.com/office/word/2010/wordml`, and Strict
/// conformance is defined on the *post-MCE* part, so a conforming MCE
/// processor strips it. See `STAGE-10-TASK.md` §2.3 and ADR-0014 before writing
/// one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParaId(Arc<str>);

impl ParaId {
    /// Creates a paragraph id from its text.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Text identifier (`w:p/@w14:textId`).
///
/// Eight hexadecimal digits, same `w:ST_LongHexNumber` typing as [`ParaId`]
/// ([MS-DOCX] §2.6.2.4, ISO/IEC 29500-1 §17.18.50) — this one is not validated
/// on the way in either.
///
/// Two rules from [MS-DOCX] that the type does not enforce: the value must be
/// greater than 0 and less than `0x80000000`, and **an element carrying
/// `textId` must also carry `paraId`**. The writer keeps that pairing; a
/// hand-built document can break it, which is what `validate` is for.
///
/// Same caveat as [`ParaId`]: a Microsoft extension, absent from both ECMA-376
/// parts, stripped by a conforming MCE processor (ADR-0014).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextId(Arc<str>);

impl TextId {
    /// Creates a text id from its text.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Numeric numbering instance id (`w:numId/@w:val`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NumId(pub u32);

/// Numeric abstract numbering definition id (`w:abstractNumId/@w:val`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AbstractNumId(pub u32);

/// Zero-based list level (`w:ilvl/@w:val`), 0..=8.
///
/// The range is in the schema, not here: `ST_DecimalNumber` is unbounded and
/// `CT_Lvl`/`CT_NumPr` do not constrain it, so `ilvl="9"` is a document the
/// schema accepts and a list level that does not exist. `MAX` is therefore part
/// of the type's contract, and [`Ilvl::is_valid`] is how a caller asks.
///
/// The reader clamps on the way in (`parse/numbering.rs`,
/// `parse/props.rs`), so a **parsed** document cannot hold an out-of-range
/// value. Only a hand-built one can, which is what `validate` is for
/// (`STAGE-10-TASK.md` I7). The writer refuses to write an invalid level rather
/// than clamping it silently, because the renderer *does* clamp
/// (`render-svg/numbering.rs`) and a silent clamp in the writer would leave the
/// two backends disagreeing about the same document without either saying so.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Ilvl(pub u8);

impl Ilvl {
    /// The highest level the schema's `0..=8` range allows.
    pub const MAX: u8 = 8;

    /// Returns whether this level is inside the schema's range.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.0 <= Self::MAX
    }
}

impl fmt::Display for Ilvl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
