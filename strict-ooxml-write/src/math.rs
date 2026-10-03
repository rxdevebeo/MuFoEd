//! OMML serialization (`m:` namespace, ISO/IEC 29500-1 §22.1).
//!
//! Formulas are written structurally, node for node, so a written formula
//! re-parses into the same tree. The one construct the model does not carry is
//! the *argument properties* of a bare argument, which the schema makes
//! optional; those are written only when the model recorded them.

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::math::{
    MathAlignment, MathArgument, MathExpression, MathNode, MathParagraph, MathRun,
    MathRunProperties,
};

use crate::ctx::Ctx;
use crate::xml::XmlWriter;

/// Writes `m:oMath`.
pub fn math_expression(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, expression: &MathExpression) {
    xml.start("m:oMath");
    for child in &expression.nodes {
        math_node(ctx, xml, child);
    }
    xml.end();
}

/// Writes `m:oMathPara`.
pub fn math_paragraph(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, paragraph: &MathParagraph) {
    xml.start("m:oMathPara");
    if let Some(properties) = &paragraph.properties {
        if let Some(justification) = properties.justification {
            xml.start("m:oMathParaPr");
            xml.empty_attr("m:jc", "m:val", math_justification(justification));
            xml.end();
        }
    }
    for expression in &paragraph.equations {
        math_expression(ctx, xml, expression);
    }
    xml.end();
}

fn math_justification(value: strict_ooxml_wml::model::math::MathJustification) -> &'static str {
    use strict_ooxml_wml::model::math::MathJustification as J;
    match value {
        J::Left => "left",
        J::CenterGroup => "centerGroup",
        J::Center => "center",
        J::Right => "right",
        J::Default => "default",
    }
}

/// The two alignment vocabularies of OMML, which are two different sets, plus
/// the element that is neither.
///
/// One function used to spell both the same way, writing `ctr`, `l` and `r`
/// everywhere, and every one of those values is outside the set its element
/// declares (`XS-12`):
///
/// | element | type | set |
/// |---|---|---|
/// | `m:mcJc` | `CT_XAlign` | left, center, right, inside, outside |
/// | `m:baseJc` | `CT_YAlign` | inline, top, center, bottom, inside, outside |
/// | `m:aln` | `CT_OnOff` | a toggle, not an alignment |
///
/// `m:aln` is the odd one out and the reason this cannot be one lookup table:
/// `CT_OnOff` carries a boolean, and the model holds a justification there. There
/// is no lexical form that says WHICH side, so the side is recorded as lost and
/// the toggle is written - which is all the element can say.
fn x_align(value: strict_ooxml_wml::model::math::MathAlignment) -> Option<&'static str> {
    use strict_ooxml_wml::model::math::MathAlignment as A;
    match value {
        A::Left => Some("left"),
        A::Center => Some("center"),
        A::Right => Some("right"),
        A::Inline => Some("inside"),
        A::Unset => None,
    }
}

/// Writes `m:baseJc`, which is a `CT_YAlign`.
///
/// The previous mapping went through the **justification** vocabulary and sent
/// `Left` to `inline` and `Right` to `bottom`. Neither is a defensible reading:
/// `inline` means "laid out with the text" and `bottom` means "aligned to the
/// bottom", and neither is what "justified left" or "justified right" means.
/// The two vocabularies overlap in two values, which is exactly why it went
/// unnoticed - the overlapping cases came out right by accident.
fn y_align(value: strict_ooxml_wml::model::math::MathVerticalAlign) -> Option<&'static str> {
    use strict_ooxml_wml::model::math::MathVerticalAlign as V;
    match value {
        V::Inline => Some("inline"),
        V::Top => Some("top"),
        V::Center => Some("center"),
        V::Bottom => Some("bottom"),
        V::Inside => Some("inside"),
        V::Outside => Some("outside"),
        V::Unset => None,
    }
}

