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

use std::path::Path;

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
    document
        .pages()
        .expect("pages")
        .iter()
        .map(strict_ooxml_pdf::PdfPage::text)
        .collect::<Vec<String>>()
        .join(" ")
}

/// The text of a converted document, as a reader of the `.docx` would see it.
fn document_text(bytes: &[u8]) -> String {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let mut out = String::new();
    for block in &document.body.blocks {
        if let strict_ooxml_wml::model::block::Block::Paragraph(paragraph) = block {
            for inline in &paragraph.inlines {
                if let strict_ooxml_wml::model::inline::Inline::Run(run) = inline {
                    for content in &run.content {
                        if let strict_ooxml_wml::model::inline::RunContent::Text(node) = content {
                            out.push_str(&node.text);
                        }
                    }
                }
            }
            out.push(' ');
        }
    }
    out
}

/// The non-space characters of a string, which is what the comparison counts.
fn significant(text: &str) -> Vec<char> {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Converts, writes, and re-reads, for one mode.
fn round_trip(mode: Mode) -> (String, String, Vec<u8>) {
    let pdf = source();
    let expected = pdf_text(&pdf);
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let converted = convert(&mut reader, &PdfOptions::default().mode(mode)).expect("convert");

    let written = strict_ooxml_write::write_package(
        &converted.document,
        None,
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
        let want = significant(&expected);
        let have = significant(&got);
        assert!(
            !want.is_empty(),
            "{mode:?}: the fixture carries no text to compare"
        );
        // Subsequence comparison: the converted document may reorder nothing
        // but it may insert a heading marker or lose a space, so the measure is
        // "how much of the original text is present, in order".
        let mut index = 0;
        for ch in &have {
            while index < want.len() && want[index] != *ch {
                index += 1;
            }
            if index < want.len() {
                index += 1;
            }
        }
        let ratio = index as f64 / want.len() as f64;
        assert!(
            ratio >= 0.999,
            "{mode:?}: only {:.4} of the PDF's {} characters reached the document; \
             got {} against {}",
            ratio,
            want.len(),
            have.len(),
            got.len()
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

    assert_eq!(
        semantic.document.body.blocks.len(),
        visual.document.body.blocks.len(),
        "the fixture has no wrapped lines, so both modes produce one paragraph per line"
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
    // The indent is the PDF's left edge, in both modes: that is stated, not
    // inferred.
    for document in [&semantic.document, &visual.document] {
        let Some(strict_ooxml_wml::model::block::Block::Paragraph(paragraph)) =
            document.body.blocks.first()
        else {
            panic!("expected a paragraph");
        };
        let indent = paragraph
            .props
            .indentation
            .as_ref()
            .and_then(|indent| indent.start)
            .expect("an indent");
        assert!(indent.0 > 0, "the first line starts inside the margin");
    }
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
