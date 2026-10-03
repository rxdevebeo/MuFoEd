#![allow(clippy::doc_markdown, clippy::unreadable_literal)]
//! OMML parsing tests — `STAGE-5C-TASK.md` §7.1 (unit/integration).
//!
//! Every construct of §3.1 is parsed from markup written here (not from a
//! binary fixture), so a parser regression points at the construct rather than
//! at a generated file.

mod common;

use strict_ooxml_wml::model::math::{
    LimitLocation, MathAlignment, MathJustification, MathNode, MathPosition, MathScript, MathStyle,
    MathVerticalAlign, MathVerticalJc,
};
use strict_ooxml_wml::model::{Block, Inline, SupportStatus};
use strict_ooxml_wml::M_NS;

use common::{document_parts, parse_parts, parse_parts_with_limits};

/// Wraps a paragraph body into a package and parses it.
fn parse_body(body: &str) -> strict_ooxml_wml::model::Document {
    parse_parts(&document_parts(&format!("<w:p>{body}</w:p>"), &[])).expect("parse")
}

/// Parses `body` and returns the formula of its single paragraph.
fn formula(body: &str) -> strict_ooxml_wml::model::math::MathExpression {
    let document = parse_body(body);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    let inline = paragraph
        .inlines
        .iter()
        .find_map(Inline::as_math)
        .expect("an inline formula");
    inline.clone()
}

/// Parses `body` and returns the first node of the formula.
fn first_node(body: &str) -> MathNode {
    formula(body).nodes.into_iter().next().expect("one node")
}

#[test]
fn an_inline_formula_is_paragraph_content_not_opaque() {
    let document = parse_body("<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>");
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    assert!(matches!(paragraph.inlines[0], Inline::Math(_)));
    assert!(paragraph
        .inlines
        .iter()
        .all(|inline| !matches!(inline, Inline::Opaque(_))));
    assert_eq!(
        document.support.get("m:oMath").map(|use_| use_.status),
        Some(SupportStatus::Supported)
    );
}

#[test]
fn a_display_formula_carries_its_justification() {
    let document = parse_body(
        "<m:oMathPara><m:oMathParaPr><m:jc m:val=\"centerGroup\"/></m:oMathParaPr>\
<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara>",
    );
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    let display = paragraph
        .inlines
        .iter()
        .find_map(Inline::as_math_paragraph)
        .expect("a display formula");
    assert_eq!(
        display
            .properties
            .as_ref()
            .and_then(|properties| properties.justification),
        Some(MathJustification::CenterGroup)
    );
    assert_eq!(display.expression.text(), "x");
}

#[test]
fn a_fraction_is_parsed_with_its_type_and_parts() {
    let document = parse_body(
        "<m:oMath><m:f><m:fPr><m:type m:val=\"skew\"/></m:fPr><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>",
    );
    eprintln!("{}", document.support.debug_summary());
    eprintln!("{document:?}");
    let MathNode::Fraction(fraction) = first_node(
        "<m:oMath><m:f><m:fPr><m:type m:val=\"skew\"/><m:smallFrac m:val=\"1\"/></m:fPr>\
<m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>",
    ) else {
        panic!("expected a fraction");
    };
    assert_eq!(fraction.bar_type.as_deref(), Some("skew"));
    assert!(fraction.small_fraction);
    assert_eq!(fraction.numerator.text(), "a");
    assert_eq!(fraction.denominator.text(), "b");
    assert_eq!(MathNode::Fraction(fraction).element_name(), "m:f");
}

#[test]
fn scripts_radicals_and_pre_scripts_are_parsed() {
    let nodes = formula(
        "<m:oMath>\
<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>\
<m:sSub><m:e><m:r><m:t>a</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub>\
<m:sSubSup><m:e><m:r><m:t>P</m:t></m:r></m:e><m:sub><m:r><m:t>1</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSubSup>\
<m:sPre><m:sub><m:r><m:t>0</m:t></m:r></m:sub><m:sup><m:r><m:t>1</m:t></m:r></m:sup><m:e><m:r><m:t>F</m:t></m:r></m:e></m:sPre>\
<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e><m:r><m:t>y</m:t></m:r></m:e></m:rad>\
<m:rad><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>z</m:t></m:r></m:e></m:rad>\
</m:oMath>",
    )
    .nodes;
    assert_eq!(nodes.len(), 6);
    assert_eq!(nodes[0].element_name(), "m:sSup");
    assert_eq!(nodes[1].element_name(), "m:sSub");
    assert_eq!(nodes[2].element_name(), "m:sSubSup");
    assert_eq!(nodes[3].element_name(), "m:sPre");
    let MathNode::Radical(hidden) = &nodes[4] else {
        panic!("expected a radical");
    };
    assert!(hidden.hide_degree);
    let MathNode::Radical(shown) = &nodes[5] else {
        panic!("expected a radical");
    };
    assert!(!shown.hide_degree);
    assert_eq!(shown.degree.text(), "3");
}

