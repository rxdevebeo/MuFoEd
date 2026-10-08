//! Page rasterization, on a PDF this workspace wrote (`STAGE-8-TASK.md` §6
//! O3).
//!
//! The fixture is produced by the real renderers and read back by the real
//! reader, so the chain under test is the one a caller runs: a document, a PDF,
//! a picture of a page of it. The pixel budget is checked with a scale that would
//! allocate 124 MiB, because a budget nobody has tried to exceed is a comment.
//!
//! The defaults are named rather than inferred, because a test that says
//! `&Default::default()` says nothing about which document it is reading.
#![allow(clippy::doc_markdown)]
// The whole file is about the `raster` feature, so it is one test with the feature
// on rather than a file that fails to compile without it. `cargo check --all-targets`
// with default features (a CI step, added because `strict-ooxml-convert` shipped a
// build that only compiled with every feature on) is what keeps that honest.
#![cfg(feature = "raster")]

use std::path::Path;

use strict_ooxml_pdf::error::LimitKind;
use strict_ooxml_pdf::raster::{RasterOptions, Rasterizer, Region, Size};
use strict_ooxml_pdf::{PdfDocument, PdfError, PdfLimits};

/// A PDF laid out and written by this workspace, from one of the corpus
/// documents.
fn source_pdf(fixture: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let bytes = std::fs::read(root.join(fixture)).expect("the fixture is present");
    let package = strict_ooxml_core::opc::Package::open_reader(
        &bytes[..],
        &strict_ooxml_core::opc::OpenOptions::default(),
    )
    .expect("open");
    let document =
        strict_ooxml_wml::parse_document(&package, &strict_ooxml_wml::ParseOptions::default())
            .expect("parse");
    let options = strict_ooxml_render_svg::RenderOptions::default();
    let placed =
        strict_ooxml_render_svg::place_pages(&document, &options, Some(&package)).expect("place");
    strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("render")
        .bytes
}

/// How many pixels of a picture are not white.
fn dark_pixels(png: &[u8]) -> usize {
    decode(png)
        .chunks(4)
        .filter(|pixel| pixel[0] < 200 || pixel[1] < 200 || pixel[2] < 200)
        .count()
}

/// The width and height a PNG declares in its `IHDR`.
fn png_size(bytes: &[u8]) -> Size {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    // 8 (signature) + 4 (length) + 4 (type) + 4 (width) puts the width here.
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Size { width, height }
}

/// A page rasterizes to a PNG of the size it said it would.
#[test]
fn a_page_rasterizes_to_the_size_it_promised() {
    let pdf = source_pdf("strict-profile.docx");
    let limits = PdfLimits::default();
    let document = PdfDocument::open(&pdf, limits).expect("open");
    assert_eq!(document.page_count(), 2, "the fixture is two pages");
    let rasterizer = document.rasterizer().expect("a rasterizer for our own PDF");
    assert_eq!(
        rasterizer.page_count(),
        2,
        "the rasterizer agrees on the page count"
    );

    let options = RasterOptions {
        scale: 1.0,
        ..RasterOptions::default()
    };
    let promised = rasterizer.page_size(1, &options).expect("size");
    let png = rasterizer.page_png(1, &options).expect("rasterize");
    let actual = png_size(&png);
    assert_eq!(
        actual, promised,
        "the picture must be the size the budget was checked against"
    );
    assert!(
        actual.width > 400 && actual.height > 600,
        "{actual:?} is not a page"
    );
}

