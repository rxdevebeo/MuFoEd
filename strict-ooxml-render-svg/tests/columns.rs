//! Section columns (`w:cols`, ISO/IEC 29500-1 §17.6.3, §17.6.4): widths,
//! right-to-left order, the separator, balancing before a continuous break,
//! next-column sections, and the objects that follow a column - footnotes,
//! floating objects and frames.
//!
//! The page is Letter with one-inch margins: the content box is 624 px wide
//! from x = 96. Two equal columns 720 twips (48 px) apart are 288 px wide, the
//! second starting at x = 432.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::format_push_string)]

mod common;

use common::{build_docx, content_types, document, open_bytes, render_body, root_rels, W};
use strict_ooxml_render_svg::{render, RenderOptions};

const PAGE: &str = "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>";

const SHAPE_DATA_URI: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingShape";

fn para(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

fn column_break() -> &'static str {
    "<w:p><w:r><w:br w:type=\"column\"/></w:r></w:p>"
}

/// `<text>` nodes containing `needle`, as `(x, y)` of the first glyph.
fn texts_at(svg: &str, needle: &str) -> Vec<(f64, f64)> {
    let document = roxmltree::Document::parse(svg).expect("svg");
    document
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "text"
                && node.text().is_some_and(|text| text.contains(needle))
        })
        .map(|node| {
            let coord = |name: &str| -> f64 {
                node.attribute(name)
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse().ok())
                    .expect(name)
            };
            (coord("x"), coord("y"))
        })
        .collect()
}

fn text_at(svg: &str, needle: &str) -> (f64, f64) {
    texts_at(svg, needle)
        .first()
        .copied()
        .unwrap_or_else(|| panic!("{needle} is not on the page"))
}

/// `(x1, y1, x2, y2)` of every `<line>`.
fn lines(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    let document = roxmltree::Document::parse(svg).expect("svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "line")
        .map(|node| {
            let value = |name: &str| -> f64 {
                node.attribute(name)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0.0)
            };
            (value("x1"), value("y1"), value("x2"), value("y2"))
        })
        .collect()
}

fn near(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1.5
}

#[test]
fn equal_columns_divide_the_content_width_leaving_the_space_between() {
    // The break and the text share a paragraph: a break in a paragraph of its
    // own carries that paragraph's mark, an empty line, into the next column.
    let body = format!(
        "{}<w:p><w:r><w:br w:type=\"column\"/><w:t>Right</w:t></w:r></w:p><w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>",
        para("Left")
    );
    let pages = render_body(&body);
    assert_eq!(pages.len(), 1);
    let (left, left_y) = text_at(&pages[0].svg, "Left");
    let (right, right_y) = text_at(&pages[0].svg, "Right");
    assert!(near(left, 96.0), "first column at the margin: {left}");
    assert!(
        near(right, 432.0),
        "second column after 288 + 48 px: {right}"
    );
    assert!(
        near(left_y, right_y),
        "both columns start at the top: {left_y} {right_y}"
    );
}

#[test]
fn unequal_columns_take_each_col_width_and_space() {
    for equal_width in [" w:equalWidth=\"0\"", ""] {
        // Without equalWidth the `w:col` children still define the columns
        // (§17.6.4: num and space are ignored when equalWidth is not set).
        let body = format!(
            "{}{}{}<w:sectPr>{PAGE}<w:cols w:num=\"3\" w:space=\"1440\"{equal_width}>\
<w:col w:w=\"2880\" w:space=\"720\"/><w:col w:w=\"5760\"/></w:cols></w:sectPr>",
            para("Narrow"),
            column_break(),
            para("Wide")
        );
        let pages = render_body(&body);
        let (wide, _) = text_at(&pages[0].svg, "Wide");
        assert!(
            near(wide, 96.0 + 192.0 + 48.0),
            "second column after the first's 2880 + 720 twips ({equal_width}): {wide}"
        );
    }
}

#[test]
fn equal_width_ignores_the_col_children() {
    let body = format!(
        "{}{}{}<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\" w:equalWidth=\"1\">\
<w:col w:w=\"1000\" w:space=\"100\"/><w:col w:w=\"1000\"/></w:cols></w:sectPr>",
        para("Left"),
        column_break(),
        para("Right")
    );
    let pages = render_body(&body);
    let (right, _) = text_at(&pages[0].svg, "Right");
    assert!(near(right, 432.0), "num and space win: {right}");
}

