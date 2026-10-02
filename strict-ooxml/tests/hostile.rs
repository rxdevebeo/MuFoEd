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

    use super::*;
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_testkit::docx::Family;

    const W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

    /// A `word/document.xml` carrying exactly `xml`.
    fn document_part(xml: &[u8]) -> Vec<u8> {
        DocxBuilder::strict()
            .part("word/document.xml", xml.to_vec())
            .build()
    }

    /// Opens a package, requiring it to be rejected, and returns the message.
    fn rejects(package: Vec<u8>) -> String {
        let error = assert_survives("open", || open(package, &OpenOptions::default()));
        match error {
            strict_ooxml::StrictError::InvalidXml { detail, .. } => detail,
            other => panic!("expected InvalidXml, got {other:?}"),
        }
    }

    #[test]
    fn a_document_truncated_inside_an_element_is_an_error() {
        let detail = rejects(document_part(
            br#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p>"#,
        ));
        assert!(detail.contains("unclosed element"), "{detail}");
    }

    #[test]
    fn a_document_with_two_roots_is_an_error() {
        let detail = rejects(document_part(
            br#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"/><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"/>"#,
        ));
        assert!(detail.contains("content after the root"), "{detail}");
    }

    #[test]
    fn a_part_with_no_root_is_an_error() {
        let detail = rejects(document_part(
            b"<?xml version=\"1.0\"?><!-- nothing here -->",
        ));
        assert!(detail.contains("no root element"), "{detail}");
    }

    #[test]
    fn an_empty_part_is_an_error() {
        let detail = rejects(document_part(b""));
        assert!(detail.contains("no root element"), "{detail}");
    }

    /// Every optional part a reader reads, cut short by exactly its closing tag.
    ///
    /// Before AUD-04 three of these loops matched `Eof` as "nothing more" and
    /// spun forever on a part that ended early; the ten-second limit is what
    /// tells "rejected" from "hung".
    #[test]
    fn every_truncated_part_is_rejected_promptly() {
        let cases: [(&str, &str, &str, &str); 7] = [
            (
                "word/fontTable.xml",
                "fontTable",
                "w:fonts",
                "<w:font w:name=\"A\"/>",
            ),
            (
                "word/settings.xml",
                "settings",
                "w:settings",
                "<m:mathPr><m:mathFont m:val=\"Cambria Math\"/>",
            ),
            (
                "word/styles.xml",
                "styles",
                "w:styles",
                "<w:style w:type=\"paragraph\"/>",
            ),
            (
                "word/numbering.xml",
                "numbering",
                "w:numbering",
                "<w:num w:numId=\"1\"/>",
            ),
            (
                "word/document.xml",
                "officeDocument",
                "w:document",
                "<w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body>",
            ),
            (
                "word/footnotes.xml",
                "footnotes",
                "w:footnotes",
                "<w:footnote w:id=\"1\"><w:p>",
            ),
            (
                "word/header1.xml",
                "header",
                "w:hdr",
                "<w:p><w:r><w:t>h</w:t></w:r></w:p>",
            ),
        ];
        for (name, rel_type, root, inner) in cases {
            let family = Family::Strict;
            let full = strict_ooxml_testkit::docx::part_xml(family, root, inner);
            let text = String::from_utf8(full).expect("utf-8");
            let closing = format!("</{root}>");
            let cut = text
                .strip_suffix(closing.as_str())
                .unwrap_or_else(|| panic!("{name}: the fixture does not end with {closing}"))
                .as_bytes()
                .to_vec();
            assert!(
                String::from_utf8_lossy(&cut).contains("<w"),
                "{name}: nothing was cut"
            );

            let file = name.rsplit('/').next().expect("a file name");
            let mut hostile = DocxBuilder::strict().part(name, cut);
            if name != "word/document.xml" {
                // Every other part is reached through a relationship, and the
                // header only through a `w:headerReference` - a relationship
                // alone does not make a reader open it.
                hostile = hostile
                    .rel("rIdPart", &family.rel_type(rel_type), file)
                    .content_type(
                        &format!("/{name}"),
                        "application/vnd.openxmlformats-officedocument.wordprocessingml.part+xml",
                    );
            }
            if name == "word/header1.xml" {
                hostile = hostile.body(
                    "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdPart\"/></w:sectPr>",
                );
            }
            let error = assert_survives("open truncated part", move || {
                open(hostile.build(), &OpenOptions::default())
            });
            assert!(
                matches!(error, strict_ooxml::StrictError::InvalidXml { .. }),
                "{name}: {error:?}"
            );
        }
    }

    #[test]
    fn the_main_document_truncated_inside_a_run_is_an_error() {
        let detail = rejects(
            DocxBuilder::strict()
                .document_bytes(format!(
                    "<w:document xmlns:w=\"{W}\"><w:body><w:p><w:r><w:t>text"
                ))
                .build(),
        );
        assert!(detail.contains("unclosed element"), "{detail}");
    }

    #[test]
    fn a_well_formed_short_part_is_still_read() {
        // The guard is on truncation, not on size: a small complete part is a
        // normal document, and the fix must not have become "reject short parts".
        let bytes = DocxBuilder::strict()
            .part_xml(
                "word/settings.xml",
                "w:settings",
                "<w:zoom w:percent=\"100\"/>",
            )
            .rel(
                "rIdSettings",
                &Family::Strict.rel_type("settings"),
                "settings.xml",
            )
            .build();
        let opened = assert_survives("open short settings", || {
            StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open")
        });
        let settings = PartId::new("/word/settings.xml");
        assert!(
            opened.package().part(&settings).is_some(),
            "the part is in the package"
        );
    }
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
