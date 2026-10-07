//! AUD-30 / ADR-0017: per-part report idempotence on a real Transitional package.
//!
//! Skips when the local (gitignored) corpus is absent — same contract as
//! `docx_corpus.rs`.
//!
//! `Package::open` does not necessarily touch every part. The property under
//! test is therefore: after a full walk of every part, a second full walk
//! leaves `report()` unchanged (replacement, not accumulation).

use std::path::Path;
use std::sync::Arc;

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};

fn manual_docx() -> Option<std::path::PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/docx/Manual.docx");
    path.is_file().then_some(path)
}

fn read_every_part(package: &Package) {
    for part in package.parts() {
        let _ = package.read_part(&part.id);
    }
}

#[test]
fn rereading_every_part_does_not_change_the_merged_report() {
    let Some(path) = manual_docx() else {
        eprintln!("skipping: Manual.docx not present in tests/docx/");
        return;
    };

    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone());
    let package = Package::open_path(&path, &options).expect("open Manual.docx");

    read_every_part(&package);
    let after_full_walk = normalizer.report();

    read_every_part(&package);
    assert_eq!(
        normalizer.report(),
        after_full_walk,
        "AUD-30: a second full walk must replace per-part reports, not sum them"
    );

    let reltype = after_full_walk
        .applied()
        .into_iter()
        .find(|record| record.id == "T2.reltype")
        .expect("Manual.docx maps relationship types");
    assert_eq!(
        reltype.count, 8,
        "Manual.docx has eight T2.reltype mappings (was x16 when double-counted)"
    );
}

/// The same property on a package built here, so it runs everywhere: a
/// Transitional document with the package `officeDocument` relationship and
/// four document relationships (styles, settings, fontTable, webSettings), all
/// under the Transitional base, maps exactly five relationship types - and a
/// second full walk does not double them.
#[test]
fn rereading_a_synthetic_package_counts_each_relationship_once() {
    use strict_ooxml_testkit::DocxBuilder;

    let mut builder = DocxBuilder::transitional();
    for (id, kind, target, root) in [
        ("rIdStyles", "styles", "styles.xml", "w:styles"),
        ("rIdSettings", "settings", "settings.xml", "w:settings"),
        ("rIdFonts", "fontTable", "fontTable.xml", "w:fonts"),
        ("rIdWeb", "webSettings", "webSettings.xml", "w:webSettings"),
    ] {
        builder = builder
            .part_xml(&format!("word/{target}"), root, "")
            .rel(id, kind, target);
    }
    let bytes = builder.build();

    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone());
    let package = Package::open_reader(bytes.as_slice(), &options).expect("open");

    read_every_part(&package);
    let first = normalizer.report();
    read_every_part(&package);
    assert_eq!(
        normalizer.report(),
        first,
        "a second walk replaces, not sums"
    );

    let reltype = first
        .applied()
        .into_iter()
        .find(|record| record.id == "T2.reltype")
        .expect("relationship types are mapped");
    assert_eq!(
        reltype.count, 5,
        "one officeDocument + four document relationships"
    );
}
