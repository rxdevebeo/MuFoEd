//! Math properties that used to stop at the model
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §7): `m:defJc` and the margins of
//! `m:mathPr`, `m:naryLim`/`m:intLim`, and `m:mcPr/m:count`.

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

fn open(body: &str, settings: &str) -> Document {
    let mut builder = DocxBuilder::strict().body(body);
    if !settings.is_empty() {
        builder = builder
            .part_xml("word/settings.xml", "w:settings", settings)
            .content_type(
                "/word/settings.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
            )
            .rel("rIdSettings", "settings", "settings.xml");
    }
    let bytes = builder.build();
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

fn texts(page: &PlacedPage) -> Vec<TextItem> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn text(page: &PlacedPage, needle: &str) -> TextItem {
    texts(page)
        .into_iter()
        .find(|item| item.text == needle)
        .unwrap_or_else(|| panic!("no text {needle:?}"))
}

const DISPLAY: &str =
    "<w:p><m:oMathPara><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara></w:p>";

#[test]
fn a_display_formula_without_justification_is_centred() {
    let x = text(&page(&open(DISPLAY, "")), "x");
    // The default 6.5-inch column runs from 96 px to 720 px.
    assert!((x.x + x.width / 2.0 - 408.0).abs() < 2.0, "{}", x.x);
}

#[test]
fn the_document_default_justification_and_margins_apply() {
    let left = text(
        &page(&open(
            DISPLAY,
            "<m:mathPr><m:defJc m:val=\"left\"/></m:mathPr>",
        )),
        "x",
    );
    assert!(left.x < 100.0, "{}", left.x);
    let indented = text(
        &page(&open(
            DISPLAY,
            "<m:mathPr><m:defJc m:val=\"left\"/><m:lMargin m:val=\"1440\"/></m:mathPr>",
        )),
        "x",
    );
    assert!((indented.x - left.x - 96.0).abs() < 0.01, "{}", indented.x);
}

fn sum(limit_location: &str) -> String {
    format!(
        "<w:p><m:oMathPara><m:oMath><m:nary><m:naryPr><m:chr m:val=\"\u{2211}\"/>{limit_location}</m:naryPr>\
         <m:sub><m:r><m:t>a</m:t></m:r></m:sub><m:sup><m:r><m:t>b</m:t></m:r></m:sup>\
         <m:e><m:r><m:t>c</m:t></m:r></m:e></m:nary></m:oMath></m:oMathPara></w:p>"
    )
}

#[test]
fn a_display_sum_stacks_its_limits_by_default() {
    let stacked = page(&open(&sum("<m:limLoc m:val=\"undOvr\"/>"), ""));
    let side = page(&open(&sum("<m:limLoc m:val=\"subSup\"/>"), ""));
    let default = page(&open(&sum(""), ""));
    let below = |page: &PlacedPage| text(page, "a").x - text(page, "c").x;
    assert!((below(&default) - below(&stacked)).abs() < 0.01);
    assert!((below(&default) - below(&side)).abs() > 1.0);
    let declared = page(&open(
        &sum(""),
        "<m:mathPr><m:naryLim m:val=\"subSup\"/></m:mathPr>",
    ));
    assert!((below(&declared) - below(&side)).abs() < 0.01);
}

#[test]
fn a_column_spec_covers_its_count_of_columns() {
    let matrix = |count: &str| {
        format!(
            "<w:p><m:oMathPara><m:oMath><m:m><m:mPr><m:mcs><m:mc><m:mcPr>{count}<m:mcJc m:val=\"right\"/></m:mcPr></m:mc></m:mcs></m:mPr>\
             <m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:t>2</m:t></m:r></m:e></m:mr>\
             <m:mr><m:e><m:r><m:t>333</m:t></m:r></m:e><m:e><m:r><m:t>444</m:t></m:r></m:e></m:mr>\
             </m:m></m:oMath></m:oMathPara></w:p>"
        )
    };
    let gap = |page: &PlacedPage| {
        let short = text(page, "2");
        let long = text(page, "444");
        (short.x + short.width) - (long.x + long.width)
    };
    let one = page(&open(&matrix(""), ""));
    let two = page(&open(&matrix("<m:count m:val=\"2\"/>"), ""));
    assert!(gap(&one).abs() > 1.0, "the second column is centred");
    assert!(
        gap(&two).abs() < 0.01,
        "the second column is right-aligned too"
    );
}
