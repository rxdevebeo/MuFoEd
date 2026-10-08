//! AUD-20 / ADR-0015: OPC URIs in written packages match the corpus oracle.
//!
//! Values are taken from Word/`LibreOffice` Strict packages on disk, not from
//! our registry. A drift between the writer and the files means we invented a
//! URI the standard does not use.

#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

/// The five OPC signals AUD-20 pins to the corpus.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OpcUris {
    rels_ns: Option<String>,
    core_properties_rel: Option<String>,
    extended_properties_rel: Option<String>,
    core_xml_ns: Option<String>,
    app_vt_ns: Option<String>,
}

impl OpcUris {
    fn as_set(&self) -> BTreeSet<String> {
        [
            self.rels_ns.as_ref(),
            self.core_properties_rel.as_ref(),
            self.extended_properties_rel.as_ref(),
            self.core_xml_ns.as_ref(),
            self.app_vt_ns.as_ref(),
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect()
    }
}

fn open_strict(bytes: &[u8]) -> Package {
    Package::open_reader(bytes, &OpenOptions::default()).expect("open strict")
}

fn open_transitional(bytes: &[u8]) -> Package {
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .normalization(TransitionalNormalizer::new());
    Package::open_reader(bytes, &options).expect("open transitional")
}

fn write_of(package: &Package) -> Vec<u8> {
    let document = parse_document(package, &ParseOptions::default()).expect("parse");
    write_package(&document, Some(package), &WriteOptions::default())
        .expect("write")
        .bytes
}

fn xmlns_default(part: &str) -> Option<String> {
    let marker = "xmlns=\"";
    let at = part.find(marker)?;
    let rest = &part[at + marker.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn xmlns_prefixed(part: &str, prefix: &str) -> Option<String> {
    let marker = format!("xmlns:{prefix}=\"");
    let at = part.find(&marker)?;
    let rest = &part[at + marker.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn collect(package: &Package) -> OpcUris {
    let root = PartId::new("/");
    let rels_ns = package
        .read_part(&PartId::new("/_rels/.rels"))
        .ok()
        .and_then(|bytes| xmlns_default(&String::from_utf8_lossy(&bytes)));

    let mut core_properties_rel = None;
    let mut extended_properties_rel = None;
    for rel in package.relationships(&root) {
        if rel.target.contains("docProps/core.xml") {
            core_properties_rel = Some(rel.raw_type.clone());
        }
        if rel.target.contains("docProps/app.xml") {
            extended_properties_rel = Some(rel.raw_type.clone());
        }
    }

    let core_xml_ns = package
        .read_part(&PartId::new("/docProps/core.xml"))
        .ok()
        .and_then(|bytes| {
            let text = String::from_utf8_lossy(&bytes);
            xmlns_prefixed(&text, "cp").or_else(|| xmlns_default(&text))
        });

    let app_vt_ns = package
        .read_part(&PartId::new("/docProps/app.xml"))
        .ok()
        .and_then(|bytes| xmlns_prefixed(&String::from_utf8_lossy(&bytes), "vt"));

    OpcUris {
        rels_ns,
        core_properties_rel,
        extended_properties_rel,
        core_xml_ns,
        app_vt_ns,
    }
}

fn list_docx(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read corpus dir")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    paths.sort();
    paths
}

/// Strict fixtures that open under `StrictOnly` and carry real OPC parts.
const STRICT_ORACLE: &[&str] = &[
    "strict-profile.docx",
    "strict-text.docx",
    "strict-stage5.docx",
    "lo-strict.docx",
    "strict01-sdk.docx",
    "05-strict-math-simple.docx",
    "06-strict-math-display.docx",
    "07-strict-drawingml-shapes.docx",
];

fn assert_field(name: &str, field: &str, got: Option<&String>, expected: Option<&String>) {
    match (got, expected) {
        (Some(got), Some(expected)) => {
            assert_eq!(got, expected, "{name}: {field}");
        }
        (None, _) => {}
        (Some(got), None) => {
            panic!("{name}: writer invented {field}={got}, reference has none");
        }
    }
}

#[test]
fn written_strict_packages_keep_the_corpus_opc_uris() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let mut checked = 0usize;
    for file in STRICT_ORACLE {
        let path = dir.join(file);
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{file}: read: {error}"));
        let source = open_strict(&bytes);
        let expected = collect(&source);
        if expected.as_set().is_empty() {
            continue;
        }
        let written = write_of(&source);
        let got = collect(&open_strict(&written));
        assert_eq!(
            got.as_set(),
            expected.as_set(),
            "{file}: writer OPC URI set drifted from the corpus oracle\nexpected: {:?}\ngot:      {:?}",
            expected.as_set(),
            got.as_set()
        );
        checked += 1;
    }
    assert!(
        checked >= 5,
        "expected several Strict fixtures with OPC parts, got {checked}"
    );
}

#[test]
fn written_transitional_packages_match_lo_strict_opc_uris() {
    let lo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/lo-strict.docx");
    let reference = collect(&open_strict(
        &std::fs::read(&lo).expect("read lo-strict.docx"),
    ));
    assert!(
        !reference.as_set().is_empty(),
        "lo-strict.docx must carry the OPC oracle signals"
    );

    // The local corpus (`.gitignore`d) when present, plus the CC0 `ci-core`
    // tier that CI fetches; a clean clone without either skips honestly.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx");
    let mut paths = if dir.is_dir() {
        list_docx(&dir)
    } else {
        Vec::new()
    };
    paths.extend(
        strict_ooxml_testkit::corpus::tier(strict_ooxml_testkit::corpus::Tier::CiCore)
            .into_iter()
            .map(|doc| doc.path),
    );
    if paths.is_empty() {
        eprintln!(
            "skipping written_transitional_packages_match_lo_strict_opc_uris: no local or CC0 corpus"
        );
        return;
    }
    let mut checked = 0usize;
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{name}: read: {error}"));
        let source = open_transitional(&bytes);
        let written = write_of(&source);
        let got = collect(&open_strict(&written));
        assert_field(
            &name,
            ".rels ns",
            got.rels_ns.as_ref(),
            reference.rels_ns.as_ref(),
        );
        assert_field(
            &name,
            "core-properties rel",
            got.core_properties_rel.as_ref(),
            reference.core_properties_rel.as_ref(),
        );
        assert_field(
            &name,
            "extended-properties rel",
            got.extended_properties_rel.as_ref(),
            reference.extended_properties_rel.as_ref(),
        );
        assert_field(
            &name,
            "core.xml ns",
            got.core_xml_ns.as_ref(),
            reference.core_xml_ns.as_ref(),
        );
        assert_field(
            &name,
            "app.xml vt",
            got.app_vt_ns.as_ref(),
            reference.app_vt_ns.as_ref(),
        );
        checked += 1;
    }
    assert!(checked >= 1, "expected at least one Transitional fixture");
}
