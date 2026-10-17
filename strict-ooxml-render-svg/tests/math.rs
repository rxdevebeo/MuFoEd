#![allow(clippy::doc_markdown, clippy::unreadable_literal)]
//! OMML rendering tests — `STAGE-5C-TASK.md` §7.1 (layout/SVG) and §9.6
//! (MathML projection).

mod common;

use strict_ooxml_render_svg::font::{BuiltinFontProvider, FontProvider};

#[test]
fn uniform_equation_font_size_scales_inline_and_display_constructions() {
    for display in [false, true] {
        let render = |size| {
            let formula = format!("<m:oMath><m:f><m:num><m:r><w:rPr><w:sz w:val=\"{size}\"/></w:rPr><m:t>a</m:t></m:r></m:num><m:den><m:r><w:rPr><w:sz w:val=\"{size}\"/></w:rPr><m:t>b</m:t></m:r></m:den></m:f></m:oMath>");
            render_body(&if display {
                format!("<w:p><m:oMathPara>{formula}</m:oMathPara></w:p>")
            } else {
                format!("<w:p>{formula}</w:p>")
            })
        };
        let small = render(22);
        let large = render(44);
        let small_size = font_size_near(&small, ">a</text>").unwrap();
        let large_size = font_size_near(&large, ">a</text>").unwrap();
        assert!((large_size / small_size - 2.0).abs() < 0.01);
        let span = |svg: &str| {
            (position(svg, ">b</text>").unwrap().1 - position(svg, ">a</text>").unwrap().1).abs()
        };
        assert!((span(&large) / span(&small) - 2.0).abs() < 0.03);
    }
}

#[test]
fn left_and_right_script_pairs_leave_a_visible_gap() {
    let font = BuiltinFontProvider::new();
    for tag in ["sPre", "sSubSup"] {
        let scripts =
            "<m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>j</m:t></m:r></m:sup>";
        let base = "<m:e><m:r><m:t>F</m:t></m:r></m:e>";
        let arguments = if tag == "sPre" {
            format!("{scripts}{base}")
        } else {
            format!("{base}{scripts}")
        };
        let svg = render_body(&format!(
            "<w:p><m:oMath><m:{tag}>{arguments}</m:{tag}></m:oMath></w:p>"
        ));
        let (_, lower_baseline) = position(&svg, ">i</text>").expect("subscript");
        let (_, upper_baseline) = position(&svg, ">j</text>").expect("superscript");
        let lower_size = font_size_near(&svg, ">i</text>").expect("size");
        let upper_size = font_size_near(&svg, ">j</text>").expect("size");
        let lower_top = lower_baseline
            - font
                .glyph_vertical_em("Cambria Math", 'i', false, true)
                .expect("glyph")
                .0
                * lower_size;
        let upper_bottom = upper_baseline
            + font
                .glyph_vertical_em("Cambria Math", 'j', false, true)
                .expect("glyph")
                .1
                * upper_size;
        assert!(
            lower_top - upper_bottom > 0.5,
            "{tag}: script glyphs must not touch: sub {lower_baseline}/{lower_top} sup {upper_baseline}/{upper_bottom}, sizes {lower_size}/{upper_size}"
        );
    }
}

#[test]
fn radical_degree_sits_inside_the_radical_height() {
    let svg = render_body("<w:p><m:oMath><m:rad><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad></m:oMath></w:p>");
    let (_, degree) = position(&svg, ">3</text>").expect("degree");
    let (_, radicand) = position(&svg, ">x</text>").expect("radicand");
    let top_rule = line_y(&svg).expect("rule");
    assert!(
        degree > top_rule && degree < radicand,
        "degree baseline must sit inside the radical's height"
    );
}

