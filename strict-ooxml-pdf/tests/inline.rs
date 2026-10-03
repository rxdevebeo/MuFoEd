//! AUD-84 / CORE-QUEUE L-9: inline images (`BI`…`ID`…`EI`).
//!
//! Binary samples between `ID` and `EI` must not be executed as operators, and
//! the picture must become a placed image the converter can write out.

#![allow(clippy::doc_markdown)]

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::PdfBuilder;

/// A page whose inline image samples spell PDF operators (`re f cm BT ET Tj`).
fn pdf_with_hostile_inline() -> Vec<u8> {
    let garbage = b"re f cm BT ETTj"; // 15 bytes = 5×1 RGB
    assert_eq!(garbage.len(), 15);
    let mut content = Vec::new();
    content.extend_from_slice(b"q 100 0 0 100 50 50 cm BI /W 5 /H 1 /BPC 8 /CS /RGB ID ");
    content.extend_from_slice(garbage);
    content.extend_from_slice(b" EI Q");
    let mut pdf = PdfBuilder::new();
    pdf.page(&content);
    pdf.build()
}

#[test]
fn inline_image_samples_are_not_drawn_as_vectors() {
    let mut document =
        PdfDocument::open(&pdf_with_hostile_inline(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let vectors = page
        .items()
        .iter()
        .filter(|item| matches!(item, Item::Vector(_)))
        .count();
    assert_eq!(
        vectors,
        0,
        "inline sample bytes must not become path operators: {:?}",
        page.items()
    );
    let images: Vec<_> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1, "expected one placed inline image");
    assert!(
        images[0].image.is_some(),
        "inline image must decode: {:?}",
        images[0].missing
    );
}

#[test]
fn inline_image_fills_placed_image_bytes() {
    let mut document =
        PdfDocument::open(&pdf_with_hostile_inline(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let Some(Item::Image(placed)) = page
        .items()
        .iter()
        .find(|item| matches!(item, Item::Image(_)))
    else {
        panic!("no image: {:?}", page.items());
    };
    let encoded = placed.image.as_ref().expect("decoded");
    let (samples, components) = encoded.raw_samples().expect("raw RGB");
    assert_eq!(components, 3);
    assert_eq!(samples.len(), 15);
}
