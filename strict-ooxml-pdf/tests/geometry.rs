//! The geometry oracle: does the placement survive into the PDF and back?
//!
//! # Why geometry rather than pixels
//!
//! The accuracy gate compares pages against WPS references by SSIM, and it is
//! blocked for the `formulas` class because the reference is set in **Cambria
//! Math** while this project draws with **STIX Two Math**. A pixel comparison
//! measures two things at once — where the layout put the glyphs, and what the
//! glyphs look like — and the second is a substitution nobody is going to
//! reverse, so the measurement is dominated by a difference that cannot be fixed
//! and the part that can be fixed is invisible inside it.
//!
//! A PDF separates the two. Every run is written as
//!
//! ```text
//! BT /F1 <size> Tf 1 0 0 1 <x> <y> Tm <glyph ids> Tj ET
//! ```
//!
//! so the run origin and size are exact numbers in the file, and every advance
//! is declared in `/W`. Nothing about the outline of a `p` takes part.
//!
//! # Runs versus glyphs
//!
//! The reader reports one item per *character* — that is what the content stream
//! contains — while the layout reports one item per *run*. The two are compared
//! by grouping the glyphs back into runs: a run is a set of glyphs on one
//! baseline whose x positions are contiguous. Anything that shifts a run by a
//! scale factor, drops a glyph or mis-scales an advance shows up as a
//! disagreement here, and nothing about glyph outlines can hide it.
//!
//! # Which reader
//!
//! `strict-ooxml-pdf`, on `lopdf` for the object model, the filters and the
//! content-stream tokenizer. It is not the writer: that is `pdf-writer`, and the
//! placement is `strict-ooxml-render-svg`. Comparing our output against our own
//! numbers would prove nothing, so the assertions below pin the recovered
//! geometry against the *layout's* numbers.
//!
//! This file replaces a hand-rolled extractor that lived in
//! `strict-ooxml-render-pdf/tests/glyph_geometry.rs` and re-implemented a
//! content-stream tokenizer to do it. The four things that tokenizer found in
//! foreign files are kept below as reader checks, because they are properties of
//! the reader rather than of a parser that no longer exists.

use std::path::Path;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_pdf::{content::Item, Glyph, PdfDocument, PdfLimits, PdfPage};
use strict_ooxml_render_svg::{place_pages, PlacedPage, RenderOptions, TextItem};
use strict_ooxml_wml::{parse_document, ParseOptions};

mod common;

/// The Strict documents the oracle runs on.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    for (name, file) in [
        ("strict-text", "strict-text.docx"),
        ("strict-profile", "strict-profile.docx"),
        ("strict-math", "05-strict-math-simple.docx"),
        ("strict-shapes", "07-strict-drawingml-shapes.docx"),
    ] {
        if let Ok(bytes) = std::fs::read(root.join(file)) {
            out.push((name, bytes));
        }
    }
    assert!(!out.is_empty(), "no Strict corpus under {}", root.display());
    out
}

/// Points per pixel at the default scale.
const PT_PER_PX: f64 = 72.0 / 96.0;

fn to_pt(px: f64) -> f64 {
    px * PT_PER_PX
}

/// Lays a document out, writes it as a PDF and lays the PDF back out.
fn round_trip(bytes: &[u8]) -> (Vec<PlacedPage>, Vec<PdfPage>) {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let placed = place_pages(&document, &options, Some(&package)).expect("place");
    let pdf = strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("render")
        .bytes;
    let read_back = PdfDocument::open(&pdf, PdfLimits::default())
        .unwrap_or_else(|error| panic!("the PDF we wrote does not read back: {error}"))
        .pages()
        .expect("pages");
    (placed, read_back)
}

fn glyphs(page: &PdfPage) -> Vec<&Glyph> {
    page.items()
        .iter()
        .filter_map(|item| match item {
            Item::Glyph(glyph) => Some(glyph),
            _ => None,
        })
        .collect()
}

