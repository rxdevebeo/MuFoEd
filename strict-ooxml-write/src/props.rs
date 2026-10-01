//! Property containers: `w:pPr`, `w:rPr`, table properties and `w:sectPr`.
//!
//! Element order inside these containers is **not** free. ISO/IEC 29500-1
//! declares them as `xsd:sequence`, so a writer that emits them in the order
//! that happens to be convenient produces a part that fails validation even
//! though every value is right.
//!
//! The order itself is not written out in each function. It lives in
//! [`crate::order`], transcribed from `strict/wml.xsd`, and every container here
//! emits through [`schema_child`], which asks that table where a child goes
//! instead of relying on the order the statements below happen to appear in.
//! `STAGE-10G-TASK.md` G21 asks for exactly this: the order comes from the
//! schema, in one place, rather than being repaired one misordered element at a
//! time - because every property added to the model in the wrong spot was a new
//! violation, and the two elements that happened to be measured were the only
//! ones anyone knew about.

use strict_ooxml_wml::model::notes::NoteProperties;
use strict_ooxml_wml::model::props::{
    CellProperties, Columns, DocGrid, HeaderFooterKind, HeaderFooterRef, PageBorder, PageBorders,
    PageMargins, PageSize, ParagraphProperties, RowProperties, RunProperties, SectionProperties,
    TableProperties,
};
use strict_ooxml_wml::model::values::{
    Border, Borders, CellMargins, Color, Fonts, HighlightOrColor, Indentation, Shading, Spacing,
    TabStop, ThemeColorRef, TriState, Width, WidthKind,
};

use crate::ctx::Ctx;
use crate::order;
use crate::xml::XmlWriter;

/// Emits a container's children in the order `sequence` declares.
///
/// `write` is called once per name the sequence declares, in that order, and
/// writes that child when the model has one. Nothing is sorted at run time and
/// nothing is collected: the order IS the loop, so the code below cannot express
/// a child in the wrong position - it can only say what each child is.
///
/// A name the writer does not implement is simply never written, which is what
/// lets the table be transcribed in full from the schema (`w:settings` has
/// ninety-four children and the writer produces a dozen) without the writer
/// having to know about the other eighty-two.
fn schema_order(
    xml: &mut XmlWriter,
    sequence: &[&str],
    mut write: impl FnMut(&str, &mut XmlWriter),
) {
    for name in sequence {
        write(name, xml);
    }
}

/// Writes `w:pPr`, or nothing when no property is set.
///
/// `ctx` is needed for the one property that names something this write moved:
/// a `w:sectPr` inside `w:pPr` carries header/footer relationship ids, and the
/// ids in the parsed document are the *source* package's. Writing them
/// unchanged points the reference at whatever this write put at that number —
/// in practice a hyperlink — so the header or footer disappears on the next
/// open, silently, because a footer that resolves to nothing is simply not
/// drawn.
pub fn paragraph_properties(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, props: &ParagraphProperties) {
    if is_empty_paragraph(props) {
        return;
    }
    xml.start("w:pPr");
    schema_order(xml, order::PPR, |name, xml| {
        paragraph_child(ctx, xml, props, name);
    });
    xml.end();
}

/// Writes the `w:pPr` child called `name`, when the model has it.
fn paragraph_child(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    props: &ParagraphProperties,
    name: &str,
) {
    match name {
        "pStyle" => {
            if let Some(style) = &props.style {
                xml.empty_attr_w("w:pStyle", "val", style.as_str());
            }
        }
        "keepNext" if props.keep_next => xml.empty("w:keepNext"),
        "keepLines" if props.keep_lines => xml.empty("w:keepLines"),
        "pageBreakBefore" if props.page_break_before => xml.empty("w:pageBreakBefore"),
        "widowControl" => match props.widow_control {
            TriState::On => xml.empty_attr_w("w:widowControl", "val", "true"),
            TriState::Off => xml.empty_attr_w("w:widowControl", "val", "false"),
            TriState::Absent => {}
        },
        "numPr" => {
            if let Some(numbering) = &props.numbering {
                if numbering.num_id.is_some() || numbering.ilvl.is_some() {
                    xml.start("w:numPr");
                    if let Some(ilvl) = numbering.ilvl {
                        xml.empty_attr_w("w:ilvl", "val", ilvl.0);
                    }
                    if let Some(num_id) = numbering.num_id {
                        xml.empty_attr_w("w:numId", "val", num_id.0);
                    }
                    xml.end();
                }
            }
        }
        "suppressLineNumbers" if props.suppress_line_numbers => {
            xml.empty("w:suppressLineNumbers");
        }
        "pBdr" => {
            if !borders_empty(&props.borders) {
                borders_element(xml, "w:pBdr", &props.borders, EdgeNames::Paragraph);
            }
        }
        "shd" => {
            if let Some(shading) = &props.shading {
                shading_element(xml, shading);
            }
        }
        "tabs" => {
            if !props.tabs.is_empty() {
                xml.start("w:tabs");
                for tab in &props.tabs {
                    tab_stop(xml, tab);
                }
                xml.end();
            }
        }
        "wordWrap" => match props.word_wrap {
            TriState::On => xml.empty_attr_w("w:wordWrap", "val", "true"),
            TriState::Off => xml.empty_attr_w("w:wordWrap", "val", "false"),
            TriState::Absent => {}
        },
        "bidi" if props.bidi => xml.empty("w:bidi"),
        "snapToGrid" => match props.snap_to_grid {
            TriState::On => xml.empty_attr_w("w:snapToGrid", "val", "true"),
            TriState::Off => xml.empty_attr_w("w:snapToGrid", "val", "false"),
            TriState::Absent => {}
        },
        "spacing" => {
            if let Some(spacing) = &props.spacing {
                spacing_element(xml, spacing);
            }
        }
        "ind" => {
            if let Some(indentation) = &props.indentation {
                indentation_element(xml, indentation);
            }
        }
        "contextualSpacing" if props.contextual_spacing => xml.empty("w:contextualSpacing"),
        "jc" => {
            if let Some(alignment) = &props.alignment {
                xml.empty_attr_w("w:jc", "val", alignment.as_str());
            }
        }
        "textDirection" => {
            if let Some(direction) = &props.text_direction {
                xml.empty_attr_w("w:textDirection", "val", direction.as_str());
            }
        }
        "outlineLvl" => {
            if let Some(level) = props.outline_level {
                xml.empty_attr_w("w:outlineLvl", "val", level);
            }
        }
        "rPr" => {
            if let Some(run_props) = &props.run_props {
                // `w:rPr` here marks the paragraph mark itself. It is written only
                // when it carries something: an empty `<w:rPr/>` is legal and means
                // nothing, and the reader does not read one back - so writing it
                // made the document change on every round trip. `w:del` inside a
                // paragraph mark's `w:rPr` is a revision marker this model does not
                // keep, and the emptiness check is what stops it from masquerading
                // as content that survived.
                if !is_empty_run(run_props) {
                    run_properties(xml, run_props);
                }
            }
        }
        "sectPr" => {
            if let Some(section) = &props.section {
                section_properties(ctx, xml, section);
            }
        }
        _ => {}
    }
}

