//! Parsing of `numbering.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::ids::{AbstractNumId, NumId, StyleId};
use crate::model::numbering::{AbstractNum, Level, LevelOverride, Num, NumberingTable};
use crate::model::support::SupportStatus;
use crate::model::values::Justification;

use super::{is_wml, parse_u32, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `numbering.xml` part.
    pub(crate) fn parse_numbering_root(&mut self) -> Result<NumberingTable> {
        self.nested(|parser| {
            let mut table = NumberingTable::new();
            parser.expect_root("numbering")?;
            parser.record(
                "w:numbering",
                SupportStatus::Supported,
                None,
                Some(parser.location()),
            );
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "abstractNum" => {
                                if let Some(abstract_num) = parser.parse_abstract_num(&attrs)? {
                                    let id = abstract_num.id.0;
                                    let location = abstract_num.location.clone();
                                    if !table.insert_abstract(abstract_num) {
                                        parser.record(
                                            "w:abstractNumId",
                                            SupportStatus::Partial,
                                            Some(format!(
                                                "duplicate abstractNumId {id}; keeping the first"
                                            )),
                                            Some(location),
                                        );
                                    }
                                }
                            }
                            "num" => {
                                if let Some(num) = parser.parse_num(&attrs)? {
                                    let id = num.num_id.0;
                                    let location = num.location.clone();
                                    if !table.insert_num(num) {
                                        parser.record(
                                            "w:numId",
                                            SupportStatus::Partial,
                                            Some(format!(
                                                "duplicate numId {id}; keeping the first"
                                            )),
                                            Some(location),
                                        );
                                    }
                                }
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of numbering part")),
                }
            }
            parser.expect_end_of_part()?;
            Ok(table)
        })
    }

    /// Parses `w:abstractNum`.
    fn parse_abstract_num(&mut self, attrs: &[Attr]) -> Result<Option<AbstractNum>> {
        let location = self.location();
        let Some(id) = wml_attr(attrs, "abstractNumId").and_then(parse_u32) else {
            self.skip_element()?;
            return Ok(None);
        };
        self.nested(|parser| {
            let mut multi_level_type = None;
            let mut num_style_link = None;
            let mut style_link = None;
            let mut levels = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "multiLevelType" => {
                                multi_level_type =
                                    val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "numStyleLink" => {
                                num_style_link = val_attr(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "styleLink" => {
                                style_link = val_attr(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "lvl" => levels.push(parser.parse_level(&attrs)?),
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of abstract numbering"))
                    }
                }
            }
            Ok(Some(AbstractNum {
                id: AbstractNumId(id),
                multi_level_type,
                num_style_link,
                style_link,
                levels,
                location,
            }))
        })
    }

    /// Parses a `w:lvl` element.
    fn parse_level(&mut self, attrs: &[Attr]) -> Result<Level> {
        let ilvl = self.clamped_ilvl(wml_attr(attrs, "ilvl").and_then(parse_u32));
        self.nested(|parser| {
            let mut level = Level::new(ilvl);
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "start" => {
                                level.start =
                                    val_attr(&attrs).map(|value| parser.clamped_start(value));
                                parser.skip_element()?;
                            }
                            "numFmt" => {
                                level.format = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "lvlText" => {
                                level.text = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "lvlJc" => {
                                level.justification =
                                    val_attr(&attrs).and_then(Justification::from_strict);
                                parser.skip_element()?;
                            }
                            "pStyle" => {
                                level.paragraph_style = val_attr(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "suff" => {
                                level.suffix = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "lvlRestart" => {
                                level.restart = val_attr(&attrs).and_then(parse_u32);
                                parser.skip_element()?;
                            }
                            "isLgl" => {
                                level.is_legal = true;
                                parser.skip_element()?;
                            }
                            "tentative" => {
                                level.tentative = true;
                                parser.skip_element()?;
                            }
                            "pPr" => {
                                level.paragraph = parser.parse_paragraph_properties()?.0;
                            }
                            "rPr" => level.run = parser.parse_run_properties()?,
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of numbering level"))
                    }
                }
            }
            Ok(level)
        })
    }

    /// Parses `w:num`.
    fn parse_num(&mut self, attrs: &[Attr]) -> Result<Option<Num>> {
        let location = self.location();
        let Some(num_id) = wml_attr(attrs, "numId").and_then(parse_u32) else {
            self.skip_element()?;
            return Ok(None);
        };
        self.nested(|parser| {
            let mut abstract_id = None;
            let mut overrides = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "abstractNumId" => {
                                abstract_id = val_attr(&attrs).and_then(parse_u32);
                                parser.skip_element()?;
                            }
                            "lvlOverride" => {
                                parser.record(
                                    "w:lvlOverride",
                                    SupportStatus::Supported,
                                    None,
                                    Some(parser.location()),
                                );
                                overrides.push(parser.parse_level_override(&attrs)?);
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of numbering instance"))
                    }
                }
            }
            let Some(abstract_num_id) = abstract_id else {
                parser.record(
                    "w:num",
                    crate::model::support::SupportStatus::Partial,
                    Some("numbering instance without abstractNumId was skipped".to_owned()),
                    Some(location),
                );
                return Ok(None);
            };
            Ok(Some(Num {
                num_id: NumId(num_id),
                abstract_num_id: AbstractNumId(abstract_num_id),
                overrides,
                location,
            }))
        })
    }

    /// Parses `w:lvlOverride`.
    fn parse_level_override(&mut self, attrs: &[Attr]) -> Result<LevelOverride> {
        let ilvl = self.clamped_ilvl(wml_attr(attrs, "ilvl").and_then(parse_u32));
        self.nested(|parser| {
            let mut over = LevelOverride {
                ilvl,
                ..LevelOverride::default()
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "startOverride" => {
                                over.start_override = val_attr(&attrs).and_then(parse_u32);
                                parser.record(
                                    "w:startOverride",
                                    SupportStatus::Supported,
                                    None,
                                    Some(parser.location()),
                                );
                                parser.skip_element()?;
                            }
                            "lvl" => over.level = Some(parser.parse_level(&attrs)?),
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of level override")),
                }
            }
            Ok(over)
        })
    }
}
