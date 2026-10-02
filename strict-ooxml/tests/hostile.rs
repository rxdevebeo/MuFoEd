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

mod escape {
    //! AUD-03: characters XML 1.0 cannot carry, in every serializer.

    use super::*;
    use strict_ooxml::model::block::Block;
    use strict_ooxml::model::inline::{Inline, RunContent};

    fn well_formed(xml: &str) -> Result<(), String> {
        roxmltree::Document::parse(xml)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[cfg(feature = "svg")]
    #[test]
    fn a_control_character_symbol_renders_to_well_formed_svg_with_a_warning() {
        let pages = assert_survives("render sym 0001", || {
            let bytes = DocxBuilder::strict()
                .body("<w:p><w:r><w:t>a</w:t><w:sym w:font=\"Symbol\" w:char=\"0001\"/><w:t>b</w:t></w:r></w:p>")
                .build();
            StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
                .expect("open")
                .render_svg(&strict_ooxml::RenderOptions::default())
                .expect("render")
        });
        well_formed(&pages[0].svg).expect("the page is XML");
        assert!(
            pages[0]
                .warnings
                .iter()
                .any(|w| w.starts_with("render.invalid-xml-char")),
            "{:?}",
            pages[0].warnings
        );
    }

    #[cfg(feature = "write")]
    #[test]
    fn a_control_character_in_the_model_is_written_as_well_formed_xml_and_reported() {
        let (document_xml, reported) = assert_survives("write U+0001", || {
            let bytes = DocxBuilder::strict()
                .body("<w:p><w:r><w:t>placeholder</w:t></w:r></w:p>")
                .build();
            let mut opened =
                StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())
                    .expect("open");
            let Block::Paragraph(paragraph) = &mut opened.document_mut().body.blocks[0] else {
                panic!("paragraph");
            };
            let Inline::Run(run) = &mut paragraph.inlines[0] else {
                panic!("run");
            };
            let RunContent::Text(text) = &mut run.content[0] else {
                panic!("text");
            };
            text.text = "a\u{1}b\u{FFFF}c".to_owned();

            let written = strict_ooxml::write_package(
                opened.document(),
                Some(opened.package()),
                &strict_ooxml::WriteOptions::default(),
            )
            .expect("write");
            let reported: Vec<String> = written
                .report
                .losses()
                .into_iter()
                .filter(|loss| loss.feature_id == "W.invalid-xml-char")
                .map(|loss| loss.locations[0].part.to_string())
                .collect();
            let package = strict_ooxml_core::opc::Package::open_reader(
                Cursor::new(written.bytes),
                &OpenOptions::default(),
            )
            .expect("reopen");
            let xml = package
                .read_part(&strict_ooxml_core::part::PartId::new("/word/document.xml"))
                .expect("document part");
            (String::from_utf8(xml).expect("utf-8"), reported)
        });
        well_formed(&document_xml).expect("the written document part is XML");
        assert!(document_xml.contains(">abc<"), "{document_xml}");
        assert_eq!(reported, ["/word/document.xml"]);
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
