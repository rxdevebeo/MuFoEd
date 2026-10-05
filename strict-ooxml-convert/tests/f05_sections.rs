//! A01 / F05: a converted PDF keeps every page's section geometry after write.

mod common;

use common::text::body_text;
use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_testkit::audit::two_page_mixed_pdf;
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::props::Section;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

const LETTER: (i32, i32) = (12_240, 15_840);
const LANDSCAPE: (i32, i32) = (16_840, 11_900);

/// Converts, writes, and re-reads. The assertions run on the package, not on
/// the converter's in-memory section list.
fn round_trip(pdf: &[u8], mode: Mode, pages: Option<(usize, usize)>) -> (Document, String) {
    let mut reader = PdfDocument::open(pdf, PdfLimits::default()).expect("open pdf");
    let mut options = PdfOptions::default().mode(mode);
    options.pages = pages;
    let converted = convert(&mut reader, &options).expect("convert");
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written = strict_ooxml_write::write_package(
        &converted.document,
        Some(&bag),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .expect("write");
    let package =
        Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");
    let xml = String::from_utf8(
        package
            .read_part(&PartId::new("/word/document.xml"))
            .expect("document.xml"),
    )
    .expect("utf-8");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    (document, xml)
}

fn page_size(section: &Section) -> (i32, i32) {
    let size = section.properties.page_size.expect("pgSz");
    (
        size.width.expect("w").value(),
        size.height.expect("h").value(),
    )
}

fn sizes(document: &Document) -> Vec<(i32, i32)> {
    document.sections.iter().map(page_size).collect()
}

/// Paragraph `w:sectPr` versus the one direct child of `w:body`.
fn sect_pr_counts(xml: &str) -> (usize, usize) {
    let document = roxmltree::Document::parse(xml).expect("xml");
    let mut paragraph = 0usize;
    let mut body = 0usize;
    for node in document.descendants() {
        if node.tag_name().name() != "sectPr" {
            continue;
        }
        match node.parent().map(|parent| parent.tag_name().name()) {
            Some("body") => body += 1,
            Some(_) => paragraph += 1,
            None => {}
        }
    }
    (paragraph, body)
}

fn assert_breaks_match_sections(document: &Document, xml: &str) {
    let (paragraph, body) = sect_pr_counts(xml);
    assert_eq!(body, 1, "the final section is the body sectPr, once: {xml}");
    assert_eq!(
        paragraph,
        document.sections.len() - 1,
        "intermediate sections live on paragraphs and the last one is not repeated: {xml}"
    );
}

fn rendered_page_count(document: &Document) -> usize {
    let options = RenderOptions::default();
    let placed = place_pages(document, &options, None).expect("place");
    let pdf = strict_ooxml_render_pdf::render(&placed, &options).expect("pdf");
    let mut opened = PdfDocument::open(&pdf.bytes, PdfLimits::default()).expect("open rendered");
    let pages = opened.pages().expect("rendered pages");
    assert_eq!(pages.len(), placed.len(), "svg and pdf page counts differ");
    for (index, (svg, pdf_page)) in placed.iter().zip(pages.iter()).enumerate() {
        let width_pt = svg.width_px * 72.0 / 96.0;
        let height_pt = svg.height_px * 72.0 / 96.0;
        assert!(
            (pdf_page.geometry.width - width_pt).abs() < 0.05,
            "page {index} width svg {width_pt} pdf {}",
            pdf_page.geometry.width
        );
        assert!(
            (pdf_page.geometry.height - height_pt).abs() < 0.05,
            "page {index} height svg {height_pt} pdf {}",
            pdf_page.geometry.height
        );
    }
    placed.len()
}

fn text_page(pdf: &mut PdfBuilder, font: u32, content: &str) {
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    pdf.page_with(content.as_bytes(), &resources);
}

fn font(pdf: &mut PdfBuilder) -> u32 {
    pdf.object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
}

/// Two pages, 612×792 then 842×595 pt, survive write and render as two pages.
#[test]
fn f05_visual_preserves_two_page_sections() {
    let (document, xml) = round_trip(&two_page_mixed_pdf(), Mode::Visual, None);
    assert_eq!(sizes(&document), vec![LETTER, LANDSCAPE]);
    assert_breaks_match_sections(&document, &xml);
    assert!(
        xml.contains("w:val=\"nextPage\""),
        "the intermediate break is nextPage: {xml}"
    );
    assert_eq!(rendered_page_count(&document), 2);
    let text = body_text(&document);
    let first = text.find("Page one").expect("page one text");
    let second = text.find("Page two").expect("page two text");
    assert!(first < second, "text order: {text}");
}

/// Three distinct page boxes stay in order.
#[test]
fn f05_three_page_sizes_survive_the_round_trip() {
    let mut pdf = PdfBuilder::new();
    let face = font(&mut pdf);
    pdf.set_media_box([0, 0, 200, 300]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 36 200 Td (Alpha) Tj ET");
    pdf.set_media_box([0, 0, 400, 500]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 36 400 Td (Beta) Tj ET");
    pdf.set_media_box([0, 0, 500, 200]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 36 100 Td (Gamma) Tj ET");
    let (document, xml) = round_trip(&pdf.build(), Mode::Visual, None);
    assert_eq!(
        sizes(&document),
        vec![(4_000, 6_000), (8_000, 10_000), (10_000, 4_000)]
    );
    assert_breaks_match_sections(&document, &xml);
    assert_eq!(rendered_page_count(&document), 3);
    let text = body_text(&document);
    assert!(text.find("Alpha") < text.find("Beta") && text.find("Beta") < text.find("Gamma"));
}

/// A blank middle page keeps its own box and does not add a fourth page.
#[test]
fn f05_empty_middle_page_keeps_its_size() {
    let mut pdf = PdfBuilder::new();
    let face = font(&mut pdf);
    pdf.set_media_box([0, 0, 612, 792]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 700 Td (Before) Tj ET");
    pdf.set_media_box([0, 0, 400, 400]);
    pdf.page(b"");
    pdf.set_media_box([0, 0, 842, 595]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 500 Td (After) Tj ET");
    let (document, xml) = round_trip(&pdf.build(), Mode::Visual, None);
    assert_eq!(sizes(&document), vec![LETTER, (8_000, 8_000), LANDSCAPE]);
    assert_breaks_match_sections(&document, &xml);
    assert_eq!(rendered_page_count(&document), 3);
    let text = body_text(&document);
    assert!(text.find("Before") < text.find("After"), "{text}");
}

/// A blank last page is only the body `sectPr` and still renders.
#[test]
fn f05_empty_last_page_is_a_body_section() {
    let mut pdf = PdfBuilder::new();
    let face = font(&mut pdf);
    pdf.set_media_box([0, 0, 612, 792]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 700 Td (Only) Tj ET");
    pdf.set_media_box([0, 0, 300, 300]);
    pdf.page(b"");
    let (document, xml) = round_trip(&pdf.build(), Mode::Visual, None);
    assert_eq!(sizes(&document), vec![LETTER, (6_000, 6_000)]);
    assert_breaks_match_sections(&document, &xml);
    assert_eq!(rendered_page_count(&document), 2);
    let placed = place_pages(&document, &RenderOptions::default(), None).expect("place");
    let last = placed.last().expect("last page");
    assert!((last.width_px - 400.0).abs() < 0.05, "{}", last.width_px);
    assert!((last.height_px - 400.0).abs() < 0.05, "{}", last.height_px);
}

/// A page range does not import the size of a page that was not converted.
#[test]
fn f05_page_range_drops_the_unselected_size() {
    let mut pdf = PdfBuilder::new();
    let face = font(&mut pdf);
    pdf.set_media_box([0, 0, 200, 200]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 36 100 Td (One) Tj ET");
    pdf.set_media_box([0, 0, 300, 400]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 36 300 Td (Two) Tj ET");
    pdf.set_media_box([0, 0, 500, 600]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 36 500 Td (Three) Tj ET");
    let (document, xml) = round_trip(&pdf.build(), Mode::Visual, Some((2, 3)));
    assert_eq!(sizes(&document), vec![(6_000, 8_000), (10_000, 12_000)]);
    assert_breaks_match_sections(&document, &xml);
    assert_eq!(rendered_page_count(&document), 2);
    let text = body_text(&document);
    assert!(!text.contains("One"), "{text}");
    assert!(text.contains("Two") && text.contains("Three"), "{text}");
}

/// Visual does not merge equal neighbours. Semantic keeps the later size change.
#[test]
fn f05_identical_neighbours_break_on_a_size_change() {
    let mut pdf = PdfBuilder::new();
    let face = font(&mut pdf);
    pdf.set_media_box([0, 0, 612, 792]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 700 Td (Same) Tj ET");
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 700 Td (Same) Tj ET");
    pdf.set_media_box([0, 0, 842, 595]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 500 Td (Changed) Tj ET");
    let bytes = pdf.build();

    let (visual, visual_xml) = round_trip(&bytes, Mode::Visual, None);
    assert_eq!(sizes(&visual), vec![LETTER, LETTER, LANDSCAPE]);
    assert_breaks_match_sections(&visual, &visual_xml);
    assert_eq!(rendered_page_count(&visual), 3);

    let (semantic, semantic_xml) = round_trip(&bytes, Mode::Semantic, None);
    let semantic_sizes = sizes(&semantic);
    assert_eq!(semantic_sizes.last().copied(), Some(LANDSCAPE));
    assert!(
        semantic_sizes.contains(&LETTER),
        "the earlier pages keep the letter box: {semantic_sizes:?}"
    );
    assert_breaks_match_sections(&semantic, &semantic_xml);
    let text = body_text(&semantic);
    let same = text.find("Same").expect("first same");
    let changed = text.find("Changed").expect("changed");
    assert!(same < changed, "{text}");
    assert_eq!(text.matches("Same").count(), 2, "{text}");
}

/// The last page is a table, so the body `sectPr` follows it with no extra paragraph.
#[test]
fn f05_last_page_table_does_not_grow_a_paragraph() {
    let mut pdf = PdfBuilder::new();
    let face = font(&mut pdf);
    pdf.set_media_box([0, 0, 612, 792]);
    text_page(&mut pdf, face, "BT /F1 12 Tf 72 700 Td (Before) Tj ET");
    // 2×2 grid in PDF user space. Top-left rows are y=100/130/160 on a 792 pt page.
    let grid = "0 0 0 RG 0.5 w \
72 692 m 330 692 l S 72 662 m 330 662 l S 72 632 m 330 632 l S \
72 632 m 72 692 l S 200 632 m 200 692 l S 330 632 m 330 692 l S \
BT /F1 12 Tf 76 674 Td (A) Tj 128 0 Td (B) Tj -128 -30 Td (C) Tj 128 0 Td (D) Tj ET";
    text_page(&mut pdf, face, grid);
    let (document, xml) = round_trip(&pdf.build(), Mode::Semantic, None);
    assert!(
        matches!(document.body.blocks.last(), Some(Block::Table(_))),
        "the table stays the last block: {:?}",
        document
            .body
            .blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(_) => "p",
                Block::Table(_) => "tbl",
                Block::SdtBlock(_) => "sdt",
                Block::AltChunk(_) => "alt",
                Block::Opaque(_) => "opaque",
            })
            .collect::<Vec<_>>()
    );
    assert_breaks_match_sections(&document, &xml);
    let text = body_text(&document);
    assert!(text.contains("Before"), "{text}");
    assert!(text.contains('A') && text.contains('D'), "{text}");
}

/// Semantic mode records each page's box on the section that owns its text.
#[test]
fn f05_semantic_blocks_keep_their_page_size() {
    let (document, xml) = round_trip(&two_page_mixed_pdf(), Mode::Semantic, None);
    assert_eq!(sizes(&document), vec![LETTER, LANDSCAPE]);
    assert_breaks_match_sections(&document, &xml);
    let text = body_text(&document);
    assert!(text.find("Page one") < text.find("Page two"), "{text}");
}
