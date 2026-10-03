//! Hostile PDFs through the reader (`REWORK-AUDIT-2026-10.md`, AUD-01/02).
//!
//! Inputs are built with `strict-ooxml-testkit`'s `PdfBuilder` and read on a
//! 1 MiB stack under a 10 s limit. CI runs this file in debug and in release.

#![allow(missing_docs)]
#![allow(clippy::format_push_string, clippy::unreadable_literal)]

use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::{assert_survives, PdfBuilder};

mod smoke {
    //! The kit itself, checked against the real reader.

    use super::*;

    #[test]
    fn a_built_pdf_opens_with_its_pages() {
        let (count, pages) = assert_survives("open pdf", || {
            let mut pdf = PdfBuilder::new();
            pdf.page(b"0 0 m 10 10 l S");
            pdf.page(b"");
            let mut document = PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
            let count = document.page_count();
            let pages = document.pages().expect("pages").len();
            (count, pages)
        });
        assert_eq!((count, pages), (2, 2));
    }
}

/// A one-page PDF with a CID font whose dictionary and `ToUnicode` are given.
fn with_cid_font(cid_entries: &str, tounicode: &str) -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let cmap_object = pdf.stream("/CMapName /A", tounicode.as_bytes(), false);
    let descendant = pdf.object(format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Test \
         /CIDSystemInfo << /Registry (Test) /Ordering (Test) /Supplement 0 >> \
         /DW 1000 {cid_entries} >>"
    ));
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H \
         /DescendantFonts [{descendant} 0 R] /ToUnicode {cmap_object} 0 R >>"
    ));
    let content = b"BT /F1 12 Tf 65 <0041> Tj ET";
    pdf.page_with(content, &format!("<< /Font << /F1 {font} 0 R >> >>"));
    pdf.build()
}

/// Opens `bytes`, reads every page, and returns the report's loss ids.
#[allow(clippy::type_complexity)]
fn read_all(bytes: Vec<u8>) -> (Vec<String>, usize, usize) {
    assert_survives("read hostile pdf", move || {
        let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
        // Reading the pages is the part that resolves fonts and images; the
        // report is filled in there, not at open.
        let pages = document.pages().expect("pages");
        let ids: Vec<String> = document
            .report()
            .losses()
            .iter()
            .map(|loss| loss.id.clone())
            .collect();
        (ids, pages.len(), document.page_count())
    })
}

mod fonts {
    //! AUD-12: `ToUnicode` tokens, `/W` and `bfrange` ranges.

    use super::*;

    #[test]
    fn a_to_unicode_token_that_is_not_ascii_hex_is_reported_not_fatal() {
        // `é` and `Ж` are not hex digits, and the reader sliced the token at a
        // fixed offset: "byte index 2 is not a char boundary" (AUD-12). A CMap
        // that writes its destinations as text is a producer bug, not a reason
        // to end the process where a font is read.
        for cmap in [
            concat!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap ",
                "1 begincodespacerange <0000> <FFFF> endcodespacerange ",
                "2 beginbfchar <0041> <00é1> <0042> <0043> endbfchar ",
                "endcmap end end"
            ),
            concat!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap ",
                "1 begincodespacerange <0000> <FFFF> endcodespacerange ",
                "2 beginbfchar <0041> <ЖЖЖЖ> <0042> <0043> endbfchar ",
                "endcmap end end"
            ),
        ] {
            let (ids, _, _) = read_all(with_cid_font("", cmap));
            assert!(
                ids.iter().any(|id| id == "pdf.font.tounicode-invalid"),
                "{ids:?}"
            );
        }
    }

    #[test]
    fn a_width_group_spanning_the_glyph_space_is_cut_and_reported() {
        // `/W [0 4294967295 500]` is four billion insertions otherwise.
        let (ids, _, _) = read_all(with_cid_font("/W [0 4294967295 500]", ""));
        assert!(
            ids.iter().any(|id| id == "pdf.font.widths-truncated"),
            "{ids:?}"
        );
    }

    #[test]
    fn a_width_group_that_runs_backwards_is_reported_and_skipped() {
        let (ids, _, _) = read_all(with_cid_font("/W [500 10 500]", ""));
        assert!(
            ids.iter().any(|id| id == "pdf.font.widths-reversed"),
            "{ids:?}"
        );
    }

    #[test]
    fn a_bfrange_over_the_whole_code_space_is_cut_and_reported() {
        let (ids, _, _) = read_all(with_cid_font(
            "",
            concat!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap ",
                "1 begincodespacerange <0000> <FFFF> endcodespacerange ",
                "1 beginbfrange <0000> <FFFFFFFF> <0041> endbfrange ",
                "endcmap end end"
            ),
        ));
        // Its own id, not `widths-truncated`: a `bfrange` is a `ToUnicode`
        // destination range and has no widths to do with, and a report that
        // names the wrong mechanism sends a caller to the wrong dictionary.
        assert!(
            ids.iter().any(|id| id == "pdf.font.bfrange-truncated"),
            "{ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id == "pdf.font.widths-truncated"),
            "{ids:?}"
        );
    }

    #[test]
    fn an_ordinary_font_produces_no_font_losses() {
        // The guard the other way: a font inside every budget must not be
        // reported, or the notes above say nothing.
        let (ids, _, _) = read_all(with_cid_font(
            "/W [65 65 500]",
            concat!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap ",
                "1 begincodespacerange <0000> <FFFF> endcodespacerange ",
                "1 beginbfchar <0041> <0042> endbfchar endcmap end end"
            ),
        ));
        assert!(!ids.iter().any(|id| id.starts_with("pdf.font.")), "{ids:?}");
    }
}

