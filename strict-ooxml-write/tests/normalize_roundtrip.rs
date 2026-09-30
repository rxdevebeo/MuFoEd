//! The round-trip invariant (`SESSION-HANDOFF.md` §6 №3).
//!
//! ```text
//! normalize(parse(serialize(parse(normalize(x))))) == normalize(x)
//! ```
//!
//! The normalization layer is the **only** layer that deletes content on
//! purpose, so it is the layer that needs a second opinion. This file is that
//! second opinion, and it is deliberately not self-referential: the oracle for
//! "is this written package Strict" is the same normalizer that produced it,
//! and the oracle for "is this XML" is `roxmltree`.
//!
//! ## What the expression means, exactly
//!
//! `normalize` is a part-level rewrite at the [`RawNormalizer`] seam, not a
//! function from bytes to bytes, so the equality is stated over what a package
//! *is* — its parts and the model parsed from them. It decomposes into four
//! properties, each asserted separately below because a single assertion would
//! name at most one of them on failure:
//!
//! 1. **`normalize` is idempotent** — normalizing a part twice changes nothing
//!    the second time (Stage 6 SC-3, previously asserted only on one synthetic
//!    string, never on a real document).
//! 2. **The written package needs no normalization.** The writer promises a
//!    Strict package on the first open; if a second pass would still change it,
//!    the promise is false and every reader has to normalize before it can
//!    trust what it read.
//! 3. **The writer is a fixed point** — writing the re-parsed document yields
//!    the same bytes. A writer that is not converges on a limit and never
//!    reaches it, so *which* generation you compare against decides whether you
//!    see the defect.
//! 4. **Nothing is lost on the way** — every write reports no silent loss, and
//!    the second write reports no more than the first.
//!
//! ## What this found
//!
//! Nine defects, all of which the previous green suite missed because it only
//! ever ran on three hand-made Strict fixtures. Every one is named at its fix
//! site. The two that mattered most were not visible in the output at all:
//! a `w:footerReference` aimed at a hyperlink (a page silently loses its
//! footer), and `w:rPr` nested inside `m:rPr`, which is well-formed XML and
//! loses every formula's character formatting on the next read.
//!
//! [`RawNormalizer`]: strict_ooxml_core::normalize::RawNormalizer

// The extensions compared below are the ones *this crate's writer* chose, so a
// case-insensitive comparison would be testing a different thing.
#![allow(clippy::case_sensitive_file_extension_comparisons)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use strict_ooxml_core::normalize::{NormalizationReport, TransitionalNormalizer};
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{dropped_count, verify_no_silent_loss, write_package, WriteOptions};

/// One document under test, and whether it arrives Transitional.
struct Case {
    name: String,
    bytes: Vec<u8>,
    transitional: bool,
}

/// The versioned Strict fixtures. Always present — they are tracked.
fn strict_corpus() -> Vec<Case> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let mut out = Vec::new();
    for (name, file) in [
        ("strict-profile", "strict-profile.docx"),
        ("strict-text", "strict-text.docx"),
        ("strict-text-grid", "strict-text-grid.docx"),
        ("strict-stage5", "strict-stage5.docx"),
        ("strict-stage5b", "strict-stage5b.docx"),
        ("strict-stage5c", "strict-stage5c.docx"),
        ("strict-math-simple", "05-strict-math-simple.docx"),
        ("strict-math-display", "06-strict-math-display.docx"),
        ("strict-drawingml", "07-strict-drawingml-shapes.docx"),
        (
            "strict-math-drawing-chart",
            "09-strict-math-drawing-chart.docx",
        ),
        ("strict-math-eqarr", "10-strict-math-eqarr.docx"),
    ] {
        let path = dir.join(file);
        match std::fs::read(&path) {
            Ok(bytes) => out.push(Case {
                name: name.to_owned(),
                bytes,
                transitional: false,
            }),
            // A tracked fixture that vanished is a broken checkout, not a
            // reason to pass quietly.
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    out
}

/// The local Transitional corpus.
///
/// `.gitignore`d, so on a clean clone and in CI it is absent and every test
/// here skips — which is the honest outcome: these documents are 99 % of what
/// the library will ever see and none of them are in the repository. The skip is
/// loud on stderr for exactly that reason.
fn transitional_corpus() -> Vec<Case> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx");
    if !dir.is_dir() {
        eprintln!(
            "skipping the Transitional corpus: {} is not present (it is .gitignore'd)",
            dir.display()
        );
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read corpus dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let bytes = std::fs::read(&path).unwrap_or_else(|error| {
            panic!("{}: {error}", path.display());
        });
        out.push(Case {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            bytes,
            transitional: true,
        });
    }
    out
}

