//! A02 / F19: a supported PDF rectangle is kept, and an unsupported path is named.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_svg::{place_pages, render, RenderOptions};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn page_pdf(content: &str) -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    let stream = format!("{content}\nBT /F1 12 Tf 72 700 Td (Here) Tj ET");
    pdf.page_with(stream.as_bytes(), &resources);
    pdf.build()
}

fn round_trip(mode: Mode, content: &str) -> (String, String, Vec<u8>) {
    let bytes = page_pdf(content);
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted = convert(&mut reader, &PdfOptions::default().mode(mode)).expect("convert");
    let report = converted.report.to_string();
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, media) in &converted.media {
        bag.insert(part.clone(), media.clone());
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
    let options = RenderOptions::default();
    let svg = render(&document, &options)
        .expect("svg")
        .into_iter()
        .next()
        .expect("page")
        .svg;
    let placed = place_pages(&document, &options, Some(&package)).expect("place");
    let pdf = strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("pdf");
    (svg, report, pdf.bytes)
}

fn red_box(svg: &str) -> (f64, f64, f64, f64) {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let node = document
        .descendants()
        .find(|node| {
            node.is_element()
                && matches!(node.tag_name().name(), "path" | "rect")
                && node
                    .attribute("fill")
                    .is_some_and(|fill| fill.eq_ignore_ascii_case("#ff0000"))
        })
        .expect("a red shape");
    if node.tag_name().name() == "rect" {
        let coord = |name: &str| {
            node.attribute(name)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.0)
        };
        return (coord("x"), coord("y"), coord("width"), coord("height"));
    }
    let d = node.attribute("d").unwrap_or("");
    let width = number_after(d, 'H').expect("path width");
    let height = number_after(d, 'V').expect("path height");
    let transform = node.attribute("transform").unwrap_or("");
    let (x, y) = translate(transform);
    (x, y, width, height)
}

fn number_after(text: &str, marker: char) -> Option<f64> {
    let rest = text.split(marker).nth(1)?;
    let token = rest.split_whitespace().next()?;
    token.parse().ok()
}

fn visible_text(svg: &str) -> String {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let mut text = String::new();
    for node in document.descendants() {
        if node.is_element() && node.tag_name().name() == "text" {
            text.push_str(node.text().unwrap_or(""));
        }
    }
    text
}

fn translate(transform: &str) -> (f64, f64) {
    let Some(start) = transform.find("translate(") else {
        return (0.0, 0.0);
    };
    let body = &transform[start + "translate(".len()..];
    let end = body.find(')').unwrap_or(body.len());
    let mut numbers = body[..end].split(|c: char| c == ',' || c.is_whitespace());
    let x = numbers
        .next()
        .and_then(|token| token.parse().ok())
        .unwrap_or(0.0);
    let y = numbers
        .next()
        .and_then(|token| token.parse().ok())
        .unwrap_or(0.0);
    (x, y)
}

fn pdf_red_box(bytes: &[u8]) -> (f64, f64, f64, f64) {
    let mut reader = PdfDocument::open(bytes, PdfLimits::default()).expect("open rendered pdf");
    let page = reader.page(1).expect("page");
    let height = page.geometry.height;
    for item in page.items() {
        let Item::Vector(vector) = item else {
            continue;
        };
        let Some(fill) = vector.fill else {
            continue;
        };
        if fill.to_rgb8() != [255, 0, 0] {
            continue;
        }
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for subpath in &vector.subpaths {
            for (x, y) in &subpath.points {
                let (x, y) = vector.ctm.apply(*x, *y);
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        return (min_x, height - max_y, max_x - min_x, max_y - min_y);
    }
    panic!("the rendered PDF has no red vector");
}

#[test]
fn f19_text_and_vector_is_not_lossless_when_vector_missing() {
    // A triangle is not a supported rectangle. Dropping it without a name
    // would be a silent loss, including on a page that still has readable text.
    for mode in [Mode::Visual, Mode::Semantic] {
        let (svg, report, _pdf) =
            round_trip(mode, "1 0 0 rg 100 100 m 160 100 l 130 150 l h f 0 0 0 rg");
        assert!(
            report.contains("vector.unsupported"),
            "{mode:?} names the triangle: {report}"
        );
        assert!(
            !report.contains("nothing inferred or lost"),
            "{mode:?} must not call the page lossless: {report}"
        );
        assert_eq!(visible_text(&svg), "Here", "{mode:?} keeps the text once");
    }
}

#[test]
fn f19_supported_rectangle_survives() {
    // 100 pt from the left, 500 pt up, 80 by 40 pt, on a 792 pt page.
    // SVG is 96 px per inch: x = 133.333, y from the top = 336, size 106.667 by 53.333.
    let expect_x = 100.0 / 72.0 * 96.0;
    let expect_y = (792.0 - 540.0) / 72.0 * 96.0;
    let expect_w = 80.0 / 72.0 * 96.0;
    let expect_h = 40.0 / 72.0 * 96.0;
    for mode in [Mode::Visual, Mode::Semantic] {
        let (svg, report, pdf) = round_trip(mode, "1 0 0 rg 100 500 80 40 re f 0 0 0 rg");
        assert!(
            !report.contains("vector.unsupported"),
            "{mode:?} must keep the rectangle, not only warn: {report}"
        );
        let (x, y, w, h) = red_box(&svg);
        assert!((x - expect_x).abs() <= 0.25, "{mode:?} x {x}");
        assert!((y - expect_y).abs() <= 0.25, "{mode:?} y {y}");
        assert!((w - expect_w).abs() <= 0.25, "{mode:?} w {w}");
        assert!((h - expect_h).abs() <= 0.25, "{mode:?} h {h}");
        assert_eq!(visible_text(&svg), "Here", "{mode:?} keeps the text once");
        let (px, py, pw, ph) = pdf_red_box(&pdf);
        assert!((px - 100.0).abs() <= 0.5, "{mode:?} pdf x {px}");
        assert!((py - 252.0).abs() <= 0.5, "{mode:?} pdf y {py}");
        assert!((pw - 80.0).abs() <= 0.5, "{mode:?} pdf w {pw}");
        assert!((ph - 40.0).abs() <= 0.5, "{mode:?} pdf h {ph}");
    }
}
