//! The font table model (`word/fontTable.xml`) and the embedded fonts it names.
//!
//! **This is the model behind `L-FONTS`, and the reason it exists is a silent
//! loss rather than a missing feature.** Before it, `word/fontTable.xml` was not
//! parsed at all: the writer derived it from the faces the styles and runs name
//! and wrote `w:font` elements with nothing else on them, so every `w:embed*`
//! element of the source and every `word/fonts/*.ttf` part behind it went
//! missing — sixteen binaries in two corpus documents, and *nothing in the loss
//! report said so* until the census gate added a part ledger and found it
//! (`W7-DROPPED`, `TZ-15`).
//!
//! An embedded font is not decoration. A document that ships a subset font with
//! it displays text in that face on a machine that does not have it installed,
//! and a document that does not ship it substitutes and re-flows. Carrying the
//! bytes is the difference between the page the producer designed and a page we
//! approximated.
//!
//! # What the model does and does not keep
//!
//! Kept, because each of them changes what the bytes mean:
//!
//! - the family `w:font/@w:name`, the key the whole entry hangs on;
//! - the **resolved part** behind each `w:embed*`, not the relationship id — the
//!   ids belong to the source's `word/_rels/fontTable.xml.rels` and mean nothing
//!   in a package we write;
//! - `w:fontKey`, which is not metadata: ECMA-376 §17.8.1 obfuscates the first 32
//!   bytes of a font whose `fontKey` is a real GUID, and a consumer de-obfuscates
//!   with the key from the file. Dropping it while keeping the bytes would
//!   produce a font that renders as garbage, which is worse than no font;
//! - `w:subsetted`, which tells a consumer whether the bytes are the whole face
//!   or a subset, and therefore whether to fall back for a missing glyph.
//!
//! Not kept, and named: `w:panose1`, `w:charset`, `w:family`, `w:pitch`,
//! `w:sig` and `w:notTrueType`. They are hints about a face, none of them is
//! required by `CT_Font`, and this project does not lay out by them. An entry
//! that carries only a name and its embeds is conformant; the earlier writer
//! wrote the `w:family` and `w:pitch` children as EMPTY elements, which made
//! every package with a font table carry two violations per face (`XS-01`), and
//! nothing was expressed by those empties.

use std::sync::Arc;

use strict_ooxml_core::part::PartId;

/// Which face of a family an embedded binary is.
///
/// The four `w:embed*` children of `CT_Font` are this enum, and keeping them as
/// four named cases rather than a string is what stops a writer from inventing a
/// fifth spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmbedKind {
    /// `w:embedRegular`.
    Regular,
    /// `w:embedBold`.
    Bold,
    /// `w:embedItalic`.
    Italic,
    /// `w:embedBoldItalic`.
    BoldItalic,
}

impl EmbedKind {
    /// The element name Strict gives this case.
    #[must_use]
    pub fn element(self) -> &'static str {
        match self {
            Self::Regular => "w:embedRegular",
            Self::Bold => "w:embedBold",
            Self::Italic => "w:embedItalic",
            Self::BoldItalic => "w:embedBoldItalic",
        }
    }

    /// Every case, in `CT_Font`'s `xsd:sequence` order.
    ///
    /// The order is the schema's and not this enum's declaration order by
    /// accident: `CT_Font` declares `embedRegular`, `embedBold`, `embedItalic`,
    /// `embedBoldItalic` as a sequence, and a child that appears earlier than the
    /// schema says is "This element is not expected" (`XS-23`'s lesson, applied
    /// to a different container).
    #[must_use]
    pub fn all() -> [Self; 4] {
        [Self::Regular, Self::Bold, Self::Italic, Self::BoldItalic]
    }
}

/// One embedded font binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedFont {
    /// The part holding the bytes.
    ///
    /// **The part, not a relationship id**, and this is the whole difficulty of
    /// the item: the id in the source names a relationship of
    /// `word/fontTable.xml`, and this write allocates its own ids for that part's
    /// own `.rels` — the same trap a header's picture fell into.
    pub part: PartId,
    /// `w:fontKey`: the GUID the first 32 bytes are obfuscated with, if any.
    ///
    /// `{00000000-0000-0000-0000-000000000000}` means the bytes are **not**
    /// obfuscated, which is what most producers emit and what `DOCX_13_Pages`
    /// emits for all ten of its faces. It is still carried, because the value is
    /// what says which case this is, and a consumer compares it rather than
    /// guessing.
    pub font_key: Option<Arc<str>>,
    /// `w:subsetted`: whether the bytes are a subset of the face.
    pub subsetted: bool,
}

/// One `w:font` entry: a family and the faces embedded for it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FontEntry {
    /// `w:font/@w:name` — the family name, and the only required attribute.
    pub name: Arc<str>,
    /// The embedded faces, keyed by [`EmbedKind`] and in that enum's order.
    ///
    /// A `BTreeMap` rather than four `Option`s so that "no `w:embedBold`" and
    /// "a `w:embedBold` we could not resolve" cannot be confused: the second is
    /// absent from the map and recorded in the loss report, and the first was
    /// never in the source either.
    pub embeds: std::collections::BTreeMap<EmbedKind, EmbeddedFont>,
}

/// The whole `word/fontTable.xml` part.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FontTable {
    /// The `w:font` entries in document order.
    ///
    /// Order is the source's, because `CT_FontsList` is a sequence and a reordered
    /// table is a different table; and because it makes a round trip a fixed
    /// point, which SC-1 requires of the writer.
    pub fonts: Vec<FontEntry>,
}

impl FontTable {
    /// Returns `true` when no face is embedded, which is the common case and the
    /// one where nothing about the part changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// Every embedded binary in the table, in entry order then face order.
    #[must_use]
    pub fn embedded_parts(&self) -> Vec<EmbeddedFont> {
        self.fonts
            .iter()
            .flat_map(|entry| {
                EmbedKind::all()
                    .into_iter()
                    .filter_map(|kind| entry.embeds.get(&kind).cloned())
            })
            .collect()
    }
}
