//! `w:tblStylePr` conditional formatting in the render cascade
//! (ISO/IEC 29500-1 §17.7.6): `firstRow`, row banding, `w:tblLook`, direct
//! cell formatting over the style, and `basedOn` table styles.

#![allow(clippy::expect_used, clippy::doc_markdown)]

mod common;

use common::open_with_styles;
use strict_ooxml_render_svg::{render, RenderOptions};

/// Header fill of the `firstRow` condition.
const HEADER_FILL: &str = "#4472c4";
/// Fill of the `band1Horz` condition.
const BAND_FILL: &str = "#d9e2f3";

/// The table style under test: `firstRow` is blue with bold white text, odd
/// row bands are pale blue.
const GRID_STYLE: &str = "<w:style w:type=\"table\" w:styleId=\"Grid\"><w:name w:val=\"Grid\"/>\
<w:tblPr><w:tblBorders>\
<w:top w:val=\"single\" w:sz=\"4\" w:color=\"000000\"/>\
<w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"000000\"/>\
</w:tblBorders></w:tblPr>\
<w:tblStylePr w:type=\"firstRow\"><w:rPr><w:b/><w:color w:val=\"FFFFFF\"/></w:rPr>\
<w:tcPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"4472C4\"/></w:tcPr></w:tblStylePr>\
<w:tblStylePr w:type=\"band1Horz\">\
<w:tcPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"D9E2F3\"/></w:tcPr></w:tblStylePr>\
</w:style>";

/// One cell holding one word, with optional `tcPr` and `rPr` children.
fn cell(text: &str, tc_pr: &str, r_pr: &str) -> String {
    format!(
        "<w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/>{tc_pr}</w:tcPr>\
<w:p><w:r><w:rPr>{r_pr}</w:rPr><w:t>{text}</w:t></w:r></w:p></w:tc>"
    )
}

/// A 3x3 table under `style` with `look` as its `w:tblLook`. Words are
/// `Head1..3`, `Body1..3` and `Tail1..3`. `first_cell` replaces `Head1`.
fn table(style: &str, look: &str, first_cell: Option<&str>) -> String {
    let mut rows = String::new();
    for (row, prefix) in ["Head", "Body", "Tail"].iter().enumerate() {
        rows.push_str("<w:tr>");
        for column in 1..=3 {
            match (first_cell, row, column) {
                (Some(first), 0, 1) => rows.push_str(first),
                _ => rows.push_str(&cell(&format!("{prefix}{column}"), "", "")),
            }
        }
        rows.push_str("</w:tr>");
    }
    format!(
        "<w:tbl><w:tblPr><w:tblStyle w:val=\"{style}\"/><w:tblW w:w=\"6000\" w:type=\"dxa\"/>\
<w:tblLook {look}/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
{rows}</w:tbl><w:p/>"
    )
}