/// Returns `true` when writing `props` would produce an empty `w:pPr`.
///
/// An empty `w:pPr` is legal but pointless, and emitting one would make a
/// round trip differ from a document that simply had none.
fn is_empty_paragraph(props: &ParagraphProperties) -> bool {
    props.style.is_none()
        && props.alignment.is_none()
        // A `w:numPr` with neither `w:numId` nor `w:ilvl` writes nothing, so it
        // must not count as content: it used to, and a producer that left an empty
        // `<w:numPr/>` in the markup got a `<w:pPr/>` back - legal, pointless, and
        // the reason a written document did not settle in one generation.
        && props
            .numbering
            .as_ref()
            .is_none_or(|numbering| numbering.num_id.is_none() && numbering.ilvl.is_none())
        && props.spacing.is_none()
        && props.indentation.is_none()
        && borders_empty(&props.borders)
        && props.shading.is_none()
        && props.tabs.is_empty()
        && !props.keep_next
        && !props.keep_lines
        && !props.page_break_before
        && props.widow_control == TriState::Absent
        && props.outline_level.is_none()
        && !props.bidi
        && props.run_props.as_ref().is_none_or(is_empty_run)
        && props.section.is_none()
        && props.text_direction.is_none()
        && !props.suppress_line_numbers
        && !props.contextual_spacing
        && props.word_wrap == TriState::Absent
        && props.snap_to_grid == TriState::Absent
}

fn spacing_element(xml: &mut XmlWriter, spacing: &Spacing) {
    xml.start("w:spacing");
    xml.attr_w_opt("before", spacing.before.map(|v| v.0));
    xml.attr_w_opt("after", spacing.after.map(|v| v.0));
    xml.attr_w_opt("line", spacing.line.map(|v| v.0));
    if let Some(rule) = &spacing.line_rule {
        xml.attr_w("lineRule", rule.as_str());
    }
    if spacing.before_autospacing {
        xml.attr_w("beforeAutospacing", "true");
    }
    if spacing.after_autospacing {
        xml.attr_w("afterAutospacing", "true");
    }
    xml.end();
}

fn indentation_element(xml: &mut XmlWriter, indentation: &Indentation) {
    xml.start("w:ind");
    // Strict uses the direction-neutral `start`/`end` pair; `left`/`right`
    // would be Transitional (stage-6 T3).
    xml.attr_w_opt("start", indentation.start.map(|v| v.0));
    xml.attr_w_opt("end", indentation.end.map(|v| v.0));
    xml.attr_w_opt("firstLine", indentation.first_line.map(|v| v.0));
    xml.attr_w_opt("hanging", indentation.hanging.map(|v| v.0));
    xml.attr_w_opt("startChars", indentation.start_chars);
    xml.attr_w_opt("endChars", indentation.end_chars);
    xml.attr_w_opt("firstLineChars", indentation.first_line_chars);
    xml.attr_w_opt("hangingChars", indentation.hanging_chars);
    xml.end();
}

fn tab_stop(xml: &mut XmlWriter, tab: &TabStop) {
    xml.start("w:tab");
    xml.attr_w("val", tab.alignment.as_str());
    xml.attr_w("pos", tab.position.0);
    if let Some(leader) = &tab.leader {
        xml.attr_w("leader", leader.as_str());
    }
    xml.end();
}

