//! AUD-87: every document in the Strict corpus opens in the viewer without panic.

#![allow(clippy::doc_markdown)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use strict_ooxml_view::{discover, render};

fn strict_corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

#[test]
fn every_strict_corpus_document_opens_without_panic() {
    let dir = strict_corpus();
    let entries = discover(&dir);
    assert!(
        !entries.is_empty(),
        "expected .docx files in {}",
        dir.display()
    );
    let mut opened = 0usize;
    let mut noted = 0usize;
    for entry in &entries {
        let outcome = catch_unwind(AssertUnwindSafe(|| render(entry, false, 96.0)));
        assert!(
            outcome.is_ok(),
            "viewer panicked on {}: {:?}",
            entry.name,
            outcome.err().map(|payload| format!("{payload:?}"))
        );
        match outcome.expect("caught above") {
            Ok(view) => {
                assert!(
                    !view.pages.is_empty() || view.note.is_some(),
                    "{}: open succeeded but produced no pages",
                    entry.name
                );
                opened += 1;
            }
            Err(_) => {
                // A document the reader rejects is a menu entry with a note in
                // production; the contract here is only "no panic".
                noted += 1;
            }
        }
    }
    assert!(
        opened + noted == entries.len(),
        "opened={opened} noted={noted} entries={}",
        entries.len()
    );
    assert!(
        opened > 0,
        "at least one Strict corpus document must render"
    );
}
