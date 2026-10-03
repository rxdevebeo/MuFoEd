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
}
