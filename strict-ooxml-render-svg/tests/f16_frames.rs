//! A18: adjacent paragraphs with one frame signature share an origin.

#![allow(clippy::expect_used, clippy::format_push_string)]

mod common;

use common::open_body;
use strict_ooxml_render_svg::{render, RenderOptions};

fn framed(x_twips: i32) -> String {
    let body = format!(
        "<w:p><w:pPr><w:framePr w:w=\"4000\" w:h=\"800\" w:x=\"{x_twips}\" w:y=\"2000\"/></w:pPr>\
<w:r><w:t>alpha</w:t></w:r></w:p>\
<w:p><w:pPr><w:framePr w:w=\"4000\" w:h=\"800\" w:x=\"{x_twips}\" w:y=\"2000\"/></w:pPr>\
<w:r><w:t>beta</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    let (_package, parsed) = open_body(&body);
    render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

fn text_at(svg: &str, needle: &str) -> (f64, f64) {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let node = document
        .descendants()
        .find(|node| {
            node.is_element()
                && node.tag_name().name() == "text"
                && node.text().is_some_and(|text| text.contains(needle))
        })
        .expect(needle);
    let coord = |name: &str| node.attribute(name).unwrap().parse().unwrap();
    (coord("x"), coord("y"))
}

#[test]
fn f16_frame_translation_moves_all_children() {
    let svg = framed(3000);
    let (alpha_x, alpha_y) = text_at(&svg, "alpha");
    let (beta_x, beta_y) = text_at(&svg, "beta");
    assert!((alpha_x - 200.0).abs() <= 0.25, "alpha x {alpha_x}");
    assert!(
        alpha_y > 133.333 && alpha_y < 133.333 + 20.0,
        "alpha baseline {alpha_y} must sit in the frame that starts at 133.333"
    );
    assert!((beta_x - alpha_x).abs() <= 0.25, "beta stays in the frame");
    assert!(
        beta_y > alpha_y + 8.0 && beta_y < alpha_y + 80.0,
        "beta {beta_y} must follow alpha {alpha_y} inside one frame"
    );
    let shifted = framed(4500);
    let (shifted_x, _) = text_at(&shifted, "alpha");
    assert!(
        (shifted_x - alpha_x - 100.0).abs() <= 0.25,
        "moving x by 1500 twips must move children by 100 px, got {shifted_x} from {alpha_x}"
    );
    let (shifted_beta, _) = text_at(&shifted, "beta");
    assert!((shifted_beta - beta_x - 100.0).abs() <= 0.25);
}

/// A page-anchored frame inside a cell is positioned on the page.
///
/// The cell still flows its ordinary paragraphs. Two paragraphs that share the
/// frame signature stack at the frame origin, not at the cell origin.
#[test]
fn f16_page_frame_inside_a_cell_uses_the_page_origin() {
    let body = "\
<w:tbl><w:tblPr><w:tblW w:w=\"3000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr>\
<w:p><w:r><w:t>stay</w:t></w:r></w:p>\
<w:p><w:pPr><w:framePr w:w=\"2000\" w:h=\"400\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5400\" w:y=\"6300\"/></w:pPr>\
<w:r><w:t>11719</w:t></w:r></w:p>\
<w:p><w:pPr><w:framePr w:w=\"2000\" w:h=\"400\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5400\" w:y=\"6300\"/></w:pPr>\
<w:r><w:t>6371</w:t></w:r></w:p>\
</w:tc></w:tr></w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let svg = {
        let (_package, parsed) = open_body(body);
        render(&parsed, &RenderOptions::default())
            .expect("render")
            .into_iter()
            .next()
            .expect("page")
            .svg
    };
    let (stay_x, stay_y) = text_at(&svg, "stay");
    let (snp_x, snp_y) = text_at(&svg, "11719");
    let (next_x, next_y) = text_at(&svg, "6371");
    assert!(
        stay_x < 40.0 && stay_y < 40.0,
        "an unframed cell paragraph stays in the cell, got ({stay_x}, {stay_y})"
    );
    // 5400 twips = 360 px, 6300 twips = 420 px. The baseline sits below the frame top.
    assert!(
        (snp_x - 360.0).abs() <= 0.25,
        "11719 x {snp_x} must be the page frame at 360 px, not the cell"
    );
    assert!(
        snp_y > 420.0 && snp_y < 450.0,
        "11719 baseline {snp_y} must sit in the frame that starts at 420 px"
    );
    assert!(
        (next_x - snp_x).abs() <= 0.25,
        "6371 stays in the same frame"
    );
    assert!(
        next_y > snp_y + 8.0,
        "6371 ({next_y}) must follow 11719 ({snp_y}) inside the frame"
    );
}

/// The same frame signature in two cells is one column, not two copies of the origin.
#[test]
fn f16_same_frame_in_separate_cells_stacks() {
    let body = "\
<w:tbl><w:tblPr><w:tblW w:w=\"3000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid>\
<w:tr><w:tc><w:p><w:pPr>\
<w:framePr w:w=\"2000\" w:h=\"4000\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5400\" w:y=\"6300\"/>\
<w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"exact\"/>\
</w:pPr><w:r><w:t>6371</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:pPr>\
<w:framePr w:w=\"2000\" w:h=\"4000\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5400\" w:y=\"6300\"/>\
<w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"exact\"/>\
</w:pPr><w:r><w:t>11719</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let svg = {
        let (_package, parsed) = open_body(body);
        render(&parsed, &RenderOptions::default())
            .expect("render")
            .into_iter()
            .next()
            .expect("page")
            .svg
    };
    let (first_x, first_y) = text_at(&svg, "6371");
    let (second_x, second_y) = text_at(&svg, "11719");
    assert!((first_x - 360.0).abs() <= 0.25, "first x {first_x}");
    assert!(
        (second_x - first_x).abs() <= 0.25,
        "second stays in the frame, x {second_x} vs {first_x}"
    );
    assert!(
        (second_y - first_y - 16.0).abs() <= 0.25,
        "exact 240 twips must separate the cells, got {first_y} then {second_y}"
    );
}

/// A right-aligned frame ends on the frame edge minus the right indent.
#[test]
fn f16_right_aligned_frame_ends_on_the_indented_edge() {
    let body = "\
<w:p><w:pPr><w:framePr w:w=\"3000\" w:h=\"400\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"1500\" w:y=\"1500\"/>\
<w:ind w:end=\"600\"/><w:jc w:val=\"end\"/></w:pPr>\
<w:r><w:t>A,C</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let (_package, parsed) = open_body(body);
    let pages = strict_ooxml_render_svg::place_pages(&parsed, &RenderOptions::default(), None)
        .expect("place");
    let end = pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| {
            let strict_ooxml_render_svg::Item::Text(text) = item else {
                return None;
            };
            text.text.contains("A,C").then_some(text.x + text.width)
        })
        .expect("caption");
    // Frame x 100 px, width 200 px, right indent 40 px. The ink ends at 260.
    assert!(
        (end - 260.0).abs() <= 0.25,
        "right edge {end} must be frame origin + width - right indent"
    );
}
