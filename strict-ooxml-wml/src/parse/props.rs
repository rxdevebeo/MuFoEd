//! Parsing of `w:pPr`/`w:rPr`/`w:tblPr`/`w:trPr`/`w:tcPr`/`w:sectPr`.
//!
//! Each `parse_*_properties` function consumes its element up to and including
//! the matching end tag; leaf children are consumed with `skip_element` by the
//! arm that handles them.

use std::sync::Arc;

use strict_ooxml_core::error::{Result, SourceLocation};
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::ids::{Ilvl, NumId, StyleId};
use crate::model::props::{
    BorderOffsetFrom, BorderZOrder, CellProperties, ColumnSpec, Columns, DocGrid, HeaderFooterKind,
    HeaderFooterRef, Language, LineNumbering, NumPr, PageBorder, PageBorders, PageMargins,
    PageSize, ParagraphProperties, RowProperties, RunProperties, SectionProperties,
    TableProperties,
};
use crate::model::support::SupportStatus;
use crate::model::values::{
    Border, BorderStyle, Borders, CellMargins, Color, DocGridType, EighthsPoint, Fonts, HalfPoints,
    HeightRule, Highlight, Indentation, Justification, LineNumberRestart, LineSpacingRule,
    PageOrientation, RowHeight, SectionType, Shading, Spacing, TabAlignment, TabLeader, TabStop,
    TableLayout, TableLook, TextDirection, ThemeColor, ThemeColorRef, TriState, Twips, Underline,
    VertAlign, VerticalJc, VerticalMerge, Width, WidthKind,
};
use crate::RELS_STRICT_NS;

use super::{
    attr_in_ns, decimal_to_i32, is_wml, parse_decimal, parse_i32, parse_measurement_or_percent,
    parse_on_off, parse_signed_twips, parse_text_scale, parse_u32, val_attr, wml_attr, PartParser,
};

/// Parses a tri-state on/off element (present without a value means `on`).
fn tristate(attrs: &[Attr]) -> TriState {
    match val_attr(attrs) {
        None => TriState::On,
        Some(value) => TriState::from_strict(value).unwrap_or(TriState::On),
    }
}

/// Parses a boolean attribute value.
fn attr_on(attrs: &[Attr], local: &str) -> bool {
    wml_attr(attrs, local).is_some_and(|value| matches!(value, "true" | "on" | "1"))
}