fn render_svg(body: &str, styles: &str) -> String {
    let (_package, document) = open_with_styles(body, styles);
    render(&document, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

/// What the page shows for one word: the fills of every rect under its
/// baseline, its `font-weight` and its text `fill`.
#[derive(Debug)]
struct Word {
    fills: Vec<String>,
    bold: bool,
    color: String,
}

fn word(svg: &str, needle: &str) -> Word {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let number = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.attribute(name)
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let text = document
        .descendants()
        .find(|node| {
            node.is_element() && node.tag_name().name() == "text" && node.text() == Some(needle)
        })
        .unwrap_or_else(|| panic!("no text {needle}"));
    let (x, y) = (number(text, "x"), number(text, "y"));
    let fills = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "rect")
        .filter(|rect| {
            let (left, top) = (number(*rect, "x"), number(*rect, "y"));
            let (width, height) = (number(*rect, "width"), number(*rect, "height"));
            x >= left && x <= left + width && y >= top && y <= top + height
        })
        .filter_map(|rect| rect.attribute("fill").map(str::to_owned))
        .filter(|fill| fill != "#ffffff")
        .collect();
    Word {
        fills,
        bold: text.attribute("font-weight") == Some("bold"),
        color: text.attribute("fill").unwrap_or("").to_owned(),
    }
}

fn has_fill(word: &Word, fill: &str) -> bool {
    word.fills.iter().any(|found| found == fill)
}

#[test]
fn first_row_and_row_bands_follow_tbl_look() {
    let svg = render_svg(
        &table("Grid", "w:firstRow=\"1\" w:noVBand=\"1\"", None),
        GRID_STYLE,
    );
    for column in 1..=3 {
        let head = word(&svg, &format!("Head{column}"));
        assert!(has_fill(&head, HEADER_FILL), "header cell shaded: {head:?}");
        assert!(
            !has_fill(&head, BAND_FILL),
            "header row is not banded: {head:?}"
        );
        assert!(head.bold, "header text is bold: {head:?}");
        assert_eq!(head.color, "#ffffff", "header text is white");

        let body = word(&svg, &format!("Body{column}"));
        assert!(
            has_fill(&body, BAND_FILL),
            "first body row is band 1: {body:?}"
        );
        assert!(!body.bold, "body text keeps its weight: {body:?}");
        assert_eq!(body.color, "#000000", "body text keeps its colour");

        let tail = word(&svg, &format!("Tail{column}"));
        assert!(tail.fills.is_empty(), "second body row is band 2: {tail:?}");
    }
}

#[test]
fn first_row_off_leaves_the_header_unstyled() {
    let svg = render_svg(
        &table("Grid", "w:firstRow=\"0\" w:noVBand=\"1\"", None),
        GRID_STYLE,
    );
    let head = word(&svg, "Head1");
    assert!(!has_fill(&head, HEADER_FILL), "no firstRow fill: {head:?}");
    assert!(!head.bold, "no firstRow bold: {head:?}");
    assert_eq!(head.color, "#000000");
    // Without a header row the banding starts at the first row.
    assert!(has_fill(&head, BAND_FILL), "row 1 is band 1: {head:?}");
    assert!(word(&svg, "Body1").fills.is_empty(), "row 2 is band 2");
}

#[test]
fn direct_cell_and_run_formatting_win_over_the_condition() {
    let first = cell(
        "Head1",
        "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"FF0000\"/>",
        "<w:color w:val=\"00FF00\"/>",
    );
    let svg = render_svg(
        &table("Grid", "w:firstRow=\"1\" w:noVBand=\"1\"", Some(&first)),
        GRID_STYLE,
    );
    let head = word(&svg, "Head1");
    assert!(has_fill(&head, "#ff0000"), "direct shading wins: {head:?}");
    assert!(
        !has_fill(&head, HEADER_FILL),
        "condition fill is replaced: {head:?}"
    );
    assert_eq!(head.color, "#00ff00", "direct run colour wins");
    assert!(head.bold, "the condition's bold still applies: {head:?}");
    let next = word(&svg, "Head2");
    assert!(
        has_fill(&next, HEADER_FILL),
        "other header cells keep the style"
    );
}

#[test]
fn based_on_table_style_inherits_and_overrides_conditions() {
    let inherit = format!(
        "{GRID_STYLE}<w:style w:type=\"table\" w:styleId=\"Child\"><w:name w:val=\"Child\"/>\
<w:basedOn w:val=\"Grid\"/></w:style>"
    );
    let svg = render_svg(
        &table("Child", "w:firstRow=\"1\" w:noVBand=\"1\"", None),
        &inherit,
    );
    let head = word(&svg, "Head1");
    assert!(
        has_fill(&head, HEADER_FILL),
        "parent firstRow fill: {head:?}"
    );
    assert!(head.bold, "parent firstRow bold: {head:?}");
    assert!(has_fill(&word(&svg, "Body1"), BAND_FILL), "parent band");

    let override_fill = format!(
        "{GRID_STYLE}<w:style w:type=\"table\" w:styleId=\"Child\"><w:name w:val=\"Child\"/>\
<w:basedOn w:val=\"Grid\"/><w:tblStylePr w:type=\"firstRow\">\
<w:tcPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"C00000\"/></w:tcPr></w:tblStylePr></w:style>"
    );
    let svg = render_svg(
        &table("Child", "w:firstRow=\"1\" w:noVBand=\"1\"", None),
        &override_fill,
    );
    let head = word(&svg, "Head1");
    assert!(
        has_fill(&head, "#c00000"),
        "child firstRow fill wins: {head:?}"
    );
    assert!(
        !has_fill(&head, HEADER_FILL),
        "parent fill is replaced: {head:?}"
    );
    assert!(head.bold, "parent firstRow bold still applies: {head:?}");
}

#[test]
fn table_style_base_borders_reach_the_cells() {
    let svg = render_svg(
        &table("Grid", "w:firstRow=\"1\" w:noVBand=\"1\"", None),
        GRID_STYLE,
    );
    let document = roxmltree::Document::parse(&svg).expect("svg");
    let lines = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "line")
        .count();
    assert!(lines > 0, "the style's tblBorders are painted");
}

/// `insideH`/`insideV` draw the grid between cells and nothing on the outer
/// edge; TableGrid, the style Word gives most tables, is drawn this way.
#[test]
fn inside_borders_draw_the_inner_grid_only() {
    const INNER: &str = "<w:style w:type=\"table\" w:styleId=\"Inner\"><w:name w:val=\"Inner\"/>\
<w:tblPr><w:tblBorders>\
<w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"FF0000\"/>\
<w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"FF0000\"/>\
</w:tblBorders></w:tblPr></w:style>";
    let svg = render_svg(
        &table(
            "Inner",
            "w:firstRow=\"0\" w:noHBand=\"1\" w:noVBand=\"1\"",
            None,
        ),
        INNER,
    );
    let document = roxmltree::Document::parse(&svg).expect("svg");
    let red = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "line")
        .filter(|node| {
            node.attribute("stroke")
                .is_some_and(|stroke| stroke.eq_ignore_ascii_case("#ff0000"))
        })
        .count();
    // 3x3 cells: each draws its interior sides - 12 horizontal, 12 vertical.
    assert_eq!(red, 24, "interior edges only");
}
