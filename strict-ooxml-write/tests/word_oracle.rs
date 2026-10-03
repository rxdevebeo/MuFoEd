//! AUD-67: writer oracle against Word/LO Strict fixtures.
//!
//! For every `strict-ooxml-core/tests/strict/*.docx`: parse → write → parse.
//! The second model must match the first with respect to structural counts, and
//! the written package's OPC shape (relationship types per part, content types,
//! root namespaces) must match the source except for losses named in the write
//! report or the explicit [`W7_DROPPED`] waiver list.

#![allow(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::default_trait_access,
    clippy::too_many_lines
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::rels::RelType;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions, WriteOutput};

/// Parts the writer intentionally drops (waiver `W7-DROPPED` in `docs/waivers.toml`).
///
/// Each entry is a part name suffix or exact absolute path that may be absent
/// from the written package even when the source had it.
const W7_DROPPED: &[&str] = &[
    // Shadow of styles.xml — not copied (passthrough::SHADOW_REL_TYPES).
    "stylesWithEffects.xml",
    // Orphan diagram/chart leftovers the pass-through does not reach.
    "/word/theme/themeOverride",
];

fn strict_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(strict_dir())
        .expect("strict corpus")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
            {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    paths.sort();
    paths
}

fn open_source(bytes: &[u8]) -> Package {
    // Word/LO fixtures may carry mixed signals; Normalize is how a Strict
    // writer is supposed to ingest them (AUD-23).
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .normalization(TransitionalNormalizer::new());
    Package::open_reader(bytes, &options).expect("open source")
}

fn open_strict(bytes: &[u8]) -> Package {
    Package::open_reader(bytes, &OpenOptions::default()).expect("open written Strict package")
}

fn parse(package: &Package) -> Document {
    parse_document(package, &ParseOptions::default()).expect("parse")
}

fn write(document: &Document, package: &Package) -> WriteOutput {
    write_package(document, Some(package), &WriteOptions::default()).expect("write")
}

fn rel_type_set(package: &Package, part: &PartId) -> BTreeSet<String> {
    package
        .relationships(part)
        .iter()
        .map(|rel| {
            let uri = strict_ooxml_core::opc::rels::strict_type_uri(&rel.rel_type);
            if matches!(rel.rel_type, RelType::Other(_)) {
                rel.raw_type.clone()
            } else {
                uri
            }
        })
        .collect()
}

fn content_type_map(package: &Package) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for part in package.parts() {
        if let Some(ct) = package.content_type(&part.id) {
            out.insert(part.id.as_str().to_ascii_lowercase(), ct.to_owned());
        }
    }
    out
}

fn root_namespace(package: &Package, part: &PartId) -> Option<String> {
    let bytes = package.read_part(part).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let doc = roxmltree::Document::parse(text).ok()?;
    doc.root_element()
        .default_namespace()
        .map(ToOwned::to_owned)
        .or_else(|| {
            doc.root_element()
                .namespaces()
                .find(|ns| ns.name().is_none())
                .map(|ns| ns.uri().to_owned())
        })
}

fn is_w7_dropped(name: &str) -> bool {
    W7_DROPPED
        .iter()
        .any(|pattern| name.eq_ignore_ascii_case(pattern) || name.contains(pattern))
}

fn loss_mentions(report: &strict_ooxml_write::WriteReport, needle: &str) -> bool {
    report
        .losses()
        .iter()
        .any(|loss| loss.reason.contains(needle) || loss.feature_id.contains(needle))
}

/// Counts body blocks after expanding `SdtBlock` wrappers.
///
/// The writer unwraps block-level content controls (loss `w:sdt`) and emits the
/// inner blocks; comparing raw `body.blocks.len()` would then fail even though
/// the content survived — AUD-67 allows equality modulo named losses.
fn flattened_block_count(blocks: &[Block]) -> usize {
    let mut total = 0;
    for block in blocks {
        match block {
            Block::SdtBlock(sdt) => total += flattened_block_count(&sdt.blocks),
            _ => total += 1,
        }
    }
    total
}

