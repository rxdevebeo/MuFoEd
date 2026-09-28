#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Corpus test: parse every available `.docx` without panicking (STAGE-2 §12.2).
//!
//! The public `strict-ooxml-core/tests/samples/` corpus is always exercised; the
//! local `tests/docx/` corpus is gitignored and skipped when absent.

mod common;

use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn docx_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    files
}

/// Parses every `.docx` in `dir`, returning `(checked, failed_to_open)`.
///
/// The contract is "never panic": a damaged or non-ZIP file is counted as a
/// failure to open and skipped, never an error that aborts the run.
fn run_corpus(dir: &Path) -> (usize, usize) {
    let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    let parse_options = ParseOptions {
        conformance: ConformancePolicy::Permissive,
        limits: strict_ooxml_core::limits::ResourceLimits::default(),
    };
    let mut checked = 0;
    let mut failed = 0;
    for path in docx_files(dir) {
        match Package::open_path(&path, &options) {
            Ok(package) => {
                // Must never panic; Transitional input may legitimately error.
                let _ = parse_document(&package, &parse_options);
                checked += 1;
            }
            Err(_) => failed += 1,
        }
    }
    (checked, failed)
}

#[test]
fn public_samples_parse_without_panicking() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/samples");
    assert!(
        samples.is_dir(),
        "public corpus {} is missing",
        samples.display()
    );
    let (checked, _failed) = run_corpus(&samples);
    assert!(checked > 0, "no public samples were checked");
}

#[test]
fn local_corpus_parses_without_panicking() {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/docx");
    let _ = run_corpus(&local);
}

#[test]
fn fixture_corpus_parses_without_panicking() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let _ = run_corpus(&fixtures);
}

#[test]
fn damaged_files_do_not_panic() {
    // A directory containing a non-ZIP `.docx` must not abort the corpus run.
    let dir = std::env::temp_dir().join(format!("strict-ooxml-wml-damaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(dir.join("broken.docx"), b"this is not a zip archive").expect("write");
    std::fs::write(dir.join("empty.docx"), b"").expect("write");
    let (checked, failed) = run_corpus(&dir);
    assert_eq!(checked, 0);
    assert_eq!(failed, 2);
    let _ = std::fs::remove_dir_all(&dir);
}
