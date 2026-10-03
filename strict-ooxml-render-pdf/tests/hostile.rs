//! Hostile inputs for the PDF writer (`REWORK-AUDIT-2026-10.md`, AUD-14).
//!
//! A PNG whose IHDR claims an absurd size must not allocate that size. The
//! render stays `Ok` and the report names the refusal.

#![allow(missing_docs)]

use std::io::Cursor;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_testkit::{assert_survives, DocxBuilder};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// A PNG whose IHDR claims `width`×`height` RGB, with no pixel data.
fn ihdr_bomb(width: u32, height: u32) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut typed = b"IHDR".to_vec();
    typed.extend_from_slice(&ihdr);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend_from_slice(&(ihdr.len() as u32).to_be_bytes());
    out.extend_from_slice(&typed);
    out.extend_from_slice(&crc32(&typed).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(b"IEND");
    out.extend_from_slice(&crc32(b"IEND").to_be_bytes());
    out
}

mod image {
    //! AUD-14: allocation from a hostile PNG IHDR.

    use super::*;

    #[test]
    fn a_png_whose_ihdr_claims_gigabytes_renders_with_a_named_loss() {
        let bomb = ihdr_bomb(65_535, 65_535);
        assert!(
            bomb.len() < 80,
            "the hostile PNG is a header, not a picture: {} bytes",
            bomb.len()
        );
        let (ids, ok) = assert_survives("ihdr bomb through render_pdf", move || {
            let body = concat!(
                "<w:p><w:r><w:drawing><wp:inline>",
                "<wp:extent cx=\"914400\" cy=\"914400\"/>",
                "<wp:docPr id=\"1\" name=\"pic\"/>",
                "<a:graphic xmlns:a=\"http://purl.oclc.org/ooxml/drawingml/main\">",
                "<a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">",
                "<pic:pic xmlns:pic=\"http://purl.oclc.org/ooxml/drawingml/picture\">",
                "<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"pic\"/></pic:nvPicPr>",
                "<pic:blipFill><a:blip r:embed=\"rId1\"/></pic:blipFill>",
                "<pic:spPr><a:xfrm><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm></pic:spPr>",
                "</pic:pic></a:graphicData></a:graphic>",
                "</wp:inline></w:drawing></w:r></w:p>"
            );
            let bytes = DocxBuilder::strict()
                .body(body)
                .rel(
                    "rId1",
                    "image",
                    "media/image1.png",
                )
                .part("word/media/image1.png", bomb)
                .build();
            let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default())
                .expect("open");
            let document = parse_document(&package, &ParseOptions::default()).expect("parse");
            let options = RenderOptions::default();
            let pages = place_pages(&document, &options, Some(&package)).expect("place");
            let output = render_with_source(&pages, &options, Some(&package)).expect("render");
            let ids: Vec<&str> = output.report.losses().iter().map(|loss| loss.id).collect();
            (ids, true)
        });
        assert!(ok);
        assert!(
            ids.iter().any(|id| *id == "pdf.image.too-large"),
            "expected pdf.image.too-large, got {ids:?}"
        );
    }
}
