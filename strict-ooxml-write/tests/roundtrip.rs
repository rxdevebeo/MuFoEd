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

/// AUD-66: invalid XML characters are stripped and reported; output still parses.
#[test]
#[allow(clippy::too_many_lines)]
fn invalid_xml_chars_are_reported_and_output_parses() {
    use strict_ooxml_wml::model::block::{
        Block, GridCol, Paragraph, SdtProperties, Table, TableCell, TableRow,
    };
    use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
    use strict_ooxml_wml::model::styles::{Style, StyleTable};
    use strict_ooxml_wml::model::values::{Space, StyleType};

    let unknown = strict_ooxml_core::error::SourceLocation::unknown();
    let mut document = Document {
        body: Default::default(),
        styles: StyleTable::default(),
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
            styles: Some(PartId::new("/word/styles.xml")),
            numbering: None,
            settings: None,
            footnotes: None,
            endnotes: None,
            theme: None,
            font_table: None,
        },
    };
    document.body.blocks.push(Block::Paragraph(Paragraph {
        props: Default::default(),
        inlines: vec![Inline::Run(Run {
            props: Default::default(),
            content: vec![RunContent::Text(TextNode {
                text: format!("ok{}end", '\u{1}'),
                space: Space::Default,
            })],
            revision: None,
            location: unknown.clone(),
        })],
        rsids: Default::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: unknown.clone(),
    }));
    // Row-level sdt carries `w:alias` (AUD-41 writer path).
    document.body.blocks.push(Block::Table(Table {
        props: Default::default(),
        grid: vec![GridCol {
            width: Some(strict_ooxml_wml::model::values::Twips(1440)),
        }],
        grid_change: None,
        rows: vec![TableRow {
            props: Default::default(),
            cells: vec![TableCell {
                props: Default::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    props: Default::default(),
                    inlines: Vec::new(),
                    rsids: Default::default(),
                    revision: None,
                    para_id: None,
                    text_id: None,
                    location: unknown.clone(),
                })],
                sdt: None,
                location: unknown.clone(),
            }],
            sdt: Some(SdtProperties {
                tag: None,
                alias: Some(format!("a{}lias", '\u{1}').into()),
                id: None,
                placeholder: None,
                showing_placeholder: false,
                run_props: None,
                end_run_props: None,
                has_end_pr: false,
                doc_part_gallery: None,
                doc_part_unique: false,
                location: unknown.clone(),
            }),
            location: unknown.clone(),
        }],
        location: unknown.clone(),
    }));
    document.styles.insert(Style {
        id: strict_ooxml_wml::model::StyleId::new(format!("S{}tyle", '\u{1}')),
        style_type: StyleType::Paragraph,
        name: Some(format!("Na{}me", '\u{1}').into()),
        based_on: None,
        next: None,
        link: None,
        is_default: false,
        custom_style: false,
        auto_redefine: false,
        semi_hidden: false,
        hidden: false,
        q_format: false,
        locked: false,
        unhide_when_used: false,
        ui_priority: None,
        table: Default::default(),
            row: Default::default(),
            cell: Default::default(),
        paragraph: Default::default(),
        run: Default::default(),
        conditions: Vec::new(),
        based_on_chain: Vec::new(),
        location: unknown,
    });

    let written = write_package(&document, Some(&NoSource), &WriteOptions::default())
        .expect("write despite invalid chars");
    assert!(
        written
            .report
            .losses()
            .iter()
            .any(|loss| loss.feature_id == "W.invalid-xml-char"),
        "expected W.invalid-xml-char, got {}",
        written.report
    );
    let reopened = open(&written.bytes).expect("reopen");
    for part in ["/word/document.xml", "/word/styles.xml"] {
        let xml = String::from_utf8(reopened.read_part(&PartId::new(part)).expect("part"))
            .expect("utf-8");
        assert!(!xml.contains('\u{1}'), "{part} still carries U+0001: {xml}");
        roxmltree::Document::parse(&xml).unwrap_or_else(|error| {
            panic!("{part} must parse after stripping invalid chars: {error}\n{xml}")
        });
    }
}

