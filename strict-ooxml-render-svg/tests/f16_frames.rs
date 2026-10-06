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
    let coord = |name: &str| {
        node.attribute(name)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
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
<w:tbl><w:tblPr><w:tblW w:w=\"3000\" w:type=\"dxa\"/>\
<w:tblCellMar><w:left w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"0\" w:type=\"dxa\"/>\
<w:top w:w=\"0\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr>\
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

/// Shared page-anchored `framePr` on every cell still keeps table topology.
///
/// Page 56 of the thesis encodes the phylogeny as a fixed-layout table whose
/// cells all repeat one frame origin. Escaping each cell as a stacked frame
/// collapses every SNP onto that origin (Δx ≈ −62 px for `11719` vs `8994`).
#[test]
fn f16_shared_page_frame_table_keeps_column_offsets() {
    let body = "\
<w:tbl>\
<w:tblPr><w:tblW w:w=\"3312\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>\
<w:tblCellMar><w:left w:w=\"10\" w:type=\"dxa\"/><w:right w:w=\"10\" w:type=\"dxa\"/>\
<w:top w:w=\"0\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"946\"/><w:gridCol w:w=\"2366\"/></w:tblGrid>\
<w:tr><w:trPr><w:trHeight w:val=\"557\" w:hRule=\"exact\"/></w:trPr>\
<w:tc><w:tcPr><w:tcW w:w=\"3312\" w:type=\"dxa\"/><w:gridSpan w:val=\"2\"/>\
<w:tcBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tcBorders></w:tcPr>\
<w:p><w:pPr><w:framePr w:w=\"3312\" w:h=\"7046\" w:wrap=\"none\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5395\" w:y=\"6370\"/>\
<w:ind w:left=\"40\"/><w:spacing w:after=\"0\" w:line=\"90\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:t>8994</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:trPr><w:trHeight w:val=\"538\" w:hRule=\"exact\"/></w:trPr>\
<w:tc><w:tcPr><w:tcW w:w=\"946\" w:type=\"dxa\"/>\
<w:tcBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tcBorders></w:tcPr>\
<w:p><w:pPr><w:framePr w:w=\"3312\" w:h=\"7046\" w:wrap=\"none\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5395\" w:y=\"6370\"/>\
<w:spacing w:after=\"0\" w:line=\"90\" w:lineRule=\"exact\"/></w:pPr></w:p></w:tc>\
<w:tc><w:tcPr><w:tcW w:w=\"2366\" w:type=\"dxa\"/>\
<w:tcBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tcBorders></w:tcPr>\
<w:p><w:pPr><w:framePr w:w=\"3312\" w:h=\"7046\" w:wrap=\"none\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5395\" w:y=\"6370\"/>\
<w:ind w:left=\"20\"/><w:spacing w:after=\"0\" w:line=\"90\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:t>11719</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let (_package, parsed) = open_body(body);
    let pages = strict_ooxml_render_svg::place_pages(&parsed, &RenderOptions::default(), None)
        .expect("place");
    let items: Vec<_> = pages.iter().flat_map(|page| &page.items).collect();
    let x_of = |needle: &str| {
        items
            .iter()
            .find_map(|item| {
                let strict_ooxml_render_svg::Item::Text(text) = item else {
                    return None;
                };
                text.text.contains(needle).then_some(text.x)
            })
            .unwrap_or_else(|| panic!("{needle}"))
    };
    let y_of = |needle: &str| {
        items
            .iter()
            .find_map(|item| {
                let strict_ooxml_render_svg::Item::Text(text) = item else {
                    return None;
                };
                text.text.contains(needle).then_some(text.baseline)
            })
            .unwrap_or_else(|| panic!("{needle}"))
    };
    let left = x_of("8994");
    let right = x_of("11719");
    // 5395 twips + 40 twips indent = 362.333 px. Page-framed tables do not add
    // tblCellMar start (WPS SNP origin).
    assert!(
        (left - 362.333).abs() <= 0.25,
        "8994 x {left} must sit in column 0 of the framed table"
    );
    // Column 1 starts 946 twips later; indent 20 twips → 424.067 px.
    assert!(
        (right - 424.067).abs() <= 0.25,
        "11719 x {right} must keep the column offset, not stack on 8994 at {left}"
    );
    let first_y = y_of("8994");
    let second_y = y_of("11719");
    assert!(
        first_y > 424.667 && first_y < 424.667 + 40.0,
        "8994 baseline {first_y} must sit in the frame that starts at 424.667"
    );
    // Exact row 557 twips = 37.133 px.
    assert!(
        (second_y - first_y - 37.133).abs() <= 0.25,
        "exact row height must separate the SNPs, got {first_y} then {second_y}"
    );
    let has_edge = items.iter().any(|item| {
        let strict_ooxml_render_svg::Item::Line(line) = item else {
            return false;
        };
        (line.x1 - 359.667).abs() <= 1.0 || (line.x2 - 359.667).abs() <= 1.0
    });
    assert!(
        has_edge,
        "cell left/top borders must paint tree edges at the frame origin"
    );
}

/// Inverse: extracting per-cell frames from a uniform table would collapse columns.
#[test]
fn f16_inverse_escaped_cell_frames_are_not_the_table_origin() {
    // Production path must keep 11719 right of 8994. This assertion is the
    // inverse of stacking both labels at the shared frame x.
    let (left, right) = {
        let svg = {
            // Reuse the same fixture through the public render path.
            let body = "\
<w:tbl>\
<w:tblPr><w:tblW w:w=\"3312\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>\
<w:tblCellMar><w:left w:w=\"10\" w:type=\"dxa\"/><w:right w:w=\"10\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"946\"/><w:gridCol w:w=\"2366\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr>\
<w:p><w:pPr><w:framePr w:w=\"3312\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5395\" w:y=\"6370\"/>\
<w:ind w:left=\"40\"/></w:pPr><w:r><w:t>8994</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:pPr><w:framePr w:w=\"3312\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5395\" w:y=\"6370\"/></w:pPr></w:p></w:tc>\
<w:tc><w:p><w:pPr><w:framePr w:w=\"3312\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"5395\" w:y=\"6370\"/>\
<w:ind w:left=\"20\"/></w:pPr><w:r><w:t>11719</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
            let (_package, parsed) = open_body(body);
            render(&parsed, &RenderOptions::default())
                .expect("render")
                .into_iter()
                .next()
                .expect("page")
                .svg
        };
        (text_at(&svg, "8994").0, text_at(&svg, "11719").0)
    };
    assert!(
        right - left > 50.0,
        "inverse: collapsing onto one frame x would leave Δx≈0, got {left} and {right}"
    );
}

#[test]
fn f16_xalign_right_keeps_the_frame_on_the_page() {
    let body = "\
<w:p><w:pPr><w:framePr w:w=\"3000\" w:h=\"400\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:xAlign=\"right\" w:y=\"1500\"/>\
<w:jc w:val=\"end\"/></w:pPr>\
<w:r><w:t>edge</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let (_package, parsed) = open_body(body);
    let pages = strict_ooxml_render_svg::place_pages(&parsed, &RenderOptions::default(), None)
        .expect("place");
    let (x, width) = pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| {
            let strict_ooxml_render_svg::Item::Text(text) = item else {
                return None;
            };
            text.text.contains("edge").then_some((text.x, text.width))
        })
        .expect("caption");
    // Page 816 px, frame 200 px, right-aligned: origin 616. Right-aligned text
    // ends at the frame edge.
    assert!(
        x + width <= 816.0 + 0.25,
        "right-aligned frame text must stay on the page, got end {}",
        x + width
    );
    assert!(
        (x + width - 816.0).abs() <= 20.0,
        "ink should sit at the right frame edge, got end {}",
        x + width
    );
}

