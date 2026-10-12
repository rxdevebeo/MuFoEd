//! Regressions for the WPS reference findings (`docs/WPS-QA-DISCREPANCIES.md`).
//!
//! Each test names its `WQ-…` entry and checks the layout the reference shows,
//! measured on the placed items rather than on pixels.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::float_cmp)]

mod common;

use common::{open_body, open_with_styles};
use strict_ooxml_render_svg::{place_pages, CellNoWrap, Item, PlacedPage, RenderOptions};

/// A small page: 4 in wide with 0.5 in margins, so the text column is 3 in
/// (288 px at 96 dpi).
const NARROW: &str = "<w:sectPr><w:pgSz w:w=\"5760\" w:h=\"8640\"/>\
<w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>";

fn pages(body: &str, options: &RenderOptions) -> Vec<PlacedPage> {
    let (package, document) = open_body(body);
    place_pages(&document, options, Some(&package)).expect("place")
}

/// `(x, baseline, text)` of every text item, groups included.
fn texts(page: &PlacedPage) -> Vec<(f64, f64, String)> {
    fn walk(items: &[Item], dx: f64, dy: f64, out: &mut Vec<(f64, f64, String)>) {
        for item in items {
            match item {
                Item::Text(text) => out.push((text.x + dx, text.baseline + dy, text.text.clone())),
                Item::Group(group) => walk(
                    &group.items,
                    dx + group.matrix[4],
                    dy + group.matrix[5],
                    out,
                ),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&page.items, 0.0, 0.0, &mut out);
    out
}

fn baselines(page: &PlacedPage) -> Vec<f64> {
    let mut values: Vec<f64> = texts(page).into_iter().map(|(_, y, _)| y).collect();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    values
}

fn images(page: &PlacedPage) -> Vec<(f64, f64, f64, f64)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some((image.x, image.y, image.w, image.h)),
            _ => None,
        })
        .collect()
}

fn inline_picture(cx: u32, effect_left: u32) -> String {
    format!(
        "<w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cx}\"/>\
<wp:effectExtent l=\"{effect_left}\" t=\"0\" r=\"0\" b=\"0\"/>\
<wp:docPr id=\"1\" name=\"pic\"/><a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"pic\"/></pic:nvPicPr><pic:blipFill/>\
<pic:spPr><a:ext cx=\"{cx}\" cy=\"{cx}\"/></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
    )
}

/// WQ-002: `w:pPr/w:rPr` formats the paragraph mark, not the text.
#[test]
fn wq002_mark_formatting_does_not_reach_the_runs() {
    let (_package, document) = open_body(
        "<w:p><w:pPr><w:rPr><w:b/><w:sz w:val=\"40\"/></w:rPr></w:pPr>\
<w:r><w:t>plain</w:t></w:r></w:p>",
    );
    let page = place_pages(&document, &RenderOptions::default(), None)
        .expect("place")
        .remove(0);
    let text = page
        .items
        .iter()
        .find_map(|item| match item {
            Item::Text(text) if text.text == "plain" => Some(text.clone()),
            _ => None,
        })
        .expect("text");
    assert!(!text.run.bold, "the mark's bold stays on the mark");
    assert!(
        (text.run.size_pt - 11.0).abs() < 0.01,
        "{}",
        text.run.size_pt
    );
}

/// WQ-015 / WQ-010: inline pictures share a line, follow the paragraph's
/// alignment, and `effectExtent` is a margin around the picture.
#[test]
fn wq015_inline_pictures_share_a_centred_line() {
    // Three 0.75 in pictures fit the 3 in column side by side.
    let pictures = inline_picture(685_800, 0).repeat(3);
    let body = format!("<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>{pictures}</w:p>{NARROW}");
    let page = pages(&body, &RenderOptions::default()).remove(0);
    let found = images(&page);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found
            .iter()
            .all(|image| (image.1 - found[0].1).abs() < 0.01),
        "one row: {found:?}"
    );
    assert!(found[0].0 < found[1].0 && found[1].0 < found[2].0);
    // 288 px column, 3 x 72 px pictures: 36 px either side.
    assert!((found[0].0 - (48.0 + 36.0)).abs() < 0.5, "{found:?}");

    let body = format!("<w:p>{}</w:p>{NARROW}", inline_picture(914_400, 19_050));
    let page = pages(&body, &RenderOptions::default()).remove(0);
    let found = images(&page);
    assert_eq!(found.len(), 1);
    assert!((found[0].2 - 96.0).abs() < 0.01, "extent, not stretched");
    assert!((found[0].0 - 50.0).abs() < 0.01, "moved by the left margin");
}