/// Writes `w:rPr`.
///
/// `w:rStyle` is written at every position this is called from. The function
/// used to take a flag suppressing it for "the run properties of a paragraph
/// mark", on the stated grounds that `CT_ParaRPr` does not allow it. It does:
/// §17.3.1.29 defines `CT_ParaRPr` as the revision-tracking group followed by
/// `EG_RPrBase`, which begins with `rStyle`. The same is true of `m:ctrlPr` and
/// of `m:r/w:rPr`. A Word footer in the local corpus carries
/// `<w:pPr><w:rPr><w:rStyle w:val="a5"/></w:rPr></w:pPr>`, so the
/// suppression dropped a style Word had written and left an empty `<w:rPr/>`
/// behind — which the next parse did not read back, so the note never settled.
pub fn run_properties(xml: &mut XmlWriter, props: &RunProperties) {
    if is_empty_run(props) {
        return;
    }
    xml.start("w:rPr");

    if let Some(style) = &props.style {
        xml.empty_attr_w("w:rStyle", "val", style.as_str());
    }
    if let Some(fonts) = &props.fonts {
        fonts_element(xml, fonts);
    }
    toggle(xml, "w:b", props.bold);
    toggle(xml, "w:i", props.italic);
    if props.caps {
        xml.empty("w:caps");
    }
    if props.small_caps {
        xml.empty("w:smallCaps");
    }
    toggle(xml, "w:strike", props.strike);
    toggle(xml, "w:dstrike", props.double_strike);
    if props.outline {
        xml.empty("w:outline");
    }
    if props.shadow {
        xml.empty("w:shadow");
    }
    if props.emboss {
        xml.empty("w:emboss");
    }
    if props.imprint {
        xml.empty("w:imprint");
    }
    if props.no_proof {
        xml.empty("w:noProof");
    }
    toggle(xml, "w:snapToGrid", props.snap_to_grid);
    if props.vanish {
        xml.empty("w:vanish");
    }
    if props.rtl {
        xml.empty("w:rtl");
    }
    if let Some(color) = &props.color {
        color_element(xml, "w:color", color, props.color_theme.as_ref());
    }
    if let Some(spacing) = props.spacing.filter(|v| v.0 != 0) {
        xml.empty_attr_w("w:spacing", "val", spacing.0);
    }
    if let Some(width) = props.scale {
        xml.empty_attr_w("w:w", "val", width);
    }
    if let Some(kerning) = props.kerning {
        xml.empty_attr_w("w:kern", "val", kerning.0);
    }
    if let Some(position) = props.position {
        xml.empty_attr_w("w:position", "val", position.0);
    }
    if let Some(size) = props.size {
        xml.empty_attr_w("w:sz", "val", size.0);
    }
    if let Some(size) = props.size_cs {
        xml.empty_attr_w("w:szCs", "val", size.0);
    }
    if let Some(highlight) = &props.highlight {
        xml.empty_attr_w("w:highlight", "val", highlight.as_str());
    }
    if let Some(underline) = &props.underline {
        xml.start("w:u");
        xml.attr_w("val", underline.as_str());
        if let Some(color) = &props.underline_color {
            xml.attr_w("color", color.as_str());
        }
        xml.end();
    }
    if !borders_empty(&props.borders) {
        run_border(xml, &props.borders);
    }
    if let Some(shading) = &props.shading {
        shading_element(xml, shading);
    }
    if let Some(vert_align) = &props.vert_align {
        xml.empty_attr_w("w:vertAlign", "val", vert_align.as_str());
    }
    if let Some(emphasis) = &props.emphasis {
        xml.empty_attr_w("w:em", "val", emphasis.as_ref());
    }
    if let Some(language) = &props.language {
        language_element(xml, language);
    }
    xml.end();
}

fn language_element(xml: &mut XmlWriter, language: &strict_ooxml_wml::model::props::Language) {
    xml.start("w:lang");
    xml.attr_w_opt("val", language.val.as_deref());
    xml.attr_w_opt("eastAsia", language.east_asia.as_deref());
    xml.attr_w_opt("bidi", language.bidi.as_deref());
    xml.end();
}

fn is_empty_run(props: &RunProperties) -> bool {
    props.style.is_none()
        && props.fonts.is_none()
        && props.bold == TriState::Absent
        && props.italic == TriState::Absent
        && props.underline.is_none()
        && props.strike == TriState::Absent
        && props.double_strike == TriState::Absent
        && props.color.is_none()
        && props.color_theme.is_none()
        && props.highlight.is_none()
        && props.size.is_none()
        && props.size_cs.is_none()
        && props.vert_align.is_none()
        && props.spacing.is_none()
        && props.position.is_none()
        && !props.caps
        && !props.small_caps
        && !props.rtl
        && !props.vanish
        && !props.emboss
        && !props.imprint
        && !props.outline
        && !props.shadow
        && !props.no_proof
        && props.snap_to_grid == TriState::Absent
        && props.scale.is_none()
        && props.kerning.is_none()
        && props.emphasis.is_none()
        && props.language.is_none()
        && borders_empty(&props.borders)
        && props.shading.is_none()
}

/// Writes a tri-state toggle, mapping `Inherit` to nothing.
fn toggle(xml: &mut XmlWriter, name: &str, state: TriState) {
    match state {
        TriState::On => xml.empty_attr(name, "w:val", "true"),
        TriState::Off => xml.empty_attr(name, "w:val", "false"),
        TriState::Absent => {}
    }
}

fn fonts_element(xml: &mut XmlWriter, fonts: &Fonts) {
    xml.start("w:rFonts");
    xml.attr_w_opt("ascii", fonts.ascii.as_deref());
    xml.attr_w_opt("hAnsi", fonts.h_ansi.as_deref());
    xml.attr_w_opt("eastAsia", fonts.east_asia.as_deref());
    xml.attr_w_opt("cs", fonts.complex_script.as_deref());
    xml.attr_w_opt("hint", fonts.hint.as_deref());
    xml.attr_w_opt("asciiTheme", fonts.ascii_theme.as_deref());
    xml.attr_w_opt("hAnsiTheme", fonts.h_ansi_theme.as_deref());
    xml.attr_w_opt("eastAsiaTheme", fonts.east_asia_theme.as_deref());
    xml.attr_w_opt("cstheme", fonts.cs_theme.as_deref());
    xml.end();
}

fn color_element(xml: &mut XmlWriter, name: &str, color: &Color, theme: Option<&ThemeColorRef>) {
    xml.start(name);
    xml.attr_w("val", color.as_str());
    if let Some(theme) = theme {
        xml.attr_w("themeColor", theme.color.as_str());
        xml.attr_w_opt("themeTint", theme.tint.as_deref());
        xml.attr_w_opt("themeShade", theme.shade.as_deref());
    }
    xml.end();
}

