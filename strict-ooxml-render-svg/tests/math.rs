#![allow(clippy::doc_markdown, clippy::unreadable_literal)]
//! OMML rendering tests — `STAGE-5C-TASK.md` §7.1 (layout/SVG) and §9.6
//! (MathML projection).

mod common;

use std::path::Path;

use strict_ooxml_core::opc::OpenOptions;
use strict_ooxml_render_svg::{
    math_expression_to_mathml, math_paragraph_to_mathml, render, RenderOptions,
};
use strict_ooxml_wml::model::math::{MathNode, MathRun, MathRunProperties};
use strict_ooxml_wml::model::{Block, Document, Inline};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// Renders a body fragment and returns the SVG of its single page.
fn render_body(body: &str) -> String {
    let pages = common::render_body(body);
    assert_eq!(pages.len(), 1, "one page expected");
    pages[0].svg.clone()
}

/// Renders a body fragment with custom options.
fn render_with(body: &str, options: &RenderOptions) -> Vec<strict_ooxml_render_svg::Page> {
    let (_package, document) = common::open_body(body);
    render(&document, options).expect("render")
}

/// Returns the first formula of the document.
fn first_formula(body: &str) -> strict_ooxml_wml::model::math::MathExpression {
    let (_package, document) = common::open_body(body);
    formulas(&document).into_iter().next().expect("a formula")
}

/// Every formula of the document, inline or display.
fn formulas(document: &Document) -> Vec<strict_ooxml_wml::model::math::MathExpression> {
    let mut out = Vec::new();
    for block in &document.body.blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &paragraph.inlines {
            match inline {
                Inline::Math(expression) => out.push(expression.clone()),
                Inline::MathParagraph(display) => out.push(display.expression.clone()),
                _ => {}
            }
        }
    }
    out
}

/// A run node with the given text.
fn run(text: &str) -> MathNode {
    MathNode::Run(MathRun {
        properties: MathRunProperties::default(),
        text: text.to_owned(),
        location: strict_ooxml_core::error::SourceLocation::default(),
    })
}

#[test]
fn an_inline_formula_is_drawn_with_the_bundled_math_face() {
    let svg = render_body(
        "<w:p><w:r><w:t>a</w:t></w:r><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>\
<w:r><w:t>b</w:t></w:r></w:p>",
    );
    assert!(svg.contains("font-family=\"STIX Two Math\""), "{svg}");
    assert!(svg.contains(">x</text>"), "{svg}");
    // The surrounding text keeps the body face.
    assert!(svg.contains("font-family=\"Carlito\""), "{svg}");
}

#[test]
fn a_fraction_draws_a_rule_between_its_parts() {
    let svg = render_body(
        "<w:p><m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>\
<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath></w:p>",
    );
    let rule = line_y(&svg).expect("the fraction rule");
    let (_, numerator) = position(&svg, ">a</text>").expect("numerator");
    let (_, denominator) = position(&svg, ">b</text>").expect("denominator");
    assert!(
        numerator < rule && rule < denominator,
        "{numerator} {rule} {denominator}"
    );
}

#[test]
fn a_superscript_is_raised_and_smaller_than_its_base() {
    let svg = render_body(
        "<w:p><m:oMath><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e>\
<m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath></w:p>",
    );
    let (_, base) = position(&svg, ">x</text>").expect("base");
    let (_, script) = position(&svg, ">2</text>").expect("script");
    assert!(
        script < base,
        "the superscript must be higher: {script} !< {base}"
    );
    let base_size = font_size_near(&svg, ">x</text>").expect("base size");
    let script_size = font_size_near(&svg, ">2</text>").expect("script size");
    assert!(script_size < base_size, "{script_size} !< {base_size}");
}

#[test]
fn a_subscript_is_lowered_below_its_base() {
    let svg = render_body(
        "<w:p><m:oMath><m:sSub><m:e><m:r><m:t>a</m:t></m:r></m:e>\
<m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub></m:oMath></w:p>",
    );
    let (_, base) = position(&svg, ">a</text>").expect("base");
    let (_, script) = position(&svg, ">i</text>").expect("script");
    assert!(
        script > base,
        "the subscript must be lower: {script} !> {base}"
    );
}

#[test]
fn a_radical_draws_its_sign_and_overline() {
    let svg = render_body(
        "<w:p><m:oMath><m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/>\
<m:e><m:r><m:t>2</m:t></m:r></m:e></m:rad></m:oMath></w:p>",
    );
    assert!(svg.contains("<path "), "the radical sign is missing: {svg}");
    assert!(svg.contains("<line "), "the overline is missing: {svg}");
}

#[test]
fn a_delimiter_is_drawn_as_a_stretchy_vector() {
    let svg = render_body(
        "<w:p><m:oMath><m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/>\
</m:dPr><m:e><m:r><m:t>a</m:t></m:r></m:e></m:d></m:oMath></w:p>",
    );
    assert!(
        svg.matches("<path ").count() >= 2,
        "both delimiters must be drawn: {svg}"
    );
    assert!(svg.contains(">a</text>"), "{svg}");
}