/// The picture has the document on it.
///
/// A rasterizer that produced a white page would pass every other test in this
/// file, because a white page is the right size, reproducible and a correct crop
/// of itself. So the ink is measured on a page that has some: the pixels that are
/// not white must form a box inside the margins the document declares, and there
/// must be a page's worth of them rather than a stray one.
#[test]
fn the_picture_has_the_document_on_it() {
    let pdf = source_pdf("strict-text-grid.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let size = rasterizer
        .page_size(
            1,
            &RasterOptions {
                scale: 1.0,
                ..RasterOptions::default()
            },
        )
        .expect("size");
    let png = rasterizer
        .page_png(
            1,
            &RasterOptions {
                scale: 1.0,
                ..RasterOptions::default()
            },
        )
        .expect("rasterize");
    let pixels = decode(&png);
    let stride = size.width as usize * 4;
    let (mut left, mut top, mut right, mut bottom) = (size.width, size.height, 0u32, 0u32);
    let mut inked = 0usize;
    for y in 0..size.height {
        for x in 0..size.width {
            let at = y as usize * stride + x as usize * 4;
            if pixels[at] < 200 || pixels[at + 1] < 200 || pixels[at + 2] < 200 {
                inked += 1;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    assert!(
        inked > 500,
        "a page of text has more than 500 inked pixels, not {inked}"
    );
    // One inch of margin, and the fixture's ink starts inside it.
    assert!(
        left >= 70 && top >= 70,
        "ink at ({left}, {top}) is outside the margins"
    );
    assert!(
        right < size.width - 40 && bottom < size.height - 40,
        "ink reaches ({right}, {bottom}) on a {size:?} page"
    );
}

/// `strict-profile` page 1 carries a chart and a SmartArt diagram. They used
/// to be reserved space only (ADR-0006), and this test pinned the resulting
/// white page; charts are now drawn from their cached data, so the page has ink
/// of its own (the diagram is still reserved space).
#[test]
fn the_chart_page_is_drawn() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let options = RasterOptions {
        scale: 1.0,
        ..RasterOptions::default()
    };
    assert_eq!(rasterizer.page_count(), 2);
    let first = rasterizer.page_png(1, &options).expect("rasterize");
    assert!(
        dark_pixels(&first) > 0,
        "the chart page has ink now that charts are drawn"
    );
    let second = rasterizer.page_png(2, &options).expect("rasterize");
    assert!(
        dark_pixels(&second) > 100,
        "and the text page does, so the rasterizer is drawing"
    );
}
/// Two runs give the same bytes. A rasterizer that parallelises is the obvious
/// way to lose this, and the project's rule is that determinism is checked and
/// not assumed.
#[test]
fn a_picture_is_reproducible() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let options = RasterOptions::default();
    let first = rasterizer.page_png(1, &options).expect("first");
    let second = rasterizer.page_png(1, &options).expect("second");
    assert_eq!(
        first, second,
        "two renders of one page must be the same bytes"
    );
    // And the same document through a second rasterizer, which parses the file
    // again: a cache that carried state between renders would show up here.
    let other = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    assert_eq!(first, other.page_png(1, &options).expect("third"));
}

/// A page bigger than the budget is refused, and refused as a limit rather than
/// as a failure deep inside somebody else's rasterizer.
#[test]
fn an_oversized_picture_is_refused_before_it_is_allocated() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let error = rasterizer
        .page_png(
            1,
            &RasterOptions {
                scale: 32.0,
                ..RasterOptions::default()
            },
        )
        .expect_err("a Letter page at scale 32 is 31 megapixels of RGBA");
    match error {
        PdfError::LimitExceeded {
            kind,
            limit,
            actual,
        } => {
            assert_eq!(kind, LimitKind::RasterPixels);
            assert_eq!(limit, 4096 * 4096);
            assert!(actual > limit, "{actual} should be over {limit}");
        }
        other => panic!("expected a limit error, got {other}"),
    }
}

/// A budget small enough to refuse a modest page also refuses it, which is the
/// point of a per-call bound: the caller decides what a picture may cost.
#[test]
fn the_budget_is_the_callers() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let error = rasterizer
        .page_png(
            1,
            &RasterOptions {
                scale: 1.0,
                max_pixels: 1000,
            },
        )
        .expect_err("1000 pixels is not a page");
    assert!(matches!(
        error,
        PdfError::LimitExceeded {
            kind: LimitKind::RasterPixels,
            ..
        }
    ));
}

/// A scale that is not a scale is refused with a message about the scale, and a
/// page number that is not a page is refused with a message about the page.
#[test]
fn nonsense_requests_are_refused_by_name() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    for scale in [0.0, -2.0, f64::INFINITY, 1_000.0] {
        let error = rasterizer
            .page_png(
                1,
                &RasterOptions {
                    scale,
                    ..RasterOptions::default()
                },
            )
            .expect_err("not a scale");
        assert!(
            matches!(error, PdfError::Malformed(ref detail) if detail.contains("scale")),
            "{scale} gave {error}"
        );
    }
    for number in [0, 3, 99] {
        let error = rasterizer
            .page_png(number, &RasterOptions::default())
            .expect_err("no such page");
        assert!(
            matches!(error, PdfError::Missing(ref what) if what.contains(&number.to_string())),
            "page {number} gave {error}"
        );
    }
}