#[test]
fn f16_xalign_center_is_the_page_midpoint() {
    let body = "\
<w:p><w:pPr><w:framePr w:w=\"3000\" w:h=\"400\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:xAlign=\"center\" w:y=\"1500\"/>\
<w:jc w:val=\"center\"/></w:pPr>\
<w:r><w:t>HVR-I</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let (_package, parsed) = open_body(body);
    let pages = strict_ooxml_render_svg::place_pages(&parsed, &RenderOptions::default(), None)
        .expect("place");
    let (x, width) = pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| {
            let strict_ooxml_render_svg::Item::Text(text) = item else {
                return None;
            };
            text.text.contains("HVR-I").then_some((text.x, text.width))
        })
        .expect("label");
    let midpoint = x + width / 2.0;
    assert!(
        (midpoint - 408.0).abs() <= 0.25,
        "centered frame text midpoint {midpoint} must be page center 408"
    );
}

/// Style-90 primer frame: stacked exact-160 lines must land near WPS page-54.
#[test]
fn f16_style90_primer_stack_matches_wps_offset() {
    // Minimal stand-in for the L / 16055 column (frame y=1340, line=160 exact).
    let mut body = String::new();
    for label in ["L15996", "H16142", "L16055"] {
        body.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"90\"/>\
<w:framePr w:w=\"8357\" w:h=\"5250\" w:hRule=\"exact\" w:wrap=\"none\" \
w:vAnchor=\"page\" w:hAnchor=\"page\" w:x=\"1787\" w:y=\"1340\"/>\
<w:spacing w:line=\"160\" w:lineRule=\"exact\"/><w:ind w:left=\"1380\"/></w:pPr>\
<w:r><w:rPr><w:rFonts w:ascii=\"Arial\"/><w:sz w:val=\"16\"/></w:rPr><w:t>{label}</w:t></w:r></w:p>"
        ));
    }
    body.push_str(
        "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>",
    );
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"90\">\
<w:pPr><w:spacing w:line=\"216\" w:lineRule=\"exact\"/></w:pPr>\
<w:rPr><w:rFonts w:ascii=\"Arial\"/><w:sz w:val=\"16\"/></w:rPr></w:style>";
    let (_package, parsed) = common::open_with_styles(&body, styles);
    let svg = render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg;
    let (_, y0) = text_at(&svg, "L15996");
    let (_, y1) = text_at(&svg, "H16142");
    let (_, y2) = text_at(&svg, "L16055");
    let slot = 160.0 / 20.0 * 96.0 / 72.0;
    let frame_y = 1340.0 / 20.0 * 96.0 / 72.0;
    assert!(
        (y1 - y0 - slot).abs() <= 0.25 && (y2 - y1 - slot).abs() <= 0.25,
        "exact-160 stack pitch, y0={y0} y1={y1} y2={y2} slot={slot}"
    );
    assert!(
        y0 > frame_y + 8.0 && y0 < frame_y + slot + 2.0,
        "first baseline in the frame box, y0={y0} frame_y={frame_y}"
    );
}

