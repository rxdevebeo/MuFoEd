//! The extension tests below are about *our own* output, whose part names this
//! crate chose, so a case-insensitive comparison would test the wrong thing.
#![allow(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::default_trait_access
)]

//! Round-trip and package-level acceptance for the writer (`STAGE-8-TASK.md`
//! SC-1, SC-3, SC-10).
//!
//! The oracle is deliberately **not** our own parser: `roxmltree` re-reads every
//! part we write, and a second parse through the Strict reader checks that the
//! model survives the trip. A writer that is self-consistent but wrong — a
//! Transitional namespace, a broken relationship — passes a self-check and fails
//! here, which is the lesson the project already learned in D-1/D-2/C-3.

use std::collections::BTreeMap;

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::ns::detect::{detect_conformance, ConformanceSignals};
use strict_ooxml_core::opc::rels::parse_relationships;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package, CONTENT_TYPES_PART};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::{DirectionalKind, DirectionalVal, Inline};
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{
    dropped_count, package::NoSource, verify_no_silent_loss, write_package, WriteOptions,
};

/// The Strict documents the round trip runs on.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    for (name, file) in [
        ("strict-profile", "strict-profile.docx"),
        ("strict-math-simple", "05-strict-math-simple.docx"),
        ("strict-drawingml", "07-strict-drawingml-shapes.docx"),
    ] {
        if let Ok(bytes) = std::fs::read(root.join(file)) {
            out.push((name, bytes));
        }
    }
    assert!(!out.is_empty(), "no Strict corpus under {}", root.display());
    out
}

fn open(bytes: &[u8]) -> Result<Package, StrictError> {
    Package::open_reader(bytes, &OpenOptions::default())
}

fn parse(package: &Package) -> Result<Document, StrictError> {
    parse_document(package, &ParseOptions::default())
}

fn write(document: &Document, package: &Package) -> strict_ooxml_write::WriteOutput {
    write_package(document, Some(package), &WriteOptions::default()).expect("write")
}

/// SC-3: a written package re-opens, re-parses, and yields the same model.
#[test]
fn round_trip_preserves_the_model() {
    let mut checked = 0;
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).unwrap_or_else(|error| panic!("{name}: parse: {error}"));
        let written = write(&document, &package);

        let reopened = open(&written.bytes)
            .unwrap_or_else(|error| panic!("{name}: reopen: {error}\nreport: {}", written.report));
        let reparsed = parse(&reopened)
            .unwrap_or_else(|error| panic!("{name}: reparse: {error}\nreport: {}", written.report));

        // The written form must be a fixed point: writing the re-parsed document
        // again produces the same bytes. Comparing the DOMs directly would
        // compare SourceLocations, which name a position in a file and cannot
        // survive a rewrite by construction.
        let rewritten = write(&reparsed, &reopened);
        assert_eq!(
            written.bytes, rewritten.bytes,
            "{name}: the written document is not a fixed point\nfirst:\n{}\nsecond:\n{}",
            written.report, rewritten.report
        );
        assert_eq!(
            document.styles.len(),
            reparsed.styles.len(),
            "{name}: style count changed\n{}",
            written.report
        );
        assert_eq!(
            document.numbering.len(),
            reparsed.numbering.len(),
            "{name}: numbering count changed\n{}",
            written.report
        );
        assert_eq!(
            document.footnotes.len(),
            reparsed.footnotes.len(),
            "{name}: footnote count changed\n{}",
            written.report
        );
        assert_eq!(
            document.media.len(),
            reparsed.media.len(),
            "{name}: media count changed\n{}",
            written.report
        );
        checked += 1;
    }
    assert!(checked >= 3, "expected at least 3 documents, got {checked}");
}

