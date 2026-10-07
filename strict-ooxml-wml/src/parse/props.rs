//! Parsing of `w:pPr`/`w:rPr`/`w:tblPr`/`w:trPr`/`w:tcPr`/`w:sectPr`.
//!
//! Each `parse_*_properties` function consumes its element up to and including
//! the matching end tag; leaf children are consumed with `skip_element` by the
//! arm that handles them.

use std::sync::Arc;

use strict_ooxml_core::error::{Result, SourceLocation};
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::ids::{NumId, StyleId};
use crate::model::props::{
    BorderOffsetFrom, BorderZOrder, CellProperties, ColumnSpec, Columns, DocGrid, FrameProperties,
    HeaderFooterKind, HeaderFooterRef, Language, LineNumbering, NumPr, PageBorder, PageBorders,
    PageMargins, PageNumberType, PageSize, ParagraphProperties, RowProperties, RunProperties,
    SectionProperties, TablePositioning, TableProperties,
};
use crate::model::revision::{Revision, RevisionKind};
use crate::model::support::SupportStatus;
use crate::model::values::{
    Border, BorderStyle, Borders, CellMargins, Color, DocGridType, EighthsPoint, Fonts, HalfPoints,
    HeightRule, Highlight, Indentation, Justification, LineNumberRestart, LineSpacingRule,
    PageOrientation, RowHeight, SectionType, Shading, Spacing, TabAlignment, TabLeader, TabStop,
    TableLayout, TableLook, TextDirection, ThemeColor, ThemeColorRef, Twips, Underline, VertAlign,
    VerticalJc, VerticalMerge, Width, WidthKind,
};
use crate::RELS_STRICT_NS;

use super::{
    attr_in_ns, decimal_to_i32, feature_id_for, is_wml, parse_decimal, parse_i32,
    parse_measurement_or_percent, parse_on_off, parse_on_off_tristate, parse_signed_twips,
    parse_text_scale, parse_u32, val_attr, wml_attr, PartParser,
};

/// Records an unmodelled `*Pr` child (AUD-46) and skips its subtree.
fn record_unmodelled_property(
    parser: &mut PartParser<'_>,
    name: &strict_ooxml_core::xml::qname::QName,
) -> Result<()> {
    let feature = feature_id_for(name);
    parser.record(
        &feature,
        SupportStatus::Unsupported,
        Some("property not modelled".to_owned()),
        Some(parser.location()),
    );
    parser.skip_element()
}

/// Revision attributes on `w:sectPr` are not written back.
fn record_revision_attrs(parser: &mut PartParser<'_>, element: &str, attrs: &[Attr]) {
    for attr in attrs {
        let local = attr.name.local();
        if !local.starts_with("rsid") {
            continue;
        }
        parser.record(
            &format!("{element}@{local}"),
            SupportStatus::Partial,
            Some("revision id is not written back".to_owned()),
            Some(parser.location()),
        );
    }
}

/// Records a property-change history element (`*Change`) as `partial` (AUD-46).
fn record_property_change(parser: &mut PartParser<'_>, feature: &str) -> Result<()> {
    parser.record(
        feature,
        SupportStatus::Partial,
        Some("property change history dropped".to_owned()),
        Some(parser.location()),
    );
    parser.skip_element()
}

/// Parses a boolean attribute value.
fn attr_on(attrs: &[Attr], local: &str) -> bool {
    wml_attr(attrs, local).is_some_and(|value| matches!(value, "true" | "on" | "1"))
}

/// `Some` when the attribute is present. `false` is explicit, not absent.
fn attr_bool(attrs: &[Attr], local: &str) -> Option<bool> {
    wml_attr(attrs, local).map(|value| matches!(value, "true" | "on" | "1"))
}

