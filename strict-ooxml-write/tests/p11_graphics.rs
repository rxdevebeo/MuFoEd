//! P11 — picture and shape identity that the schema can store.
//!
//! T-P11-3: `bwMode="auto"` is written back. It is not treated as an absent default.

use std::path::Path;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

fn open_transitional(path: &Path) -> Package {
    Package::open_reader(
        std::fs::read(path).expect("fixture").as_slice(),
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(std::sync::Arc::new(
                strict_ooxml_core::normalize::transitional::TransitionalNormalizer::new(),
            )),
    )
    .expect("open")
}

#[test]
fn picture_id_bw_mode_and_cstate_round_trip() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/docx/1. First-Steps-in-Programming.docx");
    let package = open_transitional(&path);
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written = write_package(&document, Some(&package), &WriteOptions::default())
        .expect("write")
        .bytes;
    let reopened = Package::open_reader(written.as_slice(), &OpenOptions::default()).expect("reopen");
    let document = reopened
        .read_part(&PartId::new("/word/document.xml"))
        .expect("document");
    let text = String::from_utf8_lossy(&document);
    assert!(text.contains(r#"id="150""#), "shape non-visual id was rewritten");
    assert!(
        text.contains(r#"bwMode="auto""#),
        "explicit bwMode=auto was dropped"
    );
    assert!(
        text.contains(r#"cstate="print""#),
        "blip compression state was dropped"
    );
}
