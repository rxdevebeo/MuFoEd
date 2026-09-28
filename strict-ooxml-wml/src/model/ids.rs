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

/// Sixteen-digit paragraph identifier (`w:p/@w14:paraId`).
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

/// Eight-digit text identifier (`w:p/@w14:textId`).
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Ilvl(pub u8);

impl fmt::Display for Ilvl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
