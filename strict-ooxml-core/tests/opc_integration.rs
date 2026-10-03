//! Stage-1 integration tests over synthetic packages.
//!
//! Fixtures are built in memory as minimal ZIP archives so the tests exercise
//! the whole `Package` pipeline (ZIP → content types → relationships →
//! conformance) without checking binary blobs into the repository.

use std::borrow::Cow;
use std::io::{Cursor, Read};

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::normalize::RawNormalizer;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;

mod common;

use common::build_zip;

const STRICT_W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const TRANSITIONAL_W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STRICT_DOC_REL: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const TRANSITIONAL_DOC_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="xml" ContentType="application/xml"/>
 <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

fn root_rels(doc_rel: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="{doc_rel}" Target="word/document.xml"/>
</Relationships>"#
    )
}

fn document(ns: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{ns}"><w:body/></w:document>"#
    )
}

fn open(bytes: Vec<u8>, policy: ConformancePolicy) -> strict_ooxml_core::error::Result<Package> {
    let options = OpenOptions::default().conformance(policy);
    Package::open_reader(Cursor::new(bytes), &options)
}

fn strict_docx() -> Vec<u8> {
    let doc = document(STRICT_W_NS);
    build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), true),
        ("_rels/.rels", root_rels(STRICT_DOC_REL).as_bytes(), true),
        ("word/document.xml", doc.as_bytes(), true),
    ])
}

fn transitional_docx() -> Vec<u8> {
    let doc = document(TRANSITIONAL_W_NS);
    build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), true),
        (
            "_rels/.rels",
            root_rels(TRANSITIONAL_DOC_REL).as_bytes(),
            true,
        ),
        ("word/document.xml", doc.as_bytes(), true),
    ])
}

#[test]
fn opens_strict_package() {
    let package = open(strict_docx(), ConformancePolicy::StrictOnly).unwrap();
    assert_eq!(package.conformance(), Conformance::Strict);
    assert_eq!(
        package.main_document_part().unwrap().as_str(),
        "/word/document.xml"
    );
    assert_eq!(package.parts().count(), 3);
    assert_eq!(
        package.content_type(&PartId::new("/word/document.xml")),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml")
    );
    let root = PartId::new("/");
    let rel = package.resolve_relationship(&root, "rId1").unwrap();
    assert_eq!(
        rel.resolved.as_ref().unwrap().as_str(),
        "/word/document.xml"
    );
    let bytes = package
        .read_part(&PartId::new("/word/document.xml"))
        .unwrap();
    assert!(std::str::from_utf8(&bytes).unwrap().contains("w:document"));
}

#[test]
fn rejects_transitional_under_strict_only() {
    let error = open(transitional_docx(), ConformancePolicy::StrictOnly).unwrap_err();
    assert!(matches!(
        error,
        StrictError::TransitionalNotSupported { .. }
    ));
}

#[test]
fn inspect_transitional_under_permissive() {
    let package = open(transitional_docx(), ConformancePolicy::Permissive).unwrap();
    assert_eq!(package.conformance(), Conformance::Transitional);
}

#[test]
fn normalize_without_normalizer_is_unsupported() {
    let error = open(transitional_docx(), ConformancePolicy::Normalize).unwrap_err();
    assert!(matches!(error, StrictError::Unsupported(_)));
}

#[test]
fn detects_mixed_conformance() {
    let doc = document(STRICT_W_NS);
    let styles = format!(r#"<?xml version="1.0"?><w:styles xmlns:w="{TRANSITIONAL_W_NS}"/>"#);
    let rels = format!(
        r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="{STRICT_DOC_REL}" Target="word/document.xml"/>
 <Relationship Id="rId2" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/styles" Target="word/styles.xml"/>
</Relationships>"#
    );
    let bytes = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), false),
        ("_rels/.rels", rels.as_bytes(), false),
        ("word/document.xml", doc.as_bytes(), false),
        ("word/styles.xml", styles.as_bytes(), false),
    ]);
    let error = open(bytes, ConformancePolicy::StrictOnly).unwrap_err();
    assert!(matches!(error, StrictError::MixedConformance { .. }));
}

#[test]
fn damaged_archives_return_errors_not_panics() {
    assert!(open(b"this is not a zip".to_vec(), ConformancePolicy::StrictOnly).is_err());
    let mut bytes = strict_docx();
    bytes.truncate(bytes.len() / 2);
    assert!(open(bytes, ConformancePolicy::StrictOnly).is_err());
    assert!(open(Vec::new(), ConformancePolicy::StrictOnly).is_err());
}

#[test]
fn enforces_single_uncompressed_limit() {
    let bytes = strict_docx();
    let options = OpenOptions::default().limits(ResourceLimits {
        max_single_uncompressed: 16,
        ..ResourceLimits::default()
    });
    let error = Package::open_reader(Cursor::new(bytes), &options).unwrap_err();
    assert!(matches!(
        error,
        StrictError::LimitExceeded {
            kind: strict_ooxml_core::error::LimitKind::SingleUncompressed,
            ..
        }
    ));
}

#[test]
fn enforces_compression_ratio_limit() {
    let bomb = vec![0u8; 4 * 1024 * 1024];
    let bytes = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), false),
        ("_rels/.rels", root_rels(STRICT_DOC_REL).as_bytes(), false),
        ("word/document.xml", document(STRICT_W_NS).as_bytes(), false),
        ("word/bomb.bin", &bomb, true),
    ]);
    let options = OpenOptions::default().limits(ResourceLimits {
        max_compression_ratio: 10,
        max_single_uncompressed: u64::MAX,
        ..ResourceLimits::default()
    });
    let error = Package::open_reader(Cursor::new(bytes), &options).unwrap_err();
    assert!(matches!(
        error,
        StrictError::LimitExceeded {
            kind: strict_ooxml_core::error::LimitKind::CompressionRatio,
            ..
        }
    ));
}