fn script(value: strict_ooxml_wml::model::math::MathScript) -> &'static str {
    use strict_ooxml_wml::model::math::MathScript as S;
    match value {
        // `ST_Script` hyphenates both of these, and the writer spelled them
        // without the hyphen.
        S::DoubleStruck => "double-struck",
        S::Fraktur => "fraktur",
        S::Roman => "roman",
        S::SansSerif => "sans-serif",
        S::Monospace => "monospace",
        S::Script => "script",
    }
}

fn style(value: strict_ooxml_wml::model::math::MathStyle) -> &'static str {
    value.as_str()
}

fn position(value: strict_ooxml_wml::model::math::MathPosition) -> &'static str {
    use strict_ooxml_wml::model::math::MathPosition as P;
    match value {
        P::Top => "top",
        P::Bottom => "bot",
        P::Left => "left",
        P::Right => "right",
    }
}

fn vertical_jc(value: strict_ooxml_wml::model::math::MathVerticalJc) -> &'static str {
    use strict_ooxml_wml::model::math::MathVerticalJc as V;
    match value {
        V::Top => "top",
        V::Bottom => "bot",
        V::Center => "center",
    }
}

fn limit_location(value: strict_ooxml_wml::model::math::LimitLocation) -> &'static str {
    use strict_ooxml_wml::model::math::LimitLocation as L;
    match value {
        L::UnderOver => "undOvr",
        L::SubSup => "subSup",
    }
}

