//! W7: the parts this project does not model survive a write (`STAGE-8-TASK.md`
//! §3, and the `O-1` exit criterion «2 страницы до и после `write`»).
//!
//! The fixture is `strict-profile.docx`, the one document in the corpus with a
//! chart, a SmartArt diagram and a workbook behind the chart. Before W7 a write
//! of it produced 8 parts out of 21, dropped both objects, and left the document
//! one page shorter than the original — the chart's extent is what made the
//! second page.
//!
//! The oracles are the same as the rest of the writer's tests: our own reader for
//! the relationship graph, `roxmltree` for the XML, and — for the exit criterion
//! — the layout engine, which is the only thing in the tree that can say how many
//! pages a document has.
#![allow(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::doc_markdown
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::rels::{parse_relationships, RelType, Relationship};
use strict_ooxml_core::opc::zip::write::ZipWriter;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package, CONTENT_TYPES_PART};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::package::{NoSource, RelationshipInfo, Source};
use strict_ooxml_write::{write_package, WriteOptions, WriteOutput};

/// The document the pass-through runs on.
fn fixture() -> (Document, Package) {
    let path = fixture_path();
    let bytes = std::fs::read(&path).expect("the fixture is present");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    (document, package)
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-profile.docx")
}

fn write(document: &Document, source: Option<&dyn Source>) -> WriteOutput {
    write_package(document, source, &WriteOptions::default()).expect("write")
}

/// The part names of a package, sorted.
fn part_names(package: &Package) -> Vec<String> {
    let mut names: Vec<String> = package
        .parts()
        .map(|part| part.id.as_str().to_owned())
        .collect();
    names.sort();
    names
}

/// The relationships of the main part, by id.
fn document_rels(package: &Package) -> BTreeMap<String, Relationship> {
    let main = package.main_document_part().expect("main").clone();
    package
        .relationships(&main)
        .iter()
        .map(|rel| (rel.id.clone(), rel.clone()))
        .collect()
}

/// The ids a `c:chart` / `dgm:relIds` element in `word/document.xml` carries.
fn foreign_refs(document_xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = document_xml;
    while let Some(at) = rest.find("<c:chart") {
        let element = &rest[at..];
        let end = element.find("/>").unwrap_or(element.len());
        out.push(("c:chart".to_owned(), element[..end].to_owned()));
        rest = &rest[at + 1..];
    }
    let mut rest = document_xml;
    while let Some(at) = rest.find("<dgm:relIds") {
        let element = &rest[at..];
        let end = element.find("/>").unwrap_or(element.len());
        out.push(("dgm:relIds".to_owned(), element[..end].to_owned()));
        rest = &rest[at + 1..];
    }
    out
}

/// Every `r:*` attribute value in an element string.
fn rel_ids(element: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = element;
    while let Some(at) = rest.find("r:") {
        rest = &rest[at + 2..];
        // Skip the attribute name and the `="` that opens the value.
        let Some(open) = rest.find("=\"") else {
            break;
        };
        rest = &rest[open + 2..];
        let end = rest.find('"').unwrap_or(rest.len());
        out.push(rest[..end].to_owned());
        rest = &rest[end.min(rest.len())..];
    }
    out
}

/// The document part of a written package as text.
fn main_document(package: &Package) -> String {
    let bytes = package
        .read_part(&PartId::new("/word/document.xml"))
        .expect("main part");
    String::from_utf8(bytes).expect("utf-8")
}

