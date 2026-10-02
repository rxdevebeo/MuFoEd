//! Every property container the writer produces is in the order the schema says.
//!
//! `STAGE-10G-TASK.md` G21: the order of the children of `w:pPr`, `w:settings`
//! and the rest comes from `xsd:sequence` and is stated in ONE place
//! ([`strict_ooxml_write::order`]) rather than repaired one misordered element at
//! a time. This is the check that keeps the statement true.
//!
//! It runs over the ASSEMBLED package of the whole corpus, not over a unit's
//! output, because that is the only level at which "the writer's order" is a
//! claim about anything: a test that fed a model three properties in the order
//! the writer happened to like would pass while every real document failed.
//!
//! Two things are checked, and they are different failures:
//!
//! - a container's children are in the order [`order`] declares (`XS-06`, `XS-07`);
//! - a container holds a child [`order`] does not name at all, which is not a
//!   misordering but a transcription that has fallen behind the writer.
//!
//! The transcription itself is checked a second time, against the published
//! schema, by `xtool/xsd-gate/xsd_gate.py`: it reads these constants out of
//! `src/order.rs` and compares them with the `xsd:sequence` it compiles. A table
//! that had drifted from the schema would pass this test and fail there.

use std::collections::BTreeMap;
use std::path::Path;

use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::order;

/// The containers whose order is declared, by local name.
const CONTAINERS: &[(&str, &[&str])] = &[
    ("pPr", order::PPR),
    ("settings", order::SETTINGS),
    ("sectPr", order::SECTPR),
    ("tblPr", order::TBLPR),
    ("tcPr", order::TCPR),
    ("trPr", order::TRPR),
    ("style", order::STYLE),
    ("lvl", order::LVL),
];

fn declared_sequence(local: &str) -> Option<&'static [&'static str]> {
    CONTAINERS
        .iter()
        .find(|(container, _)| *container == local)
        .map(|(_, sequence)| *sequence)
}

/// Walks `part` and returns every container's children, in the order written.
fn containers_in(part_name: &str, bytes: &[u8]) -> Vec<(String, Vec<String>)> {
    let Ok(mut reader) = XmlReader::from_vec(
        bytes.to_vec(),
        PartId::new(part_name),
        &ResourceLimits::default(),
    ) else {
        return Vec::new();
    };
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    let mut open: Option<(String, Vec<String>)> = None;
    // Nesting depth relative to the container currently open; a container's own
    // children sit at 1.
    let mut depth = 0usize;
    loop {
        match reader.next_event() {
            Ok(XmlEvent::StartElement { name, .. }) => {
                // Namespace matters: DrawingML has an `a:style` whose children
                // are `a:lnRef`/`a:fillRef`/`a:effectRef`, and a walker that
                // matched on the local name alone would check a table style
                // against a drawing's shape style.
                let local = name.local().to_owned();
                let ours = name
                    .namespace()
                    .is_some_and(|ns| ns.as_str() == strict_ooxml_wml::WML_STRICT_NS);
                match open.as_mut() {
                    Some((_, children)) => {
                        if depth == 1 && ours {
                            children.push(local);
                        }
                        depth += 1;
                    }
                    None => {
                        if ours && is_container(&local) {
                            open = Some((local, Vec::new()));
                            depth = 1;
                        }
                    }
                }
            }
            Ok(XmlEvent::EndElement { .. }) => {
                if let Some((key, children)) = open.as_mut() {
                    if depth <= 1 {
                        found.push((key.clone(), children.clone()));
                        open = None;
                        depth = 0;
                    } else {
                        depth -= 1;
                    }
                }
            }
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
    }
    found
}

fn is_container(local: &str) -> bool {
    CONTAINERS.iter().any(|(container, _)| *container == local)
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("?")
                .to_owned();
            out.push((name, bytes));
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    out
}

#[test]
fn every_container_follows_the_order_the_schema_declares() {
    let mut inspected = 0usize;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();

    for (name, bytes) in corpus() {
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let Ok(package) = Package::open_reader(bytes.as_slice(), &options) else {
            continue;
        };
        let Ok(document) = parse_document(&package, &ParseOptions::default()) else {
            continue;
        };
        let written = strict_ooxml_write::write_bytes(
            &document,
            Some(&package),
            &strict_ooxml_write::WriteOptions::default(),
        )
        .expect("write the corpus document");
        let Ok(repacked) = Package::open_reader(written.as_slice(), &options) else {
            continue;
        };
        for part in repacked.parts() {
            let part_name = part.id.as_str();
            let Some(part_name) = part_name.strip_prefix("/") else {
                continue;
            };
            if !part_name.to_ascii_lowercase().starts_with("word/")
                || Path::new(part_name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    != Some("xml")
            {
                continue;
            }
            let Ok(bytes) = repacked.read_part(&part.id) else {
                continue;
            };
            for (container, children) in containers_in(part_name, &bytes) {
                if children.is_empty() {
                    continue;
                }
                let Some(declared) = declared_sequence(&container) else {
                    continue;
                };
                inspected += 1;
                *kinds.entry(container.clone()).or_insert(0) += 1;
                let mut highest = 0usize;
                for child in &children {
                    let Some(rank) = declared.iter().position(|entry| *entry == child) else {
                        problems.push(format!(
                            "{name}: {part_name}: <w:{container}> holds <w:{child}>, which the \
                             declared sequence does not name"
                        ));
                        continue;
                    };
                    if rank < highest {
                        problems.push(format!(
                            "{name}: {part_name}: <w:{container}> writes <w:{child}> after a \
                             child the sequence puts later"
                        ));
                    }
                    highest = highest.max(rank);
                }
            }
        }
    }

    assert!(
        inspected > 500,
        "only {inspected} containers were inspected"
    );
    assert!(
        kinds.len() >= 5,
        "expected several container kinds, saw {kinds:?}"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The writer reaches a fixed point over the whole corpus, which is what makes
/// "the order is stated once" falsifiable: an order that changed on the second
/// write would leave the first one unmeasured.
#[test]
fn the_written_corpus_settles_in_one_generation() {
    let mut checked = 0usize;
    for (name, bytes) in corpus() {
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let Ok(package) = Package::open_reader(bytes.as_slice(), &options) else {
            continue;
        };
        let Ok(document) = parse_document(&package, &ParseOptions::default()) else {
            continue;
        };
        let first = strict_ooxml_write::write_bytes(
            &document,
            Some(&package),
            &strict_ooxml_write::WriteOptions::default(),
        )
        .expect("write");
        let Ok(reopened) = Package::open_reader(first.as_slice(), &options) else {
            continue;
        };
        let Ok(reparsed) = parse_document(&reopened, &ParseOptions::default()) else {
            continue;
        };
        let second = strict_ooxml_write::write_bytes(
            &reparsed,
            Some(&reopened),
            &strict_ooxml_write::WriteOptions::default(),
        )
        .expect("write");
        assert_eq!(
            first, second,
            "{name}: the written package is not a fixed point"
        );
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} documents were checked");
}
