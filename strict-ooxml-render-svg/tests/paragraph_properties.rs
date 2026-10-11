//! Paragraph properties that used to stop at the model
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §2): the strokes of `w:pBdr`,
//! `w:contextualSpacing`, autospacing, `w:beforeLines` and `w:widowControl`.

#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::doc_markdown,
    clippy::format_collect,
    clippy::needless_pass_by_value,
    clippy::similar_names
)]

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{place_pages, render, Item, PlacedPage, RenderOptions, TextItem};
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn document(body: &str) -> Document {
    document_with_styles(body, "")
}

fn document_with_styles(body: &str, styles: &str) -> Document {
    let mut builder = DocxBuilder::strict().body(body);
    if !styles.is_empty() {
        builder = builder
            .part_xml("word/styles.xml", "w:styles", styles)
            .content_type(
                "/word/styles.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
            )
            .rel("rIdStyles", "styles", "styles.xml");
    }
    let bytes = builder.build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    parse_document(&package, &ParseOptions::default()).expect("parse")
}

fn pages(doc: &Document) -> Vec<PlacedPage> {
    place_pages(doc, &RenderOptions::default(), None).expect("place")
}

#[test]
fn rtl_start_alignment_and_first_line_indent_use_the_right_edge() {
    let plain = document("<w:p><w:pPr><w:bidi/><w:jc w:val=\"start\"/><w:ind w:start=\"300\" w:end=\"900\"/></w:pPr><w:r><w:t>Label</w:t></w:r></w:p>");
    let indented = document("<w:p><w:pPr><w:bidi/><w:jc w:val=\"start\"/><w:ind w:start=\"300\" w:end=\"900\" w:firstLine=\"600\"/></w:pPr><w:r><w:t>Label</w:t></w:r></w:p>");
    let (_, normal) = text_on(&pages(&plain), "Label");
    let (_, inset) = text_on(&pages(&indented), "Label");
    assert!((normal.x - inset.x - 40.0).abs() < 0.01);
    assert!(normal.x > 400.0, "RTL start is the right edge");
}

fn text_on(pages: &[PlacedPage], needle: &str) -> (usize, TextItem) {
    for (index, page) in pages.iter().enumerate() {
        for item in &page.items {
            if let Item::Text(text) = item {
                if text.text.contains(needle) {
                    return (index, text.clone());
                }
            }
        }
    }
    panic!("no text {needle:?}");
}

/// `(x1, y1, x2, y2, width)` of every line item, all pages.
fn lines(pages: &[PlacedPage]) -> Vec<(f64, f64, f64, f64, f64)> {
    pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            Item::Line(line) => Some((line.x1, line.y1, line.x2, line.y2, line.width)),
            _ => None,
        })
        .collect()
}

fn horizontal(lines: &[(f64, f64, f64, f64, f64)]) -> Vec<f64> {
    let mut ys: Vec<f64> = lines
        .iter()
        .filter(|(_, y1, _, y2, _)| (y1 - y2).abs() < 1e-9)
        .map(|(_, y, _, _, _)| *y)
        .collect();
    ys.sort_by(f64::total_cmp);
    ys
}

const BOX: &str = "<w:pBdr>\
    <w:top w:val=\"single\" w:sz=\"8\" w:space=\"1\" w:color=\"000000\"/>\
    <w:left w:val=\"single\" w:sz=\"8\" w:space=\"4\" w:color=\"000000\"/>\
    <w:bottom w:val=\"single\" w:sz=\"8\" w:space=\"1\" w:color=\"000000\"/>\
    <w:right w:val=\"single\" w:sz=\"8\" w:space=\"4\" w:color=\"000000\"/>\
    </w:pBdr>";

