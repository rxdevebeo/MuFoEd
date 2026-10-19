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

/// ASCII85 (ISO 32000-1 §7.4.3), enough for a fixture: no `z` shortcut.
fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(word);
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..=chunk.len()]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// R1 (PDF-IMPORT-COMPARISON-2026-10-19): the SAP NetWeaver invoice shape. The
/// content stream's `/Filter` is an indirect `[/FlateDecode]`, and between two
/// lines of text sits a 1-bit stencil `BI /IM true /F [/A85 /Fl] … EI`.
fn sap_invoice_page() -> Vec<u8> {
    // 8×2 one-bit stencil: two bytes of samples, Flate then ASCII85.
    let samples = miniz_oxide::deflate::compress_to_vec_zlib(&[0b1010_1010, 0b0101_0101], 6);
    let mut content = Vec::new();
    content.extend_from_slice(b"BT /F1 12 Tf 1 0 0 1 20 80 Tm (AB) Tj ET\r\n");
    content.extend_from_slice(b"q 16 0 0 4 20 40 cm\r\n BI\r\n /W 8\r\n /H 2\r\n /BPC 1\r\n /IM true\r\n /F [/A85 /Fl]\r\n ID\r\n ");
    content.extend_from_slice(&ascii85(&samples));
    content.extend_from_slice(b"\r\n EI\r\n Q\r\n");
    // And a 2×1 gray picture behind the same chain, which does decode.
    let gray = miniz_oxide::deflate::compress_to_vec_zlib(&[0, 255], 6);
    content.extend_from_slice(b"q 16 0 0 8 60 40 cm BI /W 2 /H 1 /BPC 8 /CS /G /F [/A85 /Fl] ID ");
    content.extend_from_slice(&ascii85(&gray));
    content.extend_from_slice(b" EI Q\r\n");
    content.extend_from_slice(b"BT /F1 12 Tf 1 0 0 1 20 20 Tm (BA) Tj ET\r\n");

    let mut pdf = PdfBuilder::new().media_box([0, 0, 200, 100]);
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 66 /Widths [600 600] >>",
    );
    let filter = pdf.object("[/FlateDecode]");
    let page = pdf.page_with(b"", &format!("<< /Font << /F1 {font} 0 R >> >>"));
    // `page_with` reserves the content stream just before the page object; it
    // is rewritten here with the indirect filter the builder cannot express.
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&content, 6);
    let mut body = format!(
        "<< /Filter {filter} 0 R /Length {} >>\nstream\n",
        compressed.len()
    )
    .into_bytes();
    body.extend_from_slice(&compressed);
    body.extend_from_slice(b"\nendstream");
    pdf.set(page - 1, body);
    pdf.build()
}

#[test]
fn an_indirect_content_filter_and_an_inline_stencil_keep_the_text() {
    let mut document = PdfDocument::open(&sap_invoice_page(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let lines = strict_ooxml_pdf::text::lines(page.items());
    let texts: Vec<String> = lines
        .iter()
        .map(strict_ooxml_pdf::GlyphLine::text)
        .collect();
    assert_eq!(texts, ["AB", "BA"], "{:?}", page.items());
    let images: Vec<_> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2, "{:?}", page.items());
    // A stencil mask is not a picture (as for an XObject `/ImageMask`): it is a
    // named loss, never a page of garbage operators.
    let missing = images[0].missing.as_deref().unwrap_or_default();
    assert!(missing.contains("mask"), "{missing}");
    let gray = images[1].image.as_ref().expect("the gray picture decodes");
    assert_eq!(gray.raw_samples(), Some((&[0u8, 255][..], 1)));
}