/// SC-3: the written package is Strict, by an independent namespace check.
#[test]
fn a_written_package_is_strict() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        let reopened = open(&written.bytes)
            .unwrap_or_else(|error| panic!("{name}: reopen: {error}\n{}", written.report));

        let mut namespaces: Vec<String> = Vec::new();
        let mut relationship_types: Vec<String> = Vec::new();
        for part in reopened.parts() {
            let id = part.id.as_str();
            if !id.ends_with(".xml") || id == CONTENT_TYPES_PART {
                continue;
            }
            let bytes = reopened.read_part(&part.id).expect("part");
            if let Ok(text) = std::str::from_utf8(&bytes) {
                for uri in uris_in(text) {
                    namespaces.push(uri);
                }
            }
        }
        for relationship in reopened.relationships(&PartId::new("/")) {
            relationship_types.push(relationship.raw_type.clone());
        }
        let main = reopened.main_document_part().expect("main").clone();
        for relationship in reopened.relationships(&main) {
            relationship_types.push(relationship.raw_type.clone());
        }

        let signals = ConformanceSignals {
            namespaces: namespaces.iter().map(String::as_str).collect(),
            relationship_types: relationship_types.iter().map(String::as_str).collect(),
        };
        let conformance = detect_conformance(&signals).expect("conformance");
        assert_eq!(
            conformance,
            strict_ooxml_core::ns::Conformance::Strict,
            "{name}: a written package must be Strict; namespaces={namespaces:?} types={relationship_types:?}"
        );
    }
}

/// Every xmlns declaration in `text`, without an XML parser.
fn uris_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("xmlns") {
        rest = &rest[start + 5..];
        let quote = rest.find('"').unwrap_or(rest.len());
        let tail = &rest[quote + 1..];
        let end = tail.find('"').unwrap_or(tail.len());
        out.push(tail[..end].to_owned());
        rest = tail;
    }
    out
}

/// SC-1: two writes of the same document produce the same bytes.
#[test]
fn writing_is_reproducible() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let first = write(&document, &package);
        let second = write(&document, &package);
        assert_eq!(
            first.bytes, second.bytes,
            "{name}: output is not reproducible"
        );
        assert_eq!(
            first.part_count, second.part_count,
            "{name}: part count is not reproducible"
        );
    }
}

/// SC-10: nothing is dropped without a report entry.
#[test]
fn no_construct_is_dropped_silently() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        assert_eq!(
            verify_no_silent_loss(&written.report),
            Ok(()),
            "{name}: {}",
            written.report
        );
        // The report must also be reproducible, or SC-1 is only half true.
        let again = write(&document, &package);
        assert_eq!(
            format!("{}", written.report),
            format!("{}", again.report),
            "{name}: the report is not reproducible"
        );
    }
}

/// Every written part parses with an independent XML parser.
#[test]
fn every_written_part_is_well_formed_xml() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        let reopened = open(&written.bytes)
            .unwrap_or_else(|error| panic!("{name}: reopen: {error}\n{}", written.report));
        for part in reopened.parts() {
            let id = part.id.as_str();
            if !(id.ends_with(".xml") || id.ends_with(".rels")) {
                continue;
            }
            let bytes = reopened.read_part(&part.id).expect("part bytes");
            roxmltree::Document::parse(std::str::from_utf8(&bytes).expect("utf-8"))
                .unwrap_or_else(|error| panic!("{name}: {id} is not well formed: {error}"));
        }
    }
}

/// Every relationship a part declares points at a part that exists.
#[test]
fn every_declared_relationship_resolves() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        let reopened = open(&written.bytes)
            .unwrap_or_else(|error| panic!("{name}: reopen: {error}\n{}", written.report));
        for part in reopened.parts() {
            if !part.id.as_str().ends_with(".rels") {
                continue;
            }
            let Some(source) = strict_ooxml_core::opc::rels::source_part_for_rels(&part.id) else {
                // AUD-25: a `.rels` entry outside `_rels/` is an ordinary part.
                continue;
            };
            let bytes = reopened.read_part(&part.id).expect("rels bytes");
            let relationships = parse_relationships(bytes, &source, &ResourceLimits::default())
                .unwrap_or_else(|error| panic!("{name}: {} does not parse: {error}", part.id));
            for relationship in relationships {
                let Some(target) = relationship.resolved else {
                    continue;
                };
                assert!(
                    reopened.part(&target).is_some(),
                    "{name}: {} declares {} -> {} which is missing",
                    part.id,
                    relationship.id,
                    target
                );
            }
        }
    }
}

