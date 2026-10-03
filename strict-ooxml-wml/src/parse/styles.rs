//! Parsing of `styles.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::XmlEvent;

use crate::model::ids::StyleId;
use crate::model::props::{ParagraphProperties, RunProperties, TableProperties};
use crate::model::styles::{DocDefaults, Style, StyleTable};
use crate::model::support::SupportStatus;
use crate::model::values::StyleType;

use super::{is_wml, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `styles.xml` part.
    pub(crate) fn parse_styles_root(&mut self) -> Result<StyleTable> {
        self.nested(|parser| {
            let mut table = StyleTable::new();
            parser.expect_root("styles")?;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "style" => {
                                if let Some(style) = parser.parse_style(&attrs)? {
                                    table.insert(style);
                                }
                            }
                            "docDefaults" => {
                                table.set_defaults(parser.parse_doc_defaults()?);
                            }
                            "latentStyles" => {
                                parser.record(
                                    "w:latentStyles",
                                    SupportStatus::Ignored,
                                    None,
                                    Some(parser.location()),
                                );
                                parser.skip_element()?;
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of styles part")),
                }
            }
            parser.expect_end_of_part()?;
            Ok(table)
        })
    }

    /// Parses a `w:docDefaults` element (ISO/IEC 29500-1 §17.7.1).
    fn parse_doc_defaults(&mut self) -> Result<DocDefaults> {
        self.nested(|parser| {
            let mut defaults = DocDefaults::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "rPrDefault" => {
                                if let Some(run) = parser.parse_wrapped_run_properties()? {
                                    defaults.run = run;
                                }
                            }
                            "pPrDefault" => {
                                if let Some(paragraph) =
                                    parser.parse_wrapped_paragraph_properties()?
                                {
                                    defaults.paragraph = paragraph;
                                }
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of docDefaults")),
                }
            }
            Ok(defaults)
        })
    }

    /// Parses a wrapper element whose only relevant child is `w:rPr`.
    fn parse_wrapped_run_properties(&mut self) -> Result<Option<RunProperties>> {
        self.nested(|parser| {
            let mut parsed = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        if name.local() == "rPr" {
                            parsed = Some(parser.parse_run_properties()?);
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of rPrDefault")),
                }
            }
            Ok(parsed)
        })
    }

    /// Parses a wrapper element whose only relevant child is `w:pPr`.
    fn parse_wrapped_paragraph_properties(&mut self) -> Result<Option<ParagraphProperties>> {
        self.nested(|parser| {
            let mut parsed = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        if name.local() == "pPr" {
                            parsed = Some(parser.parse_paragraph_properties()?.0);
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of pPrDefault")),
                }
            }
            Ok(parsed)
        })
    }

    /// Parses one `w:style` element.
    fn parse_style(&mut self, attrs: &[strict_ooxml_core::xml::Attr]) -> Result<Option<Style>> {
        let location = self.location();
        self.nested(|parser| {
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
                match parser.next_event()? {
                    XmlEvent::StartElement {
                        name: element,
                        attrs,
                    } => {
                        if !is_wml(&element) {
                            parser.record_foreign(&element);
                            parser.skip_element()?;
                            continue;
                        }
                        match element.local() {
                            "name" => {
                                name = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "basedOn" => {
                                based_on = val_attr(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "next" => {
                                next = val_attr(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "link" => {
                                link = val_attr(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "uiPriority" => {
                                ui_priority =
                                    val_attr(&attrs).and_then(|value| value.trim().parse().ok());
                                parser.skip_element()?;
                            }
                            "semiHidden" | "hidden" => {
                                hidden = true;
                                parser.skip_element()?;
                            }
                            "pPr" => paragraph = parser.parse_paragraph_properties()?.0,
                            "rPr" => run = parser.parse_run_properties()?,
                            "tblPr" => table_props = parser.parse_table_properties()?,
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of style")),
                }
            }

            let (Some(id), Some(style_type)) = (style_id, style_type) else {
                parser.record(
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
        })
    }
}
