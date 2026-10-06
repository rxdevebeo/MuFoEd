//! Parsing of `word/fontTable.xml` and the embedded fonts it names.
//!
//! The part was not read at all before 2026-10-02, and everything in here
//! exists because of what that cost: the writer derived the part from the faces
//! the styles and runs name, wrote `w:font` elements with nothing else on them,
//! and every `w:embed*` element and every `word/fonts/*.ttf` behind it went
//! missing — sixteen binaries in two corpus documents, named by nothing
//! (`W7-DROPPED`).
//!
//! Two rules do the work, and both were learned from a bug rather than from the
//! schema:
//!
//! - **a relationship id means nothing outside its own part.** `w:embedRegular/
//!   @r:id` names a relationship of `word/fontTable.xml`, resolved through
//!   `word/_rels/fontTable.xml.rels`. The model therefore stores the resolved
//!   *part*, and the writer allocates its own ids for the rels it writes. A header
//!   picture fell into exactly this hole in the same session.
//! - **an unresolvable `r:id` is a loss, not an absence.** The family entry
//!   survives — the name is real and the document refers to it — but the face it
//!   named is gone, and a report showing a font table with four faces and no
//!   binaries would look identical to a document that never embedded any.

use std::sync::Arc;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::XmlEvent;

use crate::model::fonts::{EmbedKind, EmbeddedFont, FontEntry, FontHints, FontSig, FontTable};
use crate::model::support::SupportStatus;
use crate::RELS_STRICT_NS;

use super::{attr_in_ns, feature_id_for, is_wml, wml_attr, Attr, PartParser};

/// The part name a lost embedded face is given.
///
/// A name no writer will ever carry, so a face whose relationship did not resolve
/// is skipped rather than pointed at a part that does not exist. Comparing
/// against it is how the writer tells "embedded, and here is the part" from
/// "embedded, and the bytes were already gone when we read them".
pub const LOST_FONT_PART: &str = "/word/fonts/none";