/// Every part has a content type, and every override names a part that exists.
#[test]
fn content_types_cover_every_part() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        let reopened = open(&written.bytes)
            .unwrap_or_else(|error| panic!("{name}: reopen: {error}\n{}", written.report));
        for part in reopened.parts() {
            let id = part.id.as_str();
            if id == CONTENT_TYPES_PART {
                continue;
            }
            assert!(
                reopened.content_type(&part.id).is_some(),
                "{name}: no content type for {id}"
            );
        }
    }
}

/// A document with media keeps its image bytes, and reports nothing lost.
#[test]
fn media_bytes_survive() {
    let Some((name, bytes)) = corpus()
        .into_iter()
        .find(|(name, _)| name.contains("drawingml"))
    else {
        return;
    };
    let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
    let document = parse(&package).expect("parse");
    if document.media.is_empty() {
        return;
    }
    let written = write(&document, &package);
    let reopened = open(&written.bytes)
        .unwrap_or_else(|error| panic!("{name}: reopen: {error}\n{}", written.report));
    for item in document.media.iter() {
        let original = package.read_part(&item.part).expect("original media");
        let mut found = None;
        for candidate in reopened.parts() {
            if reopened.read_part(&candidate.id).ok().as_deref() == Some(original.as_slice()) {
                found = Some(candidate.id.clone());
                break;
            }
        }
        assert!(found.is_some(), "{name}: media {} was lost", item.part);
    }
}

/// A package written with no source still parses, and reports what it could
/// not resolve instead of failing.
#[test]
fn writing_without_a_source_reports_instead_of_failing() {
    let Some((name, bytes)) = corpus().into_iter().next() else {
        return;
    };
    let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
    let document = parse(&package).expect("parse");
    // Media needs the source; a document without media needs none.
    let written = write_package(&document, Some(&NoSource), &WriteOptions::default());
    if document.media.is_empty() {
        let written = written.unwrap_or_else(|error| panic!("{name}: {error}"));
        let reopened = open(&written.bytes).expect("reopen");
        assert!(parse(&reopened).is_ok(), "{name}: reparse failed");
        assert_eq!(verify_no_silent_loss(&written.report), Ok(()), "{name}");
    } else {
        // A missing image is a hard error: writing a package whose media part is
        // absent would produce a file Word reports as corrupt.
        assert!(written.is_err(), "{name}: expected an error, got a package");
    }
}

/// The writer never panics on a degenerate but valid model.
#[test]
fn an_empty_document_still_produces_a_valid_package() {
    let document = Document {
        body: Default::default(),
        styles: Default::default(),
        numbering: Default::default(),
        footnotes: Default::default(),
        endnotes: Default::default(),
        settings: Default::default(),
        font_table: None,
        theme: None,
        sections: Vec::new(),
        headers_footers: Vec::new(),
        media: Default::default(),
        support: Default::default(),
        source: strict_ooxml_wml::model::document::DocumentSource {
            main_document: PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            footnotes: None,
            endnotes: None,
            theme: None,
            font_table: None,
        },
    };
    let written = write_package(&document, Some(&NoSource), &WriteOptions::default())
        .expect("write an empty document");
    let reopened =
        open(&written.bytes).unwrap_or_else(|error| panic!("reopen: {error}\n{}", written.report));
    let reparsed = parse(&reopened).expect("parse an empty document");
    assert!(reparsed.body.blocks.is_empty());
    assert_eq!(verify_no_silent_loss(&written.report), Ok(()));
    assert_eq!(dropped_count(&written.report), 0);
}

/// `ConformancePolicy` still guards the reader after a write: a written package
/// opens under the strict policy, which is the property a caller depends on.
#[test]
fn a_written_package_opens_under_the_strict_policy() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        let options = OpenOptions {
            conformance: ConformancePolicy::StrictOnly,
            ..OpenOptions::default()
        };
        let reopened = Package::open_reader(&written.bytes[..], &options)
            .unwrap_or_else(|error| panic!("{name}: strict reopen: {error}\n{}", written.report));
        assert_eq!(
            reopened.conformance(),
            strict_ooxml_core::ns::Conformance::Strict
        );
    }
}