fn shading_element(xml: &mut XmlWriter, shading: &Shading) {
    xml.start("w:shd");
    xml.attr_w("val", shading.pattern.as_deref().unwrap_or("clear"));
    xml.attr_w_opt("color", shading.color.as_ref().map(Color::as_str));
    xml.attr_w_opt("fill", shading.fill.as_ref().map(Color::as_str));
    xml.end();
}

fn borders_empty(borders: &Borders) -> bool {
    borders.top.is_none()
        && borders.bottom.is_none()
        && borders.start.is_none()
        && borders.end.is_none()
        && borders.inside_horizontal.is_none()
        && borders.inside_vertical.is_none()
}

/// Writes a border container (`w:pBdr`, `w:tblBorders`, `w:tcBorders`).
///
/// The horizontal edges have TWO names in Strict, and which one is correct depends
/// on the container - a fact the schema makes impossible to miss and the writer
/// got backwards in both directions:
///
/// | container | sequence | horizontal edges |
/// |---|---|---|
/// | `CT_PBdr` (`w:pBdr`) | top, left, bottom, right, between, bar | `left`/`right` |
/// | `CT_TblBorders` | top, start, bottom, end, insideH, insideV | `start`/`end` |
/// | `CT_TcBorders` | the same, plus tl2br, tr2bl | `start`/`end` |
///
/// So a paragraph border written `start`/`end` and a table border written
/// `left`/`right` are each invalid, and the writer used to write `start`/`end`
/// everywhere on the strength of "Strict renamed left/right" - true of
/// `CT_TblBorders` and false of `CT_PBdr`, which Strict did not rename. Ten
/// violations on the corpus (`XS-23`).
///
/// `w:bdr` is not in this function because it is not a container at all:
/// `w:rPr/w:bdr` is a single `CT_Border`, an element with attributes and no
/// children. See [`run_border`].
fn borders_element(xml: &mut XmlWriter, name: &str, borders: &Borders, edges: EdgeNames) {
    xml.start(name);
    let (near, far) = match edges {
        EdgeNames::Paragraph => ("left", "right"),
        EdgeNames::Table => ("start", "end"),
    };
    for (local, edge) in [
        ("top", &borders.top),
        (near, &borders.start),
        ("bottom", &borders.bottom),
        (far, &borders.end),
        ("insideH", &borders.inside_horizontal),
        ("insideV", &borders.inside_vertical),
    ] {
        if let Some(edge) = edge {
            border_edge(xml, local, edge);
        }
    }
    xml.end();
}

/// Which spelling the horizontal edges take in the container being written.
#[derive(Clone, Copy)]
enum EdgeNames {
    /// `CT_PBdr` keeps `left`/`right`.
    Paragraph,
    /// `CT_TblBorders` and `CT_TcBorders` use `start`/`end`.
    Table,
}

/// Writes `w:rPr/w:bdr`, which is one edge and not a container.
///
/// `CT_Border` declares no child elements at all - `w:val`, `w:sz`, `w:space` and
/// `w:color` are attributes on the element itself. Writing a border COLLECTION
/// into it, as this writer did, can only ever produce markup no reader accepts.
///
/// The model holds a four-edge `Borders` here and there is nowhere in `CT_Border`
/// to put three of them, so the `top` edge is written and the other three are not
/// written back. That is a loss, and it is already declared where the information
/// disappears: the reader records `w:bdr` as `Partial` with the reason "run
/// borders are not retained" (`wml/src/parse/props.rs`), so a run border never
/// reaches this code at all through a round trip.
fn run_border(xml: &mut XmlWriter, borders: &Borders) {
    let Some(edge) = borders.top.as_ref() else {
        return;
    };
    xml.start("w:bdr");
    write_border_attributes(xml, edge);
    xml.end();
}

fn border_edge(xml: &mut XmlWriter, local: &str, border: &Border) {
    xml.start(&format!("w:{local}"));
    write_border_attributes(xml, border);
    xml.end();
}

fn write_border_attributes(xml: &mut XmlWriter, border: &Border) {
    xml.attr_w("val", border.style.map_or("none", |s| s.as_str()));
    xml.attr_w_opt("sz", border.size.map(|v| v.0));
    xml.attr_w_opt("space", border.space);
    xml.attr_w_opt("color", border.color.as_ref().map(Color::as_str));
    if border.shadow {
        xml.attr_w("shadow", "true");
    }
    if border.frame {
        xml.attr_w("frame", "true");
    }
}

fn cell_margins(xml: &mut XmlWriter, name: &str, margins: &CellMargins) {
    let empty = margins.top.is_none()
        && margins.start.is_none()
        && margins.bottom.is_none()
        && margins.end.is_none();
    if empty {
        return;
    }
    xml.start(name);
    for (local, value) in [
        ("top", margins.top),
        ("start", margins.start),
        ("bottom", margins.bottom),
        ("end", margins.end),
    ] {
        if let Some(value) = value {
            xml.start(&format!("w:{local}"));
            xml.attr("w:w", tbl_width_value(value.0));
            xml.attr_w("type", "dxa");
            xml.end();
        }
    }
    xml.end();
}

