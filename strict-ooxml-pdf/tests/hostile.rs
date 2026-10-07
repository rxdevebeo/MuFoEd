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
            let mut document = PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
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

mod object_streams {
    //! Waiver `PDF-OBJSTM-BOMB` (closed): `lopdf` inflates every object stream
    //! while it loads, so their budget is checked on the raw bytes first.

    use super::*;
    use strict_ooxml_pdf::{LimitKind, PdfError, OBJECT_STREAM_BUDGET_FACTOR};

    /// A zlib stream that inflates to `copies × segment` zero bytes, built
    /// without ever holding the inflated bytes: one sync-flushed deflate segment
    /// of `segment` zeros (independent of what precedes it, and byte-aligned),
    /// repeated, then a final empty block and the Adler-32 of the zeros.
    fn zero_bomb(segment: usize, copies: usize) -> Vec<u8> {
        use miniz_oxide::deflate::core::{create_comp_flags_from_zip_params, CompressorOxide};
        use miniz_oxide::deflate::stream::deflate;
        use miniz_oxide::MZFlush;

        let mut compressor = CompressorOxide::new(create_comp_flags_from_zip_params(9, -15, 0));
        let input = vec![0u8; segment];
        let mut block = vec![0u8; segment / 64 + 4096];
        let result = deflate(&mut compressor, &input, &mut block, MZFlush::Sync);
        assert_eq!(result.bytes_consumed, segment, "{result:?}");
        block.truncate(result.bytes_written);

        let mut out = vec![0x78, 0xDA];
        for _ in 0..copies {
            out.extend_from_slice(&block);
        }
        // Final fixed-Huffman block holding only end-of-block.
        out.extend_from_slice(&[0x03, 0x00]);
        let total = (segment as u64) * (copies as u64);
        let adler = ((total % 65_521) << 16) | 1;
        out.extend_from_slice(&u32::try_from(adler).expect("adler fits").to_be_bytes());
        out
    }

    /// A one-page PDF (classic xref table) carrying the given object streams.
    /// `lopdf` expands every `/Type /ObjStm` object it loads, referenced or not.
    fn with_object_streams(payloads: &[Vec<u8>]) -> Vec<u8> {
        let mut pdf = PdfBuilder::new();
        pdf.page(b"0 0 m 10 10 l S");
        for payload in payloads {
            let mut body = format!(
                "<< /Type /ObjStm /N 1 /First 4 /Length {} /Filter /FlateDecode >>\nstream\n",
                payload.len()
            )
            .into_bytes();
            body.extend_from_slice(payload);
            body.extend_from_slice(b"\nendstream");
            pdf.object(body);
        }
        pdf.build()
    }

    fn reduced() -> PdfLimits {
        PdfLimits {
            max_content_bytes: 4 * 1024 * 1024,
            ..PdfLimits::default()
        }
    }

    fn refusal(bytes: Vec<u8>, limits: PdfLimits) -> PdfError {
        assert_survives("object-stream bomb", move || {
            match PdfDocument::open(&bytes, limits) {
                Ok(_) => panic!("an object-stream bomb was loaded"),
                Err(error) => error,
            }
        })
    }

