//! AUD-36: under `feature = "parallel"`, a concurrent walk of parts produces
//! the same merged [`NormalizationReport`] as a sequential walk.
//!
//! Requires `--features parallel` (rayon). Skips when Manual.docx is absent.

#![cfg(feature = "parallel")]

use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;

fn manual_docx() -> Option<std::path::PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/docx/Manual.docx");
    path.is_file().then_some(path)
}

/// Collect every part's raw (pre-normalization) bytes by opening once with a
/// throwaway normalizer, then re-reading through a second open that installs
/// a capturing normalizer only after we have the ZIP entry list… — instead,
/// open with Normalize, then ask the *same* normalizer is wrong for raw bytes.
///
/// So: open under Normalize (so the package is accepted), gather part ids, and
/// for the equality check drive two fresh normalizers over the already-
/// normalized bytes. Those bytes are Stable Strict for most parts; the
/// concurrent-vs-sequential property still holds for whatever remains.
///
/// For a stronger check we also re-open and let package reads race.
#[test]
fn parallel_package_reread_matches_sequential_report() {
    let Some(path) = manual_docx() else {
        eprintln!("skipping: Manual.docx not present");
        return;
    };

    let sequential = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(sequential.clone());
    let package = Package::open_path(&path, &options).expect("open Manual.docx");
    let ids: Vec<PartId> = package.parts().map(|part| part.id.clone()).collect();
    for id in &ids {
        let _ = package.read_part(id);
    }
    let sequential_report = sequential.report();

    let parallel = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(parallel.clone());
    let package = Package::open_path(&path, &options).expect("open Manual.docx");
    let ids: Vec<PartId> = package.parts().map(|part| part.id.clone()).collect();
    ids.par_iter().for_each(|id| {
        let _ = package.read_part(id);
    });
    assert_eq!(
        parallel.report(),
        sequential_report,
        "AUD-36: parallel part reads must merge to the same report as sequential"
    );
}

#[test]
fn concurrent_normalize_calls_match_sequential_on_synthetic_parts() {
    let styles = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;
    let document = br#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#;
    let numbering = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;
    let parts: Vec<(PartId, &[u8])> = vec![
        (PartId::new("/word/document.xml"), document.as_slice()),
        (PartId::new("/word/styles.xml"), styles.as_slice()),
        (PartId::new("/word/numbering.xml"), numbering.as_slice()),
        (
            PartId::new("/word/_rels/document.xml.rels"),
            br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
</Relationships>"#,
        ),
    ];

    let sequential = TransitionalNormalizer::new();
    for (id, bytes) in &parts {
        sequential.normalize(id, bytes).unwrap();
    }
    let sequential_report = sequential.report();

    let parallel = Arc::new(TransitionalNormalizer::new());
    parts.par_iter().for_each(|(id, bytes)| {
        parallel.normalize(id, bytes).unwrap();
    });
    assert_eq!(parallel.report(), sequential_report);
}