/// The lexical form of a `CT_TblWidth/@w` measurement (`XS-09`).
///
/// The attribute's type is `ST_MeasurementOrPercent`, a union of
/// `ST_DecimalNumberOrPercent` and `s:ST_UniversalMeasure`. The first of those is
/// itself only `s:ST_Percentage`, whose pattern is `-?[0-9]+(\.[0-9]+)?%` - it
/// requires the sign and it has no plain-number branch. So a bare twip count is
/// not a value this attribute can hold, and 110 corpus violations were exactly
/// that.
///
/// The branch that does fit is the universal measure, whose pattern ends in
/// `mm|cm|in|pt|pc|pi`, and it is not a workaround: it is what every producer that
/// writes valid Strict writes. LibreOffice, docx4j and the Open XML SDK fixtures
/// in the corpus all put points here - `w:tcW w:w="178.05pt" w:type="dxa"` - while
/// the eight Microsoft conformance fixtures write `w:w="4788"`, which their own
/// schema rejects. That is the difference between a producer that validates and
/// one that does not, and it is 20 twips to a point.
///
/// Points are exact for twips: the model holds integers, and an integer divided
/// by 20 has at most two decimals.
///
/// [`WidthKind::Pct`] keeps its fiftieths-of-a-percent number bare, because that
/// is the value every producer writes and no fixture in the corpus uses one; the
/// schema's percentage branch would need `w:w="50%"`, and whether a consumer
/// reads that as 50% or as 50 fiftieths-of-a-percent is exactly the kind of
/// question this project answers by measurement (`Q-E5`), not by guessing.
fn tbl_width_value(twips: i32) -> String {
    let points = f64::from(twips) / 20.0;
    let mut text = format!("{points}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    format!("{text}pt")
}

fn width_element(xml: &mut XmlWriter, name: &str, width: &Width) {
    xml.start(name);
    // §17.4.64 `w:tblW` and §17.4.70 `w:tcW` both carry two attributes: the
    // unit in `w:type` and the measurement in `w:w`. The writer wrote the unit
    // as `w:w` as well, so every table and every cell produced
    // `<w:tblW w:w="dxa" w:w="17663"/>` — a repeated attribute, which is a hard
    // XML error, not a tolerated one. Eight documents in the local corpus
    // failed to reparse for exactly this and nothing else.
    xml.attr_w("type", width_kind(width.kind));
    if let Some(value) = width.value {
        if width.kind == WidthKind::Pct {
            // `pct` counts fiftieths of a percent; see [`tbl_width_value`].
            xml.attr_w("w", value);
        } else {
            xml.attr("w:w", tbl_width_value(value));
        }
    }
    xml.end();
}

fn width_kind(kind: WidthKind) -> &'static str {
    match kind {
        WidthKind::Auto => "auto",
        WidthKind::Dxa => "dxa",
        WidthKind::Pct => "pct",
        WidthKind::Nil => "nil",
    }
}

/// Writes `w:tblPr`.
pub fn table_properties(xml: &mut XmlWriter, props: &TableProperties) {
    if props.style.is_none()
        && props.width.is_none()
        && props.alignment.is_none()
        && props.layout.is_none()
        && props.shading.is_none()
        && props.look.is_none()
        && props.indent.is_none()
        && !props.bidi_visual
        && borders_empty(&props.borders)
        && props.cell_margins.top.is_none()
        && props.cell_margins.start.is_none()
        && props.cell_margins.bottom.is_none()
        && props.cell_margins.end.is_none()
    {
        return;
    }
    xml.start("w:tblPr");
    schema_order(xml, order::TBLPR, |name, xml| {
        table_child(xml, props, name);
    });
    xml.end();
}

/// Writes the `w:tblPr` child called `name`, when the model has it.
fn table_child(xml: &mut XmlWriter, props: &TableProperties, name: &str) {
    match name {
        "tblStyle" => {
            if let Some(style) = &props.style {
                xml.empty_attr_w("w:tblStyle", "val", style.as_str());
            }
        }
        "bidiVisual" if props.bidi_visual => xml.empty("w:bidiVisual"),
        "tblW" => {
            if let Some(width) = &props.width {
                width_element(xml, "w:tblW", width);
            }
        }
        "jc" => {
            if let Some(alignment) = &props.alignment {
                xml.empty_attr_w("w:jc", "val", alignment.as_str());
            }
        }
        "tblInd" => {
            if let Some(indent) = props.indent {
                // `w:tblInd` is a `CT_TblWidth` too, so it takes the same lexical
                // form as a table width - see [`tbl_width_value`].
                xml.start("w:tblInd");
                xml.attr("w:w", tbl_width_value(indent.0));
                xml.attr_w("type", "dxa");
                xml.end();
            }
        }
        "tblBorders" => {
            if !borders_empty(&props.borders) {
                borders_element(xml, "w:tblBorders", &props.borders, EdgeNames::Table);
            }
        }
        "shd" => {
            if let Some(shading) = &props.shading {
                shading_element(xml, shading);
            }
        }
        "tblLayout" => {
            if let Some(layout) = &props.layout {
                xml.empty_attr_w("w:tblLayout", "type", layout.as_str());
            }
        }
        "tblCellMar" => cell_margins(xml, "w:tblCellMar", &props.cell_margins),
        "tblLook" => {
            if let Some(look) = &props.look {
                xml.start("w:tblLook");
                xml.attr_w("firstRow", bool_str(look.first_row));
                xml.attr_w("lastRow", bool_str(look.last_row));
                xml.attr_w("firstColumn", bool_str(look.first_column));
                xml.attr_w("lastColumn", bool_str(look.last_column));
                xml.attr_w("noHBand", bool_str(look.no_h_band));
                xml.attr_w("noVBand", bool_str(look.no_v_band));
                xml.end();
            }
        }
        _ => {}
    }
}