#[test]
fn an_nary_operator_keeps_its_character_and_limit_placement() {
    let MathNode::NaryOperator(nary) = first_node(
        "<m:oMath><m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"subSup\"/>\
<m:grow m:val=\"1\"/><m:subHide m:val=\"1\"/></m:naryPr>\
<m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup>\
<m:e><m:r><m:t>k</m:t></m:r></m:e></m:nary></m:oMath>",
    ) else {
        panic!("expected an n-ary operator");
    };
    assert_eq!(nary.chr, Some('\u{2211}'));
    assert_eq!(nary.limit_location, Some(LimitLocation::SubSup));
    assert_eq!(nary.grow, Some(true));
    assert!(nary.hide_sub);
    assert!(!nary.hide_sup);
    assert_eq!(nary.subscript.text(), "i");
    assert_eq!(nary.operand.text(), "k");
}

#[test]
fn delimiters_default_and_can_be_suppressed() {
    let MathNode::Delimiter(plain) = first_node(
        "<m:oMath><m:d><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d></m:oMath>",
    ) else {
        panic!("expected a delimiter");
    };
    assert_eq!(plain.begin, Some('('));
    assert_eq!(plain.separator, Some('|'));
    assert_eq!(plain.end, Some(')'));
    assert_eq!(plain.arguments.len(), 2);

    let MathNode::Delimiter(custom) = first_node(
        "<m:oMath><m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"\"/>\
<m:grow m:val=\"0\"/><m:shp m:val=\"rect\"/></m:dPr>\
<m:e><m:r><m:t>x</m:t></m:r></m:e></m:d></m:oMath>",
    ) else {
        panic!("expected a delimiter");
    };
    assert_eq!(custom.begin, Some('['));
    // An empty `m:val` means "draw nothing".
    assert_eq!(custom.end, None);
    assert_eq!(custom.grow, Some(false));
    assert_eq!(custom.shape.as_deref(), Some("rect"));
}

#[test]
fn function_name_and_limits_are_parsed() {
    let nodes = formula(
        "<m:oMath>\
<m:func><m:fName><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>sin</m:t></m:r></m:fName>\
<m:e><m:r><m:t>x</m:t></m:r></m:e></m:func>\
<m:limLow><m:e><m:r><m:t>x</m:t></m:r></m:e><m:lim><m:r><m:t>0</m:t></m:r></m:lim></m:limLow>\
<m:limUpp><m:e><m:r><m:t>n</m:t></m:r></m:e><m:lim><m:r><m:t>∞</m:t></m:r></m:lim></m:limUpp>\
</m:oMath>",
    )
    .nodes;
    let MathNode::Function(function) = &nodes[0] else {
        panic!("expected a function");
    };
    assert_eq!(function.name.text(), "sin");
    assert_eq!(function.argument.text(), "x");
    let MathNode::Limit(low) = &nodes[1] else {
        panic!("expected a limit");
    };
    assert!(!low.above);
    let MathNode::Limit(high) = &nodes[2] else {
        panic!("expected a limit");
    };
    assert!(high.above);
}