#[test]
fn a_right_to_left_section_starts_in_the_rightmost_column() {
    let body = format!(
        "{}{}{}<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/><w:bidi/></w:sectPr>",
        para("First"),
        column_break(),
        para("Second")
    );
    let pages = render_body(&body);
    let (first, _) = text_at(&pages[0].svg, "First");
    let (second, _) = text_at(&pages[0].svg, "Second");
    assert!(
        near(first, 432.0),
        "the first column is on the right: {first}"
    );
    assert!(
        near(second, 96.0),
        "the second column is on the left: {second}"
    );
}

#[test]
fn the_separator_runs_down_the_middle_of_the_gap() {
    let body = format!(
        "{}{}{}<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\" w:sep=\"1\"/></w:sectPr>",
        para("Left"),
        column_break(),
        para("Right")
    );
    let pages = render_body(&body);
    let separators: Vec<_> = lines(&pages[0].svg)
        .into_iter()
        .filter(|(x1, _, x2, _)| near(*x1, 408.0) && near(*x2, 408.0))
        .collect();
    assert_eq!(separators.len(), 1, "one line between two columns");
    let (_, top, _, bottom) = separators[0];
    assert!(near(top, 96.0), "from the top of the columns: {top}");
    assert!(
        bottom < 200.0,
        "to the bottom of the longest column, not the page: {bottom}"
    );
}

#[test]
fn columns_are_balanced_before_a_continuous_break() {
    let mut body = String::new();
    for index in 1..=10 {
        body.push_str(&para(&format!("Item{index:02}")));
    }
    body.push_str(&format!(
        "<w:p><w:pPr><w:sectPr>{PAGE}<w:type w:val=\"continuous\"/><w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr></w:pPr></w:p>"
    ));
    body.push_str(&para("After"));
    body.push_str(&format!(
        "<w:sectPr>{PAGE}<w:type w:val=\"continuous\"/></w:sectPr>"
    ));
    let pages = render_body(&body);
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    let (first_x, first_y) = text_at(svg, "Item01");
    let in_second: Vec<_> = (1..=10)
        .map(|index| text_at(svg, &format!("Item{index:02}")))
        .filter(|(x, _)| near(*x, 432.0))
        .collect();
    assert!(near(first_x, 96.0));
    assert!(
        (4..=6).contains(&in_second.len()),
        "half the items move to the second column: {}",
        in_second.len()
    );
    assert!(
        near(in_second[0].1, first_y),
        "the second column starts at the top"
    );
    let (after_x, after_y) = text_at(svg, "After");
    let (_, last_left_y) = (1..=10)
        .map(|index| text_at(svg, &format!("Item{index:02}")))
        .rfind(|(x, _)| near(*x, 96.0))
        .expect("items in the first column");
    assert!(near(after_x, 96.0));
    assert!(
        after_y > last_left_y,
        "the next section follows the balanced columns"
    );
    assert!(
        after_y < first_y + 7.0 * 20.0,
        "and not the full ten lines of an unbalanced first column: {after_y}"
    );
}

#[test]
fn a_next_column_section_starts_at_the_top_of_the_columns_not_the_page() {
    let body = format!(
        "{}<w:p><w:pPr><w:sectPr>{PAGE}<w:type w:val=\"continuous\"/></w:sectPr></w:pPr></w:p>\
{}<w:p><w:pPr><w:sectPr>{PAGE}<w:type w:val=\"nextColumn\"/><w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr></w:pPr></w:p>\
{}<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>",
        para("Intro"),
        para("Left"),
        para("Right")
    );
    let pages = render_body(&body);
    assert_eq!(pages.len(), 1);
    let svg = &pages[0].svg;
    let (_, intro_y) = text_at(svg, "Intro");
    let (_, left_y) = text_at(svg, "Left");
    let (right_x, right_y) = text_at(svg, "Right");
    assert!(near(right_x, 432.0), "the next column: {right_x}");
    assert!(
        right_y > intro_y,
        "below the single-column text above: {right_y} {intro_y}"
    );
    assert!(
        near(right_y, left_y),
        "level with the first column's top: {right_y} {left_y}"
    );
}