/// Writes `w:trPr`.
pub fn row_properties(xml: &mut XmlWriter, props: &RowProperties) {
    if props.height.is_none()
        && !props.header
        && !props.cant_split
        && props.grid_before.is_none()
        && props.grid_after.is_none()
        && props.width_before.is_none()
        && props.width_after.is_none()
        && props.cell_margins.top.is_none()
        && props.cell_margins.start.is_none()
        && props.cell_margins.bottom.is_none()
        && props.cell_margins.end.is_none()
    {
        return;
    }
    xml.start("w:trPr");
    schema_order(xml, order::TRPR, |name, xml| {
        row_child(xml, props, name);
    });
    xml.end();
}

/// Writes the `w:trPr` child called `name`, when the model has it.
fn row_child(xml: &mut XmlWriter, props: &RowProperties, name: &str) {
    match name {
        "gridBefore" => {
            if let Some(before) = props.grid_before {
                xml.empty_attr_w("w:gridBefore", "val", before);
            }
        }
        "gridAfter" => {
            if let Some(after) = props.grid_after {
                xml.empty_attr_w("w:gridAfter", "val", after);
            }
        }
        "wBefore" => {
            if let Some(width) = &props.width_before {
                width_element(xml, "w:wBefore", width);
            }
        }
        "wAfter" => {
            if let Some(width) = &props.width_after {
                width_element(xml, "w:wAfter", width);
            }
        }
        "cantSplit" if props.cant_split => xml.empty("w:cantSplit"),
        "trHeight" => {
            if let Some(height) = &props.height {
                xml.start("w:trHeight");
                xml.attr_w_opt("val", height.value.map(|v| v.0));
                if let Some(rule) = &height.rule {
                    xml.attr_w("hRule", rule.as_str());
                }
                xml.end();
            }
        }
        "tblHeader" if props.header => xml.empty("w:tblHeader"),
        _ => {}
    }
}

/// Writes `w:tcPr`.
pub fn cell_properties(xml: &mut XmlWriter, props: &CellProperties) {
    if props.width.is_none()
        && props.grid_span.is_none()
        && props.vertical_merge.is_none()
        && props.vertical_align.is_none()
        && props.text_direction.is_none()
        && props.shading.is_none()
        && !props.hide_mark
        && !props.fit_text
        && !props.no_wrap
        && borders_empty(&props.borders)
        && props.margins.top.is_none()
        && props.margins.start.is_none()
        && props.margins.bottom.is_none()
        && props.margins.end.is_none()
    {
        return;
    }
    xml.start("w:tcPr");
    schema_order(xml, order::TCPR, |name, xml| {
        cell_child(xml, props, name);
    });
    xml.end();
}

/// Writes the `w:tcPr` child called `name`, when the model has it.
fn cell_child(xml: &mut XmlWriter, props: &CellProperties, name: &str) {
    match name {
        "tcW" => {
            if let Some(width) = &props.width {
                width_element(xml, "w:tcW", width);
            }
        }
        "gridSpan" => {
            if let Some(span) = props.grid_span {
                xml.empty_attr_w("w:gridSpan", "val", span);
            }
        }
        "vMerge" => {
            if let Some(merge) = &props.vertical_merge {
                // `continue` is the schema default and is written bare; spelling
                // it out would be equally valid but is not what Word emits.
                if *merge == strict_ooxml_wml::model::values::VerticalMerge::Restart {
                    xml.empty_attr_w("w:vMerge", "val", "restart");
                } else {
                    xml.empty("w:vMerge");
                }
            }
        }
        "tcBorders" => {
            if !borders_empty(&props.borders) {
                borders_element(xml, "w:tcBorders", &props.borders, EdgeNames::Table);
            }
        }
        "shd" => {
            if let Some(shading) = &props.shading {
                shading_element(xml, shading);
            }
        }
        "noWrap" if props.no_wrap => xml.empty("w:noWrap"),
        "tcMar" => cell_margins(xml, "w:tcMar", &props.margins),
        "textDirection" => {
            if let Some(direction) = &props.text_direction {
                xml.empty_attr_w("w:textDirection", "val", direction.as_str());
            }
        }
        "tcFitText" if props.fit_text => xml.empty("w:tcFitText"),
        "vAlign" => {
            if let Some(align) = &props.vertical_align {
                xml.empty_attr_w("w:vAlign", "val", align.as_str());
            }
        }
        "hideMark" if props.hide_mark => xml.empty("w:hideMark"),
        _ => {}
    }
}

/// Writes `w:sectPr`.
pub fn section_properties(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, section: &SectionProperties) {
    // Header/footer references carry the id this write emits, which the
    // context computed; a reference whose part this write does not emit is
    // dropped and reported rather than left pointing at an unrelated id.
    xml.start("w:sectPr");
    for reference in &section.headers {
        header_footer_reference(ctx, xml, "w:headerReference", reference);
    }
    for reference in &section.footers {
        header_footer_reference(ctx, xml, "w:footerReference", reference);
    }
    if let Some(kind) = &section.section_type {
        xml.empty_attr_w("w:type", "val", kind.as_str());
    }
    if let Some(size) = &section.page_size {
        page_size(xml, size);
    }
    if let Some(margins) = &section.page_margins {
        page_margins(xml, margins);
    }
    if let Some(borders) = &section.page_borders {
        page_borders(xml, borders);
    }
    if let Some(line_numbering) = &section.line_numbering {
        xml.start("w:lnNumType");
        xml.attr_w_opt("countBy", line_numbering.count_by);
        xml.attr_w_opt("start", line_numbering.start);
        if let Some(restart) = &line_numbering.restart {
            xml.attr_w("restart", restart.as_str());
        }
        xml.attr_w_opt("distance", line_numbering.distance.map(|v| v.0));
        xml.end();
    }
    if !section.footnote_properties.is_empty() {
        note_properties(xml, "w:footnotePr", &section.footnote_properties);
    }
    if !section.endnote_properties.is_empty() {
        note_properties(xml, "w:endnotePr", &section.endnote_properties);
    }
    if let Some(columns) = &section.columns {
        columns_element(xml, columns);
    }
    if section.bidi {
        xml.empty("w:bidi");
    }
    if section.rtl_gutter {
        xml.empty("w:rtlGutter");
    }
    if section.gutter_at_top {
        xml.empty("w:gutterAtTop");
    }
    if section.title_page {
        xml.empty("w:titlePg");
    }
    if let Some(direction) = &section.text_direction {
        xml.empty_attr_w("w:textDirection", "val", direction.as_str());
    }
    if let Some(align) = &section.vertical_align {
        xml.empty_attr_w("w:vAlign", "val", align.as_str());
    }
    if let Some(grid) = &section.doc_grid {
        doc_grid(xml, grid);
    }
    xml.end();
}