/// WQ-021: a word that does not fit breaks after its hyphen.
#[test]
fn wq021_a_hyphenated_word_breaks_after_the_hyphen() {
    let body = format!(
        "<w:p><w:r><w:t>Uncertainty about the measured nuclear cross-sections</w:t></w:r></w:p>{NARROW}"
    );
    let page = pages(&body, &RenderOptions::default()).remove(0);
    let found = texts(&page);
    assert!(
        found
            .iter()
            .any(|(_, _, text)| text.trim_end().ends_with("cross-")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(_, _, text)| text.starts_with("sections")),
        "{found:?}"
    );
}

/// WQ-024: on a line grid, 1.5 lines is 1.5 grid units, not two.
#[test]
fn wq024_grid_spacing_scales_the_snapped_line() {
    let body = "<w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"360\" w:lineRule=\"auto\"/></w:pPr>\
<w:r><w:t>first</w:t></w:r><w:r><w:br/></w:r><w:r><w:t>second</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
<w:docGrid w:type=\"lines\" w:linePitch=\"312\"/></w:sectPr>";
    let page = pages(body, &RenderOptions::default()).remove(0);
    let found = baselines(&page);
    assert_eq!(found.len(), 2, "{found:?}");
    // 15.6 pt x 1.5 = 23.4 pt = 31.2 px.
    assert!((found[1] - found[0] - 31.2).abs() < 0.05, "{found:?}");
}

/// WQ-036: a right tab past the right margin keeps the entry on one line.
#[test]
fn wq036_a_right_tab_past_the_margin_is_honoured() {
    // The stop is 0.25 in beyond the 3 in column.
    let body = format!(
        "<w:p><w:pPr><w:tabs><w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"4680\"/></w:tabs></w:pPr>\
<w:r><w:t>Entry</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>12</w:t></w:r></w:p>{NARROW}"
    );
    let page = pages(&body, &RenderOptions::default()).remove(0);
    let found = texts(&page);
    let entry = found.iter().find(|item| item.2 == "Entry").expect("entry");
    let number = found.iter().find(|item| item.2 == "12").expect("number");
    assert!((entry.1 - number.1).abs() < 0.01, "same line: {found:?}");
    assert!(number.0 > 48.0 + 288.0 - 30.0, "near the stop: {found:?}");
}

/// WQ-020: `w:noWrap` in a fixed-width cell of an automatic-layout table
/// wraps under the spec mode when the unbroken row does not fit the table;
/// `always` keeps the line whole.
#[test]
fn wq020_no_wrap_in_a_fixed_cell_follows_the_option() {
    let body = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"1304\"/><w:gridCol w:w=\"1304\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"1304\" w:type=\"dxa\"/><w:noWrap/></w:tcPr>\
<w:p><w:r><w:t>Fossil feathers (tail) of the preserved specimen</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:tcW w:w=\"1304\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl><w:p/>{NARROW}"
    );
    let lines = |body: &str, mode: CellNoWrap| {
        let page = pages(body, &RenderOptions::default().cell_no_wrap(mode)).remove(0);
        let mut found: Vec<f64> = texts(&page)
            .into_iter()
            .filter(|(_, _, text)| text != "x")
            .map(|(_, y, _)| y)
            .collect();
        found.sort_by(f64::total_cmp);
        found.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        found.len()
    };
    assert!(
        lines(&body, CellNoWrap::Spec) > 1,
        "wraps inside the 1304-twip cell"
    );
    assert_eq!(lines(&body, CellNoWrap::Always), 1, "the old behaviour");
    // A fixed-layout table keeps the text unbroken (WPS, doc 092).
    let fixed = body.replace(
        "<w:tblW w:w=\"0\" w:type=\"auto\"/>",
        "<w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLayout w:type=\"fixed\"/>",
    );
    assert_eq!(lines(&fixed, CellNoWrap::Spec), 1);
}

/// WQ-006: a `w:framePr` in the document defaults does not turn every
/// paragraph into a frame.
#[test]
fn wq006_a_default_frame_does_not_stack_the_paragraphs() {
    let styles = "<w:docDefaults><w:pPrDefault><w:pPr>\
<w:framePr w:w=\"0\" w:h=\"0\" w:xAlign=\"left\" w:y=\"0\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>";
    let body = "<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>";
    let (package, document) = open_with_styles(body, styles);
    let page = place_pages(&document, &RenderOptions::default(), Some(&package))
        .expect("place")
        .remove(0);
    let found = baselines(&page);
    assert_eq!(found.len(), 2, "two lines, not one stack: {found:?}");
}