#[test]
fn a_column_floating_object_is_positioned_in_its_column() {
    let anchor = format!(
        "<w:p><w:r><w:drawing><wp:anchor behindDoc=\"0\" relativeHeight=\"1\" simplePos=\"0\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" allowOverlap=\"1\" layoutInCell=\"1\" locked=\"0\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:posOffset>0</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>4572000</wp:posOffset></wp:positionV>\
<wp:extent cx=\"457200\" cy=\"457200\"/><wp:wrapNone/><wp:docPr id=\"1\" name=\"object\"/>\
<a:graphic><a:graphicData uri=\"{SHAPE_DATA_URI}\"><wps:wsp><wps:cNvPr id=\"2\" name=\"rect\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"457200\" cy=\"457200\"/></a:xfrm><a:prstGeom prst=\"rect\"/>\
<a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></wps:spPr></wps:wsp></a:graphicData></a:graphic>\
</wp:anchor></w:drawing></w:r><w:r><w:t>Host</w:t></w:r></w:p>"
    );
    let body = format!(
        "{}{}{anchor}<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>",
        para("Left"),
        column_break()
    );
    let pages = render_body(&body);
    assert!(
        pages[0].svg.contains("translate(432 480)"),
        "relativeFrom=column is the second column: {}",
        pages[0].svg
    );
}

#[test]
fn a_page_anchored_frame_ignores_the_column_it_is_written_in() {
    let body = format!(
        "{}{}<w:p><w:pPr><w:framePr w:w=\"2000\" w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"2880\" w:y=\"7200\"/></w:pPr><w:r><w:t>Framed</w:t></w:r></w:p>\
<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>",
        para("Left"),
        column_break()
    );
    let pages = render_body(&body);
    let (framed, _) = text_at(&pages[0].svg, "Framed");
    assert!(
        near(framed, 192.0),
        "two inches from the page edge, not from column 2: {framed}"
    );
}

#[test]
fn a_text_anchored_frame_follows_its_column() {
    let body = format!(
        "{}{}<w:p><w:pPr><w:framePr w:w=\"2000\" w:hAnchor=\"text\" w:vAnchor=\"page\" w:x=\"0\" w:y=\"7200\"/></w:pPr><w:r><w:t>Framed</w:t></w:r></w:p>\
<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>",
        para("Left"),
        column_break()
    );
    let pages = render_body(&body);
    let (framed, _) = text_at(&pages[0].svg, "Framed");
    assert!(near(framed, 432.0), "hAnchor=text is the column: {framed}");
}

#[test]
fn a_footnote_sits_at_the_foot_of_its_column_and_as_wide() {
    const FOOTNOTES_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footnotes";
    let body = format!(
        "{}{}<w:p><w:r><w:t>Cited</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
<w:sectPr>{PAGE}<w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>",
        para("Left"),
        column_break()
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdFn\" Type=\"{FOOTNOTES_REL}\" Target=\"footnotes.xml\"/></Relationships>"
    );
    let notes = format!(
        "<?xml version=\"1.0\"?><w:footnotes xmlns:w=\"{W}\"><w:footnote w:id=\"1\"><w:p><w:r><w:footnoteRef/></w:r>\
<w:r><w:t xml:space=\"preserve\"> ColumnNote</w:t></w:r></w:p></w:footnote></w:footnotes>"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/footnotes.xml", notes.into_bytes()),
    ]);
    let (_, parsed) = open_bytes(bytes);
    let pages = render(&parsed, &RenderOptions::default()).expect("render");
    let svg = &pages[0].svg;
    let (note_x, note_y) = text_at(svg, "ColumnNote");
    assert!(
        note_x > 432.0 && note_x < 480.0,
        "in the second column: {note_x}"
    );
    assert!(note_y > 900.0, "at the foot of the page: {note_y}");
    let separator = lines(svg)
        .into_iter()
        .find(|(x1, y1, x2, y2)| near(*y1, *y2) && near(*x1, 432.0) && *x2 > *x1)
        .expect("a footnote separator in the second column");
    assert!(
        near(separator.2 - separator.0, 96.0),
        "a third of the column, not of the page: {}",
        separator.2 - separator.0
    );
}