fn header_footer_reference(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    name: &str,
    reference: &HeaderFooterRef,
) {
    let Some(part) = reference.part.as_ref() else {
        return;
    };
    // The ids in the parsed document are the *source* package's. This write
    // allocates its own, so the reference has to be looked up by the part it
    // names. Writing the source id unchanged aims the reference at whatever
    // this write put at that number — in practice a hyperlink — and the header
    // or footer then simply is not there on the next open. A part the write
    // does not emit is reported instead of left dangling.
    let Some(rel_id) = ctx.header_footer_rel(part).map(str::to_owned) else {
        ctx.report_unsupported(
            name,
            &format!("header/footer part {part} is not in the written package"),
            &strict_ooxml_core::error::SourceLocation::unknown(),
        );
        return;
    };
    xml.start(name);
    xml.attr_w(
        "type",
        match reference.kind {
            HeaderFooterKind::Default => "default",
            HeaderFooterKind::First => "first",
            HeaderFooterKind::Even => "even",
        },
    );
    xml.attr_r_opt("id", Some(rel_id));
    xml.end();
}

fn page_size(xml: &mut XmlWriter, size: &PageSize) {
    xml.start("w:pgSz");
    xml.attr_w_opt("w", size.width.map(|v| v.0));
    xml.attr_w_opt("h", size.height.map(|v| v.0));
    if let Some(orientation) = &size.orientation {
        xml.attr_w("orient", orientation.as_str());
    }
    xml.end();
}

/// Writes `w:pgMar`, whose seven attributes are ALL `use="required"`.
///
/// `CT_PageMar` declares `top`, `right`, `bottom`, `left`, `header`, `footer` and
/// `gutter` as required, and the writer wrote whichever ones the model happened
/// to carry - so a section whose model has no gutter produced a `w:pgMar` with
/// six attributes and the schema rejected it (`XS-08`). A missing margin is not a
/// missing margin: Word's own default is 0, and a zero gutter is exactly what the
/// absence of a gutter means on the page.
fn page_margins(xml: &mut XmlWriter, margins: &PageMargins) {
    xml.start("w:pgMar");
    for (local, value) in [
        ("top", margins.top),
        ("right", margins.right),
        ("bottom", margins.bottom),
        ("left", margins.left),
        ("header", margins.header),
        ("footer", margins.footer),
        ("gutter", margins.gutter),
    ] {
        xml.attr_w(local, value.map_or(0, |v| v.0));
    }
    xml.end();
}

fn page_borders(xml: &mut XmlWriter, borders: &PageBorders) {
    xml.start("w:pgBorders");
    if let Some(from) = borders.offset_from {
        xml.attr_w("offsetFrom", from.as_str());
    }
    if let Some(order) = borders.z_order {
        xml.attr_w("zOrder", order.as_str());
    }
    for (local, edge) in [
        ("top", &borders.top),
        ("left", &borders.left),
        ("bottom", &borders.bottom),
        ("right", &borders.right),
    ] {
        if let Some(edge) = edge {
            page_border_edge(xml, local, edge);
        }
    }
    xml.end();
}

fn page_border_edge(xml: &mut XmlWriter, local: &str, border: &PageBorder) {
    xml.start(&format!("w:{local}"));
    xml.attr_w("val", border.style.map_or("none", |s| s.as_str()));
    xml.attr_w_opt("sz", border.size.map(|v| v.0));
    xml.attr_w_opt("space", border.space);
    // §17.6.2: the edges of `w:pgBorders` are `CT_TopPageBorder` and friends,
    // which derive from `CT_Border` — and `CT_Border` has **no child
    // elements**. `w:color` is an attribute. The writer emitted it as a child,
    // so the parser, which reads the attribute, dropped the colour on the very
    // next open: a page border that silently lost its ink. The tint and shade
    // that go with `w:themeColor` are attributes for the same reason.
    xml.attr_w_opt("color", border.color.as_ref().map(Color::as_str));
    if let Some(theme) = &border.theme_color {
        xml.attr_w("themeColor", theme.color.as_str());
        if let Some(tint) = &theme.tint {
            xml.attr_w("themeTint", tint.as_ref());
        }
        if let Some(shade) = &theme.shade {
            xml.attr_w("themeShade", shade.as_ref());
        }
    }
    if border.shadow {
        xml.attr_w("shadow", "true");
    }
    xml.end();
}

fn columns_element(xml: &mut XmlWriter, columns: &Columns) {
    xml.start("w:cols");
    xml.attr_w_opt("num", columns.count);
    xml.attr_w_opt("space", columns.space.map(|v| v.0));
    if columns.equal_width {
        xml.attr_w("equalWidth", "true");
    }
    if columns.separator {
        xml.attr_w("sep", "true");
    }
    for column in &columns.columns {
        xml.start("w:col");
        xml.attr_w_opt("w", column.width.map(|v| v.0));
        xml.attr_w_opt("space", column.space.map(|v| v.0));
        xml.end();
    }
    xml.end();
}