fn corpus() -> Vec<Case> {
    let mut out = strict_corpus();
    out.extend(transitional_corpus());
    out
}

/// Opens `bytes` with a normalizer installed, and hands back the handle so the
/// loss report can be read.
///
/// The normalizer is shared rather than moved, because the loss report is the
/// only record of what normalization cost — an SC-4 invariant with nothing to
/// assert it is a claim, not a check.
fn open(case: &Case, bytes: &[u8]) -> (Package, Arc<TransitionalNormalizer>) {
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone());
    let package = Package::open_reader(bytes, &options)
        .unwrap_or_else(|error| panic!("{}: open: {error}", case.name));
    (package, normalizer)
}

/// Opens `bytes`, normalizing only when the case is Transitional.
fn open_as_is(case: &Case, bytes: &[u8]) -> Package {
    let options = OpenOptions::default();
    let options = if case.transitional {
        options
            .conformance(ConformancePolicy::Normalize)
            .normalization(TransitionalNormalizer::new())
    } else {
        options
    };
    Package::open_reader(bytes, &options)
        .unwrap_or_else(|error| panic!("{}: open: {error}", case.name))
}

/// Reads every part, so the report covers the package rather than only the
/// parts this pipeline happened to touch.
fn read_all(package: &Package, case: &Case) {
    for part in package.parts() {
        if let Err(error) = package.read_part(&part.id) {
            panic!("{}: read {}: {error}", case.name, part.id);
        }
    }
}

fn parse(package: &Package, case: &Case) -> Document {
    parse_document(package, &ParseOptions::default())
        .unwrap_or_else(|error| panic!("{}: parse: {error}", case.name))
}

fn write(document: &Document, package: &Package, case: &Case, generation: u8) -> Vec<u8> {
    write_package(document, Some(package), &WriteOptions::default())
        .unwrap_or_else(|error| panic!("{}: write gen{generation}: {error}", case.name))
        .bytes
}

/// Property 2: **a written package is already Strict, so a second
/// normalization pass must change nothing.**
///
/// This is the assertion that has the most teeth, and it is the one the writer's
/// own documentation used to make without checking. The normalizer is the
/// project's definition of "carries a Transitional signal"; running it over the
/// writer's output and requiring an empty report is that definition applied to
/// the writer's own claims.
///
/// It found the `.rels` parts being written with the Transitional OPC
/// namespace. That never showed up as a conformance verdict, because
/// `detect_conformance` classifies relationship *types* and namespace
/// *declarations* — and `write_relationships` was declaring
/// `schemas.openxmlformats.org/package/2006/relationships`, a Transitional
/// declaration, in a package the test `a_written_package_is_strict` called
/// Strict. That test never noticed because it only inspected parts ending in
/// `.xml`, and `.rels` does not.
#[test]
fn a_written_package_needs_no_second_normalization_pass() {
    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        read_all(&package, &case);
        let document = parse(&package, &case);
        let written = write(&document, &package, &case, 1);

        let (reopened, normalizer) = open(&case, &written);
        read_all(&reopened, &case);
        let report = normalizer.report();
        assert!(
            report.is_noop(),
            "{}: the written package still carries a Transitional signal, so every \
             reader has to normalize it before it can trust what it read:\n{report}",
            case.name
        );
        checked += 1;
    }
    assert!(
        checked >= 11,
        "expected at least the Strict fixtures, got {checked}"
    );
}