/// Adjacent `w:pBdr` paragraphs in one page frame: top pad on the first, bottom
/// pad after the last, and a collapsed shared edge between them (Clio HVR-I +
/// Positions on page 54).
#[test]
fn f16_pbdr_collapses_between_bordered_frame_siblings() {
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"70\">\
<w:pPr><w:spacing w:line=\"0\" w:lineRule=\"atLeast\"/></w:pPr>\
<w:rPr><w:rFonts w:ascii=\"Arial\"/><w:sz w:val=\"15\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"81\">\
<w:pPr><w:spacing w:line=\"216\" w:lineRule=\"exact\"/></w:pPr>\
<w:rPr><w:rFonts w:ascii=\"Arial\"/><w:sz w:val=\"14\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"90\">\
<w:pPr><w:spacing w:line=\"216\" w:lineRule=\"exact\"/></w:pPr>\
<w:rPr><w:rFonts w:ascii=\"Arial\"/><w:sz w:val=\"16\"/></w:rPr></w:style>";
    let frame = "<w:framePr w:w=\"8357\" w:h=\"5250\" w:hRule=\"exact\" w:wrap=\"none\" \
w:vAnchor=\"page\" w:hAnchor=\"page\" w:x=\"1787\" w:y=\"1340\"/>";
    let bdr = "<w:pBdr>\
<w:top w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"auto\"/>\
<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"auto\"/>\
</w:pBdr>";
    let body = format!(
        "<w:p><w:pPr><w:pStyle w:val=\"70\"/>{frame}{bdr}\
<w:spacing w:line=\"150\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:t>HVR-I</w:t></w:r></w:p>\
<w:p><w:pPr><w:pStyle w:val=\"81\"/>{frame}{bdr}</w:pPr>\
<w:r><w:t>Positions</w:t></w:r></w:p>\
<w:p><w:pPr><w:pStyle w:val=\"90\"/>{frame}</w:pPr>\
<w:r><w:t>L15996</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    let (_package, parsed) = common::open_with_styles(&body, styles);
    let svg = render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg;
    let (_, hvr_y) = text_at(&svg, "HVR-I");
    let (_, pos_y) = text_at(&svg, "Positions");
    let (_, l_y) = text_at(&svg, "L15996");
    let frame_y = 1340.0 / 20.0 * 96.0 / 72.0;
    let border = (1.0 + 4.0 / 8.0) * 96.0 / 72.0;
    // HVR-I: top border + exact*0.8 for 150-twip line at 7.5pt.
    let exact_150 = 150.0 / 20.0 * 96.0 / 72.0;
    assert!(
        (hvr_y - (frame_y + border + exact_150 * 0.8)).abs() <= 0.35,
        "HVR-I baseline {hvr_y} after top border {border} in frame {frame_y}"
    );
    // Collapsed edge: Positions sits one exact-150 advance below HVR, not +2 borders.
    assert!(
        (pos_y - (hvr_y - exact_150 * 0.8 + exact_150 + 216.0 / 20.0 * 96.0 / 72.0 * 0.8)).abs()
            <= 0.5,
        "Positions {pos_y} after collapsed HVR edge, HVR={hvr_y}"
    );
    // Bottom border of Positions clears L15996.
    let exact_216 = 216.0 / 20.0 * 96.0 / 72.0;
    assert!(
        (l_y - (pos_y - exact_216 * 0.8 + exact_216 + border + exact_216 * 0.8)).abs() <= 0.5,
        "L15996 {l_y} after Positions bottom border, Positions={pos_y}"
    );
}

/// Exact line shorter than the font: baseline follows font ascent (WPS primers).
#[test]
fn f16_exact_line_shorter_than_font_uses_font_ascent() {
    let body = "\
<w:p><w:pPr><w:framePr w:w=\"4000\" w:h=\"800\" w:x=\"1921\" w:y=\"2854\"/>\
<w:spacing w:after=\"0\" w:line=\"80\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:rPr><w:rFonts w:ascii=\"Arial\"/><w:sz w:val=\"16\"/></w:rPr><w:t>L16055</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let (_package, parsed) = open_body(body);
    let svg = render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg;
    let (_, y) = text_at(&svg, "L16055");
    let frame_y = 2854.0 / 20.0 * 96.0 / 72.0;
    let exact = 80.0 / 20.0 * 96.0 / 72.0;
    // Font ascent for 8pt Arimo is well above the 80-twip line; baseline must
    // not be clamped to the line box (that left primers ~4 px too high vs WPS).
    assert!(
        y > frame_y + exact + 0.5,
        "baseline {y} must sit below frame+exact ({frame_y}+{exact}); svg={svg}"
    );
}

/// Clio page 56: style 130 (`w:spacing` 13 twips) wraps `"L3'4` inside a 480-twip
/// frame with a 100-twip indent, so the next haplogroup sits one exact line lower.
#[test]
fn f16_haplogroup_l3_wraps_from_character_spacing() {
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"130\">\
<w:pPr><w:spacing w:line=\"538\" w:lineRule=\"exact\"/></w:pPr>\
<w:rPr><w:rFonts w:ascii=\"Arial\"/><w:spacing w:val=\"13\"/><w:sz w:val=\"16\"/></w:rPr>\
</w:style>";
    let frame = "<w:framePr w:w=\"480\" w:h=\"12330\" w:hRule=\"exact\" w:wrap=\"none\" \
w:vAnchor=\"page\" w:hAnchor=\"page\" w:x=\"8635\" w:y=\"1229\"/>";
    let body = format!(
        "<w:p><w:pPr><w:pStyle w:val=\"130\"/>{frame}<w:ind w:left=\"100\"/></w:pPr>\
<w:r><w:t>\"L3'4</w:t></w:r></w:p>\
<w:p><w:pPr><w:pStyle w:val=\"130\"/>{frame}<w:ind w:left=\"100\"/>\
<w:spacing w:after=\"0\" w:line=\"538\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:t>-M</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    let (_package, parsed) = common::open_with_styles(&body, styles);
    let svg = render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg;
    assert!(
        svg.contains("L3") || svg.contains("3"),
        "expected haplogroup glyphs: {svg}"
    );
    let (_, l3_y) = text_at(&svg, "3");
    let (_, m_y) = text_at(&svg, "-M");
    let slot = 538.0 / 20.0 * 96.0 / 72.0;
    assert!(
        (m_y - l3_y - 2.0 * slot).abs() <= 0.5,
        "L3 must occupy two exact slots so -M is 2*538 twips below, L3={l3_y} M={m_y} slot={slot} {svg}"
    );
}
