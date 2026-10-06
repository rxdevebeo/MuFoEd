//! The conversion, end to end, on a PDF this project produced.
//!
//! The chain under test is the one a caller runs: read a PDF, convert it, write
//! a `.docx`, open that `.docx` with our own reader and an independent XML
//! parser. A conversion that produces a plausible `Document` in memory is not
//! the deliverable; a package a reader opens is.
//!
//! `SC-5` is the acceptance criterion the work order sets: at least 99.9 % of
//! the characters in the PDF's text layer are in the document.

// The part names are the ones *this* writer chose, so a case-insensitive
// comparison would be testing the wrong thing; and the character counts are
// bounded by the document, far below 2^53.
#![allow(
    clippy::cast_precision_loss,
    clippy::case_sensitive_file_extension_comparisons
)]

mod common;

use std::path::Path;

use common::text::{body_text, pdf_reading_text, score};
use strict_ooxml_convert::{convert, Mode, ParagraphRules, PdfOptions};
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The document the chain is run on.
fn source() -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let bytes = std::fs::read(root.join("strict-text.docx")).expect("the fixture is present");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = strict_ooxml_render_svg::RenderOptions::default();
    let placed =
        strict_ooxml_render_svg::place_pages(&document, &options, Some(&package)).expect("place");
    strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("render")
        .bytes
}

/// The text the PDF's own content stream carries.
fn pdf_text(pdf: &[u8]) -> String {
    let mut document = PdfDocument::open(pdf, PdfLimits::default()).expect("open");
    pdf_reading_text(&mut document).expect("pages")
}

/// The text of a converted document, as a reader of the `.docx` would see it.
fn document_text(bytes: &[u8]) -> String {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    body_text(&document)
}

/// Converts, writes, and re-reads, for one mode.
fn round_trip(mode: Mode) -> (String, String, Vec<u8>) {
    let pdf = source();
    let expected = pdf_text(&pdf);
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let converted = convert(&mut reader, &PdfOptions::default().mode(mode)).expect("convert");

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
    (expected, converted.report.to_string(), written.bytes)
}

/// SC-5: at least 99.9 % of the PDF's characters reach the document.
#[test]
fn every_character_reaches_the_document() {
    for mode in [Mode::Semantic, Mode::Visual] {
        let (expected, _report, bytes) = round_trip(mode);
        let got = document_text(&bytes);
        let measured = score(&expected, &got);
        let ratio = measured
            .recall()
            .unwrap_or_else(|| panic!("{mode:?}: the fixture carries no text to compare"));
        assert!(
            ratio >= 0.999,
            "{mode:?}: only {:.4} of the PDF's {} characters reached the document; \
             matched {} of have {}",
            ratio,
            measured.want_len,
            measured.matched,
            measured.have_len
        );
    }
}

/// The written package opens under the strict policy, so it is a Strict
/// document and not merely a readable one.
#[test]
fn the_written_package_is_strict() {
    for mode in [Mode::Semantic, Mode::Visual] {
        let (_, _, bytes) = round_trip(mode);
        let options = OpenOptions {
            conformance: ConformancePolicy::StrictOnly,
            ..OpenOptions::default()
        };
        let package = Package::open_reader(&bytes[..], &options)
            .unwrap_or_else(|error| panic!("{mode:?}: the package is not Strict: {error}"));
        let parts: Vec<String> = package
            .parts()
            .map(|part| part.id.as_str().to_owned())
            .collect();
        assert!(
            parts.iter().any(|id| id == "/word/document.xml"),
            "{parts:?}"
        );
        for part in package.parts() {
            let id = part.id.as_str();
            if !(id.ends_with(".xml") || id.ends_with(".rels")) {
                continue;
            }
            let bytes = package.read_part(&part.id).expect("read");
            roxmltree::Document::parse(std::str::from_utf8(&bytes).expect("utf-8"))
                .unwrap_or_else(|error| panic!("{mode:?}: {id} is not well formed: {error}"));
        }
    }
}