fn doc_grid(xml: &mut XmlWriter, grid: &DocGrid) {
    xml.start("w:docGrid");
    if let Some(kind) = &grid.grid_type {
        xml.attr_w("type", kind.as_str());
    }
    xml.attr_w_opt("linePitch", grid.line_pitch);
    xml.attr_w_opt("charSpace", grid.character_space);
    xml.end();
}

/// Writes `w:footnotePr`/`w:endnotePr`.
pub fn note_properties(xml: &mut XmlWriter, name: &str, props: &NoteProperties) {
    xml.start(name);
    if let Some(position) = &props.position {
        xml.empty_attr_w("w:pos", "val", position.as_ref());
    }
    if let Some(format) = &props.num_format {
        xml.empty_attr_w("w:numFmt", "val", format.as_ref());
    }
    if let Some(start) = props.num_start {
        xml.empty_attr_w("w:numStart", "val", start);
    }
    if let Some(restart) = &props.num_restart {
        xml.empty_attr_w("w:numRestart", "val", restart.as_ref());
    }
    xml.end();
}

/// Writes a `HighlightOrColor` attribute value.
#[must_use]
pub fn highlight_or_color_value(value: &HighlightOrColor) -> &str {
    match value {
        HighlightOrColor::Highlight(highlight) => highlight.as_str(),
        HighlightOrColor::Color(color) => color.as_str(),
    }
}

fn bool_str(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

#[cfg(test)]
mod tests {
    use strict_ooxml_core::normalize::report::NormalizationReport;
    use strict_ooxml_wml::model::props::{PageMargins, ParagraphProperties};
    use strict_ooxml_wml::model::values::{
        Border, BorderStyle, Borders, Color, EighthsPoint, Justification, Spacing, Twips,
    };

    use super::{
        borders_element, paragraph_properties, section_properties, tbl_width_value, EdgeNames,
    };
    use crate::ctx::Ctx;
    use crate::xml::XmlWriter;

    #[test]
    fn an_empty_ppr_is_not_emitted() {
        let mut xml = XmlWriter::new();
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        paragraph_properties(&mut ctx, &mut xml, &ParagraphProperties::default());
        assert!(!xml.has_open_elements());
        assert_eq!(xml.finish().expect("balanced"), "\n");
    }

    #[test]
    fn ppr_children_follow_schema_order() {
        let props = ParagraphProperties {
            alignment: Some(Justification::Center),
            spacing: Some(Spacing {
                before: Some(Twips(120)),
                ..Spacing::default()
            }),
            keep_next: true,
            ..ParagraphProperties::default()
        };
        let mut xml = XmlWriter::new();
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        paragraph_properties(&mut ctx, &mut xml, &props);
        let text = xml.finish().expect("balanced");
        let keep = text.find("keepNext").expect("keepNext");
        let spacing = text.find("w:spacing").expect("spacing");
        let jc = text.find("w:jc").expect("jc");
        assert!(keep < spacing && spacing < jc, "{text}");
    }

    #[test]
    fn sect_pr_uses_start_and_end_indirection() {
        let section = strict_ooxml_wml::model::props::SectionProperties {
            page_margins: Some(PageMargins {
                top: Some(Twips(1440)),
                left: Some(Twips(1800)),
                ..PageMargins::default()
            }),
            ..strict_ooxml_wml::model::props::SectionProperties::default()
        };
        let mut xml = XmlWriter::new();
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        section_properties(&mut ctx, &mut xml, &section);
        let text = xml.finish().expect("balanced");
        // `CT_PageMar` makes all seven attributes required, and `left`/`right` are
        // the names it uses - `w:pgMar` was never one of the containers Strict
        // renamed to start/end, which is the trap `borders_element` fell into.
        assert!(
            text.contains(
                "<w:pgMar w:top=\"1440\" w:right=\"0\" w:bottom=\"0\" w:left=\"1800\" \
                 w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_table_width_is_written_as_a_universal_measure() {
        // `ST_MeasurementOrPercent` has no plain-number branch, so bare twips are
        // not a value `CT_TblWidth/@w:w` can hold. Twenty twips to a point, and
        // an integer over twenty has at most two decimals.
        assert_eq!(tbl_width_value(0), "0pt");
        assert_eq!(tbl_width_value(108), "5.4pt");
        assert_eq!(tbl_width_value(20), "1pt");
        assert_eq!(tbl_width_value(3561), "178.05pt");
        assert_eq!(tbl_width_value(-100), "-5pt");
    }

    #[test]
    fn paragraph_and_table_borders_use_different_edge_names() {
        let border = Border {
            style: Some(BorderStyle::Single),
            size: Some(EighthsPoint(4)),
            color: Some(Color::new("#000000")),
            space: None,
            shadow: false,
            frame: false,
        };
        let borders = Borders {
            top: Some(border.clone()),
            bottom: Some(border.clone()),
            start: Some(border.clone()),
            end: Some(border),
            inside_horizontal: None,
            inside_vertical: None,
        };
        let mut xml = XmlWriter::new();
        borders_element(&mut xml, "w:pBdr", &borders, EdgeNames::Paragraph);
        let paragraph = xml.finish().expect("balanced");
        assert!(paragraph.contains("<w:left "), "{paragraph}");
        assert!(!paragraph.contains("<w:start "), "{paragraph}");

        let mut xml = XmlWriter::new();
        borders_element(&mut xml, "w:tblBorders", &borders, EdgeNames::Table);
        let table = xml.finish().expect("balanced");
        assert!(table.contains("<w:start "), "{table}");
        assert!(!table.contains("<w:left "), "{table}");
    }
}
