//! Oracle for the AUD-22 relationship-type table (**P-1**-style, independent
//! of our own parser): collects every `Type="..."` attribute of a
//! `<Relationship` element in the real Strict corpus `tests/strict/*.docx`
//! (Word and `LibreOffice` output) and checks it against [`REL_TYPES`].
//!
//! A relationship type the corpus actually uses and the table does not
//! recognize would make a real document's `officeDocument` relationship (or
//! any other) classify as `RelType::Other` where it should not, or would make
//! T2 wrongly report `T2.reltype-unknown` on a legitimate Strict package. The
//! table is "verified" (ADR language, `tables.rs`) only if every URI real
//! files use is accounted for.
//!
//! Values not covered by the table are still legitimate when they are vendor
//! (`schemas.microsoft.com`) or OPC package (`schemas.openxmlformats.org/package`)
//! namespaces — AUD-20 already treats those as family-neutral and outside this
//! table's officeDocument scope.
//!
//! A URI is checked against **both** columns, not just `strict`: one fixture
//! in this corpus (`annotation-ref-sdk-mixed-rels.docx`) is deliberately
//! Mixed-conformance (its own name says so), so it legitimately carries a
//! Transitional relationship type the table's `transitional` column already
//! names. Checking only `strict` would fail a fixture that is doing exactly
//! what it is there to do.

use std::io::Read;
use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::rels::REL_TYPES;

/// Returns the `.docx` files of the Strict corpus, sorted.
fn strict_corpus() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/strict");
    assert!(
        dir.is_dir(),
        "Strict corpus {} is missing; it is versioned",
        dir.display()
    );
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read tests/strict")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "Strict corpus contains no .docx");
    files
}

/// Extracts every `Type="..."` value from `<Relationship` tags in `bytes`.
///
/// Textual on purpose, like `corpus_oracle.rs`'s own attribute reader: this
/// test exists to check our table against the files, so it must not go
/// through our own XML reader to do it.
fn relationship_types(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for tag in text.split('<') {
        if !tag.starts_with("Relationship ") {
            continue;
        }
        let Some(start) = tag.find("Type=\"") else {
            continue;
        };
        let rest = &tag[start + "Type=\"".len()..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_owned());
        }
    }
    out
}

/// Whether `uri` is accounted for: either column of [`REL_TYPES`] names it,
/// or it is a vendor/OPC namespace this table does not claim to cover.
fn is_accounted_for(uri: &str) -> bool {
    if REL_TYPES
        .iter()
        .any(|entry| entry.transitional == uri || entry.strict == Some(uri))
    {
        return true;
    }
    if uri.starts_with("http://schemas.microsoft.com/") {
        return true;
    }
    if uri.starts_with("http://schemas.openxmlformats.org/package/") {
        return true;
    }
    // Package-level *metadata* relationships (core-properties, thumbnail) are
    // OPC's, not an officeDocument-relationship kind this table's `REL_TYPES`
    // claims to cover — but real producers have written them under more than
    // one authority. `lo-tdf116410.docx` in this corpus (an older LibreOffice
    // build, predating AUD-20's standardization on the openxmlformats URI)
    // carries `http://purl.oclc.org/ooxml/officeDocument/relationships/metadata/thumbnail`,
    // a third spelling distinct from both the standard URI and the
    // `purl.oclc.org/ooxml/package/...` one `repair_legacy_package_uri`
    // already knows (AUD-20). Extending that repair table is AUD-20's job, not
    // this one's; here it is enough that the suffix names a package-metadata
    // relationship, regardless of authority.
    uri.ends_with("/metadata/core-properties") || uri.ends_with("/metadata/thumbnail")
}

#[test]
fn every_corpus_relationship_type_is_in_the_table_or_vendor_opc() {
    let mut checked_files = 0usize;
    let mut checked_types = 0usize;
    for path in strict_corpus() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{name}: open: {e}"));
        let mut archive =
            zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("{name}: zip open: {e}"));
        checked_files += 1;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .unwrap_or_else(|e| panic!("{name}: zip entry: {e}"));
            if entry.is_dir()
                || !Path::new(entry.name())
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rels"))
            {
                continue;
            }
            let entry_name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .unwrap_or_else(|e| panic!("{name}: read {entry_name}: {e}"));
            for uri in relationship_types(&bytes) {
                checked_types += 1;
                assert!(
                    is_accounted_for(&uri),
                    "{name}: {entry_name}: relationship type {uri} is neither in REL_TYPES's \
                     Strict column nor a vendor/OPC namespace"
                );
            }
        }
    }
    assert!(checked_files > 0, "no Strict corpus files were checked");
    assert!(checked_types > 0, "no relationship types were found at all");
}
