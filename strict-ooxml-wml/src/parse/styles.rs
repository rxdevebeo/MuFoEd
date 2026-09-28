//! Parsing of `styles.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::XmlEvent;

use crate::model::ids::StyleId;
use crate::model::props::{ParagraphProperties, RunProperties, TableProperties};
use crate::model::styles::{Style, StyleTable};
use crate::model::support::SupportStatus;
use crate::model::values::StyleType;

use super::{is_wml, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `styles.xml` part.
    pub(crate) fn parse_styles_root(&mut self) -> Result<StyleTable> {
        self.enter()?;
        let mut table = StyleTable::new();
        match self.next_event()? {
            XmlEvent::StartElement { name, .. } if name.local() == "styles" => {}
            _ => return Err(self.invalid("expected 'w:styles' root element")),
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
                        "style" => {
                            if let Some(style) = self.parse_style(&attrs)? {
                                table.insert(style);
                            }
                        }
                        "docDefaults" => {
                            self.record(
                                "w:docDefaults",
                                SupportStatus::Partial,
                                Some("document defaults are not flattened".to_owned()),
                                Some(self.location()),
                            );
                            self.skip_element()?;
                        }
                        "latentStyles" => {
                            self.record(
                                "w:latentStyles",
                                SupportStatus::Ignored,
                                None,
                                Some(self.location()),
                            );
                            self.skip_element()?;
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of styles part")),
            }
        }
        self.leave();
        Ok(table)
    }

    /// Parses one `w:style` element.
    fn parse_style(&mut self, attrs: &[strict_ooxml_core::xml::Attr]) -> Result<Option<Style>> {
        let location = self.location();
        self.enter()?;
        let style_id = wml_attr(attrs, "styleId").map(StyleId::new);
        let style_type = wml_attr(attrs, "type").and_then(StyleType::from_strict);
        let is_default =
            wml_attr(attrs, "default").is_some_and(|v| matches!(v, "true" | "on" | "1"));
        let mut name = None;
        let mut based_on = None;
        let mut next = None;
        let mut link = None;
        let mut ui_priority = None;
        let mut hidden = false;
        let mut paragraph = ParagraphProperties::default();
        let mut run = RunProperties::default();
        let mut table_props = TableProperties::default();

        loop {
            match self.next_event()? {
                XmlEvent::StartElement {
                    name: element,
                    attrs,
                } => {
                    if !is_wml(&element) {
                        self.record_foreign(&element);
                        self.skip_element()?;
                        continue;
                    }
                    match element.local() {
                        "name" => {
                            name = val_attr(&attrs).map(|value| self.intern(value));
                            self.skip_element()?;
                        }
                        "basedOn" => {
                            based_on = val_attr(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "next" => {
                            next = val_attr(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "link" => {
                            link = val_attr(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "uiPriority" => {
                            ui_priority =
                                val_attr(&attrs).and_then(|value| value.trim().parse().ok());
                            self.skip_element()?;
                        }
                        "semiHidden" | "hidden" => {
                            hidden = true;
                            self.skip_element()?;
                        }
                        "pPr" => paragraph = self.parse_paragraph_properties()?,
                        "rPr" => run = self.parse_run_properties()?,
                        "tblPr" => table_props = self.parse_table_properties()?,
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of style")),
            }
        }
        self.leave();

        let (Some(id), Some(style_type)) = (style_id, style_type) else {
            self.record(
                "w:style",
                SupportStatus::Partial,
                Some("style without a valid type or id was skipped".to_owned()),
                Some(location),
            );
            return Ok(None);
        };
        Ok(Some(Style {
            id,
            style_type,
            name,
            based_on,
            next,
            link,
            is_default,
            hidden,
            ui_priority,
            paragraph,
            run,
            table: table_props,
            based_on_chain: Vec::new(),
            location,
        }))
    }
}