#[test]
fn matrices_and_equation_arrays_keep_their_grid() {
    let MathNode::Matrix(matrix) = first_node(
        "<m:oMath><m:m><m:mPr><m:baseJc m:val=\"bottom\"/><m:plcHide m:val=\"1\"/>\
<m:cSp m:val=\"240\"/><m:cGp m:val=\"120\"/><m:cGpRule m:val=\"3\"/><m:rSp m:val=\"60\"/>\
<m:rSpRule m:val=\"exact\"/><m:mcs><m:mc><m:mcPr><m:count m:val=\"2\"/><m:mcJc m:val=\"center\"/>\
</m:mcPr></m:mc></m:mcs></m:mPr>\
<m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:t>2</m:t></m:r></m:e></m:mr>\
<m:mr><m:e><m:r><m:t>3</m:t></m:r></m:e><m:e><m:r><m:t>4</m:t></m:r></m:e></m:mr></m:m></m:oMath>",
    ) else {
        panic!("expected a matrix");
    };
    assert_eq!(matrix.rows.len(), 2);
    assert_eq!(matrix.rows[1].len(), 2);
    assert_eq!(matrix.base_justification, Some(MathVerticalAlign::Bottom));
    assert!(matrix.hide_placeholders);
    assert_eq!(matrix.column_spacing, Some(240));
    assert_eq!(matrix.column_group_spacing, Some(120));
    assert_eq!(matrix.row_spacing, Some(60));
    assert_eq!(matrix.columns.len(), 1);
    assert_eq!(matrix.columns[0].count, Some(2));
    assert_eq!(matrix.columns[0].justification, Some(MathAlignment::Center));

    let MathNode::EquationArray(array) = first_node(
        "<m:oMath><m:eqArr><m:eqArrPr><m:baseJc m:val=\"center\"/><m:maxDist m:val=\"4\"/>\
<m:objDist m:val=\"3\"/><m:rSp m:val=\"120\"/><m:rSpRule m:val=\"exact\"/></m:eqArrPr>\
<m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:eqArr></m:oMath>",
    ) else {
        panic!("expected an equation array");
    };
    assert_eq!(array.rows.len(), 2);
    assert_eq!(array.max_distance, Some(4));
    assert_eq!(array.object_distance, Some(3));
    assert_eq!(array.base_justification, Some(MathVerticalAlign::Center));
}

#[test]
fn decorations_and_containers_are_parsed() {
    let nodes = formula(
        "<m:oMath>\
<m:acc><m:accPr><m:chr m:val=\"⃗\"/></m:accPr><m:e><m:r><m:t>v</m:t></m:r></m:e></m:acc>\
<m:bar><m:barPr><m:pos m:val=\"bot\"/></m:barPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:bar>\
<m:groupChr><m:groupChrPr><m:chr m:val=\"⏞\"/><m:pos m:val=\"top\"/><m:vertJc m:val=\"top\"/>\
</m:groupChrPr><m:e><m:r><m:t>z</m:t></m:r></m:e></m:groupChr>\
<m:box><m:boxPr><m:aln m:val=\"center\"/><m:sp m:val=\"60\"/></m:boxPr>\
<m:e><m:r><m:t>b</m:t></m:r></m:e></m:box>\
<m:borderBox><m:borderBoxPr><m:algn m:val=\"left\"/><m:lines m:val=\"1\"/><m:shadow m:val=\"1\"/>\
</m:borderBoxPr><m:e><m:r><m:t>p</m:t></m:r></m:e><m:e><m:r><m:t>q</m:t></m:r></m:e>\
</m:borderBox>\
<m:phant><m:phantPr><m:show m:val=\"1\"/><m:transp m:val=\"1\"/><m:zeroWid m:val=\"1\"/>\
<m:zeroAsc m:val=\"1\"/><m:zeroDesc m:val=\"1\"/><m:showAll m:val=\"1\"/><m:rtl m:val=\"1\"/>\
</m:phantPr><m:e><m:r><m:t>h</m:t></m:r></m:e></m:phant>\
</m:oMath>",
    )
    .nodes;
    let MathNode::Accent(accent) = &nodes[0] else {
        panic!("expected an accent");
    };
    assert_eq!(accent.chr, Some('\u{20D7}'));
    let MathNode::Bar(bar) = &nodes[1] else {
        panic!("expected a bar");
    };
    assert_eq!(bar.position, Some(MathPosition::Bottom));
    let MathNode::GroupCharacter(group) = &nodes[2] else {
        panic!("expected a group character");
    };
    assert_eq!(group.vertical_justification, Some(MathVerticalJc::Top));
    let MathNode::Boxed(boxed) = &nodes[3] else {
        panic!("expected a box");
    };
    assert_eq!(boxed.spacing, Some(60));
    let MathNode::BorderBox(border) = &nodes[4] else {
        panic!("expected a bordered box");
    };
    assert_eq!(border.arguments.len(), 2);
    assert!(border.shadow);
    let MathNode::Phantom(phantom) = &nodes[5] else {
        panic!("expected a phantom");
    };
    assert!(phantom.show && phantom.transparent && phantom.zero_width);
    assert!(phantom.show_all && phantom.rtl);
}