#[test]
fn a_boxed_paragraph_draws_four_edges_around_its_text() {
    let doc = document(&format!(
        "<w:p><w:pPr>{BOX}</w:pPr><w:r><w:t>Boxed</w:t></w:r></w:p>"
    ));
    let pages = pages(&doc);
    let (_, text) = text_on(&pages, "Boxed");
    let lines = lines(&pages);
    assert_eq!(lines.len(), 4, "{lines:?}");
    let ys = horizontal(&lines);
    assert_eq!(ys.len(), 2);
    assert!(
        ys[0] < text.baseline - text.size_px * 0.7,
        "top edge above the text"
    );
    assert!(ys[1] > text.baseline, "bottom edge below the baseline");
    let verticals: Vec<f64> = lines
        .iter()
        .filter(|(x1, _, x2, _, _)| (x1 - x2).abs() < 1e-9)
        .map(|(x, _, _, _, _)| *x)
        .collect();
    assert_eq!(verticals.len(), 2);
    assert!(
        verticals.iter().any(|x| *x < text.x),
        "left edge left of the text"
    );
}

#[test]
fn a_double_top_border_is_two_lines() {
    let doc = document(
        "<w:p><w:pPr><w:pBdr><w:top w:val=\"double\" w:sz=\"8\" w:space=\"1\"/></w:pBdr></w:pPr>\
         <w:r><w:t>Double</w:t></w:r></w:p>",
    );
    assert_eq!(horizontal(&lines(&pages(&doc))).len(), 2);
}

#[test]
fn paragraphs_with_the_same_borders_share_one_box() {
    let shared = "<w:pBdr>\
        <w:top w:val=\"single\" w:sz=\"8\" w:space=\"1\"/>\
        <w:bottom w:val=\"single\" w:sz=\"8\" w:space=\"1\"/>\
        <w:between w:val=\"single\" w:sz=\"8\" w:space=\"1\"/>\
        </w:pBdr>";
    let doc = document(&format!(
        "<w:p><w:pPr>{shared}</w:pPr><w:r><w:t>One</w:t></w:r></w:p>\
         <w:p><w:pPr>{shared}</w:pPr><w:r><w:t>Two</w:t></w:r></w:p>"
    ));
    let pages = pages(&doc);
    let ys = horizontal(&lines(&pages));
    // Top of the box, the line between, bottom of the box: no bottom edge
    // under the first paragraph and no top edge over the second.
    assert_eq!(ys.len(), 3, "{ys:?}");
    let (_, one) = text_on(&pages, "One");
    let (_, two) = text_on(&pages, "Two");
    assert!(ys[0] < one.baseline && ys[1] > one.baseline && ys[1] < two.baseline);
    assert!(ys[2] > two.baseline);
}

