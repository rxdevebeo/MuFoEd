//! Section and page properties that used to stop at the model
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §4): `w:gutter`, `w:mirrorMargins`,
//! `w:vAlign`, `w:cols/@w:sep` and `w:lnNumType`.

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

fn open(builder: DocxBuilder) -> Document {
    let bytes = builder.build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    parse_document(&package, &ParseOptions::default()).expect("parse")
}

fn document(body: &str) -> Document {
    open(DocxBuilder::strict().body(body))
}

fn with_settings(body: &str, settings: &str) -> Document {
    open(
        DocxBuilder::strict()
            .body(body)
            .part_xml("word/settings.xml", "w:settings", settings)
            .content_type(
                "/word/settings.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
            )
            .rel("rIdSettings", "settings", "settings.xml"),
    )
}

fn pages(doc: &Document) -> Vec<PlacedPage> {
    place_pages(doc, &RenderOptions::default(), None).expect("place")
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
        .find(|item| item.text.contains(needle))
        .unwrap_or_else(|| panic!("no text {needle:?}"))
}

fn section(extra: &str) -> String {
    format!(
        "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
         <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"2880\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/>\
         {extra}</w:sectPr>"
    )
}

#[test]
fn the_gutter_widens_the_left_margin() {
    let doc = document(
        "<w:p><w:r><w:t>Text</w:t></w:r></w:p>\
         <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
         <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:gutter=\"720\"/></w:sectPr>",
    );
    // 2160 twips is 144 px.
    assert!((text(&pages(&doc)[0], "Text").x - 144.0).abs() < 0.01);
}

#[test]
fn mirrored_margins_put_the_wide_margin_inside() {
    let body = format!(
        "<w:p><w:r><w:t>Odd</w:t></w:r></w:p>\
         <w:p><w:r><w:br w:type=\"page\"/><w:t>Even</w:t></w:r></w:p>{}",
        section("")
    );
    let plain = pages(&document(&body));
    let mirrored = pages(&with_settings(&body, "<w:mirrorMargins/>"));
    assert_eq!(mirrored.len(), 2);
    // Left 2880 twips (192 px), right 1440 twips (96 px).
    assert!((text(&plain[1], "Even").x - 192.0).abs() < 0.01);
    assert!((text(&mirrored[0], "Odd").x - 192.0).abs() < 0.01);
    assert!((text(&mirrored[1], "Even").x - 96.0).abs() < 0.01);
}

#[test]
fn vertical_alignment_centres_the_page_content() {
    let top = pages(&document(&format!(
        "<w:p><w:r><w:t>Alone</w:t></w:r></w:p>{}",
        section("")
    )));
    let centred = pages(&document(&format!(
        "<w:p><w:r><w:t>Alone</w:t></w:r></w:p>{}",
        section("<w:vAlign w:val=\"center\"/>")
    )));
    let y = text(&centred[0], "Alone").baseline;
    assert!(y > text(&top[0], "Alone").baseline + 300.0, "{y}");
    // The page is 1056 px tall: the line sits near its middle.
    assert!((y - 528.0).abs() < 20.0, "{y}");
}

#[test]
fn a_column_separator_is_drawn_between_columns() {
    let body = |sep: &str| {
        format!(
            "<w:p><w:r><w:t>Columns</w:t></w:r></w:p>{}",
            section(&format!("<w:cols w:num=\"2\" w:space=\"720\"{sep}/>"))
        )
    };
    let count = |doc: &Document| {
        pages(doc)[0]
            .items
            .iter()
            .filter(|item| matches!(item, Item::Line(line) if (line.x1 - line.x2).abs() < 1e-9))
            .count()
    };
    assert_eq!(count(&document(&body(""))), 0);
    assert_eq!(count(&document(&body(" w:sep=\"1\""))), 1);
}

#[test]
fn line_numbers_count_body_lines_in_the_margin() {
    let body = format!(
        "<w:p><w:r><w:t>One</w:t></w:r></w:p>\
         <w:p><w:pPr><w:suppressLineNumbers/></w:pPr><w:r><w:t>Skipped</w:t></w:r></w:p>\
         <w:p><w:r><w:t>Two</w:t></w:r></w:p>\
         <w:p><w:r><w:t>Three</w:t></w:r></w:p>{}",
        section("<w:lnNumType w:countBy=\"1\"/>")
    );
    let page = &pages(&document(&body))[0];
    let numbers: Vec<TextItem> = texts(page)
        .into_iter()
        .filter(|item| item.text.chars().all(|ch| ch.is_ascii_digit()))
        .collect();
    let labels: Vec<&str> = numbers.iter().map(|item| item.text.as_str()).collect();
    assert_eq!(labels, ["1", "2", "3"]);
    assert!(
        numbers.iter().all(|item| item.x + item.width < 192.0),
        "in the margin"
    );
    assert_eq!(numbers[1].baseline, text(page, "Two").baseline);

    let every_other = format!(
        "<w:p><w:r><w:t>A</w:t></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r></w:p>{}",
        section("<w:lnNumType w:countBy=\"2\" w:start=\"0\"/>")
    );
    let page = &pages(&document(&every_other))[0];
    let labels: Vec<String> = texts(page)
        .into_iter()
        .filter(|item| item.text.chars().all(|ch| ch.is_ascii_digit()))
        .map(|item| item.text)
        .collect();
    assert_eq!(labels, ["2"]);
}
