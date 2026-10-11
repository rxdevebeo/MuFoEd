//! Run properties that used to stop at the model (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §3):
//! underline style and colour, double strikethrough, small capitals,
//! `w:position`, run shading with its pattern, and the face of `w:sym`.

#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::doc_markdown,
    clippy::format_collect,
    clippy::needless_pass_by_value,
    clippy::similar_names
)]

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{place_pages, render, Item, RenderOptions, TextItem};
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn document(body: &str) -> Document {
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    parse_document(&package, &ParseOptions::default()).expect("parse")
}

#[test]
fn run_border_survives_parsing_and_is_drawn_on_all_four_sides() {
    let doc = document("<w:p><w:r><w:rPr><w:bdr w:val=\"double\" w:sz=\"8\" w:color=\"FF0000\" w:space=\"2\"/></w:rPr><w:t>Border</w:t></w:r></w:p>");
    let item = text(&doc, "Border");
    assert!(item.run.border.is_some());
    let ink = lines(&svg(&doc));
    assert_eq!(
        ink.iter().filter(|(color, _)| color == "#ff0000").count(),
        8
    );
}

#[test]
fn an_explicit_complex_script_run_uses_its_cs_formatting() {
    let doc = document("<w:p><w:r><w:rPr><w:cs/><w:bCs/><w:iCs/><w:szCs w:val=\"36\"/></w:rPr><w:t>Complex</w:t></w:r></w:p>");
    let item = text(&doc, "Complex");
    assert!(item.run.bold && item.run.italic);
    assert_eq!(item.run.size_pt, 18.0);
}