#[test]
fn enforces_entry_count_limit() {
    let bytes = strict_docx();
    let options = OpenOptions::default().limits(ResourceLimits {
        max_zip_entries: 1,
        ..ResourceLimits::default()
    });
    let error = Package::open_reader(Cursor::new(bytes), &options).unwrap_err();
    assert!(matches!(
        error,
        StrictError::LimitExceeded {
            kind: strict_ooxml_core::error::LimitKind::ZipEntries,
            ..
        }
    ));
}

#[test]
fn rejects_duplicate_part_names() {
    let bytes = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), false),
        ("_rels/.rels", root_rels(STRICT_DOC_REL).as_bytes(), false),
        ("word/document.xml", document(STRICT_W_NS).as_bytes(), false),
        ("word/document.xml", document(STRICT_W_NS).as_bytes(), false),
    ]);
    let error = open(bytes, ConformancePolicy::StrictOnly).unwrap_err();
    assert!(matches!(error, StrictError::DuplicatePart(_)));
}

#[test]
fn rejects_path_traversal_entry() {
    let bytes = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), false),
        ("_rels/.rels", root_rels(STRICT_DOC_REL).as_bytes(), false),
        ("../evil.xml", b"<a/>", false),
    ]);
    let error = open(bytes, ConformancePolicy::StrictOnly).unwrap_err();
    assert!(matches!(error, StrictError::InvalidPartName(_)));
}

#[test]
fn enforces_compressed_input_limit() {
    let bytes = strict_docx();
    let options = OpenOptions::default().limits(ResourceLimits {
        max_compressed_input: 16,
        ..ResourceLimits::default()
    });
    let error = Package::open_reader(Cursor::new(bytes), &options).unwrap_err();
    assert!(matches!(
        error,
        StrictError::LimitExceeded {
            kind: strict_ooxml_core::error::LimitKind::CompressedInput,
            ..
        }
    ));
}

/// Rewrites Transitional URIs to Strict ones (proves the normalizer is invoked).
#[derive(Debug)]
struct TransitionalToStrict;

impl RawNormalizer for TransitionalToStrict {
    fn normalize_part<'a>(
        &self,
        _part: &PartId,
        bytes: &'a [u8],
    ) -> strict_ooxml_core::error::Result<Cow<'a, [u8]>> {
        let has_ns = bytes
            .windows(TRANSITIONAL_W_NS.len())
            .any(|window| window == TRANSITIONAL_W_NS.as_bytes());
        let has_rel = bytes
            .windows(TRANSITIONAL_DOC_REL.len())
            .any(|window| window == TRANSITIONAL_DOC_REL.as_bytes());
        if !has_ns && !has_rel {
            return Ok(Cow::Borrowed(bytes));
        }
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        text = text.replace(TRANSITIONAL_W_NS, STRICT_W_NS);
        text = text.replace(TRANSITIONAL_DOC_REL, STRICT_DOC_REL);
        Ok(Cow::Owned(text.into_bytes()))
    }
}

#[test]
fn normalization_is_applied_before_conformance_detection() {
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .normalization(TransitionalToStrict);
    let package = Package::open_reader(Cursor::new(transitional_docx()), &options).unwrap();
    // The raw part is Transitional; once normalized it is detected as Strict.
    assert_eq!(package.conformance(), Conformance::Strict);
}

#[test]
fn streaming_open_part_also_applies_normalization() {
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .normalization(TransitionalToStrict);
    let package = Package::open_reader(Cursor::new(transitional_docx()), &options).unwrap();
    let id = PartId::new("/word/document.xml");
    let mut reader = package.source().open_part(&id).unwrap();
    let mut text = String::new();
    reader.read_to_string(&mut text).unwrap();
    assert!(text.contains(STRICT_W_NS), "{text}");
    assert!(!text.contains(TRANSITIONAL_W_NS), "{text}");
}

/// AUD-22: a hostile `_rels/.rels` names an `officeDocument` relationship
/// under an attacker's own authority *first*, then the real one. Before the
/// fix, `RelType::from_uri` matched on a URI's last path segment, so
/// `http://evil.example/officeDocument` classified as `OfficeDocument` just
/// as readily as the real relationship type — and `locate_main_document`
/// takes the *first* match, so the evil part would have won.
#[test]
fn an_evil_office_document_relationship_does_not_win_over_the_real_one() {
    let doc = document(STRICT_W_NS);
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://evil.example/officeDocument" Target="evil.xml"/>
 <Relationship Id="rId2" Type="{STRICT_DOC_REL}" Target="word/document.xml"/>
</Relationships>"#
    );
    let bytes = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), false),
        ("_rels/.rels", rels.as_bytes(), false),
        ("word/document.xml", doc.as_bytes(), false),
    ]);
    let package = open(bytes, ConformancePolicy::StrictOnly).unwrap();
    assert_eq!(
        package.main_document_part().unwrap().as_str(),
        "/word/document.xml"
    );
}

#[test]
fn conformance_survives_root_beyond_prefix() {
    // A long comment pushes the root element past the 64 KiB prefix window,
    // forcing the full-read fallback path.
    let filler = "x".repeat(70 * 1024);
    let doc =
        format!("<?xml version=\"1.0\"?><!--{filler}--><w:document xmlns:w=\"{STRICT_W_NS}\"/>");
    let bytes = build_zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes(), false),
        ("_rels/.rels", root_rels(STRICT_DOC_REL).as_bytes(), false),
        ("word/document.xml", doc.as_bytes(), false),
    ]);
    let package = open(bytes, ConformancePolicy::StrictOnly).unwrap();
    assert_eq!(package.conformance(), Conformance::Strict);
}
