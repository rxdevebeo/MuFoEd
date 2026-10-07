//! Parsing of `styles.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::XmlEvent;

use crate::model::ids::StyleId;
use crate::model::props::{
    CellProperties, ParagraphProperties, RowProperties, RunProperties, TableProperties,
};
use crate::model::styles::{DocDefaults, Style, StyleTable, TableStyleCondition};
use crate::model::support::SupportStatus;
use crate::model::values::StyleType;

use super::{feature_id_for, is_wml, parse_on_off_tristate, val_attr, wml_attr, PartParser};
use crate::model::values::TriState;

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
                            _ => {
                                let feature = feature_id_for(&name);
                                parser.record(
                                    &feature,
                                    SupportStatus::Unsupported,
                                    Some("property not modelled".to_owned()),
                                    Some(parser.location()),
                                );
                                parser.skip_element()?;
                            }
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
                            _ => {
                                let feature = feature_id_for(&name);
                                parser.record(
                                    &feature,
                                    SupportStatus::Unsupported,
                                    Some("property not modelled".to_owned()),
                                    Some(parser.location()),
                                );
                                parser.skip_element()?;
                            }
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
    #[allow(clippy::too_many_lines)]
    fn parse_style(&mut self, attrs: &[strict_ooxml_core::xml::Attr]) -> Result<Option<Style>> {
        let location = self.location();
        self.nested(|parser| {
            let style_id = wml_attr(attrs, "styleId").map(StyleId::new);
            let style_type = wml_attr(attrs, "type").and_then(StyleType::from_strict);
            let is_default = wml_attr(attrs, "default").is_some_and(is_on);
            let custom_style = wml_attr(attrs, "customStyle").is_some_and(is_on);
            let mut name = None;
            let mut based_on = None;
            let mut next = None;
            let mut link = None;
            let mut ui_priority = None;
            let mut semi_hidden = false;
            let mut hidden = false;
            let mut q_format = false;
            let mut locked = false;
            let mut unhide_when_used = false;
            let mut auto_redefine = false;
            let mut paragraph = ParagraphProperties::default();
            let mut run = RunProperties::default();
            let mut table_props = TableProperties::default();
            let mut row_props = RowProperties::default();
            let mut cell_props = CellProperties::default();
            let mut conditions = Vec::new();

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
                            "semiHidden" => {
                                semi_hidden = style_flag(parser, &attrs, "w:semiHidden");
                                parser.skip_element()?;
                            }
                            "hidden" => {
                                hidden = style_flag(parser, &attrs, "w:hidden");
                                parser.skip_element()?;
                            }
                            "qFormat" => {
                                q_format = style_flag(parser, &attrs, "w:qFormat");
                                parser.skip_element()?;
                            }
                            "locked" => {
                                locked = style_flag(parser, &attrs, "w:locked");
                                parser.skip_element()?;
                            }
                            "unhideWhenUsed" => {
                                unhide_when_used = style_flag(parser, &attrs, "w:unhideWhenUsed");
                                parser.skip_element()?;
                            }
                            "autoRedefine" => {
                                auto_redefine = style_flag(parser, &attrs, "w:autoRedefine");
                                parser.skip_element()?;
                            }
                            "pPr" => paragraph = parser.parse_paragraph_properties()?.0,
                            "rPr" => run = parser.parse_run_properties()?,
                            "tblPr" => table_props = parser.parse_table_properties()?,
                            "trPr" => row_props = parser.parse_row_properties()?,
                            "tcPr" => cell_props = parser.parse_cell_properties()?,
                            "tblStylePr" => {
                                conditions.push(parser.parse_table_style_condition(&attrs)?);
                            }
                            _ => {
                                let feature = feature_id_for(&element);
                                parser.record(
                                    &feature,
                                    SupportStatus::Unsupported,
                                    Some("property not modelled".to_owned()),
                                    Some(parser.location()),
                                );
                                parser.skip_element()?;
                            }
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
                custom_style,
                auto_redefine,
                semi_hidden,
                hidden,
                q_format,
                locked,
                unhide_when_used,
                ui_priority,
                paragraph,
                run,
                table: table_props,
                row: row_props,
                cell: cell_props,
                conditions,
                based_on_chain: Vec::new(),
                location,
            }))
        })
    }

    /// Reads one `w:tblStylePr` (`pPr`/`rPr`/`tblPr`/`trPr`/`tcPr`).
    fn parse_table_style_condition(
        &mut self,
        attrs: &[strict_ooxml_core::xml::Attr],
    ) -> Result<TableStyleCondition> {
        let kind =
            wml_attr(attrs, "type").map_or_else(|| std::sync::Arc::from(""), std::sync::Arc::from);
        // The condition is kept for the writer, but style resolution and
        // layout do not apply it. That gap belongs in the support report.
        let location = self.location();
        self.record(
            "w:tblStylePr",
            SupportStatus::Partial,
            Some("table style condition preserved; not applied in layout".to_owned()),
            Some(location),
        );
        self.nested(|parser| {
            let mut paragraph = ParagraphProperties::default();
            let mut run = RunProperties::default();
            let mut table = TableProperties::default();
            let mut row = RowProperties::default();
            let mut cell = CellProperties::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_wml(&name) {
                            match name.local() {
                                "pPr" => paragraph = parser.parse_paragraph_properties()?.0,
                                "rPr" => run = parser.parse_run_properties()?,
                                "tblPr" => table = parser.parse_table_properties()?,
                                "trPr" => row = parser.parse_row_properties()?,
                                "tcPr" => cell = parser.parse_cell_properties()?,
                                _ => {
                                    let location = parser.location();
                                    parser.skip_element()?;
                                    parser.record(
                                        "w:tblStylePr",
                                        SupportStatus::Partial,
                                        Some(format!(
                                            "table style condition child `{}` not modelled",
                                            name.local()
                                        )),
                                        Some(location),
                                    );
                                }
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of table style condition"))
                    }
                }
            }
            Ok(TableStyleCondition {
                kind,
                paragraph,
                run,
                table,
                row,
                cell,
            })
        })
    }
}

/// A `CT_OnOff` style flag (`w:qFormat`, `w:hidden`, …): `w:val` decides, a
/// bare element means on. These were set from the element's presence alone, so
/// `<w:qFormat w:val="0"/>` read and round-tripped as on. An invalid value is
/// recorded and read as off.
fn style_flag(
    parser: &mut PartParser<'_>,
    attrs: &[strict_ooxml_core::xml::Attr],
    feature: &str,
) -> bool {
    parse_on_off_tristate(parser, attrs, feature) == TriState::On
}

/// `ST_OnOff` for an attribute (`w:default`, `w:customStyle`).
fn is_on(value: &str) -> bool {
    matches!(value, "true" | "on" | "1")
}