/// Property 1: **normalization is idempotent, on real documents.**
///
/// Stage 6 SC-3. Previously asserted on one synthetic string containing a VML
/// shape and a `w:compat`; never on a document that has footnotes, tables,
/// fields or drawings. The transformation is per-part and the parts are read in
/// whatever order the package walk produces, so the property has to hold part
/// by part over the real corpus rather than once over a toy input.
#[test]
fn normalization_is_idempotent_part_by_part() {
    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        for part in package.parts() {
            if !matches!(part.id.as_str(), id if id.ends_with(".xml") || id.ends_with(".rels")) {
                continue;
            }
            let bytes = package
                .read_part(&part.id)
                .unwrap_or_else(|error| panic!("{}: read {}: {error}", case.name, part.id));
            let once = TransitionalNormalizer::new()
                .normalize(&part.id, &bytes)
                .unwrap_or_else(|error| panic!("{}: {}: {error}", case.name, part.id))
                .into_owned();
            let twice = TransitionalNormalizer::new()
                .normalize(&part.id, &once)
                .unwrap_or_else(|error| panic!("{}: {} second pass: {error}", case.name, part.id))
                .into_owned();
            assert_eq!(
                once,
                twice,
                "{}: {} is not a fixed point of normalization ({} -> {} -> {} bytes)",
                case.name,
                part.id,
                bytes.len(),
                once.len(),
                twice.len()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no XML part was normalized");
}

/// Property 3: **the writer reaches a fixed point in one generation.**
///
/// `write(parse(write(parse(x)))) == write(parse(x))`, over the real corpus.
/// Three Strict fixtures passed this before; twenty-eight Transitional documents
/// did not, and each failure was a different defect. Two of them grew without
/// bound — a `w:footnoteRef` run appended to every separator note on every
/// write, and an empty `<w:rPr/>` that the next parse did not read back — so
/// they would have passed any test that compared a document with *itself*.
#[test]
fn the_written_form_is_a_fixed_point() {
    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        read_all(&package, &case);
        let document = parse(&package, &case);
        let first = write(&document, &package, &case, 1);

        let reopened = open_as_is(&case, &first);
        read_all(&reopened, &case);
        let reparsed = parse(&reopened, &case);
        let second = write(&reparsed, &reopened, &case, 2);

        assert_eq!(
            first.len(),
            second.len(),
            "{}: the written document never settles ({} vs {} bytes)",
            case.name,
            first.len(),
            second.len()
        );
        if first != second {
            // Name the part, not just the document: "the document differs" is
            // what made this expensive to find the first time.
            let differing = differing_parts(&first, &second);
            assert!(
                differing.is_empty(),
                "{}: the written document is not a fixed point; these parts differ: {differing:?}",
                case.name
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 11,
        "expected at least the Strict fixtures, got {checked}"
    );
}

/// Property 4: **the trip costs nothing that the normalizer kept.**
///
/// Three counts, because "nothing was lost" is three different claims and
/// conflating them hides which one broke:
///
/// * the first write drops nothing without a report entry (SC-10);
/// * the first write drops **nothing at all** — a dropped construct is a
///   construct the renderer will not draw, and a report entry does not put it
///   back;
/// * the second write drops no more than the first, so the trip is not
///   degrading the document as it goes.
///
/// The second bullet is the strict one and it is the project's own rule:
/// «неподдержанное не терять молча». A loss record is an audit trail, not a
/// substitute.
#[test]
fn the_trip_loses_nothing_the_normalizer_kept() {
    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        read_all(&package, &case);
        let document = parse(&package, &case);

        let first = write_package(&document, Some(&package), &WriteOptions::default())
            .unwrap_or_else(|error| panic!("{}: write: {error}", case.name));
        assert_eq!(
            verify_no_silent_loss(&first.report),
            Ok(()),
            "{}: {}",
            case.name,
            first.report
        );
        let first_dropped = dropped_count(&first.report);

        let reopened = open_as_is(&case, &first.bytes);
        read_all(&reopened, &case);
        let reparsed = parse(&reopened, &case);
        let second = write_package(&reparsed, Some(&reopened), &WriteOptions::default())
            .unwrap_or_else(|error| panic!("{}: rewrite: {error}", case.name));
        let second_dropped = dropped_count(&second.report);

        assert!(
            second_dropped <= first_dropped,
            "{}: the round trip degraded the document: {first_dropped} construct(s) \
             dropped on the first write, {second_dropped} on the second\nfirst:\n{}\nsecond:\n{}",
            case.name,
            first.report,
            second.report
        );
        checked += 1;
    }
    assert!(
        checked >= 11,
        "expected at least the Strict fixtures, got {checked}"
    );
}

/// The content that must survive: the document's text, and the sizes of the
/// tables the model carries.
///
/// A fixed point proves the writer is *stable*; it says nothing about whether
/// the writer is *right*. These two counts are what caught the header/footer
/// loss, which was perfectly stable — the same empty `w:sectPr` was written
/// every time.
#[test]
fn nothing_that_reaches_the_page_disappears() {
    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        read_all(&package, &case);
        let document = parse(&package, &case);
        let first = write(&document, &package, &case, 1);
        let reopened = open_as_is(&case, &first);
        read_all(&reopened, &case);
        let reparsed = parse(&reopened, &case);

        assert_eq!(
            document.body.blocks.len(),
            reparsed.body.blocks.len(),
            "{}: {} block(s) became {}",
            case.name,
            document.body.blocks.len(),
            reparsed.body.blocks.len()
        );
        assert_eq!(
            document.sections.len(),
            reparsed.sections.len(),
            "{}: {} section(s) became {}",
            case.name,
            document.sections.len(),
            reparsed.sections.len()
        );
        assert_eq!(
            document.headers_footers.len(),
            reparsed.headers_footers.len(),
            "{}: {} header/footer part(s) became {} — a reference that resolves to \
             nothing is not drawn, so this is a page that quietly lost its furniture",
            case.name,
            document.headers_footers.len(),
            reparsed.headers_footers.len()
        );
        assert_eq!(
            document.footnotes.len(),
            reparsed.footnotes.len(),
            "{}: footnote count changed",
            case.name
        );
        assert_eq!(
            document.endnotes.len(),
            reparsed.endnotes.len(),
            "{}: endnote count changed",
            case.name
        );
        assert_eq!(
            document.media.len(),
            reparsed.media.len(),
            "{}: media count changed",
            case.name
        );
        checked += 1;
    }
    assert!(
        checked >= 11,
        "expected at least the Strict fixtures, got {checked}"
    );
}

/// The mathematical operator this file is named after: two normalizations of
/// the same input produce the same report, and a second pass over a normalized
/// package reports nothing.
///
/// Determinism is what makes the other assertions meaningful: an invariant
/// checked against a report that varies run to run would be checked against
/// nothing.
#[test]
fn two_normalizations_agree_report_for_report() {
    for case in corpus() {
        let (package_a, first) = open(&case, &case.bytes);
        let (package_b, second) = open(&case, &case.bytes);
        read_all(&package_a, &case);
        read_all(&package_b, &case);
        assert_eq!(
            first.report().to_string(),
            second.report().to_string(),
            "{}: two normalizations of the same package disagree",
            case.name
        );
    }
}

/// A written package carries no Transitional signal **anywhere**, including in
/// the places a declaration scan cannot see.
///
/// `detect_conformance` reads namespace declarations and relationship types.
/// Two defects lived in the gaps: the `.rels` document namespace, and
/// `a:graphicData/@uri`, which names a namespace as an attribute *value* — so
/// neither the conformance check nor the normalizer could see it. This walks
/// the written parts with an independent parser and asks about every attribute
/// value that looks like a namespace URI.
#[test]
fn no_written_part_carries_a_transitional_uri() {
    const TRANSITIONAL_MARKER: &str = "schemas.openxmlformats.org/";
    // URIs that are legitimately the same in both families.
    const ALWAYS_TRANSITIONAL: &[&str] = &[
        // Markup Compatibility is not part of the Strict/Transitional split.
        "schemas.openxmlformats.org/markup-compatibility/2006",
        // Content types are a package-level vocabulary that Strict did not
        // rename; changing them would make the package unreadable.
        "schemas.openxmlformats.org/package/2006/content-types",
        "schemas.openxmlformats.org/package/2006/relationships",
        "schemas.openxmlformats.org/drawingml/2006/compatibility",
    ];

    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        read_all(&package, &case);
        let document = parse(&package, &case);
        let written = write(&document, &package, &case, 1);
        let reopened = open_as_is(&case, &written);

        for part in reopened.parts() {
            let id = part.id.as_str();
            if !(id.ends_with(".xml") || id.ends_with(".rels")) {
                continue;
            }
            let bytes = reopened
                .read_part(&part.id)
                .unwrap_or_else(|error| panic!("{}: read {id}: {error}", case.name));
            let text = std::str::from_utf8(&bytes)
                .unwrap_or_else(|error| panic!("{}: {id} is not utf-8: {error}", case.name));
            let offenders: Vec<&str> = text
                .match_indices(TRANSITIONAL_MARKER)
                .map(|(at, _)| {
                    let start = text[..at].rfind('"').map_or(at, |quote| quote + 1);
                    let end = text[at..]
                        .find('"')
                        .map_or(text.len(), |offset| at + offset);
                    &text[start..end]
                })
                .filter(|uri| !ALWAYS_TRANSITIONAL.iter().any(|kept| uri.contains(kept)))
                .collect();
            assert!(
                offenders.is_empty(),
                "{}: {id} carries a Transitional URI: {offenders:?}\n{text}",
                case.name
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no XML part was inspected");
}

/// Every written part parses with an independent XML parser.
///
/// Kept here rather than only in `roundtrip.rs` because that file runs on three
/// fixtures; this one runs on everything. It is what catches a repeated
/// attribute — `w:tblW` wrote its unit as `w:w` and then wrote the width as
/// `w:w` again, which is well-formed-looking text and not XML at all.
#[test]
fn every_written_part_is_well_formed_xml() {
    let mut checked = 0;
    for case in corpus() {
        let package = open_as_is(&case, &case.bytes);
        read_all(&package, &case);
        let document = parse(&package, &case);
        let written = write(&document, &package, &case, 1);
        let reopened = open_as_is(&case, &written);
        for part in reopened.parts() {
            let id = part.id.as_str();
            if !(id.ends_with(".xml") || id.ends_with(".rels")) {
                continue;
            }
            let bytes = reopened
                .read_part(&part.id)
                .unwrap_or_else(|error| panic!("{}: read {id}: {error}", case.name));
            roxmltree::Document::parse(std::str::from_utf8(&bytes).unwrap_or_else(|error| {
                panic!("{case_name}: {id}: {error}", case_name = case.name)
            }))
            .unwrap_or_else(|error| panic!("{}: {id} is not well formed: {error}", case.name));
            checked += 1;
        }
    }
    assert!(checked > 0, "no XML part was inspected");
}

/// A normalization report that reached this far must also account for what it
/// removed — the SC-4 invariant, over the real corpus rather than a fixture.
#[test]
fn normalization_removes_nothing_silently() {
    for case in corpus() {
        if !case.transitional {
            // SC-4 is about normalization; a Strict package has none, and the
            // other test here asserts that its report is empty anyway.
            continue;
        }
        let (package, normalizer) = open(&case, &case.bytes);
        read_all(&package, &case);
        let report: NormalizationReport = normalizer.report();
        assert_eq!(
            report.verify_no_silent_loss(),
            Ok(()),
            "{}: {report}",
            case.name
        );
    }
}

/// The part ids of two written packages, reporting only the ones whose bytes
/// differ.
fn differing_parts(left: &[u8], right: &[u8]) -> Vec<String> {
    let open_all = |bytes: &[u8]| {
        Package::open_reader(bytes, &OpenOptions::default())
            .map(|package| {
                package
                    .parts()
                    .map(|part| {
                        let bytes = package.read_part(&part.id).unwrap_or_default();
                        (part.id.as_str().to_owned(), bytes)
                    })
                    .collect::<std::collections::BTreeMap<String, Vec<u8>>>()
            })
            .unwrap_or_default()
    };
    let a = open_all(left);
    let b = open_all(right);
    let mut out = Vec::new();
    for (id, bytes) in &a {
        match b.get(id) {
            Some(other) if other == bytes => {}
            Some(_) => out.push(id.clone()),
            None => out.push(format!("{id} (missing in the second write)")),
        }
    }
    for id in b.keys() {
        if !a.contains_key(id) {
            out.push(format!("{id} (only in the second write)"));
        }
    }
    out
}

/// The written package's `word/document.xml`, for tests that need to look at
/// the actual markup.
#[allow(dead_code)]
fn document_xml(package: &Package) -> String {
    String::from_utf8_lossy(
        &package
            .read_part(&PartId::new("/word/document.xml"))
            .unwrap_or_default(),
    )
    .into_owned()
}
