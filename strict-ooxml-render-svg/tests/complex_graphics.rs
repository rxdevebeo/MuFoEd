//! Colour and contour oracles over self-contained DOCX inputs.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown
)]
use std::io::Cursor;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::layout::Item;
use strict_ooxml_render_svg::{place_pages, render_with_media, RenderOptions};
use strict_ooxml_testkit::graphics::{contour_docx, fill_docx, pattern, GRADIENT};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn open(bytes: Vec<u8>) -> (Package, strict_ooxml_wml::model::Document) {
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    (package, document)
}

fn raster_fill(geometry: &str, fill: &str) -> (resvg::tiny_skia::Pixmap, u32, u32) {
    let (package, document) = open(fill_docx(geometry, fill));
    let options = RenderOptions::default();
    let placed = place_pages(&document, &options, Some(&package)).expect("place");
    let path = placed[0]
        .items
        .iter()
        .find_map(|item| {
            if let Item::Path(path) = item {
                Some(path)
            } else {
                None
            }
        })
        .expect("shape");
    assert!((path.w - 192.0).abs() < 1e-6 && (path.h - 96.0).abs() < 1e-6);
    let pages = render_with_media(&document, &options, Some(&package)).expect("render");
    let tree =
        resvg::usvg::Tree::from_str(&pages[0].svg, &resvg::usvg::Options::default()).expect("SVG");
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).expect("raster");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    (pixmap, path.x.round() as u32, path.y.round() as u32)
}

#[test]
fn gradient_pixels_change_colour_and_are_clipped_to_the_ellipse() {
    let (raster, x, y) = raster_fill("ellipse", GRADIENT);
    let left = raster.pixel(x + 8, y + 48).expect("left");
    let middle = raster.pixel(x + 96, y + 48).expect("middle");
    let right = raster.pixel(x + 184, y + 48).expect("right");
    assert!(left.red() > 200 && left.blue() < 20);
    assert!(middle.green() > 240 && middle.red() < 20 && middle.blue() < 20);
    assert!(right.blue() > 200 && right.red() < 20);
    let corner = raster.pixel(x + 1, y + 1).expect("corner");
    assert!(
        corner.red() > 250 && corner.green() > 250 && corner.blue() > 250,
        "ellipse fill must not paint its box's corner"
    );
}

#[test]
fn pattern_has_both_foreground_and_background_pixels() {
    let (raster, x, y) = raster_fill("rect", &pattern("cross"));
    let mut red = 0;
    let mut blue = 0;
    for dy in 16..40 {
        for dx in 16..40 {
            let pixel = raster.pixel(x + dx, y + dy).expect("pixel");
            red += u32::from(pixel.red());
            blue += u32::from(pixel.blue() > 180 && pixel.red() < 80);
        }
    }
    // One-pixel grid lines straddle pixel centres, so include their antialiased coverage.
    assert!(
        red > 100 * 255 && blue > 200,
        "pattern collapsed to a solid colour: red={red} blue={blue}"
    );
}

#[test]
fn dark_horizontal_hatching_adds_parallel_ink() {
    let coverage = |preset: &str| {
        let (raster, x, y) = raster_fill("rect", &pattern(preset));
        (16..40)
            .flat_map(|dy| (16..40).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| u32::from(raster.pixel(x + dx, y + dy).expect("pixel").red()))
            .sum::<u32>()
    };
    assert!(coverage("dkHorz") > coverage("horz") * 3 / 2);
}

#[test]
fn through_places_words_in_the_u_gap_and_tight_does_not() {
    let gap_text = |through| {
        let (package, document) = open(contour_docx(through));
        let pages =
            place_pages(&document, &RenderOptions::default(), Some(&package)).expect("place");
        let shape = pages[0]
            .items
            .iter()
            .find_map(|item| {
                if let Item::Path(path) = item {
                    Some(path)
                } else {
                    None
                }
            })
            .expect("U");
        let left = shape.x + 192.0 * 0.3 + 2.0;
        let right = shape.x + 192.0 * 0.7 - 2.0;
        pages[0].items.iter().filter(|item| matches!(item,Item::Text(text) if text.baseline > shape.y+80.0 && text.baseline < shape.y+190.0 && text.x >= left && text.x + text.width <= right+0.01)).count()
    };
    assert_eq!(gap_text(false), 0);
    assert!(
        gap_text(true) > 0,
        "through wrapping must admit words into the concavity"
    );
}

#[test]
fn unsupported_pattern_is_named_in_the_render_report() {
    let (package, document) = open(fill_docx("rect", &pattern("unknownPattern")));
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    assert!(pages[0]
        .warnings
        .iter()
        .any(|warning| warning.contains("render.pattern") && warning.contains("unknownPattern")));
}

#[test]
fn path_gradient_is_retained_and_explicitly_warned_instead_of_painted_as_linear() {
    let fill = GRADIENT.replace(
        "<a:lin ang=\"0\" scaled=\"1\"/>",
        "<a:path path=\"circle\"><a:fillToRect l=\"50%\" t=\"50%\" r=\"50%\" b=\"50%\"/></a:path>",
    );
    let (package, document) = open(fill_docx("ellipse", &fill));
    let options = RenderOptions::default();
    let placed = place_pages(&document, &options, Some(&package)).expect("place");
    let path = placed[0]
        .items
        .iter()
        .find_map(|item| {
            if let Item::Path(path) = item {
                Some(path)
            } else {
                None
            }
        })
        .expect("shape");
    assert!(path.paint.is_none());
    let pages = render_with_media(&document, &options, Some(&package)).expect("render");
    assert!(pages[0]
        .warnings
        .iter()
        .any(|warning| warning.contains("render.gradient") && warning.contains("path shading")));
}