#[test]
fn display_formula_preserves_explicit_paragraph_and_math_surround_spacing() {
    let baseline = "<w:p><w:r><w:t>before</w:t></w:r></w:p><w:p><m:oMathPara><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara></w:p><w:p><w:r><w:t>after</w:t></w:r></w:p>";
    let original = render_body(baseline);
    let configured = baseline.replace(
        "<w:p><m:oMathPara>",
        "<w:p><w:pPr><w:spacing w:before=\"300\" w:after=\"450\"/></w:pPr><m:oMathPara>",
    );
    let (_, mut parsed) = common::open_body(&configured);
    parsed.settings.math_properties = Some(strict_ooxml_wml::model::settings::MathProperties {
        pre_space: Some("200".into()),
        post_space: Some("400".into()),
        ..Default::default()
    });
    let pages = render(&parsed, &RenderOptions::default()).expect("render");
    let (_, equation_before) = position(&original, ">x</text>").expect("original equation");
    let (_, equation_after) = position(&pages[0].svg, ">x</text>").expect("equation");
    let (_, text_before) = position(&original, ">after</text>").expect("original after");
    let (_, text_after) = position(&pages[0].svg, ">after</text>").expect("after");
    assert!((equation_after - equation_before - 500.0 / 15.0).abs() < 0.01);
    assert!((text_after - text_before - 1350.0 / 15.0).abs() < 0.01);
}

#[test]
fn justified_inline_formula_moves_with_the_spaces_before_it() {
    let body = "<w:p><w:pPr><w:jc w:val=\"both\"/></w:pPr><w:r><w:t xml:space=\"preserve\">before words </w:t></w:r><m:oMath><m:r><m:t>MARKER</m:t></m:r></m:oMath><w:r><w:t xml:space=\"preserve\"> after words </w:t></w:r><w:r><w:br/></w:r><w:r><w:t>tail</w:t></w:r></w:p>";
    let options = RenderOptions {
        wps_times_calibration: false,
        cell_no_wrap: strict_ooxml_render_svg::CellNoWrap::Spec,
        ..RenderOptions::default()
    };
    let pages = render_with(body, &options);
    let (formula_x, _) = position(&pages[0].svg, ">MARKER</text>").expect("formula");
    let (before_x, _) = position(&pages[0].svg, ">words </text>").expect("preceding word");
    let (after_x, _) = position(&pages[0].svg, ">after </text>").expect("following word");
    assert!(
        formula_x > before_x + 60.0,
        "expanded space before formula: {before_x}, {formula_x}"
    );
    assert!(
        formula_x + 50.0 < after_x,
        "formula must not overlap following text: {formula_x}, {after_x}"
    );
}

#[test]
fn tall_inline_fraction_reserves_room_for_the_following_line() {
    let body = "<w:p><m:oMath><m:f><m:num><m:r><m:t>upper</m:t></m:r></m:num><m:den><m:r><m:t>lower</m:t></m:r></m:den></m:f></m:oMath><w:r><w:br/></w:r><w:r><w:t>following</w:t></w:r></w:p>";
    let pages = common::render_body(body);
    let svg = &pages[0].svg;
    assert!(
        pages.iter().all(|page| !page
            .warnings
            .iter()
            .any(|warning| warning.starts_with("render.math-line-overflow"))),
        "the compact fraction must fit its reserved line box"
    );
    let fraction_size = font_size_near(svg, ">lower</text>").expect("fraction size");
    let text_size = font_size_near(svg, ">following</text>").expect("text size");
    assert!(
        fraction_size < text_size,
        "inline fraction parts must use compact type"
    );
    let (_, denominator) = position(svg, ">lower</text>").expect("denominator");
    let (_, following) = position(svg, ">following</text>").expect("following");
    let font = BuiltinFontProvider::new();
    let denominator_depth = "lower"
        .chars()
        .filter_map(|ch| font.glyph_vertical_em("Cambria Math", ch, false, true))
        .map(|bounds| bounds.1 * fraction_size)
        .fold(0.0_f64, f64::max);
    let following_ascent = "following"
        .chars()
        .filter_map(|ch| font.glyph_vertical_em("Calibri", ch, false, false))
        .map(|bounds| bounds.0 * text_size)
        .fold(0.0_f64, f64::max);
    assert!(
        following - denominator > denominator_depth + following_ascent,
        "following line must clear denominator: {denominator}, {following}"
    );
}