mod images {
    //! AUD-12, AUD-13: `/SMask` cycles, decompression bombs.

    use super::*;

    /// A page that draws one image `XObject`, with the dictionary suffix added.
    fn with_image(image_dict: &str) -> Vec<u8> {
        let mut pdf = PdfBuilder::new();
        let image = pdf.stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 {image_dict}"
            ),
            &[0, 0, 0, 0],
            false,
        );
        pdf.page_with(
            b"q 10 0 0 10 10 10 cm /Im Do Q",
            &format!("<< /XObject << /Im {image} 0 R >> >>"),
        );
        pdf.build()
    }

    #[test]
    fn an_image_whose_mask_is_itself_is_refused_without_recursing() {
        // `/SMask` pointing at the image itself: the reader recursed through the
        // cache until the stack ended (AUD-12). The picture still draws, without
        // the transparency the cycle claimed.
        let mut pdf = PdfBuilder::new();
        let image = pdf.reserve();
        // The picture is its own soft mask: `/SMask` names the picture.
        pdf.set_stream(
            image,
            &format!(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask {image} 0 R"
            ),
            &[0, 0, 0, 0],
            false,
        );
        let (ids, _, _) = read_all({
            pdf.page_with(
                b"q 10 0 0 10 10 10 cm /Im Do Q",
                &format!("<< /XObject << /Im {image} 0 R >> >>"),
            );
            pdf.build()
        });
        assert!(
            ids.iter().any(|id| id == "pdf.image.smask-cycle"),
            "{ids:?}"
        );
    }

    #[test]
    fn a_cycle_of_two_masks_is_refused_without_recursing() {
        // A -> B -> A: the shape a single self-reference does not cover.
        let mut pdf = PdfBuilder::new();
        let a = pdf.reserve();
        let b = pdf.reserve();
        pdf.set_stream(
            a,
            &format!(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask {b} 0 R"
            ),
            &[0, 0, 0, 0],
            false,
        );
        pdf.set_stream(
            b,
            &format!(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask {a} 0 R"
            ),
            &[255, 255, 255, 255],
            false,
        );
        let (ids, _, _) = read_all({
            pdf.page_with(
                b"q 10 0 0 10 10 10 cm /Im Do Q",
                &format!("<< /XObject << /Im {a} 0 R >> >>"),
            );
            pdf.build()
        });
        assert!(
            ids.iter().any(|id| id == "pdf.image.smask-cycle"),
            "{ids:?}"
        );
    }

    #[test]
    fn an_ordinary_picture_reports_no_mask_cycle() {
        let (ids, _, _) = read_all(with_image(""));
        assert!(
            !ids.iter().any(|id| id == "pdf.image.smask-cycle"),
            "{ids:?}"
        );
    }
}

mod budget {
    //! AUD-13: per-page glyph and operation budgets, form reuse, flate bombs,
    //! and the font-object ceiling. AUD-84 will extend this module for inline
    //! images.

    use super::*;