#[test]
fn run_and_argument_properties_are_parsed() {
    let nodes = formula(
        "<m:oMath>\
<m:r><m:rPr><m:lit m:val=\"1\"/><m:nor m:val=\"0\"/><m:scr m:val=\"fraktur\"/>\
<m:sty m:val=\"b\"/><m:aln m:val=\"right\"/><m:brk m:val=\"2\"/>\
<m:ctrlPr><w:rPr><w:color w:val=\"FF0000\"/></w:rPr></m:ctrlPr></m:rPr><m:t>A</m:t></m:r>\
<m:box><m:e><m:argPr><m:aln m:val=\"center\"/><m:lit m:val=\"1\"/><m:nor m:val=\"1\"/>\
<m:scr m:val=\"monospace\"/><m:sty m:val=\"bi\"/></m:argPr><m:r><m:t>B</m:t></m:r></m:e></m:box>\
</m:oMath>",
    )
    .nodes;
    let MathNode::Run(run) = &nodes[0] else {
        panic!("expected a run");
    };
    assert!(run.properties.literal);
    assert!(!run.properties.normal);
    assert_eq!(run.properties.script, Some(MathScript::Fraktur));
    assert_eq!(run.properties.style, Some(MathStyle::Bold));
    assert_eq!(run.properties.alignment, Some(MathAlignment::Right));
    assert_eq!(run.properties.r#break, Some(2));
    assert!(run.properties.control.is_some());
    let MathNode::Boxed(boxed) = &nodes[1] else {
        panic!("expected a box");
    };
    let boxed_properties = &boxed.argument.properties;
    assert_eq!(boxed_properties.alignment, Some(MathAlignment::Center));
    assert_eq!(boxed_properties.literal, Some(true));
    assert_eq!(boxed_properties.normal, Some(true));
    assert_eq!(boxed_properties.script, Some(MathScript::Monospace));
    assert_eq!(boxed_properties.style, Some(MathStyle::BoldItalic));
    // The run carries no `m:rPr` of its own: the argument properties are not
    // copied onto it by the model.
    let MathNode::Run(inner) = &boxed.argument.nodes[0] else {
        panic!("expected a run");
    };
    assert_eq!(inner.properties.style, None);
    assert_eq!(inner.text, "B");
}

#[test]
fn an_unknown_construct_is_preserved_and_reported() {
    let document =
        parse_body("<m:oMath><m:futureThing><m:r><m:t>x</m:t></m:r></m:futureThing></m:oMath>");
    let feature = document.support.get("m:futureThing").expect("reported");
    assert_eq!(feature.status, SupportStatus::Partial);
    assert!(feature.first_location().is_some());
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    let expression = paragraph.inlines[0].as_math().expect("a formula");
    assert!(matches!(expression.nodes[0], MathNode::Unknown(_)));
}

#[test]
fn an_out_of_enumeration_value_is_reported_not_dropped() {
    let document = parse_body(
        "<m:oMath><m:r><m:rPr><m:sty m:val=\"nope\"/></m:rPr><m:t>x</m:t></m:r></m:oMath>",
    );
    let feature = document
        .support
        .get("m:rPr/m:sty")
        .expect("the invalid value must be reported");
    assert_eq!(feature.status, SupportStatus::Partial);
    assert!(feature
        .message
        .as_deref()
        .is_some_and(|message| message.contains("nope")));
}

#[test]
fn the_transitional_math_namespace_is_rejected() {
    // OMML is part of ISO 29500-1: only the Strict namespace is a formula, and
    // a Transitional one is reported as a foreign element, never parsed.
    let transitional = "http://schemas.openxmlformats.org/officeDocument/2006/math";
    let document = parse_body(&format!(
        "<m:oMath xmlns:m=\"{transitional}\"><m:r><m:t>x</m:t></m:r></m:oMath>"
    ));
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    assert!(paragraph
        .inlines
        .iter()
        .all(|inline| matches!(inline, Inline::Opaque(_) | Inline::Run(_))));
    assert!(!paragraph
        .inlines
        .iter()
        .any(|inline| inline.as_math().is_some()));
}

#[test]
fn a_deeply_nested_formula_is_degraded_and_reported() {
    // The nesting budget (64) stops a hostile formula long before the XML depth
    // limit. Since AUD-06 it costs that formula and not the document: one
    // hostile formula must not make a file whose text is readable unopenable.
    //
    // Seventy levels, not two hundred: two hundred nested `m:d`/`m:e` pairs are
    // 400 XML elements, and the reader's own `max_xml_depth` of 256 refuses that
    // before the formula budget is even consulted. That is the earlier defence
    // doing its job, and it is why this fixture is as deep as a formula can be
    // and still exercise the formula budget rather than the reader's.
    let deep = 70;
    let mut body = String::new();
    body.push_str("<m:oMath>");
    for _ in 0..deep {
        body.push_str("<m:d><m:e>");
    }
    body.push_str("<m:r><m:t>x</m:t></m:r>");
    for _ in 0..deep {
        body.push_str("</m:e></m:d>");
    }
    body.push_str("</m:oMath>");
    let document = parse_parts(&document_parts(
        &format!("<w:p><w:r><w:t>before</w:t></w:r>{body}<w:r><w:t>after</w:t></w:r></w:p>"),
        &[],
    ))
    .expect("a formula over budget must not take the document with it");

    assert_over_budget_formula(&document);
    assert_paragraph_text(&document, "before", "after");
}

#[test]
fn a_wide_formula_is_degraded_and_reported() {
    let mut body = String::from("<m:oMath>");
    for _ in 0..5000 {
        body.push_str("<m:r><m:t>x</m:t></m:r>");
    }
    body.push_str("</m:oMath>");
    let document = parse_parts(&document_parts(
        &format!("<w:p><w:r><w:t>before</w:t></w:r>{body}<w:r><w:t>after</w:t></w:r></w:p>"),
        &[],
    ))
    .expect("a formula over budget must not take the document with it");

    assert_over_budget_formula(&document);
    assert_paragraph_text(&document, "before", "after");
}

/// The formula is reported as `Unsupported`, at a location, and is one unknown
/// node in the model rather than 5000 of them.
fn assert_over_budget_formula(document: &strict_ooxml_wml::model::Document) {
    let use_entry = document
        .support()
        .get("m:oMath")
        .expect("m:oMath is in the report");
    assert_eq!(use_entry.status, SupportStatus::Unsupported);
    let reason = use_entry.message.as_deref().unwrap_or_default();
    assert!(
        reason.contains("max_math_nodes") || reason.contains("max_math_depth"),
        "the reason names the budget: {reason}"
    );
    assert!(
        use_entry.first_location().is_some(),
        "the record carries a location"
    );

    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("a paragraph");
    };
    let math = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Math(expression) => Some(expression),
            _ => None,
        })
        .expect("the formula is in the model");
    assert_eq!(
        math.nodes.len(),
        1,
        "the formula is one unknown node, not a partial tree"
    );
    assert!(matches!(math.nodes[0], MathNode::Unknown(_)));
}

