//! Independent-oracle corpus test (**P-1**).
//!
//! Verifies `strict-ooxml-core` against an **independent** implementation
//! (`zip`) over the real public corpus `tests/samples/`, so it cannot be fooled
//! by fixtures that merely agree with our own code:
//!
//! 1. the set of parts equals the archive's file entries;
//! 2. every part's decompressed bytes are byte-identical (SHA-free, direct
//!    comparison) to what the independent reader produces;
//! 3. the number of parsed relationships equals an independent count of
//!    `<Relationship` elements in the raw `.rels` parts;
//! 4. every `[Content_Types].xml` `<Override>` is honoured for its part.
//!
//! The public corpus is versioned and must be present (no skip).
#![allow(
    clippy::doc_markdown,
    clippy::case_sensitive_file_extension_comparisons
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use strict_ooxml_core::normalize::NoopNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;

/// Returns the `.docx` files of the public corpus, sorted.
fn samples() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples");
    assert!(
        dir.is_dir(),
        "public corpus {} is missing; it is versioned",
        dir.display()
    );
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read samples dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "public corpus contains no .docx");
    files
}

/// Reads every non-directory entry with the independent `zip` crate.
///
/// Keys are normalized to leading-slash part ids; values are decompressed bytes.
fn reference_parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let file = std::fs::File::open(path).expect("open sample");
    let mut archive = zip::ZipArchive::new(file).expect("independent zip open");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        parts.insert(format!("/{}", entry.name()), bytes);
    }
    parts
}

/// Counts non-overlapping occurrences of `needle` in `haystack`.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut total = 0;
    let mut next_allowed = 0;
    for (index, window) in haystack.windows(needle.len()).enumerate() {
        if window == needle && index >= next_allowed {
            total += 1;
            next_allowed = index + needle.len();
        }
    }
    total
}

/// Extracts an `name="value"` attribute from a tag substring.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

#[test]
fn corpus_matches_independent_zip() {
    for path in samples() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let reference = reference_parts(&path);

        // This test is about ZIP/OPC fidelity, not conformance (AUD-23 /
        // ADR-0016): `Permissive` without a normalizer now refuses
        // Transitional and Mixed content, same as `Normalize`, so a `Noop`
        // normalizer is installed to open every sample regardless of family
        // without touching a single byte - part (2) below depends on that.
        let options = OpenOptions::default()
            .conformance(ConformancePolicy::Permissive)
            .normalization(NoopNormalizer);
        let package = Package::open_path(&path, &options)
            .unwrap_or_else(|error| panic!("{name}: open failed: {error}"));

        // (1) part set
        let actual: BTreeSet<String> = package
            .parts()
            .map(|part| part.id.as_str().to_owned())
            .collect();
        let expected: BTreeSet<String> = reference.keys().cloned().collect();
        assert_eq!(
            actual, expected,
            "{name}: part set differs from the archive"
        );

        // (2) byte-identical decompression
        for (part, bytes) in &reference {
            let got = package
                .read_part(&PartId::new(part.as_str()))
                .unwrap_or_else(|error| panic!("{name}: read {part} failed: {error}"));
            assert_eq!(&got, bytes, "{name}: bytes differ for {part}");
        }

        // (3) relationships: parsed count vs independent count of `<Relationship`
        // elements (note the trailing space so the `<Relationships>` root is not
        // counted).
        let expected_rels: usize = reference
            .iter()
            .filter(|(part, _)| part.ends_with(".rels"))
            .map(|(_, bytes)| count(bytes, b"<Relationship "))
            .sum();
        let actual_rels = package.relationships(&PartId::new("/")).len()
            + package
                .parts()
                .map(|part| package.relationships(&part.id).len())
                .sum::<usize>();
        assert_eq!(
            actual_rels, expected_rels,
            "{name}: relationship count differs"
        );

        // (4) every `<Override>` in [Content_Types].xml is honoured
        if let Some(content_types) = reference.get("/[Content_Types].xml") {
            let text = String::from_utf8_lossy(content_types);
            for tag in text.split('<').filter(|tag| tag.starts_with("Override")) {
                if let (Some(part), Some(content_type)) =
                    (attribute(tag, "PartName"), attribute(tag, "ContentType"))
                {
                    assert_eq!(
                        package.content_type(&PartId::new(part.as_str())),
                        Some(content_type.as_str()),
                        "{name}: content type for {part} not honoured"
                    );
                }
            }
        }
    }
}