#[test]
fn a_matrix_places_every_cell_on_a_grid() {
    let svg = render_body(
        "<w:p><m:oMath><m:m><m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e>\
<m:e><m:r><m:t>2</m:t></m:r></m:e></m:mr><m:mr><m:e><m:r><m:t>3</m:t></m:r></m:e>\
<m:e><m:r><m:t>4</m:t></m:r></m:e></m:mr></m:m></m:oMath></w:p>",
    );
    for cell in [">1<", ">2<", ">3<", ">4<"] {
        assert!(svg.contains(cell), "cell {cell} is missing");
    }
    let (x1, y1) = position(&svg, ">1</text>").expect("cell 1");
    let (x2, y1b) = position(&svg, ">2</text>").expect("cell 2");
    let (x3, y2) = position(&svg, ">3</text>").expect("cell 3");
    let (x4, y2b) = position(&svg, ">4</text>").expect("cell 4");
    assert!((y1 - y1b).abs() < 1e-6, "row 1 must share a baseline");
    assert!((y2 - y2b).abs() < 1e-6, "row 2 must share a baseline");
    assert!(x2 > x1 && x4 > x3, "columns must advance in x");
    assert!(y2 > y1, "rows must advance in y");
}

#[test]
fn an_nary_operator_places_its_limits_over_and_under() {
    let svg = render_body(
        "<w:p><m:oMath><m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/>\
</m:naryPr><m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup>\
<m:e><m:r><m:t>k</m:t></m:r></m:e></m:nary></m:oMath></w:p>",
    );
    assert!(svg.contains(">∑</text>"), "the operator is missing: {svg}");
    let (op_x, _) = position(&svg, ">∑</text>").expect("operator");
    let (lower_x, lower_y) = position(&svg, ">i</text>").expect("lower limit");
    let (upper_x, upper_y) = position(&svg, ">n</text>").expect("upper limit");
    let (body_x, _) = position(&svg, ">k</text>").expect("operand");
    assert!(body_x > op_x, "the operand follows the operator");
    assert!(lower_y > upper_y, "the lower limit is below the upper one");
    assert!(
        (lower_x - upper_x).abs() < 6.0,
        "undOvr centres both limits: {lower_x} vs {upper_x}"
    );
}

#[test]
fn a_box_and_a_border_box_draw_rectangles() {
    let svg = render_body(
        "<w:p><m:oMath><m:box><m:e><m:r><m:t>b</m:t></m:r></m:e></m:box>\
<m:borderBox><m:e><m:r><m:t>a</m:t></m:r></m:e></m:borderBox></m:oMath></w:p>",
    );
    // Two content frames (`m:box` and `m:borderBox`); the page background is a
    // third, filled rectangle.
    assert_eq!(
        svg.matches("fill=\"none\" stroke=\"#000000\"").count(),
        2,
        "{svg}"
    );
}

#[test]
fn a_phantom_reserves_space_without_drawing() {
    let without = render_body(
        "<w:p><w:r><w:t>a</w:t></w:r><m:oMath><m:phant><m:phantPr><m:show m:val=\"0\"/>\
</m:phantPr><m:e><m:r><m:t>ppp</m:t></m:r></m:e></m:phant></m:oMath>\
<w:r><w:t>b</w:t></w:r></w:p>",
    );
    assert!(!without.contains(">ppp</text>"), "{without}");
    let plain = render_body("<w:p><w:r><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r></w:p>");
    let (x_phantom, _) = position(&without, ">b</text>").expect("b");
    let (x_plain, _) = position(&plain, ">b</text>").expect("b");
    assert!(
        x_phantom > x_plain,
        "the phantom must reserve space: {x_phantom} vs {x_plain}"
    );
}

#[test]
fn a_display_formula_honours_its_justification() {
    let centered = render_body(
        "<w:p><m:oMathPara><m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>\
<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara></w:p>",
    );
    let left = render_body(
        "<w:p><m:oMathPara><m:oMathParaPr><m:jc m:val=\"left\"/></m:oMathParaPr>\
<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara></w:p>",
    );
    let (x_center, _) = position(&centered, ">x</text>").expect("centred");
    let (x_left, _) = position(&left, ">x</text>").expect("left");
    assert!(
        x_center > x_left,
        "m:jc=center must place the formula further right ({x_center} vs {x_left})"
    );
}

#[test]
fn no_math_skips_formulas_entirely() {
    let pages = render_with(
        "<w:p><w:r><w:t>a</w:t></w:r><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></w:p>",
        &RenderOptions::default().math(false),
    );
    assert!(!pages[0].svg.contains(">x</text>"), "the formula was drawn");
    assert!(pages[0].svg.contains(">a</text>"), "the text was lost");
}

