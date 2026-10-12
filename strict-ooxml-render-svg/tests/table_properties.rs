//! Table properties that used to stop at the model
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §1): `w:vAlign`, `w:hRule`, border
//! styles, table shading, `w:tblPrEx` borders, diagonals, `w:bidiVisual` and
//! `w:tcW` without a grid.

#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::doc_markdown,
    clippy::format_collect,
    clippy::needless_pass_by_value,
    clippy::similar_names
)]

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{place_pages, Item, PlacedPage, RenderOptions, TextItem};
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn document(body: &str) -> Document {
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    parse_document(&package, &ParseOptions::default()).expect("parse")
}

fn page(doc: &Document) -> PlacedPage {
    place_pages(doc, &RenderOptions::default(), None)
        .expect("place")
        .into_iter()
        .next()
        .expect("page")
}

fn text(page: &PlacedPage, needle: &str) -> TextItem {
    page.items
        .iter()
        .find_map(|item| match item {
            Item::Text(text) if text.text.contains(needle) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {needle:?}"))
}

/// `(x1, y1, x2, y2)` of every line item.
fn lines(page: &PlacedPage) -> Vec<(f64, f64, f64, f64)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Line(line) => Some((line.x1, line.y1, line.x2, line.y2)),
            _ => None,
        })
        .collect()
}

fn one_cell(tc_pr: &str, tr_pr: &str, content: &str) -> String {
    format!(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"4000\"/></w:tblGrid>\
         <w:tr><w:trPr>{tr_pr}</w:trPr><w:tc><w:tcPr>{tc_pr}</w:tcPr>{content}</w:tc></w:tr></w:tbl>\
         <w:p><w:r><w:t>After</w:t></w:r></w:p>"
    )
}

const CELL_TEXT: &str = "<w:p><w:r><w:t>Cell</w:t></w:r></w:p>";

#[test]
fn vertical_alignment_moves_the_content_down_the_row() {
    let row = "<w:trHeight w:val=\"1440\" w:hRule=\"exact\"/>";
    let top = page(&document(&one_cell("", row, CELL_TEXT)));
    let center = page(&document(&one_cell(
        "<w:vAlign w:val=\"center\"/>",
        row,
        CELL_TEXT,
    )));
    let bottom = page(&document(&one_cell(
        "<w:vAlign w:val=\"bottom\"/>",
        row,
        CELL_TEXT,
    )));
    let top_y = text(&top, "Cell").baseline;
    let center_y = text(&center, "Cell").baseline;
    let bottom_y = text(&bottom, "Cell").baseline;
    assert!(center_y > top_y + 20.0, "{top_y} {center_y}");
    assert!(
        ((bottom_y - top_y) - 2.0 * (center_y - top_y)).abs() < 0.01,
        "bottom moves twice as far as center"
    );
}

#[test]
fn an_exact_row_keeps_its_height_and_an_auto_row_ignores_it() {
    let two_lines = "<w:p><w:r><w:t>One</w:t><w:br/><w:t>Two</w:t></w:r></w:p>";
    let exact = page(&document(&one_cell(
        "",
        "<w:trHeight w:val=\"150\" w:hRule=\"exact\"/>",
        two_lines,
    )));
    let at_least = page(&document(&one_cell(
        "",
        "<w:trHeight w:val=\"150\"/>",
        two_lines,
    )));
    let auto = page(&document(&one_cell(
        "",
        "<w:trHeight w:val=\"2880\" w:hRule=\"auto\"/>",
        two_lines,
    )));
    // 150 twips is 10 px: the row below starts that far under the table top.
    let exact_after = text(&exact, "After").baseline;
    let grown_after = text(&at_least, "After").baseline;
    assert!(
        exact_after < grown_after - 10.0,
        "{exact_after} {grown_after}"
    );
    assert!(
        (text(&auto, "After").baseline - grown_after).abs() < 0.01,
        "auto ignores the 2-inch value"
    );
}