/// Writes one `m:` node.
fn math_node(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, node: &MathNode) {
    use MathNode as N;
    match node {
        N::Run(run) => math_run(ctx, xml, run),
        N::Fraction(fraction) => {
            xml.start("m:f");
            if fraction.bar_type.is_some() || fraction.small_fraction || fraction.control.is_some()
            {
                xml.start("m:fPr");
                // §22.1.2.f: `CT_FPr` is the sequence `m:type?, m:ctrlPr?`,
                // and `m:type` is an *element* (`CT_FType`, carrying `m:val`).
                // The writer wrote it as an attribute of `m:fPr`, which parsed
                // as nothing: every fraction came back with a default bar, so a
                // `lin` or `noBar` fraction grew a rule on the second write.
                if let Some(bar_type) = fraction.bar_type.as_deref() {
                    xml.start("m:type");
                    xml.attr_m("val", bar_type);
                    xml.end();
                }
                if fraction.small_fraction {
                    // `m:smallFrac` is not a `CT_FPr` child in Strict: the
                    // sequence is `m:type?, m:ctrlPr?` and nothing else. The
                    // smallest fraction is `m:type m:val="lin"` with a small
                    // font, and the font is the run's business, so the request
                    // is recorded rather than invented.
                    ctx.report_partial(
                        "m:smallFrac",
                        "a fraction asked to be set small, but CT_FPr declares only m:type \
                         and m:ctrlPr; the request is not written and cannot be recovered",
                        &fraction.location,
                    );
                }
                control(ctx, xml, fraction.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:num", &fraction.numerator);
            argument(ctx, xml, "m:den", &fraction.denominator);
            xml.end();
        }
        N::Radical(radical) => {
            xml.start("m:rad");
            if radical.hide_degree || radical.control.is_some() {
                xml.start("m:radPr");
                if radical.hide_degree {
                    xml.empty("m:degHide");
                }
                control(ctx, xml, radical.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:deg", &radical.degree);
            argument(ctx, xml, "m:e", &radical.radicand);
            xml.end();
        }
        N::Superscript(script) => {
            xml.start("m:sSup");
            script_control(ctx, xml, script.control.as_deref());
            argument(ctx, xml, "m:e", &script.base);
            argument(ctx, xml, "m:sup", &script.superscript);
            xml.end();
        }
        N::Subscript(script) => {
            xml.start("m:sSub");
            script_control(ctx, xml, script.control.as_deref());
            argument(ctx, xml, "m:e", &script.base);
            argument(ctx, xml, "m:sub", &script.subscript);
            xml.end();
        }
        N::SubSuperscript(script) => {
            xml.start("m:sSubSup");
            script_control(ctx, xml, script.control.as_deref());
            argument(ctx, xml, "m:e", &script.base);
            argument(ctx, xml, "m:sub", &script.subscript);
            argument(ctx, xml, "m:sup", &script.superscript);
            xml.end();
        }
        N::PreScript(script) => {
            xml.start("m:sPre");
            script_control(ctx, xml, script.control.as_deref());
            argument(ctx, xml, "m:sub", &script.subscript);
            argument(ctx, xml, "m:sup", &script.superscript);
            argument(ctx, xml, "m:e", &script.base);
            xml.end();
        }
        N::NaryOperator(operator) => {
            xml.start("m:nary");
            if operator.chr.is_some()
                || operator.grow.is_some()
                || operator.hide_sub
                || operator.hide_sup
                || operator.control.is_some()
            {
                xml.start("m:naryPr");
                if let Some(chr) = operator.chr {
                    xml.empty_attr("m:chr", "m:val", format!("{chr}"));
                }
                if let Some(location) = operator.limit_location {
                    xml.empty_attr("m:limLoc", "m:val", limit_location(location));
                }
                if let Some(grow) = operator.grow {
                    xml.empty_attr("m:grow", "m:val", bool_str(grow));
                }
                if operator.hide_sub {
                    xml.empty("m:subHide");
                }
                if operator.hide_sup {
                    xml.empty("m:supHide");
                }
                control(ctx, xml, operator.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:sub", &operator.subscript);
            argument(ctx, xml, "m:sup", &operator.superscript);
            argument(ctx, xml, "m:e", &operator.operand);
            xml.end();
        }
        N::Delimiter(delimiter) => {
            xml.start("m:d");
            if delimiter.begin.is_some()
                || delimiter.separator.is_some()
                || delimiter.end.is_some()
                || delimiter.grow.is_some()
                || delimiter.shape.is_some()
            {
                xml.start("m:dPr");
                // The model holds the *effective* character: `None` means the
                // producer asked for no delimiter there, and the parser has
                // already resolved an absent element to the schema default. So
                // both cases have to be written out — `None` as the empty
                // `m:val` Word itself uses. Omitting the element instead meant
                // that a bracket-less delimiter came back bracketed on the
                // second write: `<m:begChr m:val=""/>` vanished, and re-reading
                // the result restored the default `(`. The document grew
                // brackets every generation and never settled.
                delimiter_char_element(xml, "m:begChr", delimiter.begin);
                delimiter_char_element(xml, "m:sepChr", delimiter.separator);
                delimiter_char_element(xml, "m:endChr", delimiter.end);
                if let Some(grow) = delimiter.grow {
                    xml.empty_attr("m:grow", "m:val", bool_str(grow));
                }
                if let Some(shape) = &delimiter.shape {
                    xml.empty_attr("m:shp", "m:val", shape.as_ref());
                }
                control(ctx, xml, delimiter.control.as_deref());
                xml.end();
            }
            for item in &delimiter.arguments {
                argument(ctx, xml, "m:e", item);
            }
            xml.end();
        }
        N::Function(function) => {
            xml.start("m:func");
            function_control(ctx, xml, function.control.as_deref());
            argument(ctx, xml, "m:fName", &function.name);
            argument(ctx, xml, "m:e", &function.argument);
            xml.end();
        }
        N::Limit(limit) => {
            xml.start(if limit.above { "m:limUpp" } else { "m:limLow" });
            if limit.control.is_some() {
                xml.start(if limit.above {
                    "m:limUppPr"
                } else {
                    "m:limLowPr"
                });
                control(ctx, xml, limit.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:e", &limit.base);
            argument(ctx, xml, "m:lim", &limit.limit);
            xml.end();
        }
        N::Matrix(matrix) => {
            xml.start("m:m");
            if !matrix.columns.is_empty()
                || matrix.base_justification.is_some()
                || matrix.hide_placeholders
                || matrix.column_spacing.is_some()
                || matrix.column_group_spacing.is_some()
                || matrix.row_spacing.is_some()
            {
                xml.start("m:mPr");
                if let Some(justification) = matrix.base_justification {
                    if let Some(value) = y_align(justification) {
                        xml.empty_attr("m:baseJc", "m:val", value);
                    }
                }
                if matrix.hide_placeholders {
                    xml.empty("m:plcHide");
                }
                if let Some(rule) = &matrix.column_group_rule {
                    xml.start("m:cGpRule");
                    xml.attr_m("val", rule.as_ref());
                    xml.end();
                }
                if let Some(spacing) = matrix.column_spacing {
                    xml.start("m:cSp");
                    xml.attr_m("val", spacing);
                    xml.end();
                }
                if let Some(spacing) = matrix.column_group_spacing {
                    xml.start("m:cGp");
                    xml.attr_m("val", spacing);
                    xml.end();
                }
                if let Some(rule) = &matrix.row_spacing_rule {
                    xml.start("m:rSpRule");
                    xml.attr_m("val", rule.as_ref());
                    xml.end();
                }
                if let Some(spacing) = matrix.row_spacing {
                    xml.start("m:rSp");
                    xml.attr_m("val", spacing);
                    xml.end();
                }
                if !matrix.columns.is_empty() {
                    xml.start("m:mcs");
                    for column in &matrix.columns {
                        xml.start("m:mc");
                        if column.justification.is_some() || column.count.is_some() {
                            // `CT_MC` holds one `m:mcPr`, and `CT_MCPr` is the
                            // sequence `m:count?, m:mcJc?` - the count comes
                            // first. The writer wrote `m:mcJc` and `m:count` as
                            // children of `m:mc` with no `m:mcPr` and in the
                            // other order, so every matrix column with a count
                            // was "This element is not expected" (`XS-15`).
                            xml.start("m:mcPr");
                            if let Some(count) = column.count {
                                xml.empty_attr("m:count", "m:val", count);
                            }
                            if let Some(value) = column.justification.and_then(x_align) {
                                xml.empty_attr("m:mcJc", "m:val", value);
                            }
                            xml.end();
                        }
                        xml.end();
                    }
                    xml.end();
                }
                control(ctx, xml, matrix.control.as_deref());
                xml.end();
            }
            for row in &matrix.rows {
                xml.start("m:mr");
                for cell in row {
                    argument(ctx, xml, "m:e", cell);
                }
                xml.end();
            }
            xml.end();
        }
        N::EquationArray(array) => {
            xml.start("m:eqArr");
            if array.base_justification.is_some()
                || array.max_distance.is_some()
                || array.object_distance.is_some()
                || array.row_spacing.is_some()
            {
                xml.start("m:eqArrPr");
                if let Some(justification) = array.base_justification {
                    if let Some(value) = y_align(justification) {
                        xml.empty_attr("m:baseJc", "m:val", value);
                    }
                }
                if let Some(distance) = array.max_distance {
                    xml.start("m:maxDist");
                    xml.attr_m("val", distance);
                    xml.end();
                }
                if let Some(distance) = array.object_distance {
                    xml.start("m:objDist");
                    xml.attr_m("val", distance);
                    xml.end();
                }
                if let Some(rule) = &array.row_spacing_rule {
                    xml.start("m:rSpRule");
                    xml.attr_m("val", rule.as_ref());
                    xml.end();
                }
                if let Some(spacing) = array.row_spacing {
                    xml.start("m:rSp");
                    xml.attr_m("val", spacing);
                    xml.end();
                }
                control(ctx, xml, array.control.as_deref());
                xml.end();
            }
            for row in &array.rows {
                argument(ctx, xml, "m:e", row);
            }
            xml.end();
        }
        N::Accent(accent) => {
            xml.start("m:acc");
            if accent.chr.is_some() {
                xml.start("m:accPr");
                if let Some(chr) = accent.chr {
                    xml.empty_attr("m:chr", "m:val", format!("{chr}"));
                }
                control(ctx, xml, accent.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:e", &accent.base);
            xml.end();
        }
        N::Bar(bar) => {
            xml.start("m:bar");
            if bar.position.is_some() || bar.control.is_some() {
                xml.start("m:barPr");
                if let Some(value) = bar.position {
                    xml.empty_attr("m:pos", "m:val", position(value));
                }
                control(ctx, xml, bar.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:e", &bar.base);
            xml.end();
        }
        N::GroupCharacter(group) => {
            xml.start("m:groupChr");
            if group.chr.is_some()
                || group.position.is_some()
                || group.vertical_justification.is_some()
            {
                xml.start("m:groupChrPr");
                if let Some(chr) = group.chr {
                    xml.empty_attr("m:chr", "m:val", format!("{chr}"));
                }
                if let Some(value) = group.position {
                    xml.empty_attr("m:pos", "m:val", position(value));
                }
                if let Some(value) = group.vertical_justification {
                    xml.empty_attr("m:vertJc", "m:val", vertical_jc(value));
                }
                control(ctx, xml, group.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:e", &group.base);
            xml.end();
        }
        N::Boxed(boxed) => {
            xml.start("m:box");
            if boxed.alignment.is_some() || boxed.spacing.is_some() {
                xml.start("m:boxPr");
                if let Some(value) = boxed.alignment {
                    if value != MathAlignment::Unset {
                        // `CT_OnOff`, like every `m:aln`: the toggle is the
                        // assertion, and which side it meant has no lexical form.
                        xml.empty("m:aln");
                    }
                }
                if boxed.spacing.is_some() {
                    // `CT_BoxPr` declares opEmu, noBreak, diff, brk, aln and
                    // ctrlPr. There is no `m:sp`, so the requested spacing has
                    // nowhere to go in this container.
                    ctx.report_partial(
                        "m:boxPr/m:sp",
                        &format!(
                            "CT_BoxPr declares no m:sp, so the box's spacing ({}) is not written",
                            boxed.spacing.unwrap_or_default()
                        ),
                        &boxed.location,
                    );
                }
                control(ctx, xml, boxed.control.as_deref());
                xml.end();
            }
            argument(ctx, xml, "m:e", &boxed.argument);
            xml.end();
        }
        N::BorderBox(border_box) => {
            xml.start("m:borderBox");
            xml.start("m:borderBoxPr");
            if let Some(value) = border_box.alignment {
                if let Some(value) = x_align(value) {
                    xml.empty_attr("m:aln", "m:val", value);
                }
            }
            if let Some(spacing) = border_box.spacing {
                xml.start("m:sp");
                xml.attr_m("val", spacing);
                xml.end();
            }
            if border_box.shadow {
                xml.empty("m:shadow");
            }
            if let Some(lines) = border_box.lines {
                xml.start("m:lines");
                // `attr_w("m:val", …)` produced `w:m:val` — two prefixes on one
                // attribute name, which is not a name at all. The part was not
                // well-formed XML, and only a document with an `m:borderBox` in
                // it ever noticed.
                xml.attr_m("val", lines);
                xml.end();
            }
            control(ctx, xml, border_box.control.as_deref());
            xml.end();
            for item in &border_box.arguments {
                argument(ctx, xml, "m:e", item);
            }
            xml.end();
        }
        N::Phantom(phantom) => {
            xml.start("m:phant");
            xml.start("m:phantPr");
            xml.empty_attr("m:show", "m:val", bool_str(phantom.show));
            if phantom.transparent {
                xml.empty("m:transp");
            }
            if phantom.zero_width {
                xml.empty("m:zeroWid");
            }
            if phantom.zero_ascent {
                xml.empty("m:zeroAsc");
            }
            if phantom.zero_descent {
                xml.empty("m:zeroDesc");
            }
            if phantom.show_all {
                xml.empty("m:showAll");
            }
            if phantom.rtl {
                xml.empty("m:rtl");
            }
            control(ctx, xml, phantom.control.as_deref());
            xml.end();
            argument(ctx, xml, "m:e", &phantom.argument);
            xml.end();
        }
        N::Unknown(unknown) => {
            ctx.report_unsupported(
                &format!("m:{}", unknown.local),
                "formula node kept in the model but not serializable",
                &unknown.location,
            );
        }
    }
}

/// Writes one `m:e`-style argument with its optional `m:argPr`.
///
/// `CT_OMathArgPr` declares exactly one child: `m:argSz`. The five properties
/// this model keeps on an argument - `lit`, `nor`, `scr`, `sty`, `aln` - are RUN
/// properties, `CT_RPR` children, and OMML puts them on the argument's runs
/// rather than on the argument. The writer used to write them into `m:argPr`
/// anyway, which the schema rejects with "This element is not expected" and
/// which the reader then dropped on the next parse - so a formula's script and
/// style did not survive a round trip (`XS-15`).
///
/// They are not relocated here. Moving them onto a run means deciding which run,
/// and the choice changes nothing in the model and something on the page; so the
/// part the schema allows is written and the rest is recorded, per ADR-0007.
fn argument(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, name: &'static str, argument: &MathArgument) {
    let properties = &argument.properties;
    let inexpressible = [
        ("m:lit", properties.literal == Some(true)),
        ("m:nor", properties.normal == Some(true)),
        ("m:scr", properties.script.is_some()),
        ("m:sty", properties.style.is_some()),
        (
            "m:aln",
            properties
                .alignment
                .is_some_and(|value| value != MathAlignment::Unset),
        ),
        ("m:brk", properties.r#break.is_some()),
    ];
    let inexpressible: Vec<&str> = inexpressible
        .iter()
        .filter_map(|(child, present)| present.then_some(*child))
        .collect();

    xml.start(name);
    if inexpressible.is_empty() && properties.control.is_none() {
        for child in &argument.nodes {
            math_node(ctx, xml, child);
        }
        xml.end();
        return;
    }
    if !inexpressible.is_empty() {
        ctx.report_partial(
            "m:argPr",
            &format!(
                "{inexpressible:?} are run properties, and CT_OMathArgPr declares only m:argSz; \
                 OMML puts them on the argument's runs, so they are not written here"
            ),
            &argument.location,
        );
    }
    if properties.control.is_some() {
        xml.start("m:ctrlPr");
        crate::props::run_properties(xml, properties.control.as_deref().expect("checked"));
        xml.end();
    }
    for child in &argument.nodes {
        math_node(ctx, xml, child);
    }
    xml.end();
}

/// Writes one `m:begChr`/`m:sepChr`/`m:endChr`, including the empty `m:val`
/// that suppresses a delimiter.
fn delimiter_char_element(xml: &mut XmlWriter, name: &str, character: Option<char>) {
    xml.empty_attr(
        name,
        "m:val",
        character.map_or_else(String::new, |c| c.to_string()),
    );
}

/// Writes `m:ctrlPr` for a construct whose control properties the schema places
/// directly under the construct's own properties element.
fn control(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    run_props: Option<&strict_ooxml_wml::model::props::RunProperties>,
) {
    let _ = ctx;
    if let Some(run_props) = run_props {
        xml.start("m:ctrlPr");
        crate::props::run_properties(xml, run_props);
        xml.end();
    }
}

/// `m:sSupPr`, `m:sSubPr`, `m:sSubSupPr` and `m:sPrePr` carry only `m:ctrlPr`.
fn script_control(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    run_props: Option<&strict_ooxml_wml::model::props::RunProperties>,
) {
    if run_props.is_some() {
        xml.start("m:sSupPr");
        control(ctx, xml, run_props);
        xml.end();
    }
}

/// `m:funcPr` carries only `m:ctrlPr`.
fn function_control(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    run_props: Option<&strict_ooxml_wml::model::props::RunProperties>,
) {
    if run_props.is_some() {
        xml.start("m:funcPr");
        control(ctx, xml, run_props);
        xml.end();
    }
}

/// Writes `m:r`.
///
/// ISO/IEC 29500-1 §22.1.2.79 declares `CT_R` as the sequence
/// `m:rPr?, w:rPr?, (m:t | …)*` — the two property elements are **siblings**.
/// The writer nested `w:rPr` inside `m:rPr`, which is well-formed XML and
/// therefore passed every parse check, but the parser reads `m:rPr` as the math
/// property container and ignores a `w:rPr` inside it: the character formatting
/// was dropped on the very next read, which showed up as a formula shrinking by
/// several kilobytes on a second write.
fn math_run(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, run: &MathRun) {
    xml.start("m:r");
    run_properties(ctx, xml, &run.properties);
    if let Some(run_props) = run.run_properties.as_deref() {
        crate::props::run_properties(xml, run_props);
    }
    xml.start("m:t");
    xml.text(&run.text);
    xml.end();
    xml.end();
}

fn run_properties(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, properties: &MathRunProperties) {
    if !properties.literal
        && !properties.normal
        && properties.script.is_none()
        && properties.style.is_none()
        && properties.alignment.is_none()
        && properties.control.is_none()
    {
        return;
    }
    xml.start("m:rPr");
    if properties.literal {
        xml.empty("m:lit");
    }
    if properties.normal {
        xml.empty("m:nor");
    }
    if let Some(value) = properties.script {
        xml.start("m:scr");
        xml.attr_m("val", script(value));
        xml.end();
    }
    if let Some(value) = properties.style {
        xml.start("m:sty");
        xml.attr_m("val", style(value));
        xml.end();
    }
    if let Some(value) = properties.alignment {
        if value != MathAlignment::Unset {
            // `m:aln` is `CT_OnOff`: the element is the assertion, and
            // `m:val` is a boolean. There is no lexical form that says which
            // side, so the toggle is written bare and the side is recorded
            // where the record can be read next to the loss.
            ctx.report_partial(
                "m:aln",
                &format!(
                    "the alignment is {value:?}, but m:aln is an on/off property; the toggle is \
                     written and which side the argument aligned to is not recoverable from it"
                ),
                &SourceLocation::unknown(),
            );
            xml.empty("m:aln");
        }
    }
    if let Some(control) = properties.control.as_deref() {
        xml.start("m:ctrlPr");
        crate::props::run_properties(xml, control);
        xml.end();
    }
    xml.end();
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
    use strict_ooxml_wml::model::math::{MathExpression, MathNode, MathRun, MathRunProperties};

    use super::math_expression;
    use crate::ctx::Ctx;
    use crate::xml::XmlWriter;

    #[test]
    fn a_plain_run_round_trips() {
        let expression = MathExpression {
            nodes: vec![MathNode::Run(MathRun {
                properties: MathRunProperties::default(),
                run_properties: None,
                text: "x+1".to_owned(),
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            })],
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let mut xml = XmlWriter::new();
        math_expression(&mut ctx, &mut xml, &expression);
        assert_eq!(
            xml.finish().expect("balanced"),
            "<m:oMath><m:r><m:t>x+1</m:t></m:r></m:oMath>\n"
        );
        assert!(report.losses().is_empty());
    }

    #[test]
    fn run_properties_use_the_math_vocabulary() {
        let expression = MathExpression {
            nodes: vec![MathNode::Run(MathRun {
                properties: MathRunProperties {
                    literal: true,
                    normal: false,
                    r#break: None,
                    script: Some(strict_ooxml_wml::model::math::MathScript::Fraktur),
                    style: Some(strict_ooxml_wml::model::math::MathStyle::Bold),
                    alignment: None,
                    control: None,
                },
                run_properties: None,
                text: "a".to_owned(),
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            })],
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let mut xml = XmlWriter::new();
        math_expression(&mut ctx, &mut xml, &expression);
        let text = xml.finish().expect("balanced");
        assert!(text.contains("<m:lit/>"), "{text}");
        assert!(text.contains("<m:scr m:val=\"fraktur\"/>"), "{text}");
        assert!(text.contains("<m:sty m:val=\"b\"/>"), "{text}");
    }
}