/// A region is a crop of the page: the same pixels, at the same scale, cut out
/// of the same render. The arithmetic here is the whole of the feature for a
/// figure classifier, and a silent off-by-one would crop the wrong picture.
#[test]
fn a_region_is_a_crop_of_the_page() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let options = RasterOptions {
        scale: 1.0,
        ..RasterOptions::default()
    };
    let page = rasterizer.page_png(1, &options).expect("page");
    let region = Region {
        x: 72.0,
        y: 72.0,
        width: 100.0,
        height: 50.0,
    };
    let crop = rasterizer.region_png(1, region, &options).expect("region");
    assert_eq!(
        png_size(&crop),
        Size {
            width: 100,
            height: 50
        }
    );
    let expected = crop_of(&page, 72, 72, 100, 50);
    assert_eq!(
        first_difference(&decode(&crop), &expected),
        None,
        "the region must be the page's own pixels, not a second render of it"
    );
}

/// A region that hangs off the page is clipped, and a region with no area is
/// refused: a zero-sized box on a scanned page is a caller's bug, not a picture.
#[test]
fn a_region_off_the_page_is_clipped() {
    let pdf = source_pdf("strict-profile.docx");
    let rasterizer = Rasterizer::new(&pdf, PdfLimits::default()).expect("rasterizer");
    let options = RasterOptions {
        scale: 1.0,
        ..RasterOptions::default()
    };
    let page = rasterizer.page_png(1, &options).expect("page");
    let (page_width, page_height) = (png_size(&page).width, png_size(&page).height);
    let crop = rasterizer
        .region_png(
            1,
            Region {
                x: -50.0,
                y: -50.0,
                // Deliberately past the bottom-right corner.
                width: f64::from(page_width) * 3.0,
                height: f64::from(page_height) * 3.0,
            },
            &options,
        )
        .expect("clipped");
    assert_eq!(png_size(&crop), png_size(&page), "clipped to the page");
    assert_eq!(crop, page, "a region covering the page is the page");

    for region in [
        Region {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 10.0,
        },
        Region {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: f64::NAN,
        },
    ] {
        let error = rasterizer
            .region_png(1, region, &options)
            .expect_err("a region with no area");
        assert!(
            matches!(error, PdfError::Malformed(ref detail) if detail.contains("region")),
            "{region:?} gave {error}"
        );
    }
}

/// A file this reader could not load is refused by the rasterizer too, rather
/// than turning into a page of noise.
#[test]
fn a_file_the_reader_could_not_load_is_refused() {
    let Err(error) = Rasterizer::new(b"not a pdf at all", PdfLimits::default()) else {
        panic!("garbage was accepted")
    };
    assert!(matches!(error, PdfError::Malformed(_)), "{error}");
}

/// Where two rasters first differ, in a form a person can read.
///
/// A `assert_eq!` on two pictures prints megabytes of `255, 255, 255` and hides
/// the one byte that matters.
fn first_difference(left: &[u8], right: &[u8]) -> Option<String> {
    if left.len() != right.len() {
        return Some(format!(
            "different lengths: {} against {}",
            left.len(),
            right.len()
        ));
    }
    left.iter().zip(right).position(|(a, b)| a != b).map(|at| {
        let channel = at % 4;
        let channel_name = ["red", "green", "blue", "alpha"][channel];
        format!(
            "byte {at} (pixel {}, {channel_name}): {} against {}",
            at / 4,
            left[at],
            right[at]
        )
    })
}

/// The pixels of a PNG, as raw RGBA, for the crop comparison.
fn crop_of(png: &[u8], x: u32, y: u32, width: u32, height: u32) -> Vec<u8> {
    let raster = decode(png);
    let stride = png_size(png).width as usize * 4;
    let mut out = Vec::new();
    for row in y..y + height {
        let start = row as usize * stride + x as usize * 4;
        out.extend_from_slice(&raster[start..start + width as usize * 4]);
    }
    out
}

/// Decodes a PNG this module wrote, with the crate that wrote it.
fn decode(png: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("our own PNG");
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer).expect("frame");
    out.extend_from_slice(&buffer[..info.buffer_size()]);
    out
}
