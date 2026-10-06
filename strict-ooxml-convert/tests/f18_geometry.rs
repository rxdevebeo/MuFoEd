//! A03 / F18: a PDF glyph's page position survives visual write and SVG render.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_svg::{render, RenderOptions};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn svg_of(pdf: &[u8]) -> String {
    let mut reader = PdfDocument::open(pdf, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Visual)).expect("convert");
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
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    render(&document, &RenderOptions::default())
        .expect("svg")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

fn text_xy(svg: &str, needle: &str) -> (f64, f64) {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let node = document
        .descendants()
        .find(|node| {
            node.is_element()
                && node.tag_name().name() == "text"
                && node.text().is_some_and(|text| text.contains(needle))
        })
        .expect(needle);
    // F07 emits a space-separated cluster `x` list; the first value is the
    // glyph origin this oracle measures. `y` stays a single baseline.
    let x = node
        .attribute("x")
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0);
    let y = node
        .attribute("y")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0);
    (x, y)
}

#[test]
fn f18_pdf_baseline_and_x_survive_write() {
    // 72 pt from the left and 92 pt from the top of a letter page.
    // PDF y grows upward, so the baseline is 792 - 92 = 700.
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    pdf.page_with(b"BT /F1 12 Tf 72 700 Td (Here) Tj ET", &resources);
    let svg = svg_of(&pdf.build());
    let (x, y) = text_xy(&svg, "Here");
    assert!((x - 96.0).abs() <= 0.25, "x {x} px, want 96");
    assert!((y - 122.667).abs() <= 0.25, "baseline {y} px, want 122.667");
}

#[test]
fn f18_three_text_positions() {
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    pdf.page_with(
        b"BT /F1 12 Tf 72 700 Td (A) Tj ET BT /F1 12 Tf 216 700 Td (B) Tj ET BT /F1 12 Tf 216 652 Td (C) Tj ET",
        &resources,
    );
    let svg = svg_of(&pdf.build());
    let (ax, ay) = text_xy(&svg, "A");
    let (bx, by) = text_xy(&svg, "B");
    let (cx, cy) = text_xy(&svg, "C");
    assert!(bx > ax + 10.0, "B is to the right of A: {ax} {bx}");
    assert!(cy > ay + 10.0, "C is below A: {ay} {cy}");
    assert!(bx > 100.0 && by > 50.0, "B is placed {bx},{by}");
    assert!(cx > 100.0, "C is placed {cx},{cy}");
}
