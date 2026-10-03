//! AUD-81: PNG/JPEG colour layouts survive the PDF write → hayro round trip.
//!
//! Each fixture is a solid-colour picture. The writer must pick the right
//! `/ColorSpace` (and CMYK `/Decode`), and the centre pixel hayro paints must
//! match the source within ±2/255.

#![cfg(feature = "raster")]
#![allow(clippy::doc_markdown)]

use std::io::Cursor;
use std::path::Path;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_pdf::raster::RasterOptions;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::layout::{ImageItem, Item, PlacedPage};
use strict_ooxml_render_svg::{MediaSource, RenderOptions};
use strict_ooxml_testkit::DocxBuilder;

/// A media source that serves one part.
struct OnePart {
    id: PartId,
    bytes: Vec<u8>,
}

impl MediaSource for OnePart {
    fn read_media(&self, part: &PartId) -> Result<Vec<u8>, strict_ooxml_core::error::StrictError> {
        if part == &self.id {
            Ok(self.bytes.clone())
        } else {
            Err(strict_ooxml_core::error::StrictError::MissingPart(
                part.clone(),
            ))
        }
    }
}

/// Renders a single full-page picture and rasterizes page 1 at scale 1.
fn centre_pixel(part_name: &str, bytes: Vec<u8>) -> [u8; 3] {
    let part = PartId::new(part_name);
    let media = OnePart {
        id: part.clone(),
        bytes,
    };
    let pages = [PlacedPage {
        width_px: 72.0,
        height_px: 72.0,
        section_index: 0,
        items: vec![Item::Image(ImageItem {
            x: 0.0,
            y: 0.0,
            w: 72.0,
            h: 72.0,
            href: None,
            part: Some(part),
            alt: String::new(),
            transform: None,
        })],
    }];
    let pdf = render_with_source(&pages, &RenderOptions::default(), Some(&media))
        .expect("render")
        .bytes;
    let document = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let rasterizer = document.rasterizer().expect("rasterizer");
    let png = rasterizer
        .page_png(
            1,
            &RasterOptions {
                scale: 1.0,
                ..RasterOptions::default()
            },
        )
        .expect("png");
    let rgba = decode_rgba(&png);
    let width = png_width(&png) as usize;
    let height = png_height(&png) as usize;
    let cx = width / 2;
    let cy = height / 2;
    let at = (cy * width + cx) * 4;
    [rgba[at], rgba[at + 1], rgba[at + 2]]
}

fn decode_rgba(png: &[u8]) -> Vec<u8> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("info");
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer).expect("frame");
    buffer[..info.buffer_size()].to_vec()
}

fn png_width(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]])
}

fn png_height(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]])
}

fn assert_near(got: [u8; 3], expect: [u8; 3], label: &str) {
    for (g, e) in got.iter().zip(expect.iter()) {
        let delta = i16::from(*g) - i16::from(*e);
        assert!(
            delta.unsigned_abs() <= 2,
            "{label}: centre pixel {got:?} not within ±2 of {expect:?}"
        );
    }
}

fn solid_png(color: png::ColorType, depth: png::BitDepth, pixels: &[u8]) -> Vec<u8> {
    let (w, h) = match color {
        png::ColorType::Grayscale if depth == png::BitDepth::Two => (4, 1),
        _ => (2, 2),
    };
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if color == png::ColorType::Indexed {
            encoder.set_palette(vec![200, 40, 50]);
        }
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(pixels).expect("data");
    }
    out
}

#[test]
fn png_layouts_round_trip_through_hayro() {
    // 8-bit RGB red.
    let rgb = solid_png(
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0],
    );
    assert_near(
        centre_pixel("/word/media/rgb.png", rgb),
        [255, 0, 0],
        "rgb8",
    );

    // 16-bit RGB: high bytes 0xAB,0x00,0x00 → centre ≈ (171,0,0).
    let mut rgb16 = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut rgb16, 2, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header().expect("header");
        let mut data = Vec::new();
        for _ in 0..4 {
            data.extend_from_slice(&[0xAB, 0x33, 0x00, 0x44, 0x00, 0x55]);
        }
        writer.write_image_data(&data).expect("data");
    }
    assert_near(
        centre_pixel("/word/media/rgb16.png", rgb16),
        [0xAB, 0x00, 0x00],
        "rgb16",
    );

    // Palette → RGB.
    let indexed = solid_png(png::ColorType::Indexed, png::BitDepth::Eight, &[0, 0, 0, 0]);
    assert_near(
        centre_pixel("/word/media/pal.png", indexed),
        [200, 40, 50],
        "indexed",
    );

    // 2-bit grey expanded: last sample is white.
    let gray2 = solid_png(png::ColorType::Grayscale, png::BitDepth::Two, &[0x1B]);
    // A 4×1 image placed in a square page: centre column is sample 2 → 170.
    assert_near(
        centre_pixel("/word/media/g2.png", gray2),
        [170, 170, 170],
        "gray2",
    );
}

#[test]
fn jpeg_layouts_round_trip_through_hayro() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-render-pdf/tests/fixtures");
    let rgb = std::fs::read(fixtures.join("jpeg-rgb-red.jpg")).expect("rgb jpeg");
    assert_near(
        centre_pixel("/word/media/r.jpg", rgb),
        [220, 30, 40],
        "jpeg-rgb",
    );

    let gray = std::fs::read(fixtures.join("jpeg-gray-128.jpg")).expect("gray jpeg");
    assert_near(
        centre_pixel("/word/media/g.jpg", gray),
        [128, 128, 128],
        "jpeg-gray",
    );

    // CMYK with Adobe transform=2 and /Decode invert: source was (0,200,200,0)
    // which is a red-ish CMYK; after invert+convert hayro should land near red.
    let cmyk = std::fs::read(fixtures.join("jpeg-cmyk-adobe.jpg")).expect("cmyk jpeg");
    let pixel = centre_pixel("/word/media/c.jpg", cmyk);
    assert!(
        pixel[0] > pixel[1] && pixel[0] > pixel[2],
        "jpeg-cmyk: expected a red-dominant centre, got {pixel:?}"
    );
}

/// Smoke: the same fixtures also embed through a real package (not only a
/// hand-placed page), so the writer path the CLI uses is covered.
#[test]
fn fixtures_embed_through_a_package() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-render-pdf/tests/fixtures");
    let jpeg = std::fs::read(fixtures.join("jpeg-rgb-red.jpg")).expect("jpeg");
    let body = concat!(
        "<w:p><w:r><w:drawing><wp:inline>",
        "<wp:extent cx=\"914400\" cy=\"914400\"/>",
        "<wp:docPr id=\"1\" name=\"pic\"/>",
        "<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">",
        "<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"pic\"/><pic:cNvPicPr/>",
        "</pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId1\"/></pic:blipFill>",
        "<pic:spPr><a:xfrm><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm></pic:spPr>",
        "</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    let bytes = DocxBuilder::strict()
        .body(body)
        .rel("rId1", "image", "media/image1.jpg")
        .part("word/media/image1.jpg", jpeg)
        .build();
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document =
        strict_ooxml_wml::parse_document(&package, &strict_ooxml_wml::ParseOptions::default())
            .expect("parse");
    let options = RenderOptions::default();
    let pages =
        strict_ooxml_render_svg::place_pages(&document, &options, Some(&package)).expect("place");
    let pdf = render_with_source(&pages, &options, Some(&package)).expect("render");
    assert!(
        String::from_utf8_lossy(&pdf.bytes).contains("/DeviceRGB"),
        "JPEG RGB must be tagged DeviceRGB"
    );
}