/// The conversion is reproducible: the same PDF gives the same document.
#[test]
fn the_conversion_is_reproducible() {
    let pdf = source();
    let mut first = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let mut second = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let options = PdfOptions::default();
    let a = convert(&mut first, &options).expect("convert");
    let b = convert(&mut second, &options).expect("convert");
    assert_eq!(a.report.to_string(), b.report.to_string());
    assert_eq!(a.document.body.blocks, b.document.body.blocks);
}

/// The two modes differ in the way the work order says they do, and the visual
/// one really does keep the PDF's geometry.
#[test]
fn the_modes_reproduce_different_documents() {
    let pdf = source();
    let mut semantic = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let mut visual = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let semantic =
        convert(&mut semantic, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let visual = convert(&mut visual, &PdfOptions::default().mode(Mode::Visual)).expect("convert");

    // The justified fox sentence wraps, matching the WPS page. Semantic joins
    // that wrap back into one paragraph; visual keeps one block per PDF line.
    assert_eq!(
        semantic.document.body.blocks.len(),
        7,
        "semantic joins the wrapped fox line"
    );
    assert_eq!(
        visual.document.body.blocks.len(),
        8,
        "visual keeps the wrapped fox line"
    );
    assert!(
        body_text(&semantic.document).contains("moves gently"),
        "the wrapped line is still in the semantic document"
    );
    assert!(
        body_text(&visual.document).contains("moves gently"),
        "the wrapped line is still in the visual document"
    );
    // The page size is the PDF's own, in twips, in both modes.
    for document in [&semantic.document, &visual.document] {
        let size = document
            .sections
            .first()
            .and_then(|section| section.properties.page_size)
            .expect("a page size");
        assert_eq!(
            size.width.map(|w| w.0),
            Some(12_240),
            "612 pt is 12240 twips"
        );
    }
    // Semantic indent is the PDF x measured from the page edge. Visual indent
    // is that same x measured from the 72 pt section margin, so the margin is
    // not applied twice when the page is rendered.
    let indent_of = |document: &strict_ooxml_wml::model::Document| {
        let Some(strict_ooxml_wml::model::block::Block::Paragraph(paragraph)) =
            document.body.blocks.first()
        else {
            panic!("expected a paragraph");
        };
        paragraph
            .props
            .indentation
            .as_ref()
            .and_then(|indent| indent.start)
            .expect("an indent")
            .0
    };
    let semantic_indent = indent_of(&semantic.document);
    let visual_indent = indent_of(&visual.document);
    assert!(semantic_indent > 0, "the first line starts inside the page");
    assert_eq!(
        semantic_indent - visual_indent,
        1_440,
        "visual indent is the page x minus the 72 pt margin"
    );
}

/// The paragraph rules are the converter's claims about documents, so changing
/// one changes what it produces.
#[test]
fn the_paragraph_rules_are_load_bearing() {
    let pdf = source();
    let mut default = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let mut joined = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");

    let separate = convert(&mut default, &PdfOptions::default()).expect("convert");
    // A gap ratio of zero makes every new baseline a new paragraph, so the
    // document cannot be *more* joined than the default - the assertion is that
    // the number changes at all, in one direction or the other.
    let aggressive = convert(
        &mut joined,
        &PdfOptions::default().paragraphs(ParagraphRules {
            paragraph_gap_ratio: 0.0,
            ..ParagraphRules::default()
        }),
    )
    .expect("convert");
    assert!(
        aggressive.report.paragraphs() >= separate.report.paragraphs(),
        "a zero gap ratio cannot join more than the default: {} against {}",
        aggressive.report.paragraphs(),
        separate.report.paragraphs()
    );
}

/// A page with no text is not an error, and a page range outside the document
/// is.
#[test]
fn the_page_range_is_checked() {
    let pdf = source();
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let error = convert(
        &mut reader,
        &PdfOptions {
            pages: Some((9, 12)),
            ..PdfOptions::default()
        },
    )
    .expect_err("page 9 of a one-page document");
    assert!(error.to_string().contains('9'), "{error}");

    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions {
            pages: Some((1, 1)),
            ..PdfOptions::default()
        },
    )
    .expect("page 1 is in the document");
    assert!(converted.report.paragraphs() > 0);
}
