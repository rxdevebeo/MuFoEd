//! Bi-level image filters that the CC0 corpus actually uses: 1-bit Flate
//! DeviceGray, `/CCITTFaxDecode`, and `/JBIG2Decode`. Before these were carried,
//! every such XObject was reported as `bits per component` — wrong reason for a
//! real, decodable scan.

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::PdfBuilder;

/// Pillow/libtiff Group 4 strip for an 8×2 pattern:
/// row0 = WWWWBBBB, row1 = BBBBWWWW (1 = white).
const CCITT_G4_8X2: &[u8] = &[0x26, 0xae, 0x6d, 0x80, 0x08, 0x00, 0x80];

/// Packed 1-bit rows for the same pattern (MSB first).
const GRAY1_8X2: &[u8] = &[0b1111_0000, 0b0000_1111];

fn page_with_image(dict: &str, data: &[u8], compress: bool) -> Vec<u8> {
    let mut pdf = PdfBuilder::new().media_box([0, 0, 40, 40]);
    let image = pdf.stream(dict, data, compress);
    pdf.page_with(
        b"q 20 0 0 20 10 10 cm /Im0 Do Q",
        &format!("<< /XObject << /Im0 {image} 0 R >> >>"),
    );
    pdf.build()
}

fn placed_raw(document: &mut PdfDocument) -> (Vec<u8>, u8) {
    let page = document.page(1).expect("page");
    let placed = page
        .items()
        .iter()
        .find_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .expect("image item");
    assert!(placed.missing.is_none(), "{:?}", placed.missing);
    let encoded = placed.image.as_ref().expect("decoded");
    let (samples, components) = encoded.raw_samples().expect("raw");
    (samples.to_vec(), components)
}

#[test]
fn a_one_bit_flate_gray_image_expands_to_eight_bit_raw() {
    let bytes = page_with_image(
        "/Type /XObject /Subtype /Image /Width 8 /Height 2 /ColorSpace /DeviceGray \
         /BitsPerComponent 1",
        GRAY1_8X2,
        true,
    );
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let (samples, components) = placed_raw(&mut document);
    assert_eq!(components, 1);
    assert_eq!(
        samples,
        [
            255, 255, 255, 255, 0, 0, 0, 0, //
            0, 0, 0, 0, 255, 255, 255, 255
        ]
    );
    assert!(
        !document
            .report()
            .losses()
            .iter()
            .any(|loss| loss.id == "pdf.image.missing"),
        "1-bit Flate must not be a layout loss:\n{}",
        document.report()
    );
}

#[test]
fn a_ccitt_group4_xobject_decodes_to_eight_bit_gray() {
    let bytes = page_with_image(
        "/Type /XObject /Subtype /Image /Width 8 /Height 2 /ColorSpace /DeviceGray \
         /BitsPerComponent 1 /Filter /CCITTFaxDecode \
         /DecodeParms << /K -1 /Columns 8 /Rows 2 /BlackIs1 false >>",
        CCITT_G4_8X2,
        false,
    );
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let (samples, components) = placed_raw(&mut document);
    assert_eq!(components, 1);
    assert_eq!(samples.len(), 16);
    // Pillow's Group4 strip for this pattern arrives with the opposite run
    // polarity under PDF's default `/BlackIs1 false` (hayro's `invert_black`).
    // The checkerboard geometry is what matters: four+four per row, flipped
    // between rows.
    assert_eq!(
        samples,
        [
            0, 0, 0, 0, 255, 255, 255, 255, //
            255, 255, 255, 255, 0, 0, 0, 0
        ]
    );
    assert!(
        !document
            .report()
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("is not carried")
                || loss.detail.contains("bits per component")),
        "CCITT must not look like a missing codec or bits refusal:\n{}",
        document.report()
    );
}

#[test]
fn a_broken_jbig2_stream_is_broken_not_unsupported() {
    let bytes = page_with_image(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray \
         /BitsPerComponent 1 /Filter /JBIG2Decode",
        b"not a jbig2 stream",
        false,
    );
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let placed = page
        .items()
        .iter()
        .find_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .expect("image");
    assert!(placed.image.is_none());
    let detail = placed.missing.as_deref().unwrap_or("");
    assert!(
        detail.contains("JBIG2") && !detail.contains("is not carried"),
        "expected a JBIG2 decode loss, got {detail:?}"
    );
}

#[test]
fn stroke_style_operators_are_carried_not_reported() {
    let mut pdf = PdfBuilder::new().media_box([0, 0, 100, 100]);
    pdf.page(b"1 J 2 j 8 M 2 w 10 10 m 90 10 l S");
    let bytes = pdf.build();
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let vector = page
        .items()
        .iter()
        .find_map(|item| match item {
            Item::Vector(vector) => Some(vector),
            _ => None,
        })
        .expect("stroked path");
    assert_eq!(vector.line_cap, strict_ooxml_pdf::LineCap::Round);
    assert_eq!(vector.line_join, strict_ooxml_pdf::LineJoin::Bevel);
    assert_eq!(vector.miter_limit, 8.0);
    assert!(
        !document
            .report()
            .losses()
            .iter()
            .any(|loss| loss.id == "pdf.operator"
                && (loss.detail.contains("`J`")
                    || loss.detail.contains("`j`")
                    || loss.detail.contains("`M`"))),
        "J/j/M must not be unknown operators:\n{}",
        document.report()
    );
}