impl PartParser<'_> {
    /// Parses the children of a `w:pPr`.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_paragraph_properties(
        &mut self,
    ) -> Result<(ParagraphProperties, Option<Revision>)> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = ParagraphProperties {
                location: Some(location),
                ..ParagraphProperties::default()
            };
            let mut mark_revision = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "pStyle" => {
                                props.style = parser.val_string(&attrs).map(StyleId::new);
                                parser.skip_element()?;
                            }
                            "keepNext" => {
                                props.keep_next =
                                    parse_on_off_tristate(parser, &attrs, "w:keepNext");
                                parser.skip_element()?;
                            }
                            "keepLines" => {
                                props.keep_lines =
                                    parse_on_off_tristate(parser, &attrs, "w:keepLines");
                                parser.skip_element()?;
                            }
                            "pageBreakBefore" => {
                                props.page_break_before =
                                    parse_on_off_tristate(parser, &attrs, "w:pageBreakBefore");
                                parser.skip_element()?;
                            }
                            "widowControl" => {
                                props.widow_control =
                                    parse_on_off_tristate(parser, &attrs, "w:widowControl");
                                parser.skip_element()?;
                            }
                            "numPr" => props.numbering = Some(parser.parse_num_pr()?),
                            "suppressLineNumbers" => {
                                props.suppress_line_numbers =
                                    parse_on_off_tristate(parser, &attrs, "w:suppressLineNumbers");
                                parser.skip_element()?;
                            }
                            "contextualSpacing" => {
                                props.contextual_spacing =
                                    parse_on_off_tristate(parser, &attrs, "w:contextualSpacing");
                                parser.skip_element()?;
                            }
                            "wordWrap" => {
                                props.word_wrap =
                                    parse_on_off_tristate(parser, &attrs, "w:wordWrap");
                                parser.skip_element()?;
                            }
                            "snapToGrid" => {
                                props.snap_to_grid =
                                    parse_on_off_tristate(parser, &attrs, "w:snapToGrid");
                                parser.skip_element()?;
                            }
                            "bidi" => {
                                props.bidi = parse_on_off_tristate(parser, &attrs, "w:bidi");
                                parser.skip_element()?;
                            }
                            "pBdr" => props.borders = parser.parse_borders()?,
                            "shd" => {
                                props.shading = Some(parser.parse_shading(&attrs));
                                parser.skip_element()?;
                            }
                            "tabs" => props.tabs = parser.parse_tabs()?,
                            "spacing" => {
                                props.spacing = Some(parser.parse_paragraph_spacing(&attrs));
                                parser.skip_element()?;
                            }
                            "ind" => {
                                props.indentation = Some(parser.parse_indentation(&attrs));
                                parser.skip_element()?;
                            }
                            "jc" => {
                                props.alignment =
                                    parser.val_enum(&attrs, "w:jc", Justification::from_strict);
                                parser.skip_element()?;
                            }
                            "outlineLvl" => {
                                props.outline_level = parser
                                    .val_u32(&attrs, "w:outlineLvl")
                                    .map(|value| u8::try_from(value.min(9)).unwrap_or(9));
                                parser.skip_element()?;
                            }
                            "textDirection" => {
                                props.text_direction = parser.val_enum(
                                    &attrs,
                                    "w:textDirection",
                                    TextDirection::from_strict,
                                );
                                parser.skip_element()?;
                            }
                            "rPr" => {
                                let (run_props, revision) =
                                    parser.parse_run_properties_inner(true)?;
                                props.run_props = Some(run_props);
                                if mark_revision.is_none() {
                                    mark_revision = revision;
                                }
                            }
                            "sectPr" => {
                                record_revision_attrs(parser, "w:sectPr", &attrs);
                                props.section = Some(parser.parse_section_properties()?);
                            }
                            "framePr" => {
                                props.frame = Some(Self::parse_frame_pr(&attrs, parser));
                                parser.record(
                                    "w:framePr",
                                    SupportStatus::Partial,
                                    Some("text frame kept in flow".to_owned()),
                                    Some(parser.location()),
                                );
                                parser.skip_element()?;
                            }
                            "pPrChange" => {
                                record_property_change(parser, "w:pPrChange")?;
                            }
                            _ => record_unmodelled_property(parser, &name)?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of paragraph properties"))
                    }
                }
            }
            Ok((props, mark_revision))
        })
    }

    /// Parses `w:numPr`.
    fn parse_num_pr(&mut self) -> Result<NumPr> {
        self.nested(|parser| {
            let mut num_pr = NumPr::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            match name.local() {
                                "numId" => {
                                    num_pr.num_id = parser.val_u32(&attrs, "w:numId").map(NumId);
                                }
                                "ilvl" => {
                                    num_pr.ilvl = parser
                                        .val_u32(&attrs, "w:ilvl")
                                        .map(|value| parser.clamped_ilvl(Some(value)));
                                }
                                _ => {
                                    record_unmodelled_property(parser, &name)?;
                                    continue;
                                }
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of numPr")),
                }
            }
            Ok(num_pr)
        })
    }

    /// Parses the children of a `w:rPr`.
    pub(crate) fn parse_run_properties(&mut self) -> Result<RunProperties> {
        self.parse_run_properties_inner(false)
            .map(|(props, _)| props)
    }

    /// Parses `w:rPr`. When `paragraph_mark` is set, `w:ins`/`w:del` become the
    /// paragraph-mark revision (ADR-0018); otherwise they are recorded as dropped
    /// property-change history.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_run_properties_inner(
        &mut self,
        paragraph_mark: bool,
    ) -> Result<(RunProperties, Option<Revision>)> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = RunProperties {
                location: Some(location),
                ..RunProperties::default()
            };
            let mut mark_revision = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "rStyle" => props.style = parser.val_string(&attrs).map(StyleId::new),
                            "rFonts" => {
                                let next = parser.parse_fonts(&attrs);
                                if let Some(existing) = props.fonts.as_mut() {
                                    existing.overlay(next);
                                } else {
                                    props.fonts = Some(next);
                                }
                            }
                            "b" => props.bold = parse_on_off_tristate(parser, &attrs, "w:b"),
                            "bCs" => {
                                props.bold_cs = parse_on_off_tristate(parser, &attrs, "w:bCs");
                            }
                            "i" => props.italic = parse_on_off_tristate(parser, &attrs, "w:i"),
                            "iCs" => {
                                props.italic_cs = parse_on_off_tristate(parser, &attrs, "w:iCs");
                            }
                            "u" => {
                                props.underline =
                                    parser.val_enum(&attrs, "w:u", Underline::from_strict);
                                props.underline_color = wml_attr(&attrs, "color").map(Color::new);
                                props.underline_theme = theme_color_ref(
                                    parser,
                                    &attrs,
                                    "themeColor",
                                    "themeTint",
                                    "themeShade",
                                )
                                .map(Box::new);
                            }
                            "strike" => {
                                props.strike = parse_on_off_tristate(parser, &attrs, "w:strike");
                            }
                            "dstrike" => {
                                props.double_strike =
                                    parse_on_off_tristate(parser, &attrs, "w:dstrike");
                            }
                            "color" => {
                                props.color = parser.val_string(&attrs).map(Color::new);
                                props.color_theme = theme_color_ref(
                                    parser,
                                    &attrs,
                                    "themeColor",
                                    "themeTint",
                                    "themeShade",
                                );
                            }
                            "highlight" => {
                                props.highlight =
                                    parser.val_enum(&attrs, "w:highlight", Highlight::from_strict);
                            }
                            "sz" => props.size = parser.val_i32(&attrs, "w:sz").map(HalfPoints),
                            "szCs" => {
                                props.size_cs = parser.val_i32(&attrs, "w:szCs").map(HalfPoints);
                            }
                            "vertAlign" => {
                                props.vert_align =
                                    parser.val_enum(&attrs, "w:vertAlign", VertAlign::from_strict);
                            }
                            "spacing" => {
                                props.spacing = parser.val_i32(&attrs, "w:spacing").map(Twips);
                            }
                            "position" => {
                                props.position =
                                    parser.val_i32(&attrs, "w:position").map(HalfPoints);
                            }
                            "w" => {
                                props.scale = parser.val_text_scale(&attrs, "w:rPr/w:w");
                            }
                            "kern" => {
                                props.kerning = parser.val_i32(&attrs, "w:kern").map(HalfPoints);
                            }
                            "em" => props.emphasis = parser.val_string(&attrs),
                            "lang" => props.language = Some(parser.parse_language(&attrs)),
                            "caps" => props.caps = parse_on_off_tristate(parser, &attrs, "w:caps"),
                            "smallCaps" => {
                                props.small_caps =
                                    parse_on_off_tristate(parser, &attrs, "w:smallCaps");
                            }
                            "rtl" => props.rtl = parse_on_off_tristate(parser, &attrs, "w:rtl"),
                            "vanish" => {
                                props.vanish = parse_on_off_tristate(parser, &attrs, "w:vanish");
                            }
                            "emboss" => {
                                props.emboss = parse_on_off_tristate(parser, &attrs, "w:emboss");
                            }
                            "imprint" => {
                                props.imprint = parse_on_off_tristate(parser, &attrs, "w:imprint");
                            }
                            "outline" => {
                                props.outline = parse_on_off_tristate(parser, &attrs, "w:outline");
                            }
                            "shadow" => {
                                props.shadow = parse_on_off_tristate(parser, &attrs, "w:shadow");
                            }
                            "noProof" => {
                                props.no_proof = parse_on_off_tristate(parser, &attrs, "w:noProof");
                            }
                            "snapToGrid" => {
                                props.snap_to_grid =
                                    parse_on_off_tristate(parser, &attrs, "w:snapToGrid");
                            }
                            "shd" => props.shading = Some(parser.parse_shading(&attrs)),
                            "bdr" => {
                                parser.record(
                                    "w:bdr",
                                    SupportStatus::Partial,
                                    Some("run borders are not retained".to_owned()),
                                    Some(parser.location()),
                                );
                            }
                            "ins" | "del" => {
                                let kind = if name.local() == "ins" {
                                    RevisionKind::Insert
                                } else {
                                    RevisionKind::Delete
                                };
                                if paragraph_mark && mark_revision.is_none() {
                                    let id =
                                        wml_attr(&attrs, "id").and_then(parse_u32).unwrap_or(0);
                                    let author = wml_attr(&attrs, "author")
                                        .map(|value| parser.intern(value));
                                    let date =
                                        wml_attr(&attrs, "date").map(|value| parser.intern(value));
                                    mark_revision = Some(Revision {
                                        kind,
                                        id,
                                        author,
                                        date,
                                    });
                                    parser.record(
                                        kind.feature_id(),
                                        SupportStatus::Supported,
                                        Some("paragraph mark revision".to_owned()),
                                        Some(parser.location()),
                                    );
                                } else {
                                    parser.record(
                                        kind.feature_id(),
                                        SupportStatus::Partial,
                                        Some("property change history dropped".to_owned()),
                                        Some(parser.location()),
                                    );
                                }
                            }
                            "rPrChange" => {
                                parser.record(
                                    "w:rPrChange",
                                    SupportStatus::Partial,
                                    Some("property change history dropped".to_owned()),
                                    Some(parser.location()),
                                );
                            }
                            _ => {
                                record_unmodelled_property(parser, &name)?;
                                continue;
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of run properties")),
                }
            }
            Ok((props, mark_revision))
        })
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
            hint: {
                let hint = wml_attr(attrs, "hint");
                if hint.is_some_and(|value| value.eq_ignore_ascii_case("cs")) {
                    // Strict `ST_Hint` is `default` | `eastAsia`. The face stays
                    // on `w:cs`; the hint token itself cannot be written.
                    self.record(
                        "w:rFonts@hint",
                        SupportStatus::Partial,
                        Some(
                            "Strict ST_Hint has no cs; the complex-script face stays on w:cs"
                                .to_owned(),
                        ),
                        Some(self.location()),
                    );
                }
                hint.map(|value| self.intern(value))
            },
            ascii_theme: wml_attr(attrs, "asciiTheme").map(|value| self.intern(value)),
            h_ansi_theme: wml_attr(attrs, "hAnsiTheme").map(|value| self.intern(value)),
            east_asia_theme: wml_attr(attrs, "eastAsiaTheme").map(|value| self.intern(value)),
            cs_theme: wml_attr(attrs, "cstheme").map(|value| self.intern(value)),
        }
    }

    /// Parses a border-collection element (`w:pBdr`, `w:tblBorders`, `w:tcBorders`).
    fn parse_borders(&mut self) -> Result<Borders> {
        self.nested(|parser| {
            let mut borders = Borders::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            let border = parser.parse_border_edge(&attrs);
                            match name.local() {
                                "top" => borders.top = Some(border),
                                "bottom" => borders.bottom = Some(border),
                                "start" | "left" => borders.start = Some(border),
                                "end" | "right" => borders.end = Some(border),
                                "insideH" => borders.inside_horizontal = Some(border),
                                "insideV" => borders.inside_vertical = Some(border),
                                _ => {
                                    record_unmodelled_property(parser, &name)?;
                                    continue;
                                }
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of borders")),
                }
            }
            Ok(borders)
        })
    }

    /// Parses a single border edge element.
    fn parse_border_edge(&mut self, attrs: &[Attr]) -> Border {
        Border {
            style: self.val_enum_owned(attrs, "w:border", BorderStyle::from_strict),
            size: self.measure_u16(attrs, "sz", "w:border").map(EighthsPoint),
            color: wml_attr(attrs, "color").map(Color::new),
            theme_color: theme_color_ref(self, attrs, "themeColor", "themeTint", "themeShade")
                .map(Box::new),
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
            theme_color: theme_color_ref(self, attrs, "themeColor", "themeTint", "themeShade")
                .map(Box::new),
            theme_fill: theme_color_ref(
                self,
                attrs,
                "themeFill",
                "themeFillTint",
                "themeFillShade",
            )
            .map(Box::new),
        }
    }

    /// Parses `w:spacing` (paragraph; attribute-only).
    fn parse_paragraph_spacing(&mut self, attrs: &[Attr]) -> Spacing {
        Spacing {
            before: self.measure_twips(attrs, "before", "w:spacing"),
            after: self.measure_twips(attrs, "after", "w:spacing"),
            line: self.measure_twips(attrs, "line", "w:spacing"),
            line_rule: wml_attr(attrs, "lineRule").and_then(LineSpacingRule::from_strict),
            before_lines: self.measure_i32(attrs, "beforeLines", "w:spacing"),
            after_lines: self.measure_i32(attrs, "afterLines", "w:spacing"),
            after_autospacing: attr_bool(attrs, "afterAutospacing"),
            before_autospacing: attr_bool(attrs, "beforeAutospacing"),
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
            start_chars: self
                .measure_i32(attrs, "startChars", "w:ind")
                .or_else(|| self.measure_i32(attrs, "leftChars", "w:ind")),
            end_chars: self
                .measure_i32(attrs, "endChars", "w:ind")
                .or_else(|| self.measure_i32(attrs, "rightChars", "w:ind")),
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
        self.nested(|parser| {
            let mut tabs = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == "tab" {
                            let alignment = parser
                                .enum_attr(&attrs, "val", "w:tab", TabAlignment::from_lexical)
                                .unwrap_or(TabAlignment::Start);
                            let leader =
                                parser.enum_attr(&attrs, "leader", "w:tab", TabLeader::from_strict);
                            let position = parser
                                .measure_twips(&attrs, "pos", "w:tab")
                                .unwrap_or(Twips(0));
                            tabs.push(TabStop {
                                position,
                                alignment,
                                leader,
                            });
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of tabs")),
                }
            }
            Ok(tabs)
        })
    }

    /// Parses the children of a `w:tblPr`.
    pub(crate) fn parse_table_properties(&mut self) -> Result<TableProperties> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = TableProperties {
                location: Some(location),
                ..TableProperties::default()
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
                            "tblStyle" => props.style = parser.val_string(&attrs).map(StyleId::new),
                            "tblStyleRowBandSize" => {
                                props.style_row_band_size = val_attr(&attrs).and_then(parse_i32);
                            }
                            "tblStyleColBandSize" => {
                                props.style_col_band_size = val_attr(&attrs).and_then(parse_i32);
                            }
                            "tblW" => props.width = Some(parser.parse_width(&attrs, "w:tblW")),
                            "jc" => {
                                props.alignment =
                                    parser.val_enum(&attrs, "w:jc", Justification::from_strict);
                            }
                            "tblLayout" => {
                                props.layout = parser.enum_attr(
                                    &attrs,
                                    "type",
                                    "w:tblLayout",
                                    TableLayout::from_strict,
                                );
                            }
                            "tblCellMar" => {
                                props.cell_margins = parser.parse_cell_margins()?;
                                continue;
                            }
                            "tblBorders" => {
                                props.borders = parser.parse_borders()?;
                                continue;
                            }
                            "shd" => props.shading = Some(parser.parse_shading(&attrs)),
                            "tblLook" => props.look = Some(Self::parse_table_look(&attrs)),
                            "tblInd" => {
                                props.indent = parser
                                    .measure_or_percent(&attrs, "w", "w:tblInd")
                                    .map(Twips);
                            }
                            "bidiVisual" => {
                                props.bidi_visual = parse_on_off(&attrs).unwrap_or(false);
                            }
                            "tblpPr" => {
                                props.positioning = Some(Self::parse_tblp_pr(&attrs, parser));
                                parser.record(
                                    "w:tblpPr",
                                    SupportStatus::Partial,
                                    Some("floating table kept in flow".to_owned()),
                                    Some(parser.location()),
                                );
                            }
                            "tblCellSpacing" => {
                                props.cell_spacing =
                                    Some(parser.parse_width(&attrs, "w:tblCellSpacing"));
                            }
                            "tblPrChange" => {
                                record_property_change(parser, "w:tblPrChange")?;
                                continue;
                            }
                            _ => {
                                record_unmodelled_property(parser, &name)?;
                                continue;
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of table properties"))
                    }
                }
            }
            Ok(props)
        })
    }

    /// Parses `w:trPr`.
    pub(crate) fn parse_row_properties(&mut self) -> Result<RowProperties> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = RowProperties {
                location: Some(location),
                ..RowProperties::default()
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
                            "trHeight" => props.height = Some(parser.parse_row_height(&attrs)),
                            "tblHeader" => {
                                props.header = parse_on_off(&attrs).unwrap_or(false);
                                if props.header {
                                    parser.record(
                                        "w:tblHeader",
                                        SupportStatus::Supported,
                                        None,
                                        Some(parser.location()),
                                    );
                                }
                            }
                            "cantSplit" => props.cant_split = parse_on_off(&attrs).unwrap_or(false),
                            "tblCellMar" => {
                                props.cell_margins = parser.parse_cell_margins()?;
                                continue;
                            }
                            "gridBefore" => {
                                props.grid_before = parser.val_i32(&attrs, "w:gridBefore");
                            }
                            "gridAfter" => props.grid_after = parser.val_i32(&attrs, "w:gridAfter"),
                            "wBefore" => {
                                props.width_before = Some(parser.parse_width(&attrs, "w:wBefore"));
                            }
                            "wAfter" => {
                                props.width_after = Some(parser.parse_width(&attrs, "w:wAfter"));
                            }
                            "jc" => {
                                props.alignment =
                                    parser.val_enum(&attrs, "w:jc", Justification::from_strict);
                            }
                            "tblCellSpacing" => {
                                props.cell_spacing =
                                    Some(parser.parse_width(&attrs, "w:tblCellSpacing"));
                            }
                            "rsid" => props.rsid = parser.val_string(&attrs),
                            "trPrChange" => {
                                record_property_change(parser, "w:trPrChange")?;
                                continue;
                            }
                            _ => {
                                record_unmodelled_property(parser, &name)?;
                                continue;
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of row properties")),
                }
            }
            Ok(props)
        })
    }

    /// Parses `w:tcPr`.
    pub(crate) fn parse_cell_properties(&mut self) -> Result<CellProperties> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = CellProperties {
                location: Some(location),
                ..CellProperties::default()
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
                            "tcW" => props.width = Some(parser.parse_width(&attrs, "w:tcW")),
                            "gridSpan" => {
                                props.grid_span = parser
                                    .val_u32(&attrs, "w:gridSpan")
                                    .map(|value| u16::try_from(value).unwrap_or(u16::MAX));
                                parser.record(
                                    "w:gridSpan",
                                    SupportStatus::Supported,
                                    None,
                                    Some(parser.location()),
                                );
                            }
                            "vMerge" => {
                                props.vertical_merge = Some(
                                    parser
                                        .val_enum(&attrs, "w:vMerge", VerticalMerge::from_strict)
                                        .unwrap_or(VerticalMerge::Continue),
                                );
                                parser.record(
                                    "w:vMerge",
                                    SupportStatus::Supported,
                                    None,
                                    Some(parser.location()),
                                );
                            }
                            "vAlign" => {
                                props.vertical_align =
                                    parser.val_enum(&attrs, "w:vAlign", VerticalJc::from_strict);
                            }
                            "textDirection" => {
                                props.text_direction = parser.val_enum(
                                    &attrs,
                                    "w:textDirection",
                                    TextDirection::from_strict,
                                );
                            }
                            "tcBorders" => {
                                props.borders = parser.parse_borders()?;
                                continue;
                            }
                            "shd" => props.shading = Some(parser.parse_shading(&attrs)),
                            "tcMar" => {
                                props.margins = parser.parse_cell_margins()?;
                                continue;
                            }
                            "hideMark" => props.hide_mark = parse_on_off(&attrs).unwrap_or(false),
                            "tcFitText" => props.fit_text = parse_on_off(&attrs).unwrap_or(false),
                            "noWrap" => props.no_wrap = parse_on_off(&attrs).unwrap_or(false),
                            "tcPrChange" => {
                                record_property_change(parser, "w:tcPrChange")?;
                                continue;
                            }
                            _ => {
                                record_unmodelled_property(parser, &name)?;
                                continue;
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of cell properties"))
                    }
                }
            }
            Ok(props)
        })
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
        self.nested(|parser| {
            let mut margins = CellMargins::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            let value = parser.measure_twips(&attrs, "w", "w:cellMar");
                            match name.local() {
                                "top" => margins.top = value,
                                "start" | "left" => margins.start = value,
                                "bottom" => margins.bottom = value,
                                "end" | "right" => margins.end = value,
                                _ => {
                                    record_unmodelled_property(parser, &name)?;
                                    continue;
                                }
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of cell margins")),
                }
            }
            Ok(margins)
        })
    }

    /// Parses a `w:tblLook` element.
    fn parse_table_look(attrs: &[Attr]) -> TableLook {
        let from_val = wml_attr(attrs, "val").and_then(|value| {
            let trimmed = value
                .strip_prefix("0x")
                .or_else(|| value.strip_prefix("0X"))
                .unwrap_or(value);
            u32::from_str_radix(trimmed, 16).ok()
        });
        let bit = |mask: u32, name: &str| {
            if wml_attr(attrs, name).is_some() {
                attr_on(attrs, name)
            } else {
                from_val.is_some_and(|value| value & mask != 0)
            }
        };
        TableLook {
            first_row: bit(0x0020, "firstRow"),
            last_row: bit(0x0040, "lastRow"),
            first_column: bit(0x0080, "firstColumn"),
            last_column: bit(0x0100, "lastColumn"),
            no_h_band: bit(0x0200, "noHBand"),
            no_v_band: bit(0x0400, "noVBand"),
        }
    }

    /// Parses `w:sectPr`.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_section_properties(&mut self) -> Result<SectionProperties> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = SectionProperties {
                location: Some(location),
                ..SectionProperties::default()
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
                            "headerReference" => {
                                if let Some(reference) = Self::parse_header_footer_ref(&attrs) {
                                    props.headers.push(reference);
                                    parser.record(
                                        "w:headerReference",
                                        SupportStatus::Supported,
                                        None,
                                        Some(parser.location()),
                                    );
                                } else {
                                    parser.record(
                                        "w:headerReference",
                                        SupportStatus::Partial,
                                        Some("unknown or incomplete headerReference".to_owned()),
                                        Some(parser.location()),
                                    );
                                }
                            }
                            "footerReference" => {
                                if let Some(reference) = Self::parse_header_footer_ref(&attrs) {
                                    props.footers.push(reference);
                                    parser.record(
                                        "w:footerReference",
                                        SupportStatus::Supported,
                                        None,
                                        Some(parser.location()),
                                    );
                                } else {
                                    parser.record(
                                        "w:footerReference",
                                        SupportStatus::Partial,
                                        Some("unknown or incomplete footerReference".to_owned()),
                                        Some(parser.location()),
                                    );
                                }
                            }
                            "type" => {
                                props.section_type =
                                    parser.val_enum(&attrs, "w:type", SectionType::from_strict);
                            }
                            "pgSz" => {
                                // AUD-73: missing or non-positive dimensions are
                                // kept as declared (or absent) but recorded as
                                // Partial; layout substitutes US Letter.
                                let width = parser.measure_twips(&attrs, "w", "w:pgSz");
                                let height = parser.measure_twips(&attrs, "h", "w:pgSz");
                                let width_bad = width.is_none_or(|twips| twips.value() <= 0);
                                let height_bad = height.is_none_or(|twips| twips.value() <= 0);
                                if width_bad || height_bad {
                                    parser.record(
                                        "w:pgSz",
                                        SupportStatus::Partial,
                                        Some(
                                            "non-positive or missing page size; using Letter"
                                                .to_owned(),
                                        ),
                                        Some(parser.location()),
                                    );
                                }
                                props.page_size = Some(PageSize {
                                    width,
                                    height,
                                    orientation: wml_attr(&attrs, "orient")
                                        .and_then(PageOrientation::from_strict),
                                    code: wml_attr(&attrs, "code")
                                        .and_then(|value| value.parse().ok()),
                                });
                            }
                            "pgMar" => {
                                props.page_margins = Some(PageMargins {
                                    top: parser.measure_twips(&attrs, "top", "w:pgMar"),
                                    right: parser.measure_twips(&attrs, "right", "w:pgMar"),
                                    bottom: parser.measure_twips(&attrs, "bottom", "w:pgMar"),
                                    left: parser.measure_twips(&attrs, "left", "w:pgMar"),
                                    header: parser.measure_twips(&attrs, "header", "w:pgMar"),
                                    footer: parser.measure_twips(&attrs, "footer", "w:pgMar"),
                                    gutter: parser.measure_twips(&attrs, "gutter", "w:pgMar"),
                                });
                            }
                            "cols" => {
                                props.columns = Some(parser.parse_columns(&attrs)?);
                                continue;
                            }
                            "titlePg" => props.title_page = parse_on_off(&attrs).unwrap_or(false),
                            "docGrid" => {
                                props.doc_grid = Some(DocGrid {
                                    grid_type: wml_attr(&attrs, "type")
                                        .and_then(DocGridType::from_strict),
                                    line_pitch: parser.measure_i32(
                                        &attrs,
                                        "linePitch",
                                        "w:docGrid",
                                    ),
                                    character_space: parser.measure_i32(
                                        &attrs,
                                        "charSpace",
                                        "w:docGrid",
                                    ),
                                });
                            }
                            "vAlign" => {
                                props.vertical_align =
                                    parser.val_enum(&attrs, "w:vAlign", VerticalJc::from_strict);
                            }
                            "bidi" => props.bidi = parse_on_off(&attrs).unwrap_or(false),
                            "rtlGutter" => props.rtl_gutter = parse_on_off(&attrs).unwrap_or(false),
                            // Transitional's `EG_SectPrContents` declares `w:gutterAtTop`
                            // and Strict's does not; `w:settings` is where Strict puts
                            // it. Parked on the parser and merged into the settings.
                            "gutterAtTop" => {
                                parser.section_gutter_at_top =
                                    parse_on_off(&attrs).unwrap_or(false);
                            }
                            "textDirection" => {
                                props.text_direction = parser.val_enum(
                                    &attrs,
                                    "w:textDirection",
                                    TextDirection::from_strict,
                                );
                            }
                            "lnNumType" => {
                                props.line_numbering = Some(parser.parse_line_numbering(&attrs));
                            }
                            "pgNumType" => {
                                props.page_number = Some(Self::parse_pg_num_type(&attrs, parser));
                            }
                            "footnotePr" => {
                                props.footnote_properties = parser.parse_note_properties()?;
                                continue;
                            }
                            "endnotePr" => {
                                props.endnote_properties = parser.parse_note_properties()?;
                                continue;
                            }
                            "pgBorders" => {
                                props.page_borders = Some(parser.parse_page_borders(&attrs)?);
                                parser.record(
                                    "w:pgBorders",
                                    SupportStatus::Supported,
                                    None,
                                    Some(parser.location()),
                                );
                                continue;
                            }
                            "sectPrChange" => {
                                record_property_change(parser, "w:sectPrChange")?;
                                continue;
                            }
                            _ => {
                                record_unmodelled_property(parser, &name)?;
                                continue;
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of section properties"))
                    }
                }
            }
            Ok(props)
        })
    }

    /// Parses `w:pgBorders` (start element consumed).
    fn parse_page_borders(&mut self, attrs: &[Attr]) -> Result<PageBorders> {
        let mut borders = PageBorders {
            offset_from: wml_attr(attrs, "offsetFrom").and_then(BorderOffsetFrom::from_strict),
            z_order: wml_attr(attrs, "zOrder").and_then(BorderZOrder::from_strict),
            ..PageBorders::default()
        };
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            let edge = parser.parse_page_border_edge(&attrs);
                            match name.local() {
                                "top" => borders.top = Some(edge),
                                "left" => borders.left = Some(edge),
                                "bottom" => borders.bottom = Some(edge),
                                "right" => borders.right = Some(edge),
                                _ => {
                                    record_unmodelled_property(parser, &name)?;
                                    continue;
                                }
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of page borders")),
                }
            }
            Ok(borders)
        })
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
            theme_color: theme_color_ref(self, attrs, "themeColor", "themeTint", "themeShade")
                .map(Box::new),
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
        self.nested(|parser| {
            let equal_width = match wml_attr(attrs, "equalWidth") {
                None => true,
                Some(value) => matches!(value, "true" | "on" | "1"),
            };
            let mut columns = Columns {
                count: parser.measure_u16(attrs, "num", "w:cols"),
                space: parser.measure_twips(attrs, "space", "w:cols"),
                equal_width,
                separator: attr_on(attrs, "sep"),
                columns: Vec::new(),
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == "col" {
                            columns.columns.push(ColumnSpec {
                                width: parser.measure_twips(&attrs, "w", "w:col"),
                                space: parser.measure_twips(&attrs, "space", "w:col"),
                            });
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of columns")),
                }
            }
            Ok(columns)
        })
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

    /// Parses `w:pgNumType` attributes (AUD-46).
    fn parse_pg_num_type(attrs: &[Attr], parser: &mut PartParser<'_>) -> PageNumberType {
        PageNumberType {
            format: wml_attr(attrs, "fmt").map(|value| parser.intern(value)),
            start: wml_attr(attrs, "start").and_then(parse_u32),
            chapter_style: wml_attr(attrs, "chapStyle")
                .and_then(parse_u32)
                .map(|value| u8::try_from(value.min(9)).unwrap_or(9)),
            chapter_separator: wml_attr(attrs, "chapSep").map(|value| parser.intern(value)),
        }
    }

    /// Parses `w:tblpPr` attributes (AUD-46).
    fn parse_tblp_pr(attrs: &[Attr], parser: &mut PartParser<'_>) -> TablePositioning {
        TablePositioning {
            left_from_text: parser.measure_twips(attrs, "leftFromText", "w:tblpPr"),
            right_from_text: parser.measure_twips(attrs, "rightFromText", "w:tblpPr"),
            top_from_text: parser.measure_twips(attrs, "topFromText", "w:tblpPr"),
            bottom_from_text: parser.measure_twips(attrs, "bottomFromText", "w:tblpPr"),
            vert_anchor: wml_attr(attrs, "vertAnchor").map(|value| parser.intern(value)),
            horz_anchor: wml_attr(attrs, "horzAnchor").map(|value| parser.intern(value)),
            x_align: wml_attr(attrs, "tblpXSpec").map(|value| parser.intern(value)),
            x: parser.measure_i32(attrs, "tblpX", "w:tblpPr"),
            y_align: wml_attr(attrs, "tblpYSpec").map(|value| parser.intern(value)),
            y: parser.measure_i32(attrs, "tblpY", "w:tblpPr"),
        }
    }

    /// Parses `w:framePr` attributes (AUD-46).
    fn parse_frame_pr(attrs: &[Attr], parser: &mut PartParser<'_>) -> FrameProperties {
        FrameProperties {
            drop_cap: wml_attr(attrs, "dropCap").map(|value| parser.intern(value)),
            lines: wml_attr(attrs, "lines")
                .and_then(parse_u32)
                .map(|value| u8::try_from(value.min(255)).unwrap_or(255)),
            width: parser.measure_twips(attrs, "w", "w:framePr"),
            height: parser.measure_twips(attrs, "h", "w:framePr"),
            v_space: parser.measure_twips(attrs, "vSpace", "w:framePr"),
            h_space: parser.measure_twips(attrs, "hSpace", "w:framePr"),
            wrap: wml_attr(attrs, "wrap").map(|value| parser.intern(value)),
            h_anchor: wml_attr(attrs, "hAnchor").map(|value| parser.intern(value)),
            v_anchor: wml_attr(attrs, "vAnchor").map(|value| parser.intern(value)),
            x: parser.measure_i32(attrs, "x", "w:framePr"),
            x_align: wml_attr(attrs, "xAlign").map(|value| parser.intern(value)),
            y: parser.measure_i32(attrs, "y", "w:framePr"),
            y_align: wml_attr(attrs, "yAlign").map(|value| parser.intern(value)),
            height_rule: wml_attr(attrs, "hRule").map(|value| parser.intern(value)),
            anchor_lock: wml_attr(attrs, "anchorLock")
                .is_some_and(|value| matches!(value, "true" | "on" | "1")),
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

/// Reads a theme-colour slot plus optional tint/shade attributes into a ref.
fn theme_color_ref(
    parser: &mut PartParser<'_>,
    attrs: &[Attr],
    color: &str,
    tint: &str,
    shade: &str,
) -> Option<ThemeColorRef> {
    let slot = wml_attr(attrs, color)?;
    Some(ThemeColorRef {
        color: ThemeColor::new(parser.intern(slot)),
        tint: wml_attr(attrs, tint).map(|value| parser.intern(value)),
        shade: wml_attr(attrs, shade).map(|value| parser.intern(value)),
    })
}
