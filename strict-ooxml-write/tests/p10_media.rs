//! P10 — binary media bytes survive a transitional write.
//!
//! T-P10-1: a shape fill and a locked-canvas picture keep the source digest.
//! An embedded font of zero bytes is still that part. The writer does not
//! recompress.

use std::path::Path;

use sha2::{Digest, Sha256};
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

fn corpus(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

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

fn digest(bytes: &[u8]) -> [u8; 32] {
    let hashed = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(hashed.as_slice());
    out
}

fn part_digests(bytes: &[u8]) -> Vec<[u8; 32]> {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("reopen");
    package
        .parts()
        .filter(|part| {
            let name = part.id.as_str();
            !name.ends_with(".xml") && !name.ends_with(".rels") && !name.ends_with(".vml")
        })
        .map(|part| digest(&package.read_part(&part.id).expect("part")))
        .collect()
}

fn source_digest(package: &Package, part: &str) -> [u8; 32] {
    digest(&package.read_part(&PartId::new(part)).expect(part))
}

fn write_back(package: &Package) -> Vec<u8> {
    let document = parse_document(package, &ParseOptions::default()).expect("parse");
    write_package(&document, Some(package), &WriteOptions::default())
        .expect("write")
        .bytes
}

#[test]
fn t_p10_shape_fill_png_bytes_are_identical() {
    let path = corpus("../strict-ooxml-core/tests/docx/1. First-Steps-in-Programming.docx");
    let package = open_transitional(&path);
    let expected = source_digest(&package, "/word/media/image1.png");
    let written = write_back(&package);
    assert!(
        part_digests(&written).contains(&expected),
        "shape-fill PNG digest is missing"
    );
    let reopened =
        Package::open_reader(written.as_slice(), &OpenOptions::default()).expect("reopen");
    let document = reopened
        .read_part(&PartId::new("/word/document.xml"))
        .expect("document");
    let text = String::from_utf8_lossy(&document);
    assert!(
        text.contains("a:blipFill"),
        "shape image fill was dropped from the document"
    );
}

#[test]
fn t_p10_locked_canvas_jpeg_stays_the_same_bytes() {
    let path = corpus("../testdata/CC0_DOCX/014_BG_Slokas_With_Transliteration_1_18.docx");
    let package = open_transitional(&path);
    let expected = source_digest(&package, "/word/media/image1.jpeg");
    let written = write_back(&package);
    assert!(
        part_digests(&written).contains(&expected),
        "locked-canvas JPEG digest is missing"
    );
}

#[test]
fn t_p10_empty_embedded_font_bytes_are_kept() {
    let path = corpus("../testdata/CC0_DOCX/068_Madhurya_Kadambini_Roman_Sanskrit.docx");
    let package = open_transitional(&path);
    let expected = source_digest(&package, "/word/fonts/font6.odttf");
    assert_eq!(expected, digest(b""));
    let written = write_back(&package);
    assert!(
        part_digests(&written).contains(&expected),
        "empty odttf digest is missing"
    );
}