#[test]
fn a_border_in_a_table_cell_is_drawn() {
    let doc = document(&format!(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"4000\"/></w:tblGrid><w:tr><w:tc>\
         <w:p><w:pPr>{BOX}<w:shd w:val=\"clear\" w:fill=\"EEEEEE\"/></w:pPr><w:r><w:t>Cell</w:t></w:r></w:p>\
         </w:tc></w:tr></w:tbl>"
    ));
    let svg: String = render(&doc, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .map(|page| page.svg)
        .collect();
    assert!(svg.contains("fill=\"#eeeeee\""), "cell paragraph shading");
    assert!(svg.matches("<line ").count() >= 4, "{svg}");
}

fn gap(doc: &Document, first: &str, second: &str) -> f64 {
    let pages = pages(doc);
    text_on(&pages, second).1.baseline - text_on(&pages, first).1.baseline
}

#[test]
fn contextual_spacing_drops_spacing_between_paragraphs_of_one_style() {
    let styles = "<w:style w:type=\"paragraph\" w:styleId=\"List\"><w:name w:val=\"List\"/>\
        <w:pPr><w:spacing w:before=\"240\" w:after=\"240\"/></w:pPr></w:style>\
        <w:style w:type=\"paragraph\" w:styleId=\"Tight\"><w:name w:val=\"Tight\"/>\
        <w:pPr><w:spacing w:before=\"240\" w:after=\"240\"/><w:contextualSpacing/></w:pPr></w:style>";
    let spaced = document_with_styles(
        "<w:p><w:pPr><w:pStyle w:val=\"List\"/></w:pPr><w:r><w:t>A</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"List\"/></w:pPr><w:r><w:t>B</w:t></w:r></w:p>",
        styles,
    );
    let tight = document_with_styles(
        "<w:p><w:pPr><w:pStyle w:val=\"Tight\"/></w:pPr><w:r><w:t>A</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Tight\"/></w:pPr><w:r><w:t>B</w:t></w:r></w:p>",
        styles,
    );
    // 240 twips is 16 px.
    let difference = gap(&spaced, "A", "B") - gap(&tight, "A", "B");
    assert!((difference - 16.0).abs() < 0.01, "{difference}");
}

#[test]
fn autospacing_and_spacing_in_lines_set_the_space_before() {
    let plain = document("<w:p><w:r><w:t>A</w:t></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r></w:p>");
    let auto = document(
        "<w:p><w:r><w:t>A</w:t></w:r></w:p>\
         <w:p><w:pPr><w:spacing w:before=\"0\" w:beforeAutospacing=\"1\"/></w:pPr><w:r><w:t>B</w:t></w:r></w:p>",
    );
    let lines = document(
        "<w:p><w:r><w:t>A</w:t></w:r></w:p>\
         <w:p><w:pPr><w:spacing w:before=\"0\" w:beforeLines=\"100\"/></w:pPr><w:r><w:t>B</w:t></w:r></w:p>",
    );
    let base = gap(&plain, "A", "B");
    // 14 pt is 18.667 px; one line of 12 pt is 16 px.
    assert!((gap(&auto, "A", "B") - base - 56.0 / 3.0).abs() < 0.01);
    assert!((gap(&lines, "A", "B") - base - 16.0).abs() < 0.01);
}

/// A page one inch tall holds five lines of 11 pt Calibri.
fn short_page(body: &str) -> Document {
    document(&format!(
        "{body}<w:sectPr><w:pgSz w:w=\"6000\" w:h=\"1440\"/>\
         <w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\"/>\
         </w:sectPr>"
    ))
}

fn filler(count: usize) -> String {
    (0..count)
        .map(|index| format!("<w:p><w:r><w:t>filler{index}</w:t></w:r></w:p>"))
        .collect()
}

fn lines_paragraph(widow_control: bool, words: &[&str]) -> String {
    let control = if widow_control {
        "<w:widowControl/>"
    } else {
        ""
    };
    let runs = words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let brk = if index + 1 < words.len() {
                "<w:br/>"
            } else {
                ""
            };
            format!("<w:r><w:t>{word}</w:t>{brk}</w:r>")
        })
        .collect::<String>();
    format!("<w:p><w:pPr>{control}</w:pPr>{runs}</w:p>")
}

#[test]
fn widow_control_moves_an_orphan_to_the_next_page() {
    let words = ["alpha", "beta", "gamma"];
    let free = short_page(&(filler(4) + &lines_paragraph(false, &words)));
    let kept = short_page(&(filler(4) + &lines_paragraph(true, &words)));
    assert_eq!(text_on(&pages(&free), "alpha").0, 0);
    assert_eq!(
        text_on(&pages(&kept), "alpha").0,
        1,
        "the orphan line moves on"
    );
}

#[test]
fn widow_control_pulls_a_line_over_to_keep_the_last_one_company() {
    let words = ["alpha", "beta", "gamma", "delta"];
    let free = short_page(&(filler(1) + &lines_paragraph(false, &words)));
    let kept = short_page(&(filler(1) + &lines_paragraph(true, &words)));
    let free_pages = pages(&free);
    let kept_pages = pages(&kept);
    assert_eq!(text_on(&free_pages, "gamma").0, 0);
    assert_eq!(text_on(&free_pages, "delta").0, 0);
    assert_eq!(text_on(&kept_pages, "gamma").0, 0);
    assert_eq!(text_on(&kept_pages, "delta").0, 0);
    let free = short_page(&(filler(2) + &lines_paragraph(false, &words)));
    let kept = short_page(&(filler(2) + &lines_paragraph(true, &words)));
    assert_eq!(text_on(&pages(&free), "gamma").0, 0);
    assert_eq!(text_on(&pages(&free), "delta").0, 1);
    assert_eq!(text_on(&pages(&kept), "beta").0, 0);
    assert_eq!(
        text_on(&pages(&kept), "gamma").0,
        1,
        "two lines go over together"
    );
}
