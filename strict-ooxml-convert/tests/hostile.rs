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
}

mod text {
    //! AUD-83: characters XML 1.0 does not allow.
}