impl PartParser<'_> {
    /// Parses a `word/fontTable.xml` part.
    pub(crate) fn parse_font_table_root(&mut self) -> Result<FontTable> {
        self.nested(|parser| {
            let mut table = FontTable::default();
            parser.expect_root("fonts")?;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        if name.local() == "font" {
                            let entry = parser.parse_font_entry(&attrs)?;
                            if entry.name.is_empty() {
                                // `CT_Font/@w:name` is `use="required"`, so an
                                // entry without one is a malformed element rather
                                // than an unusual family, and inventing a name for
                                // it would put a face in the written table that the
                                // document never had.
                                parser.record(
                                    "w:font",
                                    SupportStatus::Partial,
                                    Some(
                                        "w:font has no @w:name, which the schema requires"
                                            .to_owned(),
                                    ),
                                    Some(parser.location()),
                                );
                            } else {
                                table.fonts.push(entry);
                            }
                        } else {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    // Defence in depth (AUD-04): the reader already refuses to hand
                    // back `Eof` for a truncated part, and a loop that treated it as
                    // "nothing more" was the hang this arm exists to make impossible.
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of fontTable")),
                }
            }
            parser.expect_end_of_part()?;
            Ok(table)
        })
    }

    /// One `w:font` and the faces it embeds.
    ///
    /// A child loop rather than a lookahead, for the same reason every other
    /// parser here is a child loop: the parser cannot un-read an event, and
    /// `CT_Font`'s four `w:embed*` children are sparse, so a `w:font` that embeds
    /// only `w:embedBold` must not produce an empty `w:embedRegular`.
    fn parse_font_entry(&mut self, attrs: &[Attr]) -> Result<FontEntry> {
        let name =
            wml_attr(attrs, "name").map_or_else(|| Arc::from(""), |value| self.intern(value));
        let mut entry = FontEntry {
            name,
            ..FontEntry::default()
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    if let Some(kind) = EmbedKind::all()
                        .into_iter()
                        .find(|kind| kind.element().trim_start_matches("w:") == name.local())
                    {
                        let font = self.parse_embed(kind, &attrs);
                        entry.embeds.insert(kind, font);
                        // `CT_FontRel` is an empty type: it carries
                        // attributes and no child, so the end tag is the
                        // only thing left to consume.
                        self.skip_element()?;
                        continue;
                    }
                    if self.apply_font_hint(&mut entry.hints, name.local(), &attrs)? {
                        continue;
                    }
                    // A real `CT_Font` child we do not model yet: name it once
                    // per feature id so a large table does not explode the report.
                    let feature = feature_id_for(&name);
                    self.record(
                        &feature,
                        SupportStatus::Unsupported,
                        Some("font table child is not preserved by this reader".to_owned()),
                        Some(self.location()),
                    );
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => return Ok(entry),
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                // See `parse_font_table_root`: `Eof` here means the part ended
                // inside a `w:font`, which is not a font with no children.
                XmlEvent::Eof => return Err(self.invalid("unexpected end of w:font")),
            }
        }
    }

    /// Stores a known face hint. Returns `true` when the element was consumed.
    fn apply_font_hint(
        &mut self,
        hints: &mut FontHints,
        local: &str,
        attrs: &[Attr],
    ) -> Result<bool> {
        match local {
            "altName" => {
                hints.alt_name = wml_attr(attrs, "val").map(|value| self.intern(value));
                self.skip_element()?;
                Ok(true)
            }
            "panose1" => {
                hints.panose1 = wml_attr(attrs, "val").map(|value| self.intern(value));
                self.skip_element()?;
                Ok(true)
            }
            "charset" => {
                // Strict renames `@w:val` to `@w:characterSet` (`CT_Charset`).
                // Accept either so transitional and hand-built Strict packages
                // both round-trip without becoming Unsupported.
                hints.charset = wml_attr(attrs, "characterSet")
                    .or_else(|| wml_attr(attrs, "val"))
                    .map(|value| self.intern(value));
                self.skip_element()?;
                Ok(true)
            }
            "family" => {
                hints.family = wml_attr(attrs, "val").map(|value| self.intern(value));
                self.skip_element()?;
                Ok(true)
            }
            "pitch" => {
                hints.pitch = wml_attr(attrs, "val").map(|value| self.intern(value));
                self.skip_element()?;
                Ok(true)
            }
            "sig" => {
                hints.sig = Some(FontSig {
                    usb0: wml_attr(attrs, "usb0").map(|value| self.intern(value)),
                    usb1: wml_attr(attrs, "usb1").map(|value| self.intern(value)),
                    usb2: wml_attr(attrs, "usb2").map(|value| self.intern(value)),
                    usb3: wml_attr(attrs, "usb3").map(|value| self.intern(value)),
                    csb0: wml_attr(attrs, "csb0").map(|value| self.intern(value)),
                    csb1: wml_attr(attrs, "csb1").map(|value| self.intern(value)),
                });
                self.skip_element()?;
                Ok(true)
            }
            "notTrueType" => {
                hints.not_true_type =
                    wml_attr(attrs, "val").is_none_or(|value| matches!(value, "1" | "true" | "on"));
                self.skip_element()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// One `w:embed*` element.
    fn parse_embed(&mut self, kind: EmbedKind, attrs: &[Attr]) -> EmbeddedFont {
        let font_key = wml_attr(attrs, "fontKey").map(|value| self.intern(value));
        let subsetted =
            wml_attr(attrs, "subsetted").is_some_and(|value| matches!(value, "1" | "true" | "on"));
        let part = self.resolve_embedded_font(kind, attrs);
        EmbeddedFont {
            part,
            font_key,
            subsetted,
        }
    }

    /// The part a `w:embed*` relationship names, or [`LOST_FONT_PART`] and a
    /// record.
    fn resolve_embedded_font(&mut self, kind: EmbedKind, attrs: &[Attr]) -> PartId {
        let Some(rel_id) = attr_in_ns(attrs, RELS_STRICT_NS, "id").map(RelId::new) else {
            self.record(
                kind.element(),
                SupportStatus::Partial,
                Some("an embedded face carries no r:id, so its bytes cannot be found".to_owned()),
                Some(self.location()),
            );
            return PartId::new(LOST_FONT_PART);
        };
        let part = self.part.clone();
        let resolved = self
            .package
            .resolve_relationship(&part, rel_id.as_str())
            .ok()
            .and_then(|relationship| relationship.resolved.clone());
        if let Some(target) = resolved {
            return target;
        }
        self.record(
            kind.element(),
            SupportStatus::Partial,
            Some(format!(
                "relationship {rel_id} could not be resolved, so the embedded font is lost \
                 while the family entry survives"
            )),
            Some(self.location()),
        );
        PartId::new(LOST_FONT_PART)
    }
}