#[test]
fn long_inline_equation_wraps_at_operators_without_losing_terms() {
    let formula = (0..80)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join("+");
    let pages = common::render_body(&format!(
        "<w:p><m:oMath><m:r><m:t>{formula}</m:t></m:r></m:oMath></w:p>"
    ));
    let mut text = String::new();
    let mut baselines = Vec::new();
    for page in &pages {
        assert!(
            !page
                .warnings
                .iter()
                .any(|w| w.starts_with("render.object-overflow")
                    || w.starts_with("render.math-overflow")),
            "{:?}",
            page.warnings
        );
        let mut offset = 0;
        for element in page.svg.split("<text ").skip(1) {
            offset = page.svg[offset..]
                .find("<text ")
                .map_or(offset, |found| offset + found + 1);
            let (tag, rest) = element.split_once('>').expect("text");
            text.push_str(rest.split_once("</text>").expect("closing").0);
            let (_, dy) = group_offset(&page.svg, offset);
            baselines.push(attribute(tag, "y").expect("baseline") + dy);
        }
    }
    assert_eq!(text, formula);
    assert!(
        baselines.iter().any(|y| (*y - baselines[0]).abs() > 0.25),
        "equation must occupy several lines"
    );
}

#[test]
fn oversized_atomic_formula_is_reported_without_splitting_its_fraction() {
    let numerator = "abcdefghijk".repeat(20);
    let pages = common::render_body(&format!("<w:p><m:oMath><m:f><m:num><m:r><m:t>{numerator}</m:t></m:r></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den></m:f></m:oMath></w:p>"));
    assert!(pages[0]
        .warnings
        .iter()
        .any(|w| w.starts_with("render.math-overflow")));
    assert!(pages[0].svg.contains(&format!(">{numerator}</text>")));
    assert_eq!(pages[0].svg.matches("<line ").count(), 1);
}

#[test]
fn math_field_instructions_are_hidden_and_cached_result_is_kept() {
    let body = "<w:p><m:oMath><m:r><m:t>x=1 (</m:t></m:r><m:r><w:fldChar w:fldCharType=\"begin\"/></m:r><m:r><m:t> SEQ Equation \\* ARABIC </m:t></m:r><m:r><w:fldChar w:fldCharType=\"separate\"/></m:r><m:r><m:t>8</m:t></m:r><m:r><w:fldChar w:fldCharType=\"end\"/></m:r><m:r><m:t>)</m:t></m:r></m:oMath></w:p>";
    let svg = render_body(body);
    assert!(!svg.contains("SEQ Equation"));
    assert!(svg.contains(">8</text>"));
    let (_, parsed) = common::open_body(body);
    let Block::Paragraph(p) = &parsed.body.blocks[0] else {
        panic!("paragraph")
    };
    let Inline::Math(expr) = &p.inlines[0] else {
        panic!("math")
    };
    let projected = math_expression_to_mathml(expr).expect("MathML");
    assert!(!projected.contains("SEQ Equation"));
    assert!(projected.contains('8'));
}

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

#[test]
fn adjacent_display_equations_share_a_line_until_an_explicit_break() {
    for (separator, broken) in [("", false), ("<w:br/>", true), ("<w:cr/>", true)] {
        let body = format!("<w:p><m:oMathPara><m:oMath><m:r><m:t>x=1</m:t>{separator}</m:r></m:oMath><m:oMath><m:r><m:t>y=2</m:t></m:r></m:oMath></m:oMathPara></w:p>");
        let svg = render_body(&body);
        let (x, y) = position(&svg, ">x=1</text>").expect("first equation");
        let (next_x, next_y) = position(&svg, ">y=2</text>").expect("second equation");
        if broken {
            assert!(next_y > y + 5.0, "explicit break must create a new line");
        } else {
            assert!(
                (next_y - y).abs() < 0.01,
                "equation boundary is not a break"
            );
            assert!(next_x > x, "equations must not overlap");
        }
        let (_, parsed) = common::open_body(&body);
        let display = parsed.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .inlines[0]
            .as_math_paragraph()
            .expect("math paragraph");
        assert_eq!(display.equations.len(), 2, "OMML objects stay separate");
        let mathml = math_paragraph_to_mathml(display).expect("MathML");
        assert_eq!(mathml.contains("linebreak=\"newline\""), broken);
    }
}