#[test]
fn a_double_cell_border_is_two_lines() {
    let doc = document(&one_cell(
        "<w:tcBorders><w:top w:val=\"double\" w:sz=\"8\"/></w:tcBorders>",
        "",
        CELL_TEXT,
    ));
    let horizontals = lines(&page(&doc))
        .into_iter()
        .filter(|(_, y1, _, y2)| (y1 - y2).abs() < 1e-9)
        .count();
    assert_eq!(horizontals, 2);
}

#[test]
fn table_shading_fills_cells_without_their_own() {
    let doc = document(
        "<w:tbl><w:tblPr><w:shd w:val=\"clear\" w:fill=\"ABCDEF\"/></w:tblPr>\
         <w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
         <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
         <w:tc><w:tcPr><w:shd w:val=\"clear\" w:fill=\"FF0000\"/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    let fills: Vec<String> = page(&doc)
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Rect(rect) => rect.fill.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(fills, ["#abcdef", "#ff0000"]);
}

#[test]
fn a_row_exception_replaces_the_table_borders() {
    let doc = document(
        "<w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"8\" w:color=\"000000\"/></w:tblBorders></w:tblPr>\
         <w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid>\
         <w:tr><w:tblPrEx><w:tblBorders><w:top w:val=\"single\" w:sz=\"8\" w:color=\"00FF00\"/></w:tblBorders></w:tblPrEx>\
         <w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    let colours: Vec<String> = page(&doc)
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Line(line) => Some(line.color.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(colours, ["#00ff00"]);
}

#[test]
fn a_diagonal_border_crosses_the_cell() {
    let doc = document(&one_cell(
        "<w:tcBorders><w:tl2br w:val=\"single\" w:sz=\"8\"/></w:tcBorders>",
        "",
        CELL_TEXT,
    ));
    let diagonal = lines(&page(&doc))
        .into_iter()
        .filter(|(x1, y1, x2, y2)| (x1 - x2).abs() > 1.0 && (y1 - y2).abs() > 1.0)
        .count();
    assert_eq!(diagonal, 1);
}

#[test]
fn a_bidi_visual_table_puts_the_first_cell_on_the_right() {
    let row = "<w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
               <w:tr><w:tc><w:p><w:r><w:t>First</w:t></w:r></w:p></w:tc>\
               <w:tc><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:tc></w:tr>";
    let ltr = page(&document(&format!("<w:tbl>{row}</w:tbl>")));
    let rtl = page(&document(&format!(
        "<w:tbl><w:tblPr><w:bidiVisual/></w:tblPr>{row}</w:tbl>"
    )));
    assert!(text(&ltr, "First").x < text(&ltr, "Second").x);
    assert!(text(&rtl, "First").x > text(&rtl, "Second").x);
}

#[test]
fn cell_widths_stand_in_for_a_missing_grid() {
    let doc = document(
        "<w:tbl><w:tblPr><w:tblW w:w=\"4000\" w:type=\"dxa\"/></w:tblPr>\
         <w:tr><w:tc><w:tcPr><w:tcW w:w=\"1000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>Narrow</w:t></w:r></w:p></w:tc>\
         <w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>Wide</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    let page = page(&doc);
    // 1000 twips is 66.667 px.
    let distance = text(&page, "Wide").x - text(&page, "Narrow").x;
    assert!((distance - 1000.0 / 15.0).abs() < 0.01, "{distance}");
}

#[test]
fn omitted_grid_columns_keep_their_authored_widths_without_a_grid() {
    let body = "<w:tbl><w:tblPr><w:tblW w:w=\"4000\" w:type=\"dxa\"/></w:tblPr>\
        <w:tr><w:trPr><w:gridBefore w:val=\"1\"/><w:gridAfter w:val=\"1\"/><w:wBefore w:w=\"1000\" w:type=\"dxa\"/><w:wAfter w:w=\"500\" w:type=\"dxa\"/></w:trPr>\
        <w:tc><w:tcPr><w:tcW w:w=\"2500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>Inset</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let result = page(&document(body));
    let label = text(&result, "Inset");
    assert!(
        (label.x - 96.0 - 1000.0 / 15.0 - 108.0 / 15.0).abs() < 0.01,
        "{}",
        label.x
    );
}