/// The text around the formula survived.
fn assert_paragraph_text(document: &strict_ooxml_wml::model::Document, before: &str, after: &str) {
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("a paragraph");
    };
    let texts: Vec<&str> = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => run.content.iter().find_map(|content| match content {
                strict_ooxml_wml::model::inline::RunContent::Text(node) => Some(node.text.as_ref()),
                _ => None,
            }),
            _ => None,
        })
        .collect();
    assert_eq!(texts, [before, after], "the text around the formula");
}

#[test]
fn the_formula_budgets_are_the_callers() {
    // A host with a bigger stack and an appetite for a long formula can say so;
    // a hard-coded 4096 in the parser could not.
    let mut body = String::from("<m:oMath>");
    for _ in 0..40 {
        body.push_str("<m:r><m:t>x</m:t></m:r>");
    }
    body.push_str("</m:oMath>");
    let parts = document_parts(&format!("<w:p>{body}</w:p>"), &[]);

    let generous = parse_parts_with_limits(
        &parts,
        strict_ooxml_core::limits::ResourceLimits {
            max_math_nodes: 4096,
            ..strict_ooxml_core::limits::ResourceLimits::default()
        },
    )
    .expect("forty runs are inside the default budget");
    assert!(generous
        .support()
        .get("m:oMath")
        .is_some_and(|entry| entry.status == SupportStatus::Supported));

    let tight = parse_parts_with_limits(
        &parts,
        strict_ooxml_core::limits::ResourceLimits {
            max_math_nodes: 10,
            ..strict_ooxml_core::limits::ResourceLimits::default()
        },
    )
    .expect("a tight budget degrades the formula, it does not refuse the file");
    assert!(tight
        .support()
        .get("m:oMath")
        .is_some_and(|entry| entry.status == SupportStatus::Unsupported));
}

#[test]
fn the_math_namespace_constant_is_the_strict_one() {
    assert_eq!(
        M_NS, "http://purl.oclc.org/ooxml/officeDocument/math",
        "the OMML namespace must be the ISO Strict one"
    );
}