fn text_items(page: &PlacedPage) -> Vec<&TextItem> {
    page.items
        .iter()
        .filter_map(|item| match item {
            strict_ooxml_render_svg::Item::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// One baseline recovered from the PDF.
#[derive(Clone, Debug)]
struct RecoveredLine {
    /// Left edge of the first glyph.
    x: f64,
    /// Baseline, measured from the top.
    y: f64,
    /// Font size.
    size: f64,
    /// Ascent above the baseline.
    ascent: f64,
    /// The text, in reading order.
    text: String,
    /// Distance from the left edge to the end of the last glyph.
    width: f64,
}

/// Groups per-glyph items by baseline.
///
/// A run is a set of glyphs on one baseline, each starting exactly where the
/// previous one's advance ended. Whether the file wrote one `Tj` or three is
/// not visible from the result and does not matter: the numbers that have to
/// agree are the line's origin, size, text and total advance.
fn group_lines(glyphs: &[&Glyph]) -> Vec<RecoveredLine> {
    let mut lines: Vec<RecoveredLine> = Vec::new();
    for glyph in glyphs {
        match lines.last_mut() {
            Some(line) if (line.y - glyph.y).abs() < 0.01 => {
                line.width = glyph.x + glyph.width - line.x;
                line.text.push_str(&glyph.text);
            }
            _ => lines.push(RecoveredLine {
                x: glyph.x,
                y: glyph.y,
                size: glyph.size,
                ascent: glyph.ascent,
                text: glyph.text.clone(),
                width: glyph.width,
            }),
        }
    }
    lines
}

/// The layout's text items grouped by baseline, with the same measure.
fn layout_lines(page: &PlacedPage) -> Vec<(f64, f64, f64, f64, String)> {
    let to_pt_px = 72.0 / 96.0;
    let mut out: Vec<(f64, f64, f64, f64, String)> = Vec::new();
    for item in text_items(page) {
        // Both sides measure the baseline from the top of the page, so the two
        // numbers are directly comparable; reflecting here as well was one flip
        // too many.
        let y = to_pt_px * item.baseline;
        match out.last_mut() {
            Some(line) if (line.1 - y).abs() < 0.01 => {
                line.3 = to_pt_px * (item.x + item.width) - line.0;
                line.4.push_str(&item.text);
            }
            _ => out.push((
                to_pt_px * item.x,
                y,
                to_pt_px * item.size_px,
                to_pt_px * item.width,
                item.text.clone(),
            )),
        }
    }
    out
}

/// The core claim: a run's origin, size, text and width come back out of the
/// PDF as the numbers the layout put in.
#[test]
fn glyph_positions_and_advances_survive_into_the_pdf() {
    let mut lines_seen = 0usize;
    for (name, bytes) in corpus() {
        let (placed, read_back) = round_trip(&bytes);
        assert_eq!(
            placed.len(),
            read_back.len(),
            "{name}: the page count moved"
        );
        let page = read_back.first().expect("a page");
        let recovered = group_lines(&glyphs(page));
        lines_seen += recovered.len();

        // The page height, which the y conversion depends on.
        let page_height_pt = to_pt(placed[0].height_px);
        assert!(
            (page.geometry.height - page_height_pt).abs() < 0.01,
            "{name}: the page height came back as {} pt against the layout's {} pt",
            page.geometry.height,
            page_height_pt
        );

        // A page whose layout carries no text needs no glyphs: `strict-profile`
        // opens with a chart, which the writer does not carry, and demanding
        // glyphs there would be demanding a defect.
        let expected = layout_lines(&placed[0]);
        if expected.is_empty() {
            assert!(
                recovered.is_empty(),
                "{name}: page 1 has no text in the layout but {} line(s) in the PDF",
                recovered.len()
            );
            continue;
        }
        assert!(
            !recovered.is_empty(),
            "{name}: the PDF exposes no glyph positions at all - a PDF without them \
             cannot be compared to anything"
        );
        let compared = recovered.len().min(expected.len());
        for index in 0..compared {
            let got = &recovered[index];
            let (x, y, size, width, text) = &expected[index];
            assert!(
                (got.x - x).abs() < 0.01,
                "{name}: line {index} starts at {} pt, the layout put it at {x} pt",
                got.x
            );
            assert!(
                (got.y - y).abs() < 0.01,
                "{name}: line {index} has its baseline {} pt from the top, the layout {} pt",
                got.y,
                y
            );
            assert!(
                (got.size - size).abs() < 0.01,
                "{name}: line {index} is {} pt, the layout said {size} pt",
                got.size
            );
            assert_eq!(
                got.text.trim_end(),
                text.trim_end(),
                "{name}: line {index} does not come back"
            );
            assert!(
                got.ascent > 0.0,
                "{name}: no vertical metric was recovered, so the descriptor was not read"
            );
            // The total advance, which is what a converter positioning the text
            // needs and what a mis-scaled `/W` destroys.
            let tolerance = width.abs() * 0.02 + 0.05;
            assert!(
                (got.width - width).abs() <= tolerance,
                "{name}: line {index} is {} pt wide in the PDF and {width} pt wide in \
                 the layout",
                got.width
            );
        }
    }
    assert!(lines_seen > 20, "only {lines_seen} lines were recovered");
}

/// Every run of every page must be found: a page that silently loses one is the
/// failure this harness exists to catch.
#[test]
fn every_text_run_of_every_page_is_recovered() {
    for (name, bytes) in corpus() {
        let (placed, read_back) = round_trip(&bytes);
        for (index, page) in read_back.iter().enumerate() {
            let expected = text_items(&placed[index]);
            let expected_chars: usize = expected.iter().map(|item| item.text.chars().count()).sum();
            let got_chars: usize = glyphs(page)
                .iter()
                .map(|glyph| glyph.text.chars().count())
                .sum();
            assert!(
                got_chars >= expected_chars,
                "{name}: page {} lost text: {} glyph(s) in the PDF against {} in the layout",
                index + 1,
                got_chars,
                expected_chars
            );
        }
    }
}

/// The reader agrees with itself: the same bytes read twice give the same
/// numbers, or a comparison built on it measures noise.
#[test]
fn the_reader_agrees_with_itself() {
    for (name, bytes) in corpus() {
        let (_, first) = round_trip(&bytes);
        let (_, second) = round_trip(&bytes);
        for (left, right) in first.iter().zip(second.iter()) {
            let left_glyphs = glyphs(left);
            let right_glyphs = glyphs(right);
            assert_eq!(left_glyphs.len(), right_glyphs.len(), "{name}");
            for (a, b) in left_glyphs.iter().zip(right_glyphs.iter()) {
                assert_eq!(a.text, b.text, "{name}");
                assert!((a.x - b.x).abs() < 1e-9, "{name}: {} vs {}", a.x, b.x);
                assert!((a.y - b.y).abs() < 1e-9, "{name}: {} vs {}", a.y, b.y);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The four things a hand-rolled tokenizer found in PDFs this project never
// writes. They are reader properties, and they are kept as reader checks.
// ---------------------------------------------------------------------------

/// A PDF whose glyph strings are hexadecimal rather than escaped literals.
///
/// A reader that does not decode them sees the digits themselves: four "glyphs"
/// in no `ToUnicode` map, four advances of a default width. `lopdf` decodes the
/// bytes; the fixture pins that, because a reader that stopped decoding would
/// look fine on every PDF this project writes.
#[test]
fn a_hexadecimal_glyph_string_decodes() {
    // A *simple* font's codes are one byte, so the hex form is two digits.
    let simple = common::pdf_with(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nBT /F1 12 Tf 1 0 0 1 20 50 Tm <4142> Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 66 /Widths [600 600] >>",
    ]);
    let mut pdf = PdfDocument::open(&simple, PdfLimits::default()).expect("open");
    let page = pdf.page(1).expect("page");
    let runs = glyphs(&page);
    assert_eq!(runs.len(), 2, "a hex string must decode to two glyphs");
    assert_eq!(runs[0].text, "A");
    assert_eq!(runs[1].text, "B");
}

/// A `cm` transform wrapping the whole page, and one nested inside it.
///
/// A reader that ignores `cm` reports text-space numbers as page coordinates —
/// wrong by the scale factor, and indistinguishable from a layout defect.
#[test]
fn a_page_wide_transform_is_applied() {
    let bytes = common::pdf_with(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nq 2 0 0 2 0 0 cm 1 0 0 1 0 0 cm BT /F1 10 Tf 1 0 0 1 10 10 Tm (A) Tj ET Q\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 65 /Widths [600] >>",
    ]);
    let mut pdf = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let page = pdf.page(1).expect("page");
    let runs = glyphs(&page);
    assert_eq!(runs.len(), 1);
    // The glyph sits at (10, 10) in text space on a page scaled by two.
    assert!((runs[0].x - 20.0).abs() < 0.01, "{}", runs[0].x);
    assert!((runs[0].size - 20.0).abs() < 0.01, "{}", runs[0].size);
    // y is measured from the top here, so a baseline 20 pt up on a 400 pt page
    // is 380 pt from the top.
    assert!((runs[0].y - 380.0).abs() < 0.01, "{}", runs[0].y);
}

/// A `<<...>>` dictionary inline in the content stream, from marked content.
#[test]
fn an_inline_dictionary_does_not_stop_the_walk() {
    let bytes = common::pdf_with(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nBT /F1 12 Tf 1 0 0 1 20 50 Tm << /MCID 0 >> BDC (A) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 65 /Widths [600] >>",
    ]);
    let mut pdf = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let page = pdf.page(1).expect("page");
    let runs = glyphs(&page);
    assert_eq!(runs.len(), 1, "the text after an inline dictionary is lost");
    assert_eq!(runs[0].text, "A");
}

/// A CID font that states `/DW`, and one that does not.
///
/// The one-em fallback is the specification's suggestion, not a number the
/// document committed to, so the reader has to say the advance is a guess: a
/// "plausible number, and the wrong one" is worse than a flag.
#[test]
fn an_absent_dw_is_an_estimate_not_a_declaration() {
    let objects = |cid: &str| {
        vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_owned(),
            "<< /Length 0 >>\nstream\nBT /F1 12 Tf 1 0 0 1 20 50 Tm <0001> Tj ET\nendstream".to_owned(),
            "<< /Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".to_owned(),
            cid.to_owned(),
            "<< /Length 0 >>\nstream\n/CMapName /Test def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfchar <0001> <0041> endbfchar endcmap\nendstream".to_owned(),
        ]
    };
    let declared_pdf = common::pdf_from(&objects(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Test /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 500 >>",
    ));
    let guessed_pdf = common::pdf_from(&objects(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Test /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> >>",
    ));

    let mut declared = PdfDocument::open(&declared_pdf, PdfLimits::default()).expect("open");
    let _ = declared.page(1).expect("page");
    assert_eq!(
        declared.report().estimated_widths(),
        0,
        "a font that states /DW must not produce an estimated advance"
    );

    let mut guessed = PdfDocument::open(&guessed_pdf, PdfLimits::default()).expect("open");
    let _ = guessed.page(1).expect("page");
    assert_eq!(
        guessed.report().estimated_widths(),
        1,
        "a font with no /DW must report the advance as estimated"
    );
}

/// A PDF from another writer, if one is pointed at.
///
/// The four checks above came from feeding a reader a Chromium export and seeing
/// which constructs a hand-rolled tokenizer had to special-case. The check stays:
/// the reader is the thing under test, and a foreign document is the only input
/// that is not ours.
#[test]
fn a_foreign_pdf_is_readable() {
    let Ok(path) = std::env::var("STRICT_GEOMETRY_PROBE_PDF") else {
        eprintln!("set STRICT_GEOMETRY_PROBE_PDF to run the foreign-PDF check");
        return;
    };
    let bytes = std::fs::read(&path).expect("the probe file is readable");
    let mut pdf = PdfDocument::open(&bytes, PdfLimits::default())
        .unwrap_or_else(|error| panic!("{path}: {error}"));
    let pages = pdf.pages().expect("pages");
    let total: usize = pages.iter().map(|page| glyphs(page).len()).sum();
    println!("{path}: {} page(s), {total} glyph(s)", pages.len());
    for loss in pdf.report().losses() {
        println!("  {loss}");
    }
    assert!(
        total > 0,
        "{path}: a PDF with no readable glyphs cannot be compared to anything"
    );
}