/// AUD-42: `w:dir` / `w:bdo` round-trip; `w:customXml` records an Ignorable write loss.
#[test]
fn directional_round_trips_and_custom_xml_is_reported() {
    let body = "\
<w:p><w:dir w:val=\"rtl\"><w:r><w:t>rtl</w:t></w:r></w:dir>\
<w:bdo w:val=\"ltr\"><w:r><w:t>ltr</w:t></w:r></w:bdo></w:p>\
<w:customXml><w:p><w:r><w:t>kept</w:t></w:r></w:p></w:customXml>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let paragraph = document.body.blocks[0].as_paragraph().unwrap();
    assert!(matches!(
        &paragraph.inlines[0],
        Inline::Directional(d)
            if d.kind == DirectionalKind::Dir && d.val == DirectionalVal::Rtl
    ));
    assert!(document.support.get("w:customXml").is_some());

    let written = write(&document, &package);
    assert!(
        written
            .report
            .losses()
            .iter()
            .any(|loss| loss.feature_id == "W.custom-xml-wrapper"),
        "expected W.custom-xml-wrapper loss, got {}",
        written.report
    );
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");
    let paragraph = reparsed.body.blocks[0].as_paragraph().unwrap();
    assert!(matches!(
        &paragraph.inlines[0],
        Inline::Directional(d) if d.kind == DirectionalKind::Dir
    ));
    assert!(matches!(
        &paragraph.inlines[1],
        Inline::Directional(d) if d.kind == DirectionalKind::Bdo
    ));
    // customXml content survives as a plain paragraph
    assert!(matches!(reparsed.body.blocks[1], Block::Paragraph(_)));
}

/// AUD-41: row-level `w:sdt` round-trips (parse → write → parse → write is a fixed point).
#[test]
fn row_level_sdt_round_trips() {
    let body = "\
<w:tbl><w:tblGrid><w:gridCol w:w=\"1440\"/></w:tblGrid>\
<w:sdt><w:sdtPr><w:tag w:val=\"repeat\"/><w:alias w:val=\"Items\"/></w:sdtPr>\
<w:sdtContent>\
<w:tr><w:tc><w:p><w:r><w:t>one</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>three</w:t></w:r></w:p></w:tc></w:tr>\
</w:sdtContent></w:sdt></w:tbl>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows.iter().all(|row| {
        row.sdt
            .as_ref()
            .is_some_and(|sdt| sdt.tag.as_deref() == Some("repeat"))
    }));

    let written = write(&document, &package);
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");
    let Block::Table(table2) = &reparsed.body.blocks[0] else {
        panic!("expected table after round trip");
    };
    assert_eq!(table2.rows.len(), 3);
    assert!(table2.rows.iter().all(|row| {
        row.sdt
            .as_ref()
            .is_some_and(|sdt| sdt.tag.as_deref() == Some("repeat"))
    }));
    let rewritten = write(&reparsed, &reopened);
    assert_eq!(
        written.bytes, rewritten.bytes,
        "row-level sdt write is not a fixed point\n{}",
        written.report
    );
}

/// The `document.xml.rels` ids the document uses are the ones that exist.
#[test]
fn document_relationship_ids_exist() {
    for (name, bytes) in corpus() {
        let package = open(&bytes).unwrap_or_else(|error| panic!("{name}: open: {error}"));
        let document = parse(&package).expect("parse");
        let written = write(&document, &package);
        let reopened = open(&written.bytes)
            .unwrap_or_else(|error| panic!("{name}: reopen: {error}\n{}", written.report));
        let main = reopened.main_document_part().expect("main").clone();
        let declared: BTreeMap<String, String> = reopened
            .relationships(&main)
            .iter()
            .map(|relationship| (relationship.id.clone(), relationship.target.clone()))
            .collect();
        let document_xml = reopened
            .read_part(&main)
            .ok()
            .and_then(|bytes| std::str::from_utf8(&bytes).ok().map(str::to_owned))
            .unwrap_or_default();
        for attributes in roxmltree::Document::parse(&document_xml)
            .expect("well formed")
            .root_element()
            .descendants()
            .flat_map(|node| node.attributes())
            .filter(|attribute| {
                attribute.namespace()
                    == Some("http://purl.oclc.org/ooxml/officeDocument/relationships")
            })
        {
            assert!(
                declared.contains_key(attributes.value()),
                "{name}: r:id {} is not declared",
                attributes.value()
            );
        }
    }
}
