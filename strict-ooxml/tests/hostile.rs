//! Hostile inputs through the public API (`REWORK-AUDIT-2026-10.md`, AUD-01/02).
//!
//! Every test builds its input with `strict-ooxml-testkit` and runs it on a
//! 1 MiB stack under a 10 s limit, so a panic, a hang and a stack overflow are
//! three distinct failures rather than one green test that happened to have a
//! big enough stack. CI runs this file in debug and in release.
//!
//! Each module belongs to the AUD task that fills it; a defect's regression test
//! is added by the task that fixes it, not before.

#![allow(missing_docs)]

use std::io::Cursor;

use strict_ooxml::{OpenOptions, StrictDocument};
use strict_ooxml_testkit::{assert_survives, DocxBuilder};

fn open(bytes: Vec<u8>, options: &OpenOptions) -> strict_ooxml::StrictError {
    match StrictDocument::open_reader(Cursor::new(bytes), options) {
        Ok(_) => panic!("expected the input to be rejected"),
        Err(error) => error,
    }
}

mod smoke {
    //! The kit itself, checked against the real reader.

    use super::*;
    use strict_ooxml_core::ns::Conformance;

    #[test]
    fn a_strict_package_opens_as_strict() {
        let conformance = assert_survives("open strict", || {
            let bytes = DocxBuilder::strict()
                .body("<w:p><w:r><w:t>x</w:t></w:r></w:p>")
                .build();
            StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
                .expect("open")
                .package()
                .conformance()
        });
        assert_eq!(conformance, Conformance::Strict);
    }

    #[test]
    fn a_deflated_strict_package_opens() {
        assert_survives("open deflated", || {
            let bytes = DocxBuilder::strict().body("<w:p/>").deflated().build();
            StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
        });
    }

    #[test]
    fn a_transitional_package_is_rejected_under_strict_only() {
        assert_survives("reject transitional", || {
            open(
                DocxBuilder::transitional().body("<w:p/>").build(),
                &OpenOptions::default(),
            );
        });
    }
}

mod xml {
    //! AUD-04: truncated parts, content after the root, no root.
}

mod nesting {
    //! AUD-05, AUD-07: nested blocks against `max_block_nesting`.
}

mod math {
    //! AUD-06: formulas over `max_math_nodes` / `max_math_depth`.
}

mod table {
    //! AUD-08, AUD-09: rows wider than `tblGrid`, overflowing grid sums.
}

mod numbering {
    //! AUD-09, AUD-47: counters at `u32::MAX`, `numStyleLink` cycles.
}

mod writer {
    //! AUD-10, AUD-11: pass-through byte scanning, ZIP field widths.
}

mod opc {
    //! AUD-20, AUD-22, AUD-24, AUD-25: relationship types, part names, `.rels`
    //! outside `_rels/`.
}

mod render {
    //! AUD-71, AUD-72: non-finite geometry, output amplification.
}
