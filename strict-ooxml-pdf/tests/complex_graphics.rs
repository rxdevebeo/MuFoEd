//! Independent PDF pixels for native gradient and pattern fills.
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::io::Cursor;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::layout::Item;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_testkit::graphics::{fill_docx, pattern, GRADIENT};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn pixels(fill: &str) -> (Vec<u8>, usize) {
    let package = Package::open_reader(
        Cursor::new(fill_docx("ellipse", fill)),
        &OpenOptions::default(),
    )
    .expect("open");
    let doc = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let placed = place_pages(&doc, &options, Some(&package)).expect("place");
    let shape = placed[0]
        .items
        .iter()
        .find_map(|item| {
            if let Item::Path(path) = item {
                Some(path)
            } else {
                None
            }
        })
        .expect("ellipse");
    let pdf = strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("PDF");
    let independent =
        strict_ooxml_pdf::Document::open(&pdf.bytes, strict_ooxml_pdf::PdfLimits::default())
            .expect("independent reader");
    let raster = independent.rasterizer().expect("rasterizer");
    let png = raster
        .region_png(
            1,
            strict_ooxml_pdf::raster::Region {
                x: shape.x * 0.75,
                y: shape.y * 0.75,
                width: shape.w * 0.75,
                height: shape.h * 0.75,
            },
            &strict_ooxml_pdf::raster::RasterOptions {
                scale: 4.0 / 3.0,
                ..Default::default()
            },
        )
        .expect("pixels");
    let mut reader = png::Decoder::new(Cursor::new(png))
        .read_info()
        .expect("PNG");
    let mut bytes = vec![0; reader.output_buffer_size().expect("size")];
    let info = reader.next_frame(&mut bytes).expect("frame");
    assert_eq!(info.width, 192);
    assert_eq!(info.height, 96);
    assert_eq!(info.color_type, png::ColorType::Rgba);
    bytes.truncate(info.buffer_size());
    (bytes, info.width as usize)
}

#[test]
fn independent_pdf_rasterizer_sees_gradient_colours_and_ellipse_clipping() {
    let (bytes, width) = pixels(GRADIENT);
    let at = |x, y| &bytes[(y * width + x) * 4..(y * width + x) * 4 + 4];
    assert!(at(8, 48)[0] > 200 && at(8, 48)[2] < 20);
    assert!(at(96, 48)[1] > 230 && at(96, 48)[0] < 20);
    assert!(at(184, 48)[2] > 200 && at(184, 48)[0] < 20);
    assert!(at(1, 1)[0] > 250 && at(1, 1)[1] > 250 && at(1, 1)[2] > 250);
}

#[test]
fn independent_pdf_rasterizer_sees_both_pattern_colours() {
    let (bytes, width) = pixels(&pattern("cross"));
    let mut red_coverage = 0_u32;
    let mut blue_pixels = 0;
    for y in 32..64 {
        for x in 64..96 {
            let offset = (y * width + x) * 4;
            red_coverage += u32::from(bytes[offset]);
            blue_pixels += u32::from(bytes[offset + 2] > 180 && bytes[offset] < 80);
        }
    }
    assert!(red_coverage > 100 * 255 && blue_pixels > 200);
}

#[test]
fn reader_retains_axial_stops_and_the_shape_clip_instead_of_dropping_shading() {
    let package = Package::open_reader(
        Cursor::new(fill_docx("ellipse", GRADIENT)),
        &OpenOptions::default(),
    )
    .expect("open");
    let doc = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let placed = place_pages(&doc, &options, Some(&package)).expect("place");
    let pdf = strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("PDF");
    let mut reader =
        strict_ooxml_pdf::Document::open(&pdf.bytes, strict_ooxml_pdf::PdfLimits::default())
            .expect("read");
    let page = reader.page(1).expect("page");
    let vector = page
        .items()
        .iter()
        .find_map(|item| match item {
            strict_ooxml_pdf::content::Item::Vector(vector) if vector.gradient.is_some() => {
                Some(vector)
            }
            _ => None,
        })
        .expect("gradient vector");
    let gradient = vector.gradient.as_ref().expect("fill");
    assert_eq!(
        gradient
            .stops
            .iter()
            .map(|stop| stop.1.to_rgb8())
            .collect::<Vec<_>>(),
        [[255, 0, 0], [0, 255, 0], [0, 0, 255]]
    );
    assert_eq!(vector.subpaths.len(), 1);
    assert!(
        vector.subpaths[0].points.len() > 8,
        "ellipse clip must survive as a contour"
    );
    assert!(page
        .report
        .losses()
        .iter()
        .all(|loss| loss.id != "pdf.shading"));
}