/// The chart, the diagram and the workbook behind the chart are all in the
/// written package, and every reference in the body resolves to one of them.
#[test]
fn an_unmodelled_part_and_its_reference_survive() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let text = written.report.to_string();
    assert!(
        !text.contains("c:chart"),
        "the chart must not be reported as a loss:\n{text}"
    );
    assert!(
        !text.contains("dgm:relIds"),
        "the diagram must not be reported as a loss:\n{text}"
    );

    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{text}"));
    let names = part_names(&reopened);
    for expected in [
        "/word/charts/chart1.xml",
        "/word/charts/_rels/chart1.xml.rels",
        "/word/charts/style1.xml",
        "/word/charts/colors1.xml",
        "/word/diagrams/data1.xml",
        "/word/diagrams/layout1.xml",
        "/word/diagrams/quickStyle1.xml",
        "/word/diagrams/colors1.xml",
        "/word/embeddings/Microsoft_Excel_Worksheet.xlsx",
        "/word/webSettings.xml",
    ] {
        assert!(
            names.contains(&expected.to_owned()),
            "{expected} is missing from {names:?}"
        );
    }

    // Every id the body carries must resolve, through the written `.rels`, to a
    // part that is actually there. A reference that resolves to nothing is what
    // Word calls unreadable content.
    let rels = document_rels(&reopened);
    let body = main_document(&reopened);
    let elements = foreign_refs(&body);
    assert_eq!(elements.len(), 2, "a chart and a diagram: {elements:?}");
    for (element, text) in &elements {
        let ids = rel_ids(text);
        assert!(!ids.is_empty(), "{element} carries no id: {text}");
        for id in ids {
            let relationship = rels
                .get(&id)
                .unwrap_or_else(|| panic!("{element} points at {id}, which is not declared"));
            let target = PartId::new(format!("/word/{}", relationship.target).as_str());
            assert!(
                reopened.part(&target).is_some(),
                "{element} -> {id} -> {} is not in the package",
                target.as_str()
            );
        }
    }
}

/// A copied part's own relationship ids are preserved, because the part refers to
/// them from inside (`<c:externalData r:id="rId3"/>`). Renumbering them would
/// produce a chart that cannot open its data, and nothing in this project would
/// notice. The one thing the pass-through does change is the OPC namespace
/// declaration, which is a declaration and not content.
#[test]
fn a_copied_parts_own_relationships_keep_their_ids() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("open");
    let chart_rels = PartId::new("/word/charts/_rels/chart1.xml.rels");
    let copied = reopened
        .read_part(&chart_rels)
        .expect("the chart's rels are there");
    let original = package.read_part(&chart_rels).expect("source rels");
    assert_ne!(
        copied, original,
        "the OPC relationship namespace is rewritten to the Strict one"
    );
    let ids = |bytes: &[u8]| -> Vec<String> {
        let text = String::from_utf8_lossy(bytes);
        text.match_indices("Id=\"")
            .map(|(at, _)| {
                let rest = &text[at + 4..];
                rest[..rest.find('"').unwrap_or(0)].to_owned()
            })
            .collect()
    };
    assert_eq!(
        ids(&copied),
        ids(&original),
        "the ids a copied part refers to from inside must not be renumbered"
    );
    let parsed = parse_relationships(
        copied.as_slice(),
        &chart_rels,
        &strict_ooxml_core::limits::ResourceLimits::default(),
    )
    .expect("parse");
    assert!(
        parsed.iter().any(|rel| rel.target.contains("embeddings")),
        "the workbook is still reachable from the chart: {parsed:?}"
    );
}
/// SC-1 and SC-3 with pass-through: writing the written package again gives the
/// same bytes, so the pass-through is a fixed point rather than a one-shot.
#[test]
fn a_write_with_pass_through_is_a_fixed_point() {
    let (document, package) = fixture();
    let first = write(&document, Some(&package));
    let reopened = Package::open_reader(&first.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{}", first.report));
    let reparsed = parse_document(&reopened, &ParseOptions::default()).expect("reparse");
    let second = write(&reparsed, Some(&reopened));
    assert_eq!(
        first.bytes, second.bytes,
        "the written package is not a fixed point\nfirst:\n{}\nsecond:\n{}",
        first.report, second.report
    );
    assert_eq!(part_names(&reopened).len(), second.part_count);
}

/// Two runs of the same write are the same bytes, pass-through included.
#[test]
fn pass_through_is_reproducible() {
    let (document, package) = fixture();
    let first = write(&document, Some(&package));
    let second = write(&document, Some(&package));
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.report.to_string(), second.report.to_string());
}

