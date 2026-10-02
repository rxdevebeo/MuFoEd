//! Hostile PDFs through the reader (`REWORK-AUDIT-2026-10.md`, AUD-01/02).
//!
//! Inputs are built with `strict-ooxml-testkit`'s `PdfBuilder` and read on a
//! 1 MiB stack under a 10 s limit. CI runs this file in debug and in release.

#![allow(missing_docs)]

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

mod fonts {
    //! AUD-12: `ToUnicode` tokens, `/W` and `bfrange` ranges.
}

mod images {
    //! AUD-12, AUD-13: `/SMask` cycles, decompression bombs.
}

mod budget {
    //! AUD-13, AUD-84: per-page glyph and operation budgets, form reuse,
    //! inline images.
}