fn texts(doc: &Document) -> Vec<TextItem> {
    place_pages(doc, &RenderOptions::default(), None)
        .expect("place")
        .into_iter()
        .flat_map(|page| page.items)
        .filter_map(|item| match item {
            Item::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

fn text(doc: &Document, needle: &str) -> TextItem {
    texts(doc)
        .into_iter()
        .find(|item| item.text.contains(needle))
        .unwrap_or_else(|| panic!("no text {needle:?}"))
}

fn svg(doc: &Document) -> String {
    render(doc, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .map(|page| page.svg)
        .collect()
}

/// `(stroke, y1)` of every `<line>` in the SVG.
fn lines(svg: &str) -> Vec<(String, f64)> {
    let parsed = roxmltree::Document::parse(svg).expect("svg");
    parsed
        .descendants()
        .filter(|node| node.tag_name().name() == "line")
        .map(|node| {
            (
                node.attribute("stroke").unwrap_or("").to_owned(),
                node.attribute("y1")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(f64::NAN),
            )
        })
        .collect()
}

#[test]
fn a_double_underline_in_its_own_colour_is_two_lines_below_the_baseline() {
    let doc = document(
        "<w:p><w:r><w:rPr><w:u w:val=\"double\" w:color=\"FF0000\"/></w:rPr><w:t>Under</w:t></w:r></w:p>",
    );
    let baseline = text(&doc, "Under").baseline;
    let svg = svg(&doc);
    assert!(!svg.contains("text-decoration"), "no SVG text decoration");
    let red: Vec<f64> = lines(&svg)
        .into_iter()
        .filter(|(stroke, _)| stroke == "#ff0000")
        .map(|(_, y)| y)
        .collect();
    assert_eq!(red.len(), 2, "{svg}");
    assert!(red.iter().all(|y| *y > baseline), "below the baseline");
}

#[test]
fn a_dotted_underline_is_a_dashed_path() {
    let doc =
        document("<w:p><w:r><w:rPr><w:u w:val=\"dotted\"/></w:rPr><w:t>Dots</w:t></w:r></w:p>");
    assert!(svg(&doc).contains("stroke-dasharray"));
}

#[test]
fn double_strikethrough_is_two_lines_and_single_is_one() {
    let double = document("<w:p><w:r><w:rPr><w:dstrike/></w:rPr><w:t>Gone</w:t></w:r></w:p>");
    let single = document("<w:p><w:r><w:rPr><w:strike/></w:rPr><w:t>Gone</w:t></w:r></w:p>");
    let baseline = text(&double, "Gone").baseline;
    let double_lines = lines(&svg(&double));
    assert_eq!(double_lines.len(), 2);
    assert!(
        double_lines.iter().all(|(_, y)| *y < baseline),
        "above the baseline"
    );
    assert_eq!(lines(&svg(&single)).len(), 1);
}

#[test]
fn small_caps_draws_lowercase_as_smaller_capitals() {
    let doc = document(
        "<w:p><w:r><w:rPr><w:smallCaps/><w:sz w:val=\"20\"/></w:rPr><w:t>Hello</w:t></w:r></w:p>",
    );
    let capital = text(&doc, "H");
    let small = text(&doc, "ELLO");
    assert_eq!(capital.run.size_pt, 10.0);
    assert!(
        (small.run.size_pt - 8.0).abs() < 1e-9,
        "{}",
        small.run.size_pt
    );
    assert!(small.x > capital.x);
    assert!(texts(&doc).iter().all(|item| !item.text.contains("ello")));
}

#[test]
fn position_raises_and_lowers_the_baseline() {
    let doc = document(
        "<w:p><w:r><w:t xml:space=\"preserve\">base </w:t></w:r>\
         <w:r><w:rPr><w:position w:val=\"12\"/></w:rPr><w:t xml:space=\"preserve\">up </w:t></w:r>\
         <w:r><w:rPr><w:position w:val=\"-12\"/></w:rPr><w:t>down</w:t></w:r></w:p>",
    );
    let base = text(&doc, "base").baseline;
    let up = text(&doc, "up").baseline;
    let down = text(&doc, "down").baseline;
    // 12 half-points is 6 pt, which is 8 px at the default scale.
    assert!((base - up - 8.0).abs() < 0.01, "{base} {up}");
    assert!((down - base - 8.0).abs() < 0.01, "{base} {down}");
}

#[test]
fn run_shading_is_painted_behind_the_text() {
    let doc = document(
        "<w:p><w:r><w:rPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"FFCC00\"/></w:rPr><w:t>Shaded</w:t></w:r></w:p>",
    );
    let svg = svg(&doc);
    let rect = svg.find("fill=\"#ffcc00\"").expect("shading rectangle");
    let glyphs = svg.find(">Shaded<").expect("text");
    assert!(rect < glyphs, "the shading is under the glyphs");
}

#[test]
fn solid_and_percent_patterns_blend_the_pattern_colour() {
    let solid = document(
        "<w:p><w:r><w:rPr><w:shd w:val=\"solid\" w:color=\"333333\" w:fill=\"auto\"/></w:rPr><w:t>Dark</w:t></w:r></w:p>",
    );
    let item = text(&solid, "Dark");
    assert_eq!(item.run.shading.as_deref(), Some("#333333"));
    assert_eq!(
        item.run.color.as_deref(),
        Some("#ffffff"),
        "auto text turns white on a dark fill"
    );

    let percent = document(
        "<w:p><w:r><w:rPr><w:shd w:val=\"pct20\" w:color=\"auto\" w:fill=\"auto\"/></w:rPr><w:t>Grey</w:t></w:r></w:p>",
    );
    let item = text(&percent, "Grey");
    assert_eq!(item.run.shading.as_deref(), Some("#cccccc"));
    assert_eq!(
        item.run.color, None,
        "auto text stays black on a light fill"
    );

    let nil = document(
        "<w:p><w:r><w:rPr><w:shd w:val=\"nil\" w:fill=\"FF0000\"/></w:rPr><w:t>None</w:t></w:r></w:p>",
    );
    assert_eq!(text(&nil, "None").run.shading, None);
}

#[test]
fn a_symbol_is_drawn_in_its_own_face() {
    let doc = document(
        "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/></w:rPr>\
         <w:sym w:font=\"Symbol\" w:char=\"F061\"/></w:r></w:p>",
    );
    let item = text(&doc, "\u{F061}");
    assert_eq!(item.run.family, "Symbol");
    assert!(svg(&doc).contains(">α<"), "the Symbol `a` is an alpha");
}

#[test]
fn an_east_asian_face_is_chosen_for_cjk_text_only() {
    let doc = document(
        "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:eastAsia=\"Times New Roman\"/></w:rPr>\
         <w:t xml:space=\"preserve\">Latin </w:t></w:r>\
         <w:r><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:eastAsia=\"Times New Roman\"/></w:rPr>\
         <w:t>漢字</w:t></w:r></w:p>",
    );
    let latin = text(&doc, "Latin");
    let cjk = text(&doc, "漢字");
    assert_eq!(
        strict_ooxml_render_svg::style::chosen_family(&latin.run, &latin.text),
        "Arimo"
    );
    assert_eq!(
        strict_ooxml_render_svg::style::chosen_family(&cjk.run, &cjk.text),
        "Tinos"
    );
}