/// The exit criterion of `O-1`: the page count does not change. The chart's
/// declared extent is what made the original two pages, so this is the assertion
/// that says the object is really back rather than merely present.
#[test]
fn the_page_count_does_not_change() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let before = place_pages(&document, &package);
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{}", written.report));
    let reparsed = parse_document(&reopened, &ParseOptions::default()).expect("reparse");
    let after = place_pages(&reparsed, &reopened);
    assert_eq!(before, 2, "the fixture is two pages before the write");
    assert_eq!(
        before, after,
        "2 pages before and after: the chart's extent is what makes the second one"
    );
}

fn place_pages(document: &Document, package: &Package) -> usize {
    let options = strict_ooxml_render_svg::RenderOptions::default();
    strict_ooxml_render_svg::place_pages(document, &options, Some(package))
        .expect("place")
        .len()
}

/// Without the source package the parts are not available, so the references
/// cannot be written — and saying so is the whole point. What must **not** happen
/// is a `c:chart` pointing at a relationship nobody declared.
#[test]
fn without_the_source_the_reference_is_refused_not_dangling() {
    let (document, _package) = fixture();
    let written = write(&document, None);
    let text = written.report.to_string();
    assert!(text.contains("c:chart"), "the chart is reported:\n{text}");
    assert!(
        text.contains("dgm:relIds"),
        "the diagram is reported:\n{text}"
    );
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{text}"));
    let body = main_document(&reopened);
    assert!(
        !body.contains("<c:chart"),
        "a reference with no part behind it is worse than no object:\n{body}"
    );
    assert!(!body.contains("<dgm:relIds"), "{body}");
    assert!(
        foreign_refs(&body).is_empty(),
        "no foreign reference may be written without its parts"
    );
}

/// `docProps/core.xml` is a set of statements **about the document** — title,
/// subject, creator, keywords, description, language, revision, and the dates the
/// producer recorded — and all of them stay true of the content this write copies.
/// So it is carried, with the relationship that reaches it, and it is not reported
/// as a loss (`O-1a`).
///
/// The handoff's worry was `dcterms:modified`: a stale date "passed off as a new
/// one". It is not, for two reasons that are worth stating rather than asserting.
/// A modification date describes the **content's** history, not this container's,
/// and the content is what was copied; and a fresh date cannot be written at all,
/// because SC-1 makes the output reproducible and a clock cannot be part of that.
/// What *would* be a lie is inventing one.
#[test]
fn the_document_properties_survive_a_write() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{}", written.report));

    let id = PartId::new("/docProps/core.xml");
    let core = String::from_utf8(reopened.read_part(&id).expect("core.xml")).expect("utf-8");
    let source_core =
        String::from_utf8(package.read_part(&id).expect("source core.xml")).expect("utf-8");

    // The values are the producer's, byte for byte.
    for element in [
        "dc:title",
        "dc:creator",
        "dcterms:created",
        "dcterms:modified",
    ] {
        let value = |text: &str| {
            let at = text
                .find(element)
                .unwrap_or_else(|| panic!("{element} in {text}"));
            text[at..]
                .split_once('>')
                .map(|(_, rest)| rest.split_once('<').map_or("", |(value, _)| value))
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(
            value(&core),
            value(&source_core),
            "{element} is a statement about the document, and it is the producer's"
        );
    }

    // The namespace is ours: Strict renamed it, and a package carrying the
    // Transitional spelling is one every reader has to normalize first. This is the
    // same exception as the `.rels` namespace, for the same reason — a
    // declaration is not content.
    assert!(
        !core.contains("schemas.openxmlformats.org/package/2006/metadata/core-properties"),
        "the copied part must carry the Strict core-properties namespace:\n{core}"
    );
    assert!(
        core.contains("purl.oclc.org/ooxml/package/metadata/coreProperties"),
        "and it must be the Strict one"
    );

    // The relationship that reaches it, with the Strict type, from the root.
    let root = reopened.relationships(&PartId::new("/"));
    let core_rel = root
        .iter()
        .find(|rel| rel.target.contains("docProps/core.xml"))
        .unwrap_or_else(|| panic!("no relationship to core.xml in {root:?}"));
    assert_eq!(
        core_rel.raw_type,
        "http://purl.oclc.org/ooxml/package/relationships/metadata/core-properties",
        "and it is declared with the Strict type"
    );
    assert_eq!(
        reopened.content_type(&id),
        Some("application/vnd.openxmlformats-package.core-properties+xml"),
        "with the content type the source declared"
    );
    let report = written.report.to_string();
    assert!(
        !report.contains("docProps/core.xml"),
        "a part that survives is not a loss, and it is not named as one:\n{report}"
    );
}