#[test]
fn adjacent_display_equations_wrap_without_losing_terms() {
    use std::fmt::Write;
    let mut equations = String::new();
    for index in 0..60 {
        write!(
            equations,
            "<m:oMath><m:r><m:t>x{index}</m:t></m:r></m:oMath>"
        )
        .expect("string");
    }
    let svg = render_body(&format!(
        "<w:p><m:oMathPara>{equations}</m:oMathPara></w:p>"
    ));
    let (_, first_y) = position(&svg, ">x0</text>").expect("first");
    let (_, last_y) = position(&svg, ">x59</text>").expect("last");
    assert!(last_y > first_y + 5.0, "the paragraph must wrap");
    for index in 0..60 {
        let (x, _) = position(&svg, &format!(">x{index}</text>")).expect("each equation");
        assert!(x < 720.0, "equation must stay within the text column");
    }
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
                Inline::MathParagraph(display) => {
                    out.extend(display.equations.iter().cloned());
                }
                _ => {}
            }
        }
    }
    out
}

/// A run node with the given text.
fn run(text: &str) -> MathNode {
    MathNode::Run(MathRun {
        field_char: None,
        properties: MathRunProperties::default(),
        run_properties: None,
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
    assert!(svg.contains("font-family=\"Strict Math\""));
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
fn inline_reference_brackets_surround_the_number_above_its_baseline() {
    for number in ["1", "17"] {
        let svg = render_body(&format!(
            "<w:p><m:oMath><m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"]\"/>\
</m:dPr><m:e><m:r><m:t>{number}</m:t></m:r></m:e></m:d></m:oMath>\
<w:r><w:tab/><w:t>Reference</w:t></w:r></w:p>"
        ));
        // One-line content keeps the font's own brackets, on the number's
        // baseline and around it, as Word and WPS draw them (WQ-030).
        let (x, baseline) = position(&svg, &format!(">{number}</text>")).expect("number");
        let (open_x, open_baseline) = position(&svg, ">[</text>").expect("[");
        let (close_x, close_baseline) = position(&svg, ">]</text>").expect("]");
        assert!(open_x < x && x < close_x, "{open_x} < {x} < {close_x}");
        assert!((open_baseline - baseline).abs() < 0.5, "{svg}");
        assert!((close_baseline - baseline).abs() < 0.5, "{svg}");
        assert_eq!(svg.matches("<path ").count(), 0, "{svg}");
    }
}

#[test]
fn a_delimiter_is_drawn_as_a_stretchy_vector() {
    // A delimiter around content taller than a line (here a fraction) is a
    // stretched outline.
    let svg = render_body(
        "<w:p><m:oMath><m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/>\
</m:dPr><m:e><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>\
<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:e></m:d></m:oMath></w:p>",
    );
    assert!(
        svg.matches("<path ").count() >= 2,
        "both delimiters must be drawn: {svg}"
    );
    assert!(svg.contains(">a</text>"), "{svg}");
    // Around one line of text the font's glyphs are used instead (WQ-030).
    let svg = render_body(
        "<w:p><m:oMath><m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/>\
</m:dPr><m:e><m:r><m:t>a</m:t></m:r></m:e></m:d></m:oMath></w:p>",
    );
    assert!(
        svg.contains(">(</text>") && svg.contains(">)</text>"),
        "{svg}"
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

fn check_display_operator_limits(
    assets: Option<&std::sync::Arc<strict_ooxml_render_svg::font::FontAssets>>,
) {
    use strict_ooxml_render_svg::{Item, RenderOptions};
    for operator in ['∑', '∏', '∫'] {
        for grow in [false, true] {
            let body = format!("<w:p><m:oMathPara><m:oMath><m:nary><m:naryPr><m:chr m:val=\"{operator}\"/><m:limLoc m:val=\"undOvr\"/><m:grow m:val=\"{}\"/></m:naryPr><m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary></m:oMath></m:oMathPara></w:p>", u8::from(grow));
            let xml = common::document(&body);
            let bytes = strict_ooxml_testkit::DocxBuilder::strict()
                .part("word/document.xml", xml.as_bytes())
                .build();
            let package = strict_ooxml_core::opc::Package::open_reader(
                &bytes[..],
                &strict_ooxml_core::opc::OpenOptions::default(),
            )
            .unwrap();
            let document = strict_ooxml_wml::parse_document(
                &package,
                &strict_ooxml_wml::ParseOptions::default(),
            )
            .unwrap();
            let options = RenderOptions {
                font_assets: assets.cloned(),
                ..RenderOptions::default()
            };
            let pages =
                strict_ooxml_render_svg::place_pages(&document, &options, Some(&package)).unwrap();
            let leaves: Vec<_> = pages[0].items.iter().flat_map(Item::leaves).collect();
            let text = |value: &str| {
                leaves
                    .iter()
                    .find_map(|i| match i {
                        Item::Text(t) if t.text == value => Some(t),
                        _ => None,
                    })
                    .unwrap()
            };
            let operand = text("x");
            let em = operand.size_px;
            let glyph = leaves
                .iter()
                .find_map(|i| match i {
                    Item::Path(p) if p.h > em => Some(p),
                    _ => None,
                })
                .expect("display operator must use its large form even with a tall font line box");
            assert!(
                glyph.h >= 1.8 * em && glyph.h <= 2.4 * em,
                "{operator}: {} / {em}",
                glyph.h
            );
            let lower = text("i");
            let upper = text("n");
            let bottom = glyph.y + glyph.h;
            assert!(
                lower.baseline > bottom && lower.baseline - bottom < em,
                "{operator} lower limit is detached: {} vs {bottom}",
                lower.baseline
            );
            assert!(
                upper.baseline < glyph.y && glyph.y - upper.baseline < em,
                "upper limit detached"
            );
            let center = glyph.x + glyph.w / 2.0;
            assert!((lower.x + lower.width / 2.0 - center).abs() < 1.0);
            assert!((upper.x + upper.width / 2.0 - center).abs() < 1.0);
        }
    }
}

#[test]
fn nary_limits_use_ink_with_bundled_and_supplied_math_fonts() {
    use strict_ooxml_render_svg::font::{FontAsset, FontAssetLimits, FontAssets};
    check_display_operator_limits(None);
    // A caller-supplied font follows the same program path as system and
    // embedded faces, including OS/2 Windows line metrics. Keep this portable.
    let source =
        strict_ooxml_render_svg::font::builtin::face_source("Cambria Math", false, false).unwrap();
    let asset = FontAsset::new("Cambria Math", false, false, source.data.to_vec());
    check_display_operator_limits(Some(&std::sync::Arc::new(
        FontAssets::new(vec![asset], FontAssetLimits::default()).unwrap(),
    )));
}

#[test]
#[cfg(feature = "system-fonts")]
fn nary_limits_stay_attached_with_installed_cambria_math() {
    use strict_ooxml_render_svg::font::{system, FontAssetLimits, FontAssets};
    let assets = system::assets_for(
        &["Cambria Math".to_owned()],
        system::index(),
        FontAssetLimits::default(),
    )
    .unwrap();
    if assets.is_empty() {
        return;
    }
    check_display_operator_limits(Some(&std::sync::Arc::new(
        FontAssets::new(assets, FontAssetLimits::default()).unwrap(),
    )));
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
    let ink = common::without_font_faces(&svg);
    assert!(!ink.contains("NaN") && !ink.contains("inf"), "{ink}");
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

/// STAGE-5C-REWORK-1 C1: `m:eqArr` row spacing is `ST_UnsignedTwipsMeasure`,
/// not a point value. Reading `m:rSp="120"` as 120 pt made a two-row array
/// 175 px tall instead of 35 px and blew the display block apart.
#[test]
fn an_equation_array_row_spacing_is_twips() {
    let narrow = common::without_font_faces(&render_body(
        "<w:p><m:oMathPara><m:oMath><m:eqArr><m:eqArrPr/><m:e><m:r><m:t>a</m:t></m:r></m:e>\
<m:e><m:r><m:t>b</m:t></m:r></m:e></m:eqArr></m:oMath></m:oMathPara></w:p>",
    ));
    let wide = common::without_font_faces(&render_body(
        "<w:p><m:oMathPara><m:oMath><m:eqArr><m:eqArrPr><m:rSpRule m:val=\"exact\"/>\
<m:rSp m:val=\"120\"/></m:eqArrPr><m:e><m:r><m:t>a</m:t></m:r></m:e>\
<m:e><m:r><m:t>b</m:t></m:r></m:e></m:eqArr></m:oMath></m:oMathPara></w:p>",
    ));
    let row = |svg: &str| -> f64 {
        // The two rows are the only `<text>` runs; their baselines differ by the
        // row pitch.
        let ys: Vec<f64> = svg
            .match_indices("Strict Math")
            .map(|(index, _)| {
                let tag = &svg[index - 40..index];
                attribute(tag, "y").expect("baseline")
            })
            .collect();
        ys.windows(2)
            .map(|pair| pair[1] - pair[0])
            .fold(0.0, f64::max)
    };
    let narrow_pitch = row(&narrow);
    let wide_pitch = row(&wide);
    // 120 twips = 6 pt = 8 px of extra pitch.
    assert!(
        (wide_pitch - narrow_pitch - 8.0).abs() < 1.0,
        "120 twips must add 8px of row pitch: {narrow_pitch} -> {wide_pitch}"
    );
    // Without the rule the natural row pitch is one line of the math font.
    assert!(
        narrow_pitch > 14.0 && narrow_pitch < 20.0,
        "a two-row array must be about one line per row, got {narrow_pitch}"
    );
}

/// STAGE-5C-REWORK-1 C1: `w:docGrid` without `w:type` is *no* document grid
/// (ISO/IEC 29500-1 §17.6.6), so the line pitch must not snap the text lines.
#[test]
fn a_default_document_grid_does_not_snap_line_heights() {
    let body = "<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>";
    let free = render_body(&format!(
        "<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr></w:pPr></w:p>{body}"
    ));
    let default_grid = render_body(&format!(
        "<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:docGrid w:linePitch=\"360\"/></w:sectPr></w:pPr></w:p>{body}"
    ));
    let line_grid = render_body(&format!(
        "<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:docGrid w:type=\"lines\" w:linePitch=\"360\"/></w:sectPr></w:pPr></w:p>{body}"
    ));
    let pitch = |svg: &str| {
        let ys: Vec<f64> = svg
            .match_indices(">one</text>")
            .chain(svg.match_indices(">two</text>"))
            .map(|(index, _)| {
                let start = svg[..index].rfind("<text ").expect("text");
                attribute(&svg[start..index], "y").expect("baseline")
            })
            .collect();
        ys[1] - ys[0]
    };
    assert!(
        (pitch(&default_grid) - pitch(&free)).abs() < 0.01,
        "the default grid must not snap lines: {} vs {}",
        pitch(&default_grid),
        pitch(&free)
    );
    assert!(
        pitch(&line_grid) > pitch(&free),
        "w:type=\"lines\" must snap to the 360-twip grid"
    );
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
        "Strict Math",
        "<line ",
        "<path ",
        "<rect ",
        ">∑</text>",
        ">sin</text>",
    ] {
        assert!(svg.contains(expected), "missing {expected}");
    }
}

#[test]
fn ctrl_pr_invalid_color_uses_default_fill() {
    // AUD-77: auto / garbage → default fill; no `#auto` / `#zz` in the SVG.
    for val in ["auto", "zz"] {
        let pages = render_with(
            &format!(
                "<w:p><m:oMath><m:f><m:fPr><m:ctrlPr><w:rPr>\
<w:color w:val=\"{val}\"/></w:rPr></m:ctrlPr></m:fPr>\
<m:num><m:r><m:t>a</m:t></m:r></m:num>\
<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath></w:p>"
            ),
            &RenderOptions::default(),
        );
        assert_eq!(pages.len(), 1, "{val}");
        let svg = &pages[0].svg;
        assert!(
            svg.contains("fill=\"#000000\""),
            "default fill missing for {val}: {svg}"
        );
        assert!(
            !svg.contains(&format!("fill=\"#{val}\"")),
            "raw invalid colour leaked for {val}: {svg}"
        );
        assert!(
            pages[0]
                .warnings
                .iter()
                .any(|w| w.contains("render.math-ctrlpr-color")),
            "expected colour warning for {val}: {:?}",
            pages[0].warnings
        );
    }
}

/// Returns the `(x, y)` of the `<text>` element holding `marker`.
fn position(svg: &str, marker: &str) -> Option<(f64, f64)> {
    let index = svg.find(marker)?;
    let start = svg[..index].rfind("<text ")?;
    let tag = &svg[start..index];
    let (dx, dy) = group_offset(svg, start);
    Some((attribute(tag, "x")? + dx, attribute(tag, "y")? + dy))
}

/// The translation of the `<g>` elements still open at byte `index`: inline
/// formulas are painted in a group placed by its `transform`.
fn group_offset(svg: &str, index: usize) -> (f64, f64) {
    let mut open: Vec<(f64, f64)> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = svg[cursor..index].find("<g").map(|at| cursor + at) {
        let close = svg[cursor..found].matches("</g>").count();
        for _ in 0..close {
            open.pop();
        }
        let end = found + svg[found..].find('>').unwrap_or(0);
        let tag = &svg[found..end];
        let shift = tag
            .split_once("transform=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map_or((0.0, 0.0), |(transform, _)| translation(transform));
        open.push(shift);
        cursor = end;
    }
    for _ in 0..svg[cursor..index].matches("</g>").count() {
        open.pop();
    }
    open.iter()
        .fold((0.0, 0.0), |(x, y), (dx, dy)| (x + dx, y + dy))
}

/// The translation part of an SVG `matrix(...)` or `translate(...)`.
fn translation(transform: &str) -> (f64, f64) {
    let numbers = |body: &str| -> Vec<f64> {
        body.split([' ', ','])
            .filter_map(|value| value.parse().ok())
            .collect()
    };
    if let Some(body) = transform
        .strip_prefix("matrix(")
        .and_then(|rest| rest.split_once(')'))
    {
        let values = numbers(body.0);
        if let [_, _, _, _, e, f] = values[..] {
            return (e, f);
        }
    }
    if let Some(body) = transform
        .strip_prefix("translate(")
        .and_then(|rest| rest.split_once(')'))
    {
        let values = numbers(body.0);
        return (
            values.first().copied().unwrap_or(0.0),
            values.get(1).copied().unwrap_or(0.0),
        );
    }
    (0.0, 0.0)
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
    Some(attribute(&svg[start..end], "y1")? + group_offset(svg, start).1)
}

/// Reads a numeric attribute from an SVG tag.
fn attribute(tag: &str, name: &str) -> Option<f64> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    tag[start..end].split_whitespace().next()?.parse().ok()
}

#[test]
fn script_alphabets_keep_source_text_and_use_distinct_faces() {
    for (script, family) in [
        ("script", "Strict Math Script"),
        ("fraktur", "Strict Math Fraktur"),
        ("double-struck", "Strict Math DoubleStruck"),
        ("sans-serif", "Arimo"),
        ("monospace", "Cousine"),
    ] {
        let pages = render_with(&format!("<w:p><m:oMath><m:r><m:rPr><m:scr m:val=\"{script}\"/><m:sty m:val=\"p\"/></m:rPr><m:t>R+3</m:t></m:r></m:oMath></w:p>"), &RenderOptions::default());
        assert_eq!(pages.len(), 1);
        assert!(
            pages[0].svg.contains(&format!("font-family=\"{family}\"")),
            "{script} family"
        );
        assert!(
            pages[0].svg.contains(">R+3</text>"),
            "source text preserved for {script}"
        );
    }
}