#[test]
fn rendering_is_deterministic() {
    let body = "<w:p><m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>\
<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath></w:p>";
    let first = render_body(body);
    let second = render_body(body);
    assert_eq!(first, second, "the output must be byte-stable");
}

#[test]
fn every_coordinate_is_finite_and_the_svg_is_well_formed() {
    let svg = render_body(
        "<w:p><m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>\
<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e>\
<m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup><m:d><m:e><m:r><m:t>q</m:t></m:r></m:e></m:d>\
</m:oMath></w:p>",
    );
    assert!(!svg.contains("NaN") && !svg.contains("inf"), "{svg}");
    // An independent XML parser must accept the document.
    roxmltree::Document::parse(&svg).expect("valid SVG");
}

#[test]
fn the_mathml_projection_mirrors_the_model() {
    let expression = first_formula(
        "<w:p><m:oMath><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num>\
<m:den><m:r><m:t>2</m:t></m:r></m:den></m:f><m:rad><m:deg/><m:e><m:r><m:t>x</m:t></m:r>\
</m:e></m:rad><m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr>\
<m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup>\
<m:e><m:r><m:t>k</m:t></m:r></m:e></m:nary></m:oMath></w:p>",
    );
    let mathml = math_expression_to_mathml(&expression).expect("mathml");
    assert!(mathml.contains("<mfrac>"), "{mathml}");
    assert!(mathml.contains("<msqrt>"), "{mathml}");
    assert!(mathml.contains("<munderover>"), "{mathml}");
    // The projection is a pure function of the model.
    assert_eq!(
        mathml,
        math_expression_to_mathml(&expression).expect("mathml")
    );
    // And it is itself well-formed XML.
    roxmltree::Document::parse(&mathml).expect("valid MathML");
}

#[test]
fn a_display_formula_projects_with_its_justification() {
    let (_package, document) = common::open_body(
        "<w:p><m:oMathPara><m:oMathParaPr><m:jc m:val=\"right\"/></m:oMathParaPr>\
<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara></w:p>",
    );
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    let display = paragraph.inlines[0]
        .as_math_paragraph()
        .expect("a display formula");
    let mathml = math_paragraph_to_mathml(display).expect("mathml");
    assert!(mathml.contains("display=\"block\""), "{mathml}");
    assert!(mathml.contains("data-justification=\"right\""), "{mathml}");
}

#[test]
fn an_unmodelled_construct_is_reported_by_the_mathml_projection() {
    let expression = strict_ooxml_wml::model::math::MathExpression {
        nodes: vec![
            run("x"),
            MathNode::Unknown(Box::new(strict_ooxml_wml::model::math::UnknownMathNode {
                local: "m:newThing".into(),
                location: strict_ooxml_core::error::SourceLocation::default(),
            })),
        ],
        location: strict_ooxml_core::error::SourceLocation::default(),
    };
    let error = math_expression_to_mathml(&expression).expect_err("must be reported");
    assert!(error.to_string().contains("m:newThing"), "{error}");
}

/// The Stage-5C fixture renders and is covered by the SSIM gate.
#[test]
fn the_stage5c_fixture_renders_two_pages() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-stage5c.docx");
    assert!(path.is_file(), "missing fixture {}", path.display());
    let package =
        strict_ooxml_core::opc::Package::open_path(&path, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let pages = render(&document, &RenderOptions::default()).expect("render");
    assert_eq!(pages.len(), 2, "the fixture must keep its page invariant");
    let svg: String = pages.iter().map(|page| page.svg.as_str()).collect();
    for expected in [
        "STIX Two Math",
        "<line ",
        "<path ",
        "<rect ",
        ">∑</text>",
        ">sin</text>",
    ] {
        assert!(svg.contains(expected), "missing {expected}");
    }
}

/// Returns the `(x, y)` of the `<text>` element holding `marker`.
fn position(svg: &str, marker: &str) -> Option<(f64, f64)> {
    let index = svg.find(marker)?;
    let start = svg[..index].rfind("<text ")?;
    let tag = &svg[start..index];
    Some((attribute(tag, "x")?, attribute(tag, "y")?))
}

/// Returns the `font-size` of the `<text>` element holding `marker`.
fn font_size_near(svg: &str, marker: &str) -> Option<f64> {
    let index = svg.find(marker)?;
    let start = svg[..index].rfind("<text ")?;
    let tag = &svg[start..index];
    attribute(tag, "font-size")
}

/// The `y` of the first `<line>` element.
fn line_y(svg: &str) -> Option<f64> {
    let start = svg.find("<line ")?;
    let end = start + svg[start..].find('>')? + 1;
    attribute(&svg[start..end], "y1")
}

/// Reads a numeric attribute from an SVG tag.
fn attribute(tag: &str, name: &str) -> Option<f64> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    tag[start..end].parse().ok()
}