/// `docProps/app.xml` is a set of statements about a **rendering**: pages, words,
/// characters, lines, paragraphs, editing minutes, the template it was built from.
/// This writer lays nothing out, so it cannot produce those numbers and copying the
/// producer's would assert a page count for a document nobody re-paginated — so it
/// is named, and the name says which kind of claim was dropped (SC-10).
#[test]
fn the_page_statistics_are_named_rather_than_invented() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let text = written.report.to_string();
    assert!(
        text.contains("W7.package-properties"),
        "app.xml must be reported:\n{text}"
    );
    assert!(text.contains("docProps/app.xml"), "{text}");
    assert!(
        text.contains("statistics about a rendering this writer does not perform"),
        "and the reason must say what kind of claim it was:\n{text}"
    );

    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{}", written.report));
    assert!(
        reopened
            .parts()
            .all(|part| !part.id.as_str().contains("docProps/app.xml")),
        "a part that is only statistics is better absent than wrong"
    );
}

/// A part the source declares but cannot supply is a record, and the rest of the
/// pass-through still happens: one unreadable part must not cost the document the
/// others.
#[test]
fn an_unreadable_part_is_reported_and_the_rest_survives() {
    let (document, package) = fixture();
    let broken = HidingSource {
        inner: &package,
        hidden: PartId::new("/word/diagrams/quickStyle1.xml"),
    };
    let written = write(&document, Some(&broken));
    let text = written.report.to_string();
    assert!(
        text.contains("W7.passthrough"),
        "the unreadable part is named:\n{text}"
    );
    assert!(text.contains("quickStyle1.xml"), "{text}");
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{text}"));
    let names = part_names(&reopened);
    assert!(
        names.contains(&"/word/charts/chart1.xml".to_owned()),
        "the chart still came through: {names:?}"
    );
    assert!(
        !names.contains(&"/word/diagrams/quickStyle1.xml".to_owned()),
        "a part that could not be read is not in the package: {names:?}"
    );
}

/// A source that hides one part, to exercise the failure path without a
/// hand-built package.
struct HidingSource<'a> {
    inner: &'a Package,
    hidden: PartId,
}

impl Source for HidingSource<'_> {
    fn read_part(&self, part: &PartId) -> strict_ooxml_core::error::Result<Vec<u8>> {
        if part == &self.hidden {
            return Err(strict_ooxml_core::error::StrictError::MissingPart(
                part.clone(),
            ));
        }
        self.inner.read_part(part)
    }

    fn relationship(&self, from: &PartId, rel_id: &str) -> Option<RelationshipInfo> {
        Source::relationship(self.inner, from, rel_id)
    }

    fn relationships(&self, from: &PartId) -> Vec<RelationshipInfo> {
        Source::relationships(self.inner, from)
    }

    fn content_type(&self, part: &PartId) -> Option<String> {
        Source::content_type(self.inner, part)
    }
}

