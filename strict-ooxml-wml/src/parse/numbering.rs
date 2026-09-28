//! Parsing of `numbering.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::ids::{AbstractNumId, Ilvl, NumId, StyleId};
use crate::model::numbering::{AbstractNum, Level, LevelOverride, Num, NumberingTable};
use crate::model::values::Justification;

use super::{is_wml, parse_u32, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `numbering.xml` part.
    pub(crate) fn parse_numbering_root(&mut self) -> Result<NumberingTable> {
        self.enter()?;
        let mut table = NumberingTable::new();
        match self.next_event()? {
            XmlEvent::StartElement { name, .. } if name.local() == "numbering" => {}
            _ => return Err(self.invalid("expected 'w:numbering' root element")),
        }
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "abstractNum" => {
                            if let Some(abstract_num) = self.parse_abstract_num(&attrs)? {
                                table.insert_abstract(abstract_num);
                            }
                        }
                        "num" => {
                            if let Some(num) = self.parse_num(&attrs)? {
                                table.insert_num(num);
                            }
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of numbering part")),
            }
        }
        self.leave();
        Ok(table)
    }

    /// Parses `w:abstractNum`.
    fn parse_abstract_num(&mut self, attrs: &[Attr]) -> Result<Option<AbstractNum>> {
        let location = self.location();
        let Some(id) = wml_attr(attrs, "abstractNumId").and_then(parse_u32) else {
            self.skip_element()?;
            return Ok(None);
        };
        self.enter()?;
        let mut multi_level_type = None;
        let mut num_style_link = None;
        let mut style_link = None;
        let mut levels = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "multiLevelType" => {
                            multi_level_type = val_attr(&attrs).map(|value| self.intern(value));
                            self.skip_element()?;
                        }
                        "numStyleLink" => {
                            num_style_link = val_attr(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "styleLink" => {
                            style_link = val_attr(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "lvl" => levels.push(self.parse_level(&attrs)?),
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of abstract numbering")),
            }
        }
        self.leave();
        Ok(Some(AbstractNum {
            id: AbstractNumId(id),
            multi_level_type,
            num_style_link,
            style_link,
            levels,
            location,
        }))
    }

    /// Parses a `w:lvl` element.
    fn parse_level(&mut self, attrs: &[Attr]) -> Result<Level> {
        let ilvl = wml_attr(attrs, "ilvl")
            .and_then(parse_u32)
            .map_or(Ilvl(0), |value| {
                Ilvl(u8::try_from(value.min(8)).unwrap_or(8))
            });
        self.enter()?;
        let mut level = Level::new(ilvl);
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "start" => {
                            level.start = val_attr(&attrs).and_then(parse_u32);
                            self.skip_element()?;
                        }
                        "numFmt" => {
                            level.format = val_attr(&attrs).map(|value| self.intern(value));
                            self.skip_element()?;
                        }
                        "lvlText" => {
                            level.text = val_attr(&attrs).map(|value| self.intern(value));
                            self.skip_element()?;
                        }
                        "lvlJc" => {
                            level.justification =
                                val_attr(&attrs).and_then(Justification::from_strict);
                            self.skip_element()?;
                        }
                        "pStyle" => {
                            level.paragraph_style = val_attr(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "suff" => {
                            level.suffix = val_attr(&attrs).map(|value| self.intern(value));
                            self.skip_element()?;
                        }
                        "lvlRestart" => {
                            level.restart = val_attr(&attrs).and_then(parse_u32);
                            self.skip_element()?;
                        }
                        "isLgl" => {
                            level.is_legal = true;
                            self.skip_element()?;
                        }
                        "tentative" => {
                            level.tentative = true;
                            self.skip_element()?;
                        }
                        "pPr" => level.paragraph = self.parse_paragraph_properties()?,
                        "rPr" => level.run = self.parse_run_properties()?,
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of numbering level")),
            }
        }
        self.leave();
        Ok(level)
    }

    /// Parses `w:num`.
    fn parse_num(&mut self, attrs: &[Attr]) -> Result<Option<Num>> {
        let location = self.location();
        let Some(num_id) = wml_attr(attrs, "numId").and_then(parse_u32) else {
            self.skip_element()?;
            return Ok(None);
        };
        self.enter()?;
        let mut abstract_id = None;
        let mut overrides = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "abstractNumId" => {
                            abstract_id = val_attr(&attrs).and_then(parse_u32);
                            self.skip_element()?;
                        }
                        "lvlOverride" => overrides.push(self.parse_level_override(&attrs)?),
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of numbering instance")),
            }
        }
        self.leave();
        let Some(abstract_num_id) = abstract_id else {
            self.record(
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
    }

    /// Parses `w:lvlOverride`.
    fn parse_level_override(&mut self, attrs: &[Attr]) -> Result<LevelOverride> {
        let ilvl = wml_attr(attrs, "ilvl")
            .and_then(parse_u32)
            .map_or(Ilvl(0), |value| {
                Ilvl(u8::try_from(value.min(8)).unwrap_or(8))
            });
        self.enter()?;
        let mut over = LevelOverride {
            ilvl,
            ..LevelOverride::default()
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "startOverride" => {
                            over.start_override = val_attr(&attrs).and_then(parse_u32);
                            self.skip_element()?;
                        }
                        "lvl" => over.level = Some(self.parse_level(&attrs)?),
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of level override")),
            }
        }
        self.leave();
        Ok(over)
    }
}