impl PartParser<'_> {
    /// Parses the children of a `w:pPr`.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_paragraph_properties(&mut self) -> Result<ParagraphProperties> {
        let location = self.location();
        self.enter()?;
        let mut props = ParagraphProperties {
            location: Some(location),
            ..ParagraphProperties::default()
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
                        "pStyle" => {
                            props.style = self.val_string(&attrs).map(StyleId::new);
                            self.skip_element()?;
                        }
                        "keepNext" => {
                            props.keep_next = parse_on_off(&attrs);
                            self.skip_element()?;
                        }
                        "keepLines" => {
                            props.keep_lines = parse_on_off(&attrs);
                            self.skip_element()?;
                        }
                        "pageBreakBefore" => {
                            props.page_break_before = parse_on_off(&attrs);
                            self.skip_element()?;
                        }
                        "widowControl" => {
                            props.widow_control = tristate(&attrs);
                            self.skip_element()?;
                        }
                        "numPr" => props.numbering = Some(self.parse_num_pr()?),
                        "suppressLineNumbers" => {
                            props.suppress_line_numbers = parse_on_off(&attrs);
                            self.skip_element()?;
                        }
                        "contextualSpacing" => {
                            props.contextual_spacing = parse_on_off(&attrs);
                            self.skip_element()?;
                        }
                        "wordWrap" => {
                            props.word_wrap = tristate(&attrs);
                            self.skip_element()?;
                        }
                        "snapToGrid" => {
                            props.snap_to_grid = tristate(&attrs);
                            self.skip_element()?;
                        }
                        "bidi" => {
                            props.bidi = parse_on_off(&attrs);
                            self.skip_element()?;
                        }
                        "pBdr" => props.borders = self.parse_borders()?,
                        "shd" => {
                            props.shading = Some(self.parse_shading(&attrs));
                            self.skip_element()?;
                        }
                        "tabs" => props.tabs = self.parse_tabs()?,
                        "spacing" => {
                            props.spacing = Some(self.parse_paragraph_spacing(&attrs));
                            self.skip_element()?;
                        }
                        "ind" => {
                            props.indentation = Some(self.parse_indentation(&attrs));
                            self.skip_element()?;
                        }
                        "jc" => {
                            props.alignment =
                                self.val_enum(&attrs, "w:jc", Justification::from_strict);
                            self.skip_element()?;
                        }
                        "outlineLvl" => {
                            props.outline_level = self
                                .val_u32(&attrs, "w:outlineLvl")
                                .map(|value| u8::try_from(value.min(9)).unwrap_or(9));
                            self.skip_element()?;
                        }
                        "textDirection" => {
                            props.text_direction = self.val_enum(
                                &attrs,
                                "w:textDirection",
                                TextDirection::from_strict,
                            );
                            self.skip_element()?;
                        }
                        "rPr" => props.run_props = Some(self.parse_run_properties()?),
                        "sectPr" => props.section = Some(self.parse_section_properties()?),
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of paragraph properties")),
            }
        }
        self.leave();
        Ok(props)
    }

    /// Parses `w:numPr`.
    fn parse_num_pr(&mut self) -> Result<NumPr> {
        self.enter()?;
        let mut num_pr = NumPr::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) {
                        match name.local() {
                            "numId" => {
                                num_pr.num_id = self.val_u32(&attrs, "w:numId").map(NumId);
                            }
                            "ilvl" => {
                                num_pr.ilvl = self
                                    .val_u32(&attrs, "w:ilvl")
                                    .map(|value| Ilvl(u8::try_from(value.min(8)).unwrap_or(8)));
                            }
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of numPr")),
            }
        }
        self.leave();
        Ok(num_pr)
    }

    /// Parses the children of a `w:rPr`.
    pub(crate) fn parse_run_properties(&mut self) -> Result<RunProperties> {
        let location = self.location();
        self.enter()?;
        let mut props = RunProperties {
            location: Some(location),
            ..RunProperties::default()
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
                        "rStyle" => props.style = self.val_string(&attrs).map(StyleId::new),
                        "rFonts" => props.fonts = Some(self.parse_fonts(&attrs)),
                        "b" => props.bold = tristate(&attrs),
                        "i" => props.italic = tristate(&attrs),
                        "u" => {
                            props.underline = self.val_enum(&attrs, "w:u", Underline::from_strict);
                            props.underline_color = wml_attr(&attrs, "color").map(Color::new);
                        }
                        "strike" => props.strike = tristate(&attrs),
                        "dstrike" => props.double_strike = tristate(&attrs),
                        "color" => {
                            props.color = self.val_string(&attrs).map(Color::new);
                            if let Some(theme) = wml_attr(&attrs, "themeColor") {
                                props.color_theme = Some(ThemeColorRef {
                                    color: ThemeColor::new(self.intern(theme)),
                                    tint: wml_attr(&attrs, "themeTint")
                                        .map(|value| self.intern(value)),
                                    shade: wml_attr(&attrs, "themeShade")
                                        .map(|value| self.intern(value)),
                                });
                            }
                        }
                        "highlight" => {
                            props.highlight =
                                self.val_enum(&attrs, "w:highlight", Highlight::from_strict);
                        }
                        "sz" => props.size = self.val_i32(&attrs, "w:sz").map(HalfPoints),
                        "szCs" => props.size_cs = self.val_i32(&attrs, "w:szCs").map(HalfPoints),
                        "vertAlign" => {
                            props.vert_align =
                                self.val_enum(&attrs, "w:vertAlign", VertAlign::from_strict);
                        }
                        "spacing" => props.spacing = self.val_i32(&attrs, "w:spacing").map(Twips),
                        "position" => {
                            props.position = self.val_i32(&attrs, "w:position").map(HalfPoints);
                        }
                        "w" => {
                            props.scale = self.val_text_scale(&attrs, "w:rPr/w:w");
                        }
                        "kern" => props.kerning = self.val_i32(&attrs, "w:kern").map(HalfPoints),
                        "em" => props.emphasis = self.val_string(&attrs),
                        "lang" => props.language = Some(self.parse_language(&attrs)),
                        "caps" => props.caps = parse_on_off(&attrs),
                        "smallCaps" => props.small_caps = parse_on_off(&attrs),
                        "rtl" => props.rtl = parse_on_off(&attrs),
                        "vanish" => props.vanish = parse_on_off(&attrs),
                        "emboss" => props.emboss = parse_on_off(&attrs),
                        "imprint" => props.imprint = parse_on_off(&attrs),
                        "outline" => props.outline = parse_on_off(&attrs),
                        "shadow" => props.shadow = parse_on_off(&attrs),
                        "noProof" => props.no_proof = parse_on_off(&attrs),
                        "snapToGrid" => props.snap_to_grid = tristate(&attrs),
                        "shd" => props.shading = Some(self.parse_shading(&attrs)),
                        "bdr" => {
                            self.record(
                                "w:bdr",
                                SupportStatus::Partial,
                                Some("run borders are not retained".to_owned()),
                                Some(self.location()),
                            );
                        }
                        _ => {}
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of run properties")),
            }
        }
        self.leave();
        Ok(props)
    }

    /// Parses a `w:lang` element.
    fn parse_language(&mut self, attrs: &[Attr]) -> Language {
        Language {
            val: wml_attr(attrs, "val").map(|value| self.intern(value)),
            east_asia: wml_attr(attrs, "eastAsia").map(|value| self.intern(value)),
            bidi: wml_attr(attrs, "bidi").map(|value| self.intern(value)),
        }
    }

    /// Parses `w:rFonts`.
    fn parse_fonts(&mut self, attrs: &[Attr]) -> Fonts {
        Fonts {
            ascii: wml_attr(attrs, "ascii").map(|value| self.intern(value)),
            h_ansi: wml_attr(attrs, "hAnsi").map(|value| self.intern(value)),
            east_asia: wml_attr(attrs, "eastAsia").map(|value| self.intern(value)),
            complex_script: wml_attr(attrs, "cs").map(|value| self.intern(value)),
            hint: wml_attr(attrs, "hint").map(|value| self.intern(value)),
            ascii_theme: wml_attr(attrs, "asciiTheme").map(|value| self.intern(value)),
            h_ansi_theme: wml_attr(attrs, "hAnsiTheme").map(|value| self.intern(value)),
            east_asia_theme: wml_attr(attrs, "eastAsiaTheme").map(|value| self.intern(value)),
            cs_theme: wml_attr(attrs, "cstheme").map(|value| self.intern(value)),
        }
    }

    /// Parses a border-collection element (`w:pBdr`, `w:tblBorders`, `w:tcBorders`).
    fn parse_borders(&mut self) -> Result<Borders> {
        self.enter()?;
        let mut borders = Borders::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) {
                        let border = self.parse_border_edge(&attrs);
                        match name.local() {
                            "top" => borders.top = Some(border),
                            "bottom" => borders.bottom = Some(border),
                            "start" | "left" => borders.start = Some(border),
                            "end" | "right" => borders.end = Some(border),
                            "insideH" => borders.inside_horizontal = Some(border),
                            "insideV" => borders.inside_vertical = Some(border),
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of borders")),
            }
        }
        self.leave();
        Ok(borders)
    }

    /// Parses a single border edge element.
    fn parse_border_edge(&mut self, attrs: &[Attr]) -> Border {
        Border {
            style: self.val_enum_owned(attrs, "w:border", BorderStyle::from_strict),
            size: self.measure_u16(attrs, "sz", "w:border").map(EighthsPoint),
            color: wml_attr(attrs, "color").map(Color::new),
            space: self.measure_u16(attrs, "space", "w:border"),
            shadow: attr_on(attrs, "shadow"),
            frame: attr_on(attrs, "frame"),
        }
    }

    /// Parses a `w:shd` element (attribute-only).
    fn parse_shading(&mut self, attrs: &[Attr]) -> Shading {
        Shading {
            pattern: wml_attr(attrs, "val").map(|value| self.intern(value)),
            color: wml_attr(attrs, "color").map(Color::new),
            fill: wml_attr(attrs, "fill").map(Color::new),
        }
    }

    /// Parses `w:spacing` (paragraph; attribute-only).
    fn parse_paragraph_spacing(&mut self, attrs: &[Attr]) -> Spacing {
        Spacing {
            before: self.measure_twips(attrs, "before", "w:spacing"),
            after: self.measure_twips(attrs, "after", "w:spacing"),
            line: self.measure_twips(attrs, "line", "w:spacing"),
            line_rule: wml_attr(attrs, "lineRule").and_then(LineSpacingRule::from_strict),
            after_autospacing: attr_on(attrs, "afterAutospacing"),
            before_autospacing: attr_on(attrs, "beforeAutospacing"),
        }
    }

    /// Parses `w:ind` (attribute-only).
    fn parse_indentation(&mut self, attrs: &[Attr]) -> Indentation {
        let start = if wml_attr(attrs, "start").is_some() {
            self.measure_twips(attrs, "start", "w:ind")
        } else {
            self.measure_twips(attrs, "left", "w:ind")
        };
        let end = if wml_attr(attrs, "end").is_some() {
            self.measure_twips(attrs, "end", "w:ind")
        } else {
            self.measure_twips(attrs, "right", "w:ind")
        };
        Indentation {
            start,
            end,
            first_line: self.measure_twips(attrs, "firstLine", "w:ind"),
            hanging: self.measure_twips(attrs, "hanging", "w:ind"),
            start_chars: self.measure_i32(attrs, "startChars", "w:ind"),
            end_chars: self.measure_i32(attrs, "endChars", "w:ind"),
            first_line_chars: self.measure_i32(attrs, "firstLineChars", "w:ind"),
            hanging_chars: self.measure_i32(attrs, "hangingChars", "w:ind"),
        }
    }

    /// Parses `w:tabs` (`CT_TabStop`).
    ///
    /// Per the schema the position is `w:pos` (`ST_SignedTwipsMeasure`), the
    /// alignment is `w:val` (`ST_TabJc`) and the fill is `w:leader`; there is no
    /// `w:jc` on `w:tab` (STAGE-2-WORK-ORDER D-1). The stop is always retained;
    /// a missing/invalid value is recorded rather than dropped.
    fn parse_tabs(&mut self) -> Result<Vec<TabStop>> {
        self.enter()?;
        let mut tabs = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) && name.local() == "tab" {
                        let alignment = self
                            .enum_attr(&attrs, "val", "w:tab", TabAlignment::from_lexical)
                            .unwrap_or(TabAlignment::Start);
                        let leader =
                            self.enum_attr(&attrs, "leader", "w:tab", TabLeader::from_strict);
                        let position = self
                            .measure_twips(&attrs, "pos", "w:tab")
                            .unwrap_or(Twips(0));
                        tabs.push(TabStop {
                            position,
                            alignment,
                            leader,
                        });
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of tabs")),
            }
        }
        self.leave();
        Ok(tabs)
    }

    /// Parses the children of a `w:tblPr`.
    pub(crate) fn parse_table_properties(&mut self) -> Result<TableProperties> {
        let location = self.location();
        self.enter()?;
        let mut props = TableProperties {
            location: Some(location),
            ..TableProperties::default()
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
                        "tblStyle" => props.style = self.val_string(&attrs).map(StyleId::new),
                        "tblW" => props.width = Some(self.parse_width(&attrs, "w:tblW")),
                        "jc" => {
                            props.alignment =
                                self.val_enum(&attrs, "w:jc", Justification::from_strict);
                        }
                        "tblLayout" => {
                            props.layout = self.enum_attr(
                                &attrs,
                                "type",
                                "w:tblLayout",
                                TableLayout::from_strict,
                            );
                        }
                        "tblCellMar" => {
                            props.cell_margins = self.parse_cell_margins()?;
                            continue;
                        }
                        "tblBorders" => {
                            props.borders = self.parse_borders()?;
                            continue;
                        }
                        "shd" => props.shading = Some(self.parse_shading(&attrs)),
                        "tblLook" => props.look = Some(Self::parse_table_look(&attrs)),
                        "tblInd" => {
                            props.indent =
                                self.measure_or_percent(&attrs, "w", "w:tblInd").map(Twips);
                        }
                        "bidiVisual" => props.bidi_visual = parse_on_off(&attrs),
                        _ => {}
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of table properties")),
            }
        }
        self.leave();
        Ok(props)
    }

    /// Parses `w:trPr`.
    pub(crate) fn parse_row_properties(&mut self) -> Result<RowProperties> {
        let location = self.location();
        self.enter()?;
        let mut props = RowProperties {
            location: Some(location),
            ..RowProperties::default()
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
                        "trHeight" => props.height = Some(self.parse_row_height(&attrs)),
                        "tblHeader" => {
                            props.header = parse_on_off(&attrs);
                            if props.header {
                                self.record(
                                    "w:tblHeader",
                                    SupportStatus::Supported,
                                    None,
                                    Some(self.location()),
                                );
                            }
                        }
                        "cantSplit" => props.cant_split = parse_on_off(&attrs),
                        "tblCellMar" => {
                            props.cell_margins = self.parse_cell_margins()?;
                            continue;
                        }
                        "gridBefore" => props.grid_before = self.val_i32(&attrs, "w:gridBefore"),
                        "gridAfter" => props.grid_after = self.val_i32(&attrs, "w:gridAfter"),
                        "wBefore" => {
                            props.width_before = Some(self.parse_width(&attrs, "w:wBefore"));
                        }
                        "wAfter" => {
                            props.width_after = Some(self.parse_width(&attrs, "w:wAfter"));
                        }
                        "rsid" => props.rsid = self.val_string(&attrs),
                        _ => {}
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of row properties")),
            }
        }
        self.leave();
        Ok(props)
    }

    /// Parses `w:tcPr`.
    pub(crate) fn parse_cell_properties(&mut self) -> Result<CellProperties> {
        let location = self.location();
        self.enter()?;
        let mut props = CellProperties {
            location: Some(location),
            ..CellProperties::default()
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
                        "tcW" => props.width = Some(self.parse_width(&attrs, "w:tcW")),
                        "gridSpan" => {
                            props.grid_span = self
                                .val_u32(&attrs, "w:gridSpan")
                                .map(|value| u16::try_from(value).unwrap_or(u16::MAX));
                            self.record(
                                "w:gridSpan",
                                SupportStatus::Supported,
                                None,
                                Some(self.location()),
                            );
                        }
                        "vMerge" => {
                            props.vertical_merge = Some(
                                self.val_enum(&attrs, "w:vMerge", VerticalMerge::from_strict)
                                    .unwrap_or(VerticalMerge::Continue),
                            );
                            self.record(
                                "w:vMerge",
                                SupportStatus::Supported,
                                None,
                                Some(self.location()),
                            );
                        }
                        "vAlign" => {
                            props.vertical_align =
                                self.val_enum(&attrs, "w:vAlign", VerticalJc::from_strict);
                        }
                        "textDirection" => {
                            props.text_direction = self.val_enum(
                                &attrs,
                                "w:textDirection",
                                TextDirection::from_strict,
                            );
                        }
                        "tcBorders" => {
                            props.borders = self.parse_borders()?;
                            continue;
                        }
                        "shd" => props.shading = Some(self.parse_shading(&attrs)),
                        "tcMar" => {
                            props.margins = self.parse_cell_margins()?;
                            continue;
                        }
                        "hideMark" => props.hide_mark = parse_on_off(&attrs),
                        "tcFitText" => props.fit_text = parse_on_off(&attrs),
                        "noWrap" => props.no_wrap = parse_on_off(&attrs),
                        _ => {}
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of cell properties")),
            }
        }
        self.leave();
        Ok(props)
    }

    /// Parses a width attribute set (`w:tblW`, `w:tcW`, `w:wBefore`, `w:wAfter`).
    ///
    /// `ST_MeasurementOrPercent` accepts decimals and percentages (D-2).
    fn parse_width(&mut self, attrs: &[Attr], feature: &str) -> Width {
        Width {
            kind: wml_attr(attrs, "type")
                .and_then(WidthKind::from_strict)
                .unwrap_or_default(),
            value: self.measure_or_percent(attrs, "w", feature),
        }
    }

    /// Parses a `w:trHeight` element.
    fn parse_row_height(&mut self, attrs: &[Attr]) -> RowHeight {
        RowHeight {
            value: self.measure_twips(attrs, "val", "w:trHeight"),
            rule: wml_attr(attrs, "hRule").and_then(HeightRule::from_strict),
        }
    }

    /// Parses a cell-margin container (`w:tblCellMar`, `w:tcMar`).
    fn parse_cell_margins(&mut self) -> Result<CellMargins> {
        self.enter()?;
        let mut margins = CellMargins::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) {
                        let value = self.measure_twips(&attrs, "w", "w:cellMar");
                        match name.local() {
                            "top" => margins.top = value,
                            "start" | "left" => margins.start = value,
                            "bottom" => margins.bottom = value,
                            "end" | "right" => margins.end = value,
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of cell margins")),
            }
        }
        self.leave();
        Ok(margins)
    }

    /// Parses a `w:tblLook` element.
    fn parse_table_look(attrs: &[Attr]) -> TableLook {
        TableLook {
            first_row: attr_on(attrs, "firstRow"),
            last_row: attr_on(attrs, "lastRow"),
            first_column: attr_on(attrs, "firstColumn"),
            last_column: attr_on(attrs, "lastColumn"),
            no_h_band: attr_on(attrs, "noHBand"),
            no_v_band: attr_on(attrs, "noVBand"),
        }
    }

    /// Parses `w:sectPr`.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_section_properties(&mut self) -> Result<SectionProperties> {
        let location = self.location();
        self.enter()?;
        let mut props = SectionProperties {
            location: Some(location),
            ..SectionProperties::default()
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
                        "headerReference" => {
                            if let Some(reference) = Self::parse_header_footer_ref(&attrs) {
                                props.headers.push(reference);
                                self.record(
                                    "w:headerReference",
                                    SupportStatus::Supported,
                                    None,
                                    Some(self.location()),
                                );
                            }
                        }
                        "footerReference" => {
                            if let Some(reference) = Self::parse_header_footer_ref(&attrs) {
                                props.footers.push(reference);
                                self.record(
                                    "w:footerReference",
                                    SupportStatus::Supported,
                                    None,
                                    Some(self.location()),
                                );
                            }
                        }
                        "type" => {
                            props.section_type =
                                self.val_enum(&attrs, "w:type", SectionType::from_strict);
                        }
                        "pgSz" => {
                            props.page_size = Some(PageSize {
                                width: self.measure_twips(&attrs, "w", "w:pgSz"),
                                height: self.measure_twips(&attrs, "h", "w:pgSz"),
                                orientation: wml_attr(&attrs, "orient")
                                    .and_then(PageOrientation::from_strict),
                            });
                        }
                        "pgMar" => {
                            props.page_margins = Some(PageMargins {
                                top: self.measure_twips(&attrs, "top", "w:pgMar"),
                                right: self.measure_twips(&attrs, "right", "w:pgMar"),
                                bottom: self.measure_twips(&attrs, "bottom", "w:pgMar"),
                                left: self.measure_twips(&attrs, "left", "w:pgMar"),
                                header: self.measure_twips(&attrs, "header", "w:pgMar"),
                                footer: self.measure_twips(&attrs, "footer", "w:pgMar"),
                                gutter: self.measure_twips(&attrs, "gutter", "w:pgMar"),
                            });
                        }
                        "cols" => {
                            props.columns = Some(self.parse_columns(&attrs)?);
                            continue;
                        }
                        "titlePg" => props.title_page = parse_on_off(&attrs),
                        "docGrid" => {
                            props.doc_grid = Some(DocGrid {
                                grid_type: wml_attr(&attrs, "type")
                                    .and_then(DocGridType::from_strict),
                                line_pitch: self.measure_i32(&attrs, "linePitch", "w:docGrid"),
                                character_space: self.measure_i32(&attrs, "charSpace", "w:docGrid"),
                            });
                        }
                        "vAlign" => {
                            props.vertical_align =
                                self.val_enum(&attrs, "w:vAlign", VerticalJc::from_strict);
                        }
                        "bidi" => props.bidi = parse_on_off(&attrs),
                        "rtlGutter" => props.rtl_gutter = parse_on_off(&attrs),
                        // Transitional's `EG_SectPrContents` declares `w:gutterAtTop`
                        // and Strict's does not; `w:settings` is where Strict puts
                        // it. Parked on the parser and merged into the settings.
                        "gutterAtTop" => self.section_gutter_at_top = parse_on_off(&attrs),
                        "textDirection" => {
                            props.text_direction = self.val_enum(
                                &attrs,
                                "w:textDirection",
                                TextDirection::from_strict,
                            );
                        }
                        "lnNumType" => {
                            props.line_numbering = Some(self.parse_line_numbering(&attrs));
                        }
                        "footnotePr" => {
                            props.footnote_properties = self.parse_note_properties()?;
                            continue;
                        }
                        "endnotePr" => {
                            props.endnote_properties = self.parse_note_properties()?;
                            continue;
                        }
                        "pgBorders" => {
                            props.page_borders = Some(self.parse_page_borders(&attrs)?);
                            self.record(
                                "w:pgBorders",
                                SupportStatus::Supported,
                                None,
                                Some(self.location()),
                            );
                            continue;
                        }
                        _ => {}
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of section properties")),
            }
        }
        self.leave();
        Ok(props)
    }

    /// Parses `w:pgBorders` (start element consumed).
    fn parse_page_borders(&mut self, attrs: &[Attr]) -> Result<PageBorders> {
        let mut borders = PageBorders {
            offset_from: wml_attr(attrs, "offsetFrom").and_then(BorderOffsetFrom::from_strict),
            z_order: wml_attr(attrs, "zOrder").and_then(BorderZOrder::from_strict),
            ..PageBorders::default()
        };
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) {
                        let edge = self.parse_page_border_edge(&attrs);
                        match name.local() {
                            "top" => borders.top = Some(edge),
                            "left" => borders.left = Some(edge),
                            "bottom" => borders.bottom = Some(edge),
                            "right" => borders.right = Some(edge),
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of page borders")),
            }
        }
        self.leave();
        Ok(borders)
    }

    /// Parses one page-border edge element's attributes.
    fn parse_page_border_edge(&mut self, attrs: &[Attr]) -> PageBorder {
        PageBorder {
            style: wml_attr(attrs, "val").and_then(BorderStyle::from_strict),
            size: wml_attr(attrs, "sz")
                .and_then(|value| value.trim().parse::<u16>().ok())
                .map(EighthsPoint),
            space: wml_attr(attrs, "space").and_then(|value| value.trim().parse::<u16>().ok()),
            color: wml_attr(attrs, "color").map(Color::new),
            theme_color: wml_attr(attrs, "themeColor").map(|slot| ThemeColorRef {
                color: ThemeColor::new(slot),
                tint: wml_attr(attrs, "themeTint").map(|value| self.intern(value)),
                shade: wml_attr(attrs, "themeShade").map(|value| self.intern(value)),
            }),
            shadow: attr_on(attrs, "shadow"),
        }
    }

    /// Parses a `w:headerReference`/`w:footerReference` (attribute-only).
    fn parse_header_footer_ref(attrs: &[Attr]) -> Option<HeaderFooterRef> {
        let kind = match wml_attr(attrs, "type") {
            None | Some("default") => HeaderFooterKind::Default,
            Some("first") => HeaderFooterKind::First,
            Some("even") => HeaderFooterKind::Even,
            Some(_) => return None,
        };
        let rel_id = attr_in_ns(attrs, RELS_STRICT_NS, "id")?;
        Some(HeaderFooterRef {
            kind,
            rel_id: strict_ooxml_core::opc::rels::RelId::new(rel_id),
            part: None,
        })
    }

    /// Parses `w:cols`.
    fn parse_columns(&mut self, attrs: &[Attr]) -> Result<Columns> {
        self.enter()?;
        let equal_width = match wml_attr(attrs, "equalWidth") {
            None => true,
            Some(value) => matches!(value, "true" | "on" | "1"),
        };
        let mut columns = Columns {
            count: self.measure_u16(attrs, "num", "w:cols"),
            space: self.measure_twips(attrs, "space", "w:cols"),
            equal_width,
            separator: attr_on(attrs, "sep"),
            columns: Vec::new(),
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) && name.local() == "col" {
                        columns.columns.push(ColumnSpec {
                            width: self.measure_twips(&attrs, "w", "w:col"),
                            space: self.measure_twips(&attrs, "space", "w:col"),
                        });
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of columns")),
            }
        }
        self.leave();
        Ok(columns)
    }

    /// Parses `w:lnNumType` (attribute-only).
    fn parse_line_numbering(&mut self, attrs: &[Attr]) -> LineNumbering {
        LineNumbering {
            count_by: self.measure_u16(attrs, "countBy", "w:lnNumType"),
            start: self.measure_u16(attrs, "start", "w:lnNumType"),
            restart: wml_attr(attrs, "restart").and_then(LineNumberRestart::from_strict),
            distance: self.measure_twips(attrs, "distance", "w:lnNumType"),
        }
    }

    /// Records a measurement/enum value that could not be applied (D-2).
    pub(crate) fn record_value(&mut self, feature: &str, value: &str, location: &SourceLocation) {
        self.record(
            feature,
            SupportStatus::Partial,
            Some(format!("could not apply value '{value}'")),
            Some(location.clone()),
        );
    }

    /// Reads a twip measurement (`ST_SignedTwipsMeasure`), recording failures.
    pub(crate) fn measure_twips(
        &mut self,
        attrs: &[Attr],
        attribute: &str,
        feature: &str,
    ) -> Option<Twips> {
        let value = wml_attr(attrs, attribute)?;
        let Some(number) = parse_signed_twips(value) else {
            let location = self.location();
            self.record_value(feature, value, &location);
            return None;
        };
        Some(Twips(number))
    }

    /// Reads a `ST_MeasurementOrPercent` value, recording failures.
    pub(crate) fn measure_or_percent(
        &mut self,
        attrs: &[Attr],
        attribute: &str,
        feature: &str,
    ) -> Option<i32> {
        let value = wml_attr(attrs, attribute)?;
        let Some(number) = parse_measurement_or_percent(value) else {
            let location = self.location();
            self.record_value(feature, value, &location);
            return None;
        };
        Some(number)
    }

    /// Reads a decimal integer (counts, grid units), recording failures.
    pub(crate) fn measure_i32(
        &mut self,
        attrs: &[Attr],
        attribute: &str,
        feature: &str,
    ) -> Option<i32> {
        let value = wml_attr(attrs, attribute)?;
        let Some(number) = parse_decimal(value).map(decimal_to_i32) else {
            let location = self.location();
            self.record_value(feature, value, &location);
            return None;
        };
        Some(number)
    }

    /// Reads a non-negative decimal integer, recording failures.
    pub(crate) fn measure_u16(
        &mut self,
        attrs: &[Attr],
        attribute: &str,
        feature: &str,
    ) -> Option<u16> {
        let value = wml_attr(attrs, attribute)?;
        match parse_decimal(value) {
            Some(number) if number >= 0.0 && number <= f64::from(u16::MAX) => {
                Some(decimal_to_i32(number) as u16)
            }
            _ => {
                let location = self.location();
                self.record_value(feature, value, &location);
                None
            }
        }
    }

    /// Reads a `w:val` string into an interned handle.
    fn val_string(&mut self, attrs: &[Attr]) -> Option<Arc<str>> {
        val_attr(attrs).map(|value| self.intern(value))
    }

    /// Reads and validates an `i32` `w:val`.
    fn val_i32(&mut self, attrs: &[Attr], feature: &str) -> Option<i32> {
        let value = val_attr(attrs)?;
        let Some(number) = parse_i32(value) else {
            let location = self.location();
            self.record_enum(feature, value, &location);
            return None;
        };
        Some(number)
    }

    /// Reads and validates a `u32` `w:val`.
    fn val_u32(&mut self, attrs: &[Attr], feature: &str) -> Option<u32> {
        let value = val_attr(attrs)?;
        let Some(number) = parse_u32(value) else {
            let location = self.location();
            self.record_enum(feature, value, &location);
            return None;
        };
        Some(number)
    }

    /// Reads a `ST_TextScale` `w:val`: `90` or `90%`, both meaning 90 percent.
    ///
    /// Recorded through the enum channel rather than the value one, because the
    /// failure that matters here is a value outside the schema's domain
    /// (`601%`), and that is a shape of defect the report already names for
    /// enumerations - `record_enum` files it under the feature and the offending
    /// string, which is what a person reading a loss report needs.
    fn val_text_scale(&mut self, attrs: &[Attr], feature: &str) -> Option<u16> {
        let value = val_attr(attrs)?;
        let Some(scale) = parse_text_scale(value) else {
            let location = self.location();
            self.record_enum(feature, value, &location);
            return None;
        };
        Some(scale)
    }

    /// Reads and validates an enum `w:val`.
    fn val_enum<T>(
        &mut self,
        attrs: &[Attr],
        feature: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        let value = val_attr(attrs)?;
        let Some(parsed) = parse(value) else {
            let location = self.location();
            self.record_enum(feature, value, &location);
            return None;
        };
        Some(parsed)
    }

    /// Reads and validates an enum from a named WML attribute.
    fn enum_attr<T>(
        &mut self,
        attrs: &[Attr],
        attribute: &str,
        feature: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        let value = wml_attr(attrs, attribute)?;
        let Some(parsed) = parse(value) else {
            let location = self.location();
            self.record_enum(feature, value, &location);
            return None;
        };
        Some(parsed)
    }

    /// Validates an enum from an explicit `w:val` attribute value.
    fn val_enum_owned<T>(
        &mut self,
        attrs: &[Attr],
        feature: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        let value = wml_attr(attrs, "val")?;
        let Some(parsed) = parse(value) else {
            let location = self.location();
            self.record_enum(feature, value, &location);
            return None;
        };
        Some(parsed)
    }
}