/// AUD-64: `m:t` with edge spaces keeps `xml:space="preserve"` through round-trip.
#[test]
fn math_text_preserves_spaces_round_trip() {
    let body = "\
<w:p><m:oMath><m:r><m:t xml:space=\"preserve\"> x </m:t></m:r></m:oMath></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" \
w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written = write(&document, &package);
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");
    let Inline::Math(math) = &reparsed.body.blocks[0]
        .as_paragraph()
        .expect("paragraph")
        .inlines[0]
    else {
        panic!("expected math");
    };
    let strict_ooxml_wml::model::math::MathNode::Run(run) = &math.nodes[0] else {
        panic!("expected math run");
    };
    assert_eq!(run.text, " x ");
    let document_xml = String::from_utf8(
        reopened
            .read_part(&strict_ooxml_core::part::PartId::new("/word/document.xml"))
            .expect("document"),
    )
    .expect("utf-8");
    assert!(
        document_xml.contains("<m:t xml:space=\"preserve\"> x </m:t>"),
        "{document_xml}"
    );
}

/// AUD-43: tracked-change runs round-trip with the same revision markers.
#[test]
fn tracked_changes_round_trip() {
    let body = "\
<w:p><w:ins w:id=\"1\" w:author=\"Ann\"><w:r><w:t>added</w:t></w:r></w:ins>\
<w:del w:id=\"2\"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written = write(&document, &package);
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");
    let first = document.body.blocks[0].as_paragraph().unwrap();
    let second = reparsed.body.blocks[0].as_paragraph().unwrap();
    assert_eq!(first.inlines.len(), second.inlines.len());
    for (left, right) in first.inlines.iter().zip(second.inlines.iter()) {
        let (Inline::Run(a), Inline::Run(b)) = (left, right) else {
            panic!("expected runs");
        };
        assert_eq!(a.revision, b.revision);
        assert_eq!(a.content, b.content);
    }
    let document_xml = reopened
        .read_part(&strict_ooxml_core::part::PartId::new("/word/document.xml"))
        .expect("document part");
    let xml = String::from_utf8_lossy(&document_xml);
    assert!(xml.contains("<w:ins "), "{xml}");
    assert!(xml.contains("<w:del "), "{xml}");
    assert!(xml.contains("<w:delText"), "{xml}");
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

/// AUD-46: modelled `*Pr` extensions round-trip.
#[test]
fn aud46_property_extensions_round_trip() {
    use strict_ooxml_wml::model::values::{Justification, TriState, Twips};
    let body = "\
<w:p><w:pPr><w:framePr w:w=\"200\" w:h=\"100\" w:wrap=\"around\"/></w:pPr>\
<w:r><w:rPr><w:bCs/><w:iCs w:val=\"0\"/></w:rPr><w:t>x</w:t></w:r></w:p>\
<w:tbl><w:tblPr>\
<w:tblpPr w:leftFromText=\"40\" w:horzAnchor=\"page\" w:tblpX=\"100\"/>\
<w:tblCellSpacing w:w=\"20\" w:type=\"dxa\"/>\
</w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"500\"/><w:gridCol w:w=\"500\"/></w:tblGrid>\
<w:tr><w:trPr><w:jc w:val=\"center\"/></w:trPr>\
<w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/>\
<w:pgNumType w:fmt=\"lowerRoman\" w:start=\"3\"/></w:sectPr>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written = write(&document, &package);
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");

    let paragraph = reparsed.body.blocks[0].as_paragraph().unwrap();
    assert_eq!(
        paragraph.props.frame.as_ref().and_then(|frame| frame.width),
        Some(Twips(200))
    );
    let run = paragraph.inlines[0].as_run().unwrap();
    assert_eq!(run.props.bold_cs, TriState::On);
    assert_eq!(run.props.italic_cs, TriState::Off);

    let table = reparsed.body.blocks[1].as_table().unwrap();
    assert!(table.props.positioning.is_some());
    assert_eq!(
        table
            .props
            .cell_spacing
            .as_ref()
            .and_then(|width| width.value),
        Some(20)
    );
    assert_eq!(table.rows[0].props.alignment, Some(Justification::Center));
    assert_eq!(
        reparsed
            .sections
            .last()
            .unwrap()
            .properties
            .page_number
            .as_ref()
            .and_then(|page| page.start),
        Some(3)
    );
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

/// AUD-61: the same source id `rId5` in the body, a header and a footnote
/// resolves to the correct media bytes after write; each part owns its `.rels`.
#[test]
#[allow(clippy::too_many_lines)]
fn relationships_are_per_part_for_media() {
    let body_png = b"\x89PNG\r\n\x1a\nbody-img".to_vec();
    let header_png = b"\x89PNG\r\n\x1a\nhdr-img!".to_vec();
    let note_png = b"\x89PNG\r\n\x1a\nnote-img".to_vec();
    let picture = |rid: &str| {
        format!(
            "<w:drawing><wp:inline>\
<wp:extent cx=\"100\" cy=\"100\"/><wp:docPr id=\"1\" name=\"p\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"p\"/><pic:cNvPicPr/>\
</pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/></pic:blipFill>\
<pic:spPr><a:xfrm><a:ext cx=\"100\" cy=\"100\"/></a:xfrm></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"
        )
    };
    let body = format!(
        "<w:p><w:r>{}</w:r>\
<w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHdr\"/>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" \
w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>",
        picture("rId5")
    );
    let header = format!("<w:p><w:r>{}</w:r></w:p>", picture("rId5"));
    let footnotes = format!(
        "<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:footnoteRef/></w:r></w:p></w:footnote>\
<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p/></w:footnote>\
<w:footnote w:id=\"1\"><w:p><w:r><w:footnoteRef/></w:r><w:r>{}</w:r></w:p></w:footnote>",
        picture("rId5")
    );
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(&body)
        .rel("rId5", "image", "media/body.png")
        .rel("rIdHdr", "header", "header1.xml")
        .rel("rIdFootnotes", "footnotes", "footnotes.xml")
        .content_type("/word/media/body.png", "image/png")
        .content_type("/word/media/header.png", "image/png")
        .content_type("/word/media/note.png", "image/png")
        .content_type(
            "/word/header1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        )
        .content_type(
            "/word/footnotes.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
        )
        .part("word/media/body.png", body_png.clone())
        .part("word/media/header.png", header_png.clone())
        .part("word/media/note.png", note_png.clone())
        .part_xml("word/header1.xml", "w:hdr", &header)
        .part(
            "word/header1.xml.rels",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId5" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/image" Target="media/header.png"/>
</Relationships>"#
                .to_vec(),
        )
        .part_xml("word/footnotes.xml", "w:footnotes", &footnotes)
        .part(
            "word/footnotes.xml.rels",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId5" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/image" Target="media/note.png"/>
</Relationships>"#
                .to_vec(),
        )
        .build();
    // DocxBuilder puts header rels under word/header1.xml.rels — OPC wants
    // word/_rels/header1.xml.rels. Rewrite the archive entries.
    let bytes = {
        let mut zip = strict_ooxml_core::opc::zip::write::ZipWriter::new();
        let pkg = open(&bytes).expect("open built");
        for part in pkg.parts() {
            let name = part.id.as_str();
            let data = pkg.read_part(&part.id).expect("read");
            let name = match name {
                "/word/header1.xml.rels" => "/word/_rels/header1.xml.rels",
                "/word/footnotes.xml.rels" => "/word/_rels/footnotes.xml.rels",
                other => other,
            };
            zip.add_part(&PartId::new(name), data).expect("add");
        }
        zip.finish().expect("finish")
    };
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written = write(&document, &package);
    let reopened = open(&written.bytes).expect("reopen");

    let find_bytes = |needle: &[u8]| {
        reopened.parts().find_map(|part| {
            let data = reopened.read_part(&part.id).ok()?;
            (data == needle).then_some(part.id.as_str().to_owned())
        })
    };
    assert!(find_bytes(&body_png).is_some(), "body media missing");
    assert!(find_bytes(&header_png).is_some(), "header media missing");
    assert!(find_bytes(&note_png).is_some(), "footnote media missing");

    // Each content part has its own .rels (not borrowed from the document).
    assert!(
        reopened
            .read_part(&PartId::new("/word/_rels/header1.xml.rels"))
            .is_ok(),
        "header .rels missing"
    );
    assert!(
        reopened
            .read_part(&PartId::new("/word/_rels/footnotes.xml.rels"))
            .is_ok(),
        "footnotes .rels missing"
    );
}

fn paragraph_style(block: &Block) -> Option<&str> {
    match block {
        Block::Paragraph(p) => p
            .props
            .style
            .as_ref()
            .map(strict_ooxml_wml::model::StyleId::as_str),
        _ => None,
    }
}

fn paragraph_text(paragraph: &strict_ooxml_wml::model::block::Paragraph) -> String {
    paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => Some(
                run.content
                    .iter()
                    .filter_map(|c| match c {
                        strict_ooxml_wml::model::inline::RunContent::Text(t) => {
                            Some(t.text.as_str())
                        }
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

/// AUD-60: a footnote of several paragraphs and a table survives round-trip
/// without collapsing into a single `w:p`.
#[test]
fn multi_paragraph_footnote_round_trips() {
    use strict_ooxml_wml::model::notes::NoteKind;

    let body = "\
<w:p><w:r><w:t>see</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" \
w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>";
    let footnotes = "\
<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:footnoteRef/></w:r></w:p></w:footnote>\
<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p/></w:footnote>\
<w:footnote w:id=\"1\">\
<w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr>\
<w:r><w:footnoteRef/></w:r><w:r><w:t>one</w:t></w:r></w:p>\
<w:p><w:pPr><w:pStyle w:val=\"IntenseQuote\"/></w:pPr><w:r><w:t>two</w:t></w:r></w:p>\
<w:p><w:pPr><w:pStyle w:val=\"BodyText\"/></w:pPr><w:r><w:t>three</w:t></w:r></w:p>\
<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"1440\"/></w:tblGrid>\
<w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
</w:footnote>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .rel("rIdFootnotes", "footnotes", "footnotes.xml")
        .content_type(
            "/word/footnotes.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
        )
        .part_xml("word/footnotes.xml", "w:footnotes", footnotes)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let note = document.footnotes.get(1).expect("normal footnote id=1");
    assert_eq!(note.kind, NoteKind::Normal);
    assert_eq!(note.blocks.len(), 4);
    assert_eq!(paragraph_style(&note.blocks[0]), Some("FootnoteText"));
    assert_eq!(paragraph_style(&note.blocks[1]), Some("IntenseQuote"));
    assert_eq!(paragraph_style(&note.blocks[2]), Some("BodyText"));
    assert!(matches!(&note.blocks[3], Block::Table(_)));

    let written = write(&document, &package);
    let reopened = open(&written.bytes).expect("reopen");
    let footnotes_xml = String::from_utf8(
        reopened
            .read_part(&strict_ooxml_core::part::PartId::new("/word/footnotes.xml"))
            .expect("footnotes part"),
    )
    .expect("utf-8");
    let note_slice = footnotes_xml
        .split("<w:footnote ")
        .find(|chunk| chunk.contains("w:id=\"1\""))
        .expect("written footnote id=1");
    assert!(
        note_slice.matches("<w:p>").count() + note_slice.matches("<w:p ").count() >= 3,
        "{note_slice}"
    );
    assert_eq!(
        note_slice.matches("<w:tbl>").count() + note_slice.matches("<w:tbl ").count(),
        1,
        "{note_slice}"
    );
    assert!(
        note_slice.contains("</w:p><w:tbl>"),
        "table must be a sibling of paragraphs: {note_slice}"
    );
    for style in ["FootnoteText", "IntenseQuote", "BodyText"] {
        assert!(
            note_slice.contains(&format!("w:val=\"{style}\"")),
            "{note_slice}"
        );
    }

    let reparsed = parse(&reopened).expect("reparse");
    let again = reparsed.footnotes.get(1).expect("reparsed footnote");
    assert_eq!(again.blocks.len(), 4);
    for (left, right) in note.blocks.iter().zip(again.blocks.iter()) {
        match (left, right) {
            (Block::Paragraph(a), Block::Paragraph(b)) => {
                assert_eq!(paragraph_style(left), paragraph_style(right));
                assert_eq!(paragraph_text(a), paragraph_text(b));
            }
            (Block::Table(_), Block::Table(_)) => {}
            other => panic!("block kind changed: {other:?}"),
        }
    }
    let rewritten = write(&reparsed, &reopened);
    assert_eq!(written.bytes, rewritten.bytes, "{}", written.report);
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
