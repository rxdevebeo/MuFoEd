//! `/JPXDecode` (JPEG 2000) through the public PDF reader.
//!
//! Unit coverage of the decoder lives next to `decode_jpx_bytes` in `image.rs`.
//! These tests pin the `XObject` and inline paths: a document that used to report
//! `pdf.image.missing` for every JPX scan must now place real samples.

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::PdfBuilder;

/// Lossless 2×2 RGB JP2 — same bytes as `image::tests::JPX_RGB_2X2`.
const JPX_RGB_2X2: &[u8] = &[
    0, 0, 0, 12, 106, 80, 32, 32, 13, 10, 135, 10, 0, 0, 0, 20, 102, 116, 121, 112, 106, 112, 50,
    32, 0, 0, 0, 0, 106, 112, 50, 32, 0, 0, 0, 45, 106, 112, 50, 104, 0, 0, 0, 22, 105, 104, 100,
    114, 0, 0, 0, 2, 0, 0, 0, 2, 0, 3, 7, 7, 0, 0, 0, 0, 0, 15, 99, 111, 108, 114, 1, 0, 0, 0, 0,
    0, 16, 0, 0, 0, 153, 106, 112, 50, 99, 255, 79, 255, 81, 0, 47, 0, 0, 0, 0, 0, 2, 0, 0, 0, 2,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 7, 1, 1, 7, 1, 1,
    7, 1, 1, 255, 82, 0, 12, 0, 0, 0, 1, 0, 1, 4, 4, 0, 1, 255, 92, 0, 7, 64, 64, 72, 72, 80, 255,
    100, 0, 37, 0, 1, 67, 114, 101, 97, 116, 101, 100, 32, 98, 121, 32, 79, 112, 101, 110, 74, 80,
    69, 71, 32, 118, 101, 114, 115, 105, 111, 110, 32, 50, 46, 53, 46, 52, 255, 144, 0, 10, 0, 0,
    0, 0, 0, 30, 0, 1, 255, 147, 128, 128, 128, 147, 243, 2, 0, 223, 207, 192, 4, 0, 167, 224, 2,
    0, 255, 217,
];

fn jpx_page(dict_extra: &str) -> Vec<u8> {
    let mut pdf = PdfBuilder::new().media_box([0, 0, 40, 40]);
    let image = pdf.stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 {dict_extra} /Filter /JPXDecode"
        ),
        JPX_RGB_2X2,
        false,
    );
    pdf.page_with(
        b"q 20 0 0 20 10 10 cm /Im0 Do Q",
        &format!("<< /XObject << /Im0 {image} 0 R >> >>"),
    );
    pdf.build()
}

#[test]
fn a_jpx_xobject_is_decoded_and_placed() {
    // Colour space and BPC omitted: ISO 32000-1 §7.4.9 — JPX carries both.
    let mut document = PdfDocument::open(&jpx_page(""), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let images: Vec<_> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1, "{:?}", page.items());
    let placed = images[0];
    assert!(placed.missing.is_none(), "{:?}", placed.missing);
    let encoded = placed.image.as_ref().expect("decoded");
    assert_eq!(encoded.pixel_size(), (2, 2));
    assert_eq!(
        encoded.raw_samples().map(|(_, c)| c),
        Some(3),
        "JPX RGB → Raw"
    );
    assert_eq!(
        encoded.raw_samples().map(|(s, _)| s),
        Some(&[255_u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255][..])
    );
    assert!(
        !document
            .report()
            .losses()
            .iter()
            .any(|loss| loss.id == "pdf.image.missing"),
        "JPX must not be reported as an unsupported filter:\n{}",
        document.report()
    );
}

#[test]
fn a_jpx_xobject_tolerates_a_lying_bits_per_component() {
    // Producers sometimes still write `/BitsPerComponent 8` (or a wrong value).
    // The codestream wins; we must not refuse the image for dictionary BPC.
    let mut document = PdfDocument::open(
        &jpx_page("/ColorSpace /DeviceRGB /BitsPerComponent 16"),
        PdfLimits::default(),
    )
    .expect("open");
    let page = document.page(1).expect("page");
    let placed = page
        .items()
        .iter()
        .find_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .expect("image");
    assert!(placed.missing.is_none(), "{:?}", placed.missing);
    assert!(placed.image.is_some());
}

#[test]
fn a_broken_jpx_xobject_is_named_broken_not_unsupported() {
    let mut pdf = PdfBuilder::new().media_box([0, 0, 40, 40]);
    let image = pdf.stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /Filter /JPXDecode",
        b"this is not jpeg2000",
        false,
    );
    pdf.page_with(
        b"q 20 0 0 20 10 10 cm /Im0 Do Q",
        &format!("<< /XObject << /Im0 {image} 0 R >> >>"),
    );
    let mut document = PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
    let _ = document.pages().expect("pages");
    let losses: Vec<_> = document
        .report()
        .losses()
        .iter()
        .map(|loss| (loss.id.as_str(), loss.detail.as_str()))
        .collect();
    assert!(
        losses
            .iter()
            .any(|(id, detail)| *id == "pdf.image.missing" && detail.contains("JPEG 2000")),
        "expected a JPX decode loss, got {losses:?}"
    );
    assert!(
        losses
            .iter()
            .all(|(_, detail)| !detail.contains("is not carried")),
        "must not look like an unsupported filter: {losses:?}"
    );
}

#[test]
fn an_inline_jpx_image_is_decoded() {
    let mut stream = Vec::new();
    stream.extend_from_slice(b"q 10 0 0 10 0 0 cm BI /W 2 /H 2 /F /JPX ID ");
    stream.extend_from_slice(JPX_RGB_2X2);
    stream.extend_from_slice(b" EI Q");
    let mut pdf = PdfBuilder::new().media_box([0, 0, 40, 40]);
    pdf.page_with(&stream, "<< >>");
    let mut document = PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let placed = page
        .items()
        .iter()
        .find_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .expect("inline image");
    assert!(placed.missing.is_none(), "{:?}", placed.missing);
    let encoded = placed.image.as_ref().expect("decoded");
    assert_eq!(encoded.pixel_size(), (2, 2));
    assert_eq!(
        encoded.raw_samples().map(|(s, _)| s),
        Some(&[255_u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255][..])
    );
}