    #[test]
    fn a_quarter_megabyte_object_stream_bomb_is_refused_before_load() {
        // 256 MiB of zeros in about a quarter of a megabyte of file. Before the
        // guard `load_mem` inflated all of it (and a gigabyte-sized one, the
        // same way) before the reader saw an object.
        let bytes = with_object_streams(&[zero_bomb(1024 * 1024, 256)]);
        assert!(bytes.len() < 512 * 1024, "{} bytes", bytes.len());
        let limits = reduced();
        let started = std::time::Instant::now();
        let error = refusal(bytes, limits);
        let budget = (limits.max_content_bytes * OBJECT_STREAM_BUDGET_FACTOR) as u64;
        match error {
            PdfError::LimitExceeded {
                kind: LimitKind::ObjectStreamBytes,
                limit,
                actual,
            } => {
                assert_eq!(limit, budget);
                // The scan stops one scratch buffer past the budget.
                assert!(actual > budget && actual <= budget + 64 * 1024, "{actual}");
            }
            other => panic!("expected an object-stream refusal, got {other:?}"),
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn many_object_streams_are_held_to_one_budget_together() {
        // Each stream is under the budget; their sum is not.
        let streams: Vec<Vec<u8>> = (0..8).map(|_| zero_bomb(1024 * 1024, 4)).collect();
        let error = refusal(with_object_streams(&streams), reduced());
        assert!(
            matches!(
                error,
                PdfError::LimitExceeded {
                    kind: LimitKind::ObjectStreamBytes,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn the_budget_is_named_in_the_error() {
        let limits = PdfLimits::default();
        let error = limits.exceeded(LimitKind::ObjectStreamBytes, 1);
        assert_eq!(LimitKind::ObjectStreamBytes.as_str(), "object_stream_bytes");
        assert!(error.to_string().contains("object_stream_bytes"), "{error}");
        assert!(
            error
                .to_string()
                .contains(&(128u64 * 1024 * 1024).to_string()),
            "{error}"
        );
    }

    #[test]
    fn a_file_written_with_object_streams_still_opens() {
        let mut source = PdfBuilder::new();
        source.page(b"0 0 m 10 10 l S");
        source.page(b"BT ET");
        let mut document = lopdf::Document::load_mem(&source.build()).expect("lopdf load");
        let mut modern = Vec::new();
        document
            .save_modern(&mut modern)
            .expect("save with object streams");
        assert!(
            modern
                .windows(b"ObjStm".len())
                .any(|window| window == b"ObjStm"),
            "the fixture must actually carry an object stream"
        );
        for limits in [PdfLimits::default(), reduced()] {
            let bytes = modern.clone();
            let (count, pages) = assert_survives("open a modern pdf", move || {
                let mut pdf = PdfDocument::open(&bytes, limits).expect("open");
                (pdf.page_count(), pdf.pages().expect("pages").len())
            });
            assert_eq!((count, pages), (2, 2));
        }
    }

    #[test]
    fn a_small_object_stream_under_budget_still_loads() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"1 0 << /A 1 >>", 6);
        let bytes = with_object_streams(&[payload]);
        let pages = assert_survives("open", move || {
            PdfDocument::open(&bytes, reduced())
                .expect("open")
                .page_count()
        });
        assert_eq!(pages, 1);
    }
}

/// AUD-88: vendored `hayro` must not stack-overflow or hang on hostile PDFs.
#[cfg(feature = "raster")]
mod raster {
    use strict_ooxml_pdf::raster::{RasterOptions, Rasterizer};
    use strict_ooxml_pdf::{PdfDocument, PdfLimits};
    use strict_ooxml_testkit::harness::{bounded, Outcome};

    fn rasterize_ok(bytes: Vec<u8>) -> Outcome<Result<(), String>> {
        bounded(move || {
            let rasterizer =
                Rasterizer::new(&bytes, PdfLimits::default()).map_err(|error| error.to_string())?;
            let options = RasterOptions {
                scale: 1.0,
                ..RasterOptions::default()
            };
            let _ = rasterizer
                .page_png(1, &options)
                .map_err(|error| error.to_string())?;
            Ok(())
        })
    }

    /// Self-referencing tiling pattern (`PrintCraft` / fuzz): must not overflow.
    #[test]
    fn self_referencing_tiling_pattern_terminates() {
        let pattern_body = "/Pattern cs /P1 scn 0 0 10 10 re f";
        let pdf = format!(
            "%PDF-1.7\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R \
             /Resources << /Pattern << /P1 5 0 R >> >> >> endobj\n\
             4 0 obj << /Length 30 >> stream\n\
             /Pattern cs /P1 scn 0 0 40 40 re f\n\
             endstream endobj\n\
             5 0 obj << /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 \
             /BBox [0 0 10 10] /XStep 10 /YStep 10 /Length {} >> stream\n\
             {pattern_body}\n\
             endstream endobj\n\
             trailer << /Root 1 0 R >>\n\
             %%EOF\n",
            pattern_body.len()
        );
        match rasterize_ok(pdf.into_bytes()) {
            Outcome::Returned(Ok(())) => {}
            other => panic!("self-referencing tiling pattern must terminate: {other:?}"),
        }
    }

    /// Self-referencing Type 3 glyph: must not overflow.
    #[test]
    fn self_referencing_type3_glyph_terminates() {
        let proc_body = "1 0 d0 BT /F1 1 Tf (A) Tj ET 0 0 1 1 re f";
        let pdf = format!(
            "%PDF-1.7\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >> endobj\n\
             4 0 obj << /Length 31 >> stream\n\
             BT /F1 20 Tf 10 10 Td (A) Tj ET\n\
             endstream endobj\n\
             5 0 obj << /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] \
             /FontMatrix [1 0 0 1 0 0] /FirstChar 65 /LastChar 65 /Widths [1] \
             /Encoding << /Differences [65 /a] >> /CharProcs << /a 6 0 R >> >> endobj\n\
             6 0 obj << /Length {} >> stream\n\
             {proc_body}\n\
             endstream endobj\n\
             trailer << /Root 1 0 R >>\n\
             %%EOF\n",
            proc_body.len()
        );
        match rasterize_ok(pdf.into_bytes()) {
            Outcome::Returned(Ok(())) => {}
            other => panic!("self-referencing Type 3 glyph must terminate: {other:?}"),
        }
    }

    /// JBIG2 image `XObject` with absurd dictionary size (hayro#1259 family).
    ///
    /// Dictionary `/Width`×`/Height` is rejected in `ImageXObject::new` before
    /// decode; the vendored `jbig2::pixel_budget_ok` covers the bitstream-claimed
    /// size that upstream OOMs on. Either way the rasterizer must return.
    #[test]
    fn absurd_jbig2_dimensions_are_skipped() {
        let pdf = b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R \
/Resources << /XObject << /Im0 5 0 R >> >> >> endobj\n\
4 0 obj << /Length 28 >> stream\n\
q 20 0 0 20 5 5 cm /Im0 Do Q\n\
endstream endobj\n\
5 0 obj << /Type /XObject /Subtype /Image /Width 4294967295 /Height 2 \
/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode /Length 4 >> stream\n\
XXXX\n\
endstream endobj\n\
trailer << /Root 1 0 R >>\n\
%%EOF\n";
        match rasterize_ok(pdf.to_vec()) {
            Outcome::Returned(Ok(())) => {}
            other => panic!("absurd JBIG2 dimensions must not hang/OOM: {other:?}"),
        }
    }

    /// Inline image with absurd `/W`: must not hang.
    #[test]
    fn absurd_image_dimensions_are_skipped() {
        let content =
            "q 20 0 0 20 5 5 cm BI /W 4294967295 /H 2 /BPC 8 /CS /G ID \0\u{ff}\u{ff}\0 EI Q \
             1 0 0 rg 0 0 4 4 re f";
        let pdf = format!(
            "%PDF-1.7\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R >> endobj\n\
             4 0 obj << /Length {} >> stream\n\
             {content}\n\
             endstream endobj\n\
             trailer << /Root 1 0 R >>\n\
             %%EOF\n",
            content.len()
        );
        match rasterize_ok(pdf.into_bytes()) {
            Outcome::Returned(Ok(())) => {}
            other => panic!("absurd image dimensions must not hang: {other:?}"),
        }
    }

    /// CID `/W` / `/W2` spanning all of `u32`: must finish quickly.
    #[test]
    fn huge_cid_width_ranges_terminate() {
        let pdf = b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 100 40] /Contents 4 0 R \
/Resources << /Font << /F1 5 0 R >> >> >> endobj\n\
4 0 obj << /Length 35 >> stream\n\
BT /F1 12 Tf 10 10 Td <0041> Tj ET\n\
endstream endobj\n\
5 0 obj << /Type /Font /Subtype /Type0 /BaseFont /Helvetica /Encoding /Identity-H \
/DescendantFonts [6 0 R] >> endobj\n\
6 0 obj << /Type /Font /Subtype /CIDFontType2 /BaseFont /Helvetica \
/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
/FontDescriptor 7 0 R /W [0 4294967295 500] /W2 [0 4294967295 -1000 250 880] >> endobj\n\
7 0 obj << /Type /FontDescriptor /FontName /Helvetica /Flags 32 \
/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 800 /Descent -200 \
/CapHeight 700 /StemV 80 >> endobj\n\
trailer << /Root 1 0 R >>\n\
%%EOF\n";
        match rasterize_ok(pdf.to_vec()) {
            Outcome::Returned(Ok(())) => {}
            other => panic!("huge CID width ranges must terminate: {other:?}"),
        }
    }

    /// Deep literal `<<` nesting in the trailer (AUD-96 / `dep-hayro-deep-dict`).
    /// Vendored hayro-syntax must return `Err`/`None`, not abort the process.
    #[test]
    fn deep_literal_dict_nesting_is_refused() {
        let nest = 2_000usize;
        let mut pdf = String::from(
            "%PDF-1.7\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R >> endobj\n\
             4 0 obj << /Length 0 >> stream\n\
             endstream endobj\n\
             trailer << /Root 1 0 R /Info ",
        );
        for _ in 0..nest {
            pdf.push_str("<< /X ");
        }
        pdf.push_str("null");
        for _ in 0..nest {
            pdf.push_str(" >>");
        }
        pdf.push_str(" >>\n%%EOF\n");
        let bytes = pdf.into_bytes();
        let outcome = bounded(
            move || match Rasterizer::new(&bytes, PdfLimits::default()) {
                Ok(_) => Ok(()),
                Err(error) => Err(error.to_string()),
            },
        );
        assert!(
            matches!(outcome, Outcome::Returned(_)),
            "deep dict nesting must not abort: {outcome:?}"
        );
    }

    /// `/Kids` cycle: our lopdf path must not abort; hayro-syntax guard covers the
    /// rasterizer. Object-stream bombs are refused before load (`object_streams`
    /// above; waiver `PDF-OBJSTM-BOMB` closed).
    #[test]
    fn page_tree_kids_cycle_is_handled() {
        let cyclic = || {
            b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >> endobj\n\
4 0 obj << /Type /Pages /Parent 3 0 R /Kids [3 0 R] /Count 1 >> endobj\n\
trailer << /Root 1 0 R >>\n\
%%EOF\n"
                .to_vec()
        };
        let reader = bounded({
            let bytes = cyclic();
            move || match PdfDocument::open(&bytes, PdfLimits::default()) {
                Ok(_) => Ok(()),
                Err(error) => Err(error.to_string()),
            }
        });
        assert!(
            matches!(reader, Outcome::Returned(_)),
            "Kids cycle must not hang/abort the reader: {reader:?}"
        );
        let raster = bounded({
            let bytes = cyclic();
            move || match Rasterizer::new(&bytes, PdfLimits::default()) {
                Ok(_) => Ok(()),
                Err(error) => Err(error.to_string()),
            }
        });
        assert!(
            matches!(raster, Outcome::Returned(_)),
            "Kids cycle must not hang/abort the rasterizer: {raster:?}"
        );
    }
}
