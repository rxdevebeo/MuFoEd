//! Public synthetic `.docx` corpus (`tests/samples/`).
//!
//! Unlike the local, `.gitignore`d `tests/docx/` corpus, `tests/samples/` is
//! versioned public test material and **must** be exercised by the suite: the
//! directory is expected to be present and non-empty, so this test does not skip
//! when it is missing (rework R9).
//!
//! The current corpus is Transitional, so the assertions cover both the
//! permissive inspection path and the `StrictOnly` rejection path. The checks
//! are written to also hold if a future sample is Strict.

use std::path::Path;

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};

/// Returns the sorted list of `.docx` paths in `tests/samples/`.
fn sample_files() -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples");
    assert!(
        dir.is_dir(),
        "public corpus {} is missing; it is versioned and must be present",
        dir.display()
    );
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read samples dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    files
}

#[test]
fn public_corpus_is_present() {
    let files = sample_files();
    assert!(!files.is_empty(), "public corpus contains no .docx files");
}

#[test]
fn public_corpus_opens_detects_and_reads() {
    let files = sample_files();
    assert!(!files.is_empty(), "no samples to check");

    let permissive = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    let strict_only = OpenOptions::default();

    let mut checked = 0usize;
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

        // Permissive inspection must never panic and must classify the file.
        let package = Package::open_path(path, &permissive)
            .unwrap_or_else(|error| panic!("{name}: permissive open failed: {error}"));
        assert_ne!(
            package.conformance(),
            Conformance::Unknown,
            "{name}: conformance could not be determined"
        );

        // The main document part must exist and its bytes must be readable
        // (this exercises decompression + CRC verification end to end).
        let main = package
            .main_document_part()
            .unwrap_or_else(|error| panic!("{name}: no main document: {error}"))
            .clone();
        let bytes = package
            .read_part(&main)
            .unwrap_or_else(|error| panic!("{name}: reading {main} failed: {error}"));
        assert!(!bytes.is_empty(), "{name}: main document part is empty");

        // Enumerating parts and relationships must be panic-free.
        for part in package.parts() {
            let _ = package.relationships(&part.id);
        }

        // Conformance policy behaviour.
        match package.conformance() {
            Conformance::Transitional => {
                let error = Package::open_path(path, &strict_only).unwrap_err();
                assert!(
                    matches!(error, StrictError::TransitionalNotSupported { .. }),
                    "{name}: expected TransitionalNotSupported, got {error:?}"
                );
            }
            Conformance::Strict => {
                Package::open_path(path, &strict_only)
                    .unwrap_or_else(|error| panic!("{name}: StrictOnly open failed: {error}"));
            }
            Conformance::Mixed => panic!("{name}: Mixed conformance in a valid sample"),
            Conformance::Unknown => unreachable!("checked above"),
        }

        checked += 1;
    }

    assert!(checked > 0, "no sample files were checked");
}