/// The content types of the copied parts are declared, because a package whose
/// part has no content type is not a valid OPC package.
#[test]
fn a_copied_part_has_a_content_type() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("open");
    for part in [
        "/word/charts/chart1.xml",
        "/word/embeddings/Microsoft_Excel_Worksheet.xlsx",
        "/word/diagrams/data1.xml",
    ] {
        let id = PartId::new(part);
        assert!(
            reopened.content_type(&id).is_some(),
            "{part} has no content type in the written package"
        );
    }
    // And the index parses, which is what a reader does with it.
    let bytes = reopened
        .read_part(&PartId::new(CONTENT_TYPES_PART))
        .expect("content types");
    let index = strict_ooxml_core::opc::content_types::ContentTypeIndex::parse(
        bytes,
        PartId::new(CONTENT_TYPES_PART),
        &strict_ooxml_core::limits::ResourceLimits::default(),
    )
    .expect("parse content types");
    assert!(index
        .content_type_for(&PartId::new("/word/charts/chart1.xml"))
        .is_some());
}

/// Every copied part is byte-identical to the source's, and every written part is
/// well-formed XML per an independent parser.
#[test]
fn copied_parts_are_verbatim_and_written_parts_are_well_formed() {
    let (document, package) = fixture();
    let written = write(&document, Some(&package));
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{}", written.report));
    for name in [
        "/word/charts/chart1.xml",
        "/word/diagrams/data1.xml",
        "/word/embeddings/Microsoft_Excel_Worksheet.xlsx",
        "/word/webSettings.xml",
    ] {
        let id = PartId::new(name);
        assert_eq!(
            reopened.read_part(&id).expect("part"),
            package.read_part(&id).expect("source part"),
            "{name} was copied, not rewritten"
        );
    }
    for part in reopened.parts() {
        let id = part.id.as_str();
        if !id.ends_with(".xml") {
            continue;
        }
        let bytes = reopened.read_part(&part.id).expect("part");
        roxmltree::Document::parse(std::str::from_utf8(&bytes).expect("utf-8"))
            .unwrap_or_else(|error| panic!("{id} is not well formed: {error}"));
    }
}

/// A document with no unmodelled parts is written exactly as before: the
/// pass-through adds no parts and no relationships to a plain document.
#[test]
fn a_document_with_nothing_to_pass_through_is_untouched() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let bytes = std::fs::read(root.join("05-strict-math-simple.docx")).expect("fixture");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written = write(&document, Some(&package));
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("open");
    let rels = document_rels(&reopened);
    assert!(
        rels.values()
            .all(|rel| !matches!(rel.rel_type, RelType::Other(_))),
        "no foreign relationship may be invented for a document with none: {rels:?}"
    );
    let text = written.report.to_string();
    assert!(
        !text.contains("W7.passthrough"),
        "nothing to pass through, nothing to report:\n{text}"
    );
}

/// `NoSource` is a legitimate answer for a document built from scratch: the
/// reference cannot be written, and it is reported.
#[test]
fn a_document_built_from_scratch_reports_the_reference_it_cannot_write() {
    let (document, _package) = fixture();
    // The same model, written with the source that cannot supply anything.
    let written =
        write_package(&document, Some(&NoSource), &WriteOptions::default()).expect("write");
    let text = written.report.to_string();
    assert!(text.contains("c:chart"), "{text}");
}

/// A Transitional document with a chart produces a written package with
/// **Transitional parts in it**, and that is the declared consequence of ADR-0007
/// rather than a defect: the copied part is the producer's own bytes, and
/// rewriting them is a semantic edit this project does not make.
///
/// The property that still has to hold is narrower and is the one worth a test:
/// **no part this writer produced carries a Transitional signal.** A part the
/// writer wrote is Strict; a part the writer copied is the source's, and the
/// difference is exactly the pass-through's part list. Without this test the
/// round-trip file's "a written package needs no second normalization pass" is
/// false for exactly the documents ADR-0007 says it is false for, and nobody
/// would find out — the tracked corpus has no Transitional document with a chart
/// (the Transitional corpus is local, `strict-ooxml-core/tests/docx/`).
/// The URIs that mean "this part is Transitional", and the ones that look like it
/// and are not.
///
/// The second list is not a loophole: markup compatibility, content types and the
/// relationships namespace are one vocabulary in both families, and the `r:`
/// prefix of a Strict document is still
/// `schemas.openxmlformats.org/officeDocument/2006/relationships`. The same pair
/// of lists is in `normalize_roundtrip.rs`, where they guard the writer's parts.
const TRANSITIONAL_MARKER: &str = "schemas.openxmlformats.org/";
const ALWAYS_TRANSITIONAL: &[&str] = &[
    "schemas.openxmlformats.org/markup-compatibility/2006",
    "schemas.openxmlformats.org/package/2006/content-types",
    "schemas.openxmlformats.org/package/2006/relationships",
    "schemas.openxmlformats.org/drawingml/2006/compatibility",
    "schemas.openxmlformats.org/officeDocument/2006/relationships",
];