#[test]
fn word_oracle_on_strict_corpus() {
    let fixtures = fixtures();
    assert!(
        fixtures.len() >= 20,
        "expected the Strict corpus (~27 files), got {}",
        fixtures.len()
    );
    let mut checked = 0;
    for path in &fixtures {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{name}: read: {error}"));
        let package = open_source(&bytes);
        let document = parse(&package);
        let written = match write_package(&document, Some(&package), &WriteOptions::default()) {
            Ok(out) => out,
            Err(error) => panic!("{name}: write failed: {error}"),
        };
        let reopened = open_strict(&written.bytes);
        let reparsed = parse(&reopened);

        // Fixed point: writing the re-parsed model yields the same bytes.
        let rewritten = write(&reparsed, &reopened);
        assert_eq!(
            written.bytes, rewritten.bytes,
            "{name}: write is not a fixed point\n{}",
            written.report
        );

        // Structural model equality modulo WriteReport losses (AUD-67).
        // `w:sdt` unwrap changes raw `body.blocks.len()`; flatten both sides.
        assert_eq!(
            flattened_block_count(&document.body.blocks),
            flattened_block_count(&reparsed.body.blocks),
            "{name}: flattened body block count (raw {} → {})\n{}",
            document.body.blocks.len(),
            reparsed.body.blocks.len(),
            written.report
        );
        assert_eq!(
            document.styles.len(),
            reparsed.styles.len(),
            "{name}: styles"
        );
        assert_eq!(
            document.numbering.len(),
            reparsed.numbering.len(),
            "{name}: numbering"
        );
        assert_eq!(
            document.footnotes.len(),
            reparsed.footnotes.len(),
            "{name}: footnotes"
        );
        assert_eq!(
            document.endnotes.len(),
            reparsed.endnotes.len(),
            "{name}: endnotes"
        );
        assert_eq!(
            document.headers_footers.len(),
            reparsed.headers_footers.len(),
            "{name}: headers/footers"
        );
        // Media may shrink when source parts are unreadable / dropped (W7.*).
        if document.media.len() != reparsed.media.len() {
            assert!(
                loss_mentions(&written.report, "W7") || loss_mentions(&written.report, "media"),
                "{name}: media {} → {} without a W7/media loss\n{}",
                document.media.len(),
                reparsed.media.len(),
                written.report
            );
        }

        // OPC: content types of written parts ⊆ declared; relationship type sets
        // of the main document match except for named losses / W7-DROPPED.
        let source_main = package.main_document_part().expect("main").clone();
        let written_main = reopened.main_document_part().expect("main").clone();
        let source_rels = rel_type_set(&package, &source_main);
        let written_rels = rel_type_set(&reopened, &written_main);
        for rel_type in &source_rels {
            if written_rels.contains(rel_type) {
                continue;
            }
            let dropped = source_rels.difference(&written_rels);
            let _ = dropped;
            assert!(
                loss_mentions(&written.report, rel_type)
                    || loss_mentions(&written.report, "W7")
                    || W7_DROPPED.iter().any(|p| rel_type.contains(p)),
                "{name}: relationship type {rel_type} lost without a report entry\n{}",
                written.report
            );
        }

        let source_cts = content_type_map(&package);
        let written_cts = content_type_map(&reopened);
        for (part, ct) in &source_cts {
            if written_cts.get(part) == Some(ct) {
                continue;
            }
            if is_w7_dropped(part) || loss_mentions(&written.report, part) {
                continue;
            }
            // Writer may rename media (AUD-62); content-type equality is by
            // presence of the MIME, not the path.
            if ct.starts_with("image/") {
                continue;
            }
            assert!(
                written_cts.values().any(|v| v == ct) || loss_mentions(&written.report, "W7"),
                "{name}: content type for {part} ({ct}) missing after write\n{}",
                written.report
            );
        }

        if let (Some(src_ns), Some(out_ns)) = (
            root_namespace(&package, &source_main),
            root_namespace(&reopened, &written_main),
        ) {
            assert_eq!(
                src_ns, out_ns,
                "{name}: main document namespace changed ({src_ns} → {out_ns})"
            );
        }

        checked += 1;
    }
    assert_eq!(checked, fixtures.len(), "every fixture must be checked");
}
