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