/// The whole URIs in a part's text that carry the Transitional marker, minus the
/// ones that are the same in both families.
fn transitional_uris(text: &str) -> Vec<&str> {
    text.match_indices(TRANSITIONAL_MARKER)
        .map(|(at, _)| {
            let start = text[..at].rfind('"').map_or(at, |quote| quote + 1);
            let end = text[at..]
                .find('"')
                .map_or(text.len(), |offset| at + offset);
            &text[start..end]
        })
        .filter(|uri| !ALWAYS_TRANSITIONAL.iter().any(|kept| uri.contains(kept)))
        .collect()
}

#[test]
fn a_transitional_chart_stays_transitional_and_only_the_copied_parts_do() {
    let source = Package::open_reader(
        &std::fs::read(fixture_path()).expect("fixture")[..],
        &OpenOptions::default(),
    )
    .expect("open");

    // Make the source Transitional the way a real one is: the main document part
    // carries the Transitional namespace, and nothing else changes. Only that part
    // is touched, because a blanket namespace swap would also rewrite the
    // DrawingML the theme parser checks — a different test, and a false failure.
    // The parts the pass-through copies keep their own bytes, which is the point:
    // a producer's chart is whatever the producer wrote.
    let mut writer = ZipWriter::new();
    for part in source.parts() {
        let id = part.id.clone();
        let bytes = source.read_part(&part.id).expect("source part");
        let bytes = if id.as_str() == "/word/document.xml" {
            String::from_utf8(bytes)
                .expect("utf-8")
                .replace(
                    "http://purl.oclc.org/ooxml/wordprocessingml/main",
                    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                )
                .into_bytes()
        } else {
            bytes
        };
        writer.add_part(&id, bytes).expect("add part");
    }
    let bytes = writer.finish().expect("finish");
    // Opened the way a caller opens a real Transitional document: read through the
    // normalizer, so what the writer sees is the Strict model of it.
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .normalization(TransitionalNormalizer::new());
    let transitional = Package::open_reader(&bytes[..], &options).expect("open");
    let reparsed = parse_document(&transitional, &ParseOptions::default()).expect("parse");

    let written = write(&reparsed, Some(&transitional));
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default())
        .unwrap_or_else(|error| panic!("reopen: {error}\n{}", written.report));

    // The document the writer produced is Strict, even though its source was not:
    // that is what the normalizer did on the way in, and the writer's promise.
    let main = String::from_utf8(
        reopened
            .read_part(&PartId::new("/word/document.xml"))
            .expect("main"),
    )
    .expect("utf-8");
    let offenders = transitional_uris(&main);
    assert!(
        offenders.is_empty(),
        "the part this writer wrote must be Strict, and it carries {offenders:?}"
    );

    // And the copied chart is the source's bytes, Transitional signal and all.
    let chart = String::from_utf8(
        reopened
            .read_part(&PartId::new("/word/charts/chart1.xml"))
            .expect("the chart is in the written package"),
    )
    .expect("utf-8");
    assert!(
        chart.contains("schemas.openxmlformats.org"),
        "ADR-0007 copies a Transitional part as it is: rewriting it would edit a \
         producer's semantics, and the chart would no longer be the chart"
    );
}
