//! Parsing of `footnotes.xml` / `endnotes.xml` and note properties
//! (`w:footnotePr`/`w:endnotePr`).

use strict_ooxml_core::error::{Result, SourceLocation};
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::notes::{Note, NoteKind, NoteProperties, NoteTable};
use crate::model::support::SupportStatus;

use super::{is_wml, parse_i32, parse_u32, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `footnotes.xml` part.
    pub(crate) fn parse_footnotes_root(&mut self) -> Result<(NoteTable, SourceLocation)> {
        self.parse_notes_root("footnotes", "footnote", "w:footnotes", "w:footnote")
    }

    /// Parses an `endnotes.xml` part.
    pub(crate) fn parse_endnotes_root(&mut self) -> Result<(NoteTable, SourceLocation)> {
        self.parse_notes_root("endnotes", "endnote", "w:endnotes", "w:endnote")
    }

    /// Parses a notes part (`w:footnotes`/`w:endnotes`).
    fn parse_notes_root(
        &mut self,
        root: &str,
        item: &str,
        root_feature: &str,
        item_feature: &str,
    ) -> Result<(NoteTable, SourceLocation)> {
        self.nested(|parser| {
            parser.expect_root(root)?;
            let location = parser.location();
            parser.record(
                root_feature,
                SupportStatus::Supported,
                None,
                Some(location.clone()),
            );
            let mut table = NoteTable::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == item {
                            let note = parser.parse_note(&attrs, item_feature)?;
                            table.insert(note);
                        } else {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of notes part")),
                }
            }
            parser.expect_end_of_part()?;
            Ok((table, location))
        })
    }

    /// Parses one `w:footnote`/`w:endnote`; its start element was consumed.
    fn parse_note(&mut self, attrs: &[Attr], feature: &str) -> Result<Note> {
        let location = self.location();
        let id = wml_attr(attrs, "id").and_then(parse_i32).unwrap_or(0);
        let kind = wml_attr(attrs, "type")
            .and_then(NoteKind::from_strict)
            .unwrap_or(NoteKind::Normal);
        self.record(
            feature,
            SupportStatus::Supported,
            None,
            Some(location.clone()),
        );
        let blocks = self.nested_block(PartParser::parse_block_children)?;
        Ok(Note {
            id,
            kind,
            blocks,
            location,
        })
    }

    /// Parses a `w:footnotePr`/`w:endnotePr` element (start consumed).
    pub(crate) fn parse_note_properties(&mut self) -> Result<NoteProperties> {
        self.nested(|parser| {
            let mut props = NoteProperties::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            match name.local() {
                                "pos" => {
                                    props.position = val_attr(&attrs).map(|v| parser.intern(v));
                                }
                                "numFmt" => {
                                    props.num_format = val_attr(&attrs).map(|v| parser.intern(v));
                                }
                                "numStart" => {
                                    props.num_start = val_attr(&attrs).and_then(parse_u32);
                                }
                                "numRestart" => {
                                    props.num_restart = val_attr(&attrs).map(|v| parser.intern(v));
                                }
                                // `w:footnote`/`w:endnote` here are REFERENCES - the
                                // separator (-1) and the continuation separator (0) -
                                // not notes. They are what draws the rule above a note
                                // block, so they are page content, and the writer used
                                // to emit `w:footnotePr` only when a position or a format
                                // was present: a document whose only note settings are
                                // these two ids produced no element at all.
                                "footnote" | "endnote" => {
                                    // `@w:id`, not `@w:val`. Reading @w:val here is the
                                    // kind of slip that looks right: every sibling in
                                    // CT_FtnProps uses it, and these two do not.
                                    if let Some(id) = wml_attr(&attrs, "id").and_then(parse_i32) {
                                        if let Ok(id) = u32::try_from(id) {
                                            props.separator_ids.push(id);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of note properties"))
                    }
                }
            }
            Ok(props)
        })
    }
}
