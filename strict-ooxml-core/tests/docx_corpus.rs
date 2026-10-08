//! Optional local `.docx` corpus (rework R9).
//!
//! `strict-ooxml-core/tests/docx/` is a local, non-published corpus (it is
//! `.gitignore`d). When the directory is absent — as on a clean clone or in CI —
//! this test skips gracefully instead of failing.

use std::path::Path;

use strict_ooxml_core::opc::{OpenOptions, Package};

#[test]
fn local_docx_corpus_opens_without_panicking() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/docx");
    if !dir.is_dir() {
        eprintln!("skipping: local corpus {} is not present", dir.display());
        return;
    }

    let mut checked = 0usize;
    let entries = std::fs::read_dir(&dir).expect("read corpus dir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
            continue;
        }
        // Opening must never panic; both Ok and Err are acceptable.
        let result = Package::open_path(&path, &OpenOptions::default());
        eprintln!("{}: {}", path.display(), result.is_ok());
        checked += 1;
    }
    eprintln!("checked {checked} local docx files");
    assert!(
        checked > 0,
        "corpus directory present but contains no .docx"
    );
}

/// The CC0 `ci-core` tier (`docs/CC0_CORPUS_MIGRATION_PLAN.md`): every document
/// opens. Skipped when the tier has not been fetched, required in CI
/// (`STRICT_OOXML_CORPUS=require`).
#[test]
fn cc0_core_corpus_opens() {
    use strict_ooxml_core::normalize::transitional::TransitionalNormalizer;
    use strict_ooxml_core::opc::ConformancePolicy;
    use strict_ooxml_testkit::corpus::{tier, Tier};

    let docs = tier(Tier::CiCore);
    if docs.is_empty() {
        eprintln!("SKIP cc0 ci-core corpus not fetched");
        return;
    }
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(std::sync::Arc::new(TransitionalNormalizer::new()));
    for doc in docs {
        if let Err(error) = Package::open_path(&doc.path, &options) {
            panic!("{} ({}): {error}", doc.id, doc.path.display());
        }
    }
}
