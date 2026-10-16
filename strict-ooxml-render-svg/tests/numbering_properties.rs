//! Numbering and note properties that used to stop at the model
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §5): numbering through a paragraph
//! style, `w:lvlJc`, `w:isLgl`, and per-section footnote numbering with
//! `w:numRestart`.

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

const NUMBERING_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const STYLES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const FOOTNOTES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";

fn with_parts(body: &str, numbering: &str, styles: &str) -> Document {
    let mut builder = DocxBuilder::strict().body(body);
    if !numbering.is_empty() {
        builder = builder
            .part_xml("word/numbering.xml", "w:numbering", numbering)
            .content_type("/word/numbering.xml", NUMBERING_TYPE)
            .rel("rIdNumbering", "numbering", "numbering.xml");
    }
    if !styles.is_empty() {
        builder = builder
            .part_xml("word/styles.xml", "w:styles", styles)
            .content_type("/word/styles.xml", STYLES_TYPE)
            .rel("rIdStyles", "styles", "styles.xml");
    }
    open(builder)
}

fn open(builder: DocxBuilder) -> Document {
    let bytes = builder.build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    parse_document(&package, &ParseOptions::default()).expect("parse")
}

fn pages(doc: &Document) -> Vec<PlacedPage> {
    place_pages(doc, &RenderOptions::default(), None).expect("place")
}

