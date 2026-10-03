//! Hostile PDFs through the converter (`REWORK-AUDIT-2026-10.md`, AUD-01/02).
//!
//! Inputs are built with `strict-ooxml-testkit`'s `PdfBuilder` and converted on
//! a 1 MiB stack under a 10 s limit. CI runs this file in debug and in release.

#![allow(missing_docs)]

use strict_ooxml_convert::{convert, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::{assert_survives, PdfBuilder};

mod smoke {
    //! The kit itself, checked against the real converter.

    use super::*;

    #[test]
    fn an_empty_page_converts() {
        assert_survives("convert empty page", || {
            let mut pdf = PdfBuilder::new();
            pdf.page(b"");
            let mut document = PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
            convert(&mut document, &PdfOptions::default()).expect("convert");
        });
    }
}

mod tables {
    //! AUD-15: line counts and grid sizes.

    use super::*;

    /// 10 000 horizontal and 10 000 vertical short rules — past
    /// `max_table_lines` (4000), so the converter skips table search.
    fn many_rules_page() -> Vec<u8> {
        use std::fmt::Write as _;
        let mut content = String::new();
        for index in 0..10_000 {
            let y = f64::from(index) * 0.05;
            let x = f64::from(index) * 0.05;
            // Thin strokes that pass `max_rule_thickness` / `min_rule_length`.
            let _ = write!(content, "0.5 w 10 {y} m 40 {y} l S ");
            let _ = write!(content, "0.5 w {x} 10 m {x} 40 l S ");
        }
        let mut pdf = PdfBuilder::new();
        pdf.page(content.as_bytes());
        pdf.build()
    }

    #[test]
    fn twenty_thousand_ruling_lines_stop_table_search() {
        let (ids, elapsed_ms) = assert_survives("20k rules", || {
            let started = std::time::Instant::now();
            let mut document =
                PdfDocument::open(&many_rules_page(), PdfLimits::default()).expect("open");
            let output = convert(&mut document, &PdfOptions::default()).expect("convert");
            let ids: Vec<String> = output
                .report
                .losses
                .iter()
                .map(|loss| loss.id.clone())
                .collect();
            (ids, started.elapsed().as_millis())
        });
        assert!(
            ids.iter().any(|id| id == "convert.table.budget"),
            "expected convert.table.budget, got {ids:?}"
        );
        assert!(
            elapsed_ms < 10_000,
            "conversion must finish inside 10 s, took {elapsed_ms} ms"
        );
    }
}

mod text {
    //! AUD-83: characters XML 1.0 does not allow.

    use super::*;
    use strict_ooxml_core::opc::{OpenOptions, Package};
    use strict_ooxml_write::{write_package, WriteOptions};

    /// A page whose `ToUnicode` maps `A` to U+0001 (forbidden in XML 1.0).
    fn pdf_with_control_char() -> Vec<u8> {
        let mut pdf = PdfBuilder::new();
        let cmap = pdf.stream(
            "",
            br"/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
/CMapName /Adobe-Identity-UCS def /CMapType 2 def \
1 begincodespacerange <41> <41> endcodespacerange \
1 beginbfchar <0041> <0001> endbfchar endcmap \
CMapName currentdict /CMap defineresource pop end end",
            false,
        );
        let font = pdf.object(format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 65 \
/Widths [600] /ToUnicode {cmap} 0 R >>"
        ));
        pdf.page_with(
            b"BT /F1 12 Tf 1 0 0 1 72 720 Tm (A) Tj ET",
            &format!("<< /Font << /F1 {font} 0 R >> >>"),
        );
        pdf.build()
    }

    #[test]
    fn a_control_character_is_stripped_and_named() {
        let (ids, xml_ok) = assert_survives("control char through convert", || {
            let mut document =
                PdfDocument::open(&pdf_with_control_char(), PdfLimits::default()).expect("open");
            let output = convert(&mut document, &PdfOptions::default()).expect("convert");
            let ids: Vec<String> = output
                .report
                .losses()
                .iter()
                .map(|loss| loss.id.clone())
                .collect();
            let written =
                write_package(&output.document, None, &WriteOptions::default()).expect("write");
            let package =
                Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");
            let part = package
                .read_part(&strict_ooxml_core::part::PartId::new("/word/document.xml"))
                .expect("document.xml");
            let text = String::from_utf8(part).expect("utf-8");
            let xml_ok = roxmltree::Document::parse(&text).is_ok();
            (ids, xml_ok)
        });
        assert!(
            xml_ok,
            "document.xml must parse after the control was stripped"
        );
        assert!(
            ids.iter().any(|id| id == "convert.invalid-xml-char"),
            "expected convert.invalid-xml-char, got {ids:?}"
        );
    }
}