    /// A form whose content draws `glyph_count` glyphs of `A`, under `/F1`.
    fn form_of_glyphs(pdf: &mut PdfBuilder, glyph_count: usize) -> u32 {
        let text = "A".repeat(glyph_count);
        let content = format!("BT /F1 12 Tf ({text}) Tj ET");
        pdf.stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
            content.as_bytes(),
            false,
        )
    }

    /// A Type1 font dictionary good enough for `Tj` of ASCII.
    fn simple_font(pdf: &mut PdfBuilder) -> u32 {
        pdf.object(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             /Encoding /WinAnsiEncoding >>",
        )
    }

    #[test]
    fn a_form_drawn_many_times_stops_on_the_page_glyph_budget() {
        // 10 000 × 1 000 glyphs would be ten million without a shared page
        // budget; with one, the page stops at `max_glyphs` and records
        // `pdf.page.budget`. The form is decoded once (the cache), so the
        // harness finishes well inside 10 s.
        let (ids, pages, _) = assert_survives("form glyph budget", || {
            let mut pdf = PdfBuilder::new();
            let font = simple_font(&mut pdf);
            let form = form_of_glyphs(&mut pdf, 1_000);
            let do_ops = "/Fm Do ".repeat(10_000);
            pdf.page_with(
                do_ops.as_bytes(),
                &format!("<< /Font << /F1 {font} 0 R >> /XObject << /Fm {form} 0 R >> >>"),
            );
            let bytes = pdf.build();
            let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
            let pages = document.pages().expect("pages");
            let ids: Vec<String> = document
                .report()
                .losses()
                .iter()
                .map(|loss| loss.id.clone())
                .collect();
            (ids, pages.len(), document.page_count())
        });
        assert_eq!(pages, 1);
        assert!(
            ids.iter().any(|id| id == "pdf.page.budget"),
            "expected pdf.page.budget, got {ids:?}"
        );
    }

    #[test]
    fn a_flate_bomb_in_page_content_is_refused_without_hanging() {
        // ~40 MiB of zeros compress to a few KiB; the page content budget is
        // 32 MiB, so inflate stops and the page is refused (AUD-13).
        let zeros = vec![0u8; 40 * 1024 * 1024];
        let err = assert_survives("content flate bomb", move || {
            let mut pdf = PdfBuilder::new();
            pdf.page_flate(&zeros, "<< >>");
            let bytes = pdf.build();
            assert!(
                bytes.len() < 256 * 1024,
                "compressed page should be tiny, got {} bytes",
                bytes.len()
            );
            PdfDocument::open(&bytes, PdfLimits::default())
                .expect("open")
                .page(1)
                .expect_err("40 MiB of content is past max_content_bytes")
        });
        let text = err.to_string();
        assert!(
            text.contains("decompress") || text.contains("limit") || text.contains("Memory"),
            "{text}"
        );
    }

    #[test]
    fn a_flate_bomb_in_an_image_is_refused_without_hanging() {
        // The picture's samples expand past `max_image_bytes`; bounded
        // decompress refuses before the allocation (AUD-13). The page still
        // opens; the picture is recorded as missing / too large.
        let zeros = vec![0u8; 65 * 1024 * 1024];
        let (ids, _, _) = assert_survives("image flate bomb", move || {
            let mut pdf = PdfBuilder::new();
            // `compress: true` writes `/Filter /FlateDecode` and a tiny payload.
            let image = pdf.stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &zeros,
                true,
            );
            pdf.page_with(
                b"q 10 0 0 10 10 10 cm /Im Do Q",
                &format!("<< /XObject << /Im {image} 0 R >> >>"),
            );
            let built = pdf.build();
            assert!(
                built.len() < 256 * 1024,
                "compressed image should be tiny, got {} bytes",
                built.len()
            );
            let mut document = PdfDocument::open(&built, PdfLimits::default()).expect("open");
            let _ = document.pages().expect("pages");
            let ids: Vec<String> = document
                .report()
                .losses()
                .iter()
                .map(|loss| loss.id.clone())
                .collect();
            (ids, 0usize, 0usize)
        });
        assert!(
            ids.iter()
                .any(|id| id == "pdf.image.missing" || id.starts_with("pdf.image.")),
            "expected an image loss, got {ids:?}"
        );
    }

    #[test]
    fn ten_thousand_fonts_trip_the_font_budget() {
        let (ids, _, _) = assert_survives("font budget", || {
            let mut pdf = PdfBuilder::new();
            let mut font_entries = String::new();
            for index in 0..10_000 {
                let id = pdf.object(format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /F{index} \
                     /Encoding /WinAnsiEncoding >>"
                ));
                font_entries.push_str(&format!("/F{index} {id} 0 R "));
            }
            // One glyph so the resources are actually walked.
            pdf.page_with(
                b"BT /F0 12 Tf (A) Tj ET",
                &format!("<< /Font << {font_entries} >> >>"),
            );
            let mut document =
                PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
            let _ = document.pages().expect("pages");
            let ids: Vec<String> = document
                .report()
                .losses()
                .iter()
                .map(|loss| loss.id.clone())
                .collect();
            (ids, 0usize, 0usize)
        });
        assert!(
            ids.iter().any(|id| id == "pdf.font.budget"),
            "expected pdf.font.budget, got {ids:?}"
        );
    }
}