fn texts(pages: &[PlacedPage]) -> Vec<TextItem> {
    pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            Item::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn text(pages: &[PlacedPage], needle: &str) -> TextItem {
    texts(pages)
        .into_iter()
        .find(|item| item.text == needle)
        .unwrap_or_else(|| panic!("no text {needle:?}"))
}

fn numbering(levels: &str) -> String {
    format!(
        "<w:abstractNum w:abstractNumId=\"0\">{levels}</w:abstractNum>\
         <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>"
    )
}

fn level(ilvl: u8, format: &str, text: &str, extra: &str) -> String {
    format!(
        "<w:lvl w:ilvl=\"{ilvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/>\
         <w:lvlText w:val=\"{text}\"/>{extra}<w:pPr><w:ind w:left=\"1440\" w:hanging=\"720\"/></w:pPr></w:lvl>"
    )
}

#[test]
fn a_paragraph_style_with_numbering_numbers_its_paragraphs() {
    let styles =
        "<w:style w:type=\"paragraph\" w:styleId=\"Numbered\"><w:name w:val=\"Numbered\"/>\
        <w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>";
    let doc = with_parts(
        "<w:p><w:pPr><w:pStyle w:val=\"Numbered\"/></w:pPr><w:r><w:t>First</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Numbered\"/></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>",
        &numbering(&level(0, "decimal", "%1.", "")),
        styles,
    );
    let pages = pages(&doc);
    assert!(text(&pages, "1.").baseline < text(&pages, "2.").baseline);
    assert_eq!(text(&pages, "2.").baseline, text(&pages, "Second").baseline);
}

#[test]
fn a_right_justified_marker_ends_on_the_number_position() {
    let marker = |justification: &str| {
        let doc = with_parts(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
             <w:r><w:t>Item</w:t></w:r></w:p>",
            &numbering(&level(0, "upperRoman", "%1.", justification)),
            "",
        );
        text(&pages(&doc), "I.")
    };
    let left = marker("<w:lvlJc w:val=\"start\"/>");
    let right = marker("<w:lvlJc w:val=\"end\"/>");
    // The number position is the first-line indent: 1440 - 720 twips past the
    // 1-inch margin, 144 px.
    assert!((left.x - 144.0).abs() < 0.01, "{}", left.x);
    assert!((right.x + right.width - 144.0).abs() < 0.01, "{}", right.x);
}

#[test]
fn legal_numbering_writes_every_level_in_digits() {
    let levels =
        |legal: &str| level(0, "upperRoman", "%1.", "") + &level(1, "decimal", "%1.%2", legal);
    let body = "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Part</w:t></w:r></w:p>\
                <w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Clause</w:t></w:r></w:p>";
    let plain = pages(&with_parts(body, &numbering(&levels("")), ""));
    let legal = pages(&with_parts(body, &numbering(&levels("<w:isLgl/>")), ""));
    assert!(texts(&plain).iter().any(|item| item.text == "I.1"));
    assert!(texts(&legal).iter().any(|item| item.text == "1.1"));
}

#[test]
fn footnotes_restart_in_each_section_that_asks() {
    let note = |id: u32| {
        format!(
            "<w:footnote w:id=\"{id}\"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>note{id}</w:t></w:r></w:p></w:footnote>"
        )
    };
    let footnotes = format!(
        "<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>{}{}",
        note(1),
        note(2)
    );
    let body = |restart: &str| {
        format!(
            "<w:p><w:pPr><w:sectPr><w:footnotePr>{restart}</w:footnotePr></w:sectPr></w:pPr>\
             <w:r><w:t>One</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
             <w:p><w:r><w:t>Two</w:t></w:r><w:r><w:footnoteReference w:id=\"2\"/></w:r></w:p>\
             <w:sectPr><w:footnotePr>{restart}<w:numFmt w:val=\"lowerLetter\"/></w:footnotePr></w:sectPr>"
        )
    };
    let markers = |restart: &str| {
        let doc = open(
            DocxBuilder::strict()
                .body(&body(restart))
                .part_xml("word/footnotes.xml", "w:footnotes", &footnotes)
                .content_type("/word/footnotes.xml", FOOTNOTES_TYPE)
                .rel("rIdFootnotes", "footnotes", "footnotes.xml"),
        );
        let pages = pages(&doc);
        // Each section starts a page: look for the marker on the word's page.
        let after = |word: &str| {
            let page = pages
                .iter()
                .find(|page| {
                    texts(std::slice::from_ref(*page))
                        .iter()
                        .any(|item| item.text == word)
                })
                .expect("page");
            let on_page = texts(std::slice::from_ref(page));
            let anchor = text(std::slice::from_ref(page), word);
            on_page
                .into_iter()
                .find(|item| {
                    (item.baseline - anchor.baseline).abs() < anchor.size_px / 2.0
                        && item.x > anchor.x
                        && item.text != word
                })
                .map(|item| item.text)
                .unwrap_or_default()
        };
        (after("One"), after("Two"))
    };
    assert_eq!(markers(""), ("1".to_owned(), "b".to_owned()));
    assert_eq!(
        markers("<w:numRestart w:val=\"eachSect\"/>"),
        ("1".to_owned(), "a".to_owned())
    );
}

/// A number at a first-line indent (here past the text indent) stays there,
/// and its tab takes the text on to the next stop, as Word lays it out: the
/// marker no longer moves left of the margin with the text glued to it.
#[test]
fn a_number_at_a_first_line_indent_is_followed_by_its_tab() {
    let doc = with_parts(
        "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>\
         <w:ind w:left=\"0\" w:firstLine=\"720\"/></w:pPr><w:r><w:t>Item</w:t></w:r></w:p>\
         <w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>\
         <w:ind w:left=\"0\" w:firstLine=\"0\"/></w:pPr>\
         <w:r><w:t>Other</w:t></w:r></w:p>",
        &numbering(&level(0, "decimal", "%1.", "")),
        "",
    );
    let pages = pages(&doc);
    // 1-inch margin, number half an inch in: 96 + 48 px.
    let number = text(&pages, "1.");
    assert!((number.x - 144.0).abs() < 0.01, "{}", number.x);
    // The next default stop after the number: one inch in, 192 px.
    let item = text(&pages, "Item");
    assert!((item.x - 192.0).abs() < 0.01, "{}", item.x);
    // With no first-line indent the number stands on the margin and the
    // text on the first stop after it, half an inch in.
    let second = text(&pages, "2.");
    assert!((second.x - 96.0).abs() < 0.01, "{}", second.x);
    let other = text(&pages, "Other");
    assert!((other.x - 144.0).abs() < 0.01, "{}", other.x);
}

/// `w:firstLineChars="0"` on a list item cancels a first-line indent in
/// characters from its style; it does not zero the level's hanging indent,
/// so the number still hangs left of the text.
#[test]
fn zero_first_line_characters_keep_the_levels_hanging_indent() {
    let styles = "<w:style w:type=\"paragraph\" w:styleId=\"List\"><w:name w:val=\"List\"/>\
        <w:pPr><w:ind w:firstLineChars=\"200\" w:firstLine=\"420\"/></w:pPr></w:style>";
    let doc = with_parts(
        "<w:p><w:pPr><w:pStyle w:val=\"List\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/>\
         </w:numPr><w:ind w:firstLineChars=\"0\"/></w:pPr><w:r><w:t>Item</w:t></w:r></w:p>",
        &numbering(&level(0, "decimal", "%1.", "")),
        styles,
    );
    let pages = pages(&doc);
    // The level: 1440 left, 720 hanging past the 1-inch margin.
    let number = text(&pages, "1.");
    assert!((number.x - 144.0).abs() < 0.01, "{}", number.x);
    let item = text(&pages, "Item");
    assert!((item.x - 192.0).abs() < 0.01, "{}", item.x);
}
