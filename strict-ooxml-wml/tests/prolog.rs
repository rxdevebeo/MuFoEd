#![allow(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Regression tests for the XML prolog before the root element (REWORK-WML-1,
//! finding C-3): declaration, whitespace, comments and processing instructions
//! may precede the root on separate lines, as real Strict producers write them.

mod common;

use common::{document_parts, parse_parts, rels, W_NS};

const STYLES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/styles";
const NUMBERING: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";
const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

/// Builds a one-part package whose `document.xml` has `prolog` between the
/// declaration and the root element.
fn document_with_prolog(prolog: &str) -> Vec<(String, Vec<u8>)> {
    let xml = format!(
        "{DECL}{prolog}<w:document xmlns:w=\"{W_NS}\"><w:body><w:p/></w:body></w:document>"
    );
    vec![("word/document.xml".to_owned(), xml.into_bytes())]
}

/// Builds a document package sharing the standard relationship wiring.
fn aux_parts(part: &str, rel_type: &str, target: &str, xml: String) -> Vec<(String, Vec<u8>)> {
    document_parts(
        "<w:p/>",
        &[
            (part, xml.into_bytes()),
            (
                "word/_rels/document.xml.rels",
                rels(&[("rIdAux", rel_type, target)]),
            ),
        ],
    )
}

#[test]
fn document_root_prolog_forms_parse() {
    // Both the one-line and the newline-separated forms must parse.
    for prolog in [
        "",
        "\n",
        "\r\n",
        "\r\n\r\n  ",
        "\n<!-- c -->\n<?pi data?>\n",
    ] {
        let result = parse_parts(&document_with_prolog(prolog));
        assert!(result.is_ok(), "prolog {prolog:?} failed: {result:?}");
    }
}

#[test]
fn styles_numbering_settings_prolog_parses() {
    let styles = format!("{DECL}\r\n<w:styles xmlns:w=\"{W_NS}\"><w:style w:type=\"paragraph\" w:styleId=\"C\"/></w:styles>");
    let document = parse_parts(&aux_parts("word/styles.xml", STYLES, "styles.xml", styles))
        .expect("styles with prolog must parse");
    assert_eq!(document.styles.len(), 1);

    let numbering = format!("{DECL}\n<w:numbering xmlns:w=\"{W_NS}\"><w:num w:numId=\"5\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>");
    let document = parse_parts(&aux_parts(
        "word/numbering.xml",
        NUMBERING,
        "numbering.xml",
        numbering,
    ))
    .expect("numbering with prolog must parse");
    assert!(!document.numbering.is_empty());

    let settings =
        format!("{DECL}\r\n\r\n<w:settings xmlns:w=\"{W_NS}\"><w:evenAndOddHeaders/></w:settings>");
    let document = parse_parts(&aux_parts(
        "word/settings.xml",
        SETTINGS,
        "settings.xml",
        settings,
    ))
    .expect("settings with prolog must parse");
    assert!(document.settings.even_and_odd_headers);
}

#[test]
fn significant_text_before_root_is_rejected() {
    let result = parse_parts(&document_with_prolog("oops"));
    assert!(result.is_err(), "non-whitespace prolog must be an error");
}

#[test]
fn non_strict_conformance_matches_root_by_local_name() {
    // The root is matched by local name whenever conformance is not `Strict`,
    // so a root element outside either family's namespace still parses.
    //
    // AUD-22 closed the hole an earlier version of this test relied on: the
    // `officeDocument` relationship used to carry a bogus authority
    // (`https://example.invalid/officeDocument`), which the pre-fix
    // `RelType::from_uri` matched by trailing path segment regardless of who
    // wrote the rest of the URI — the same bug that made
    // `http://evil.example/officeDocument` indistinguishable from the real
    // relationship type. `locate_main_document` now only recognizes the two
    // real `officeDocument` URIs, and `ns::registry::classify_relationship`
    // reads both of those as a conformance signal, so a package whose main
    // document actually resolves can no longer be conformance-`Unknown`. The
    // real Transitional URI is used instead; conformance comes out
    // `Transitional`, not `Strict`, which is what the parser's root check
    // keys on (`expect_root_ns`'s `require_strict_ns`) and so is exactly as
    // good a fixture for "root matched by local name" as `Unknown` was.
    let content_types = "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/xml\"/></Types>";
    let rels = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let document =
        "<?xml version=\"1.0\"?><w:document xmlns:w=\"urn:custom\"><w:body/></w:document>";
    let bytes = common::zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ]);
    let open_options = strict_ooxml_core::opc::OpenOptions::default()
        .conformance(strict_ooxml_core::opc::ConformancePolicy::Permissive);
    let package =
        strict_ooxml_core::opc::Package::open_reader(std::io::Cursor::new(bytes), &open_options)
            .expect("open package");
    assert_eq!(
        package.conformance(),
        strict_ooxml_core::ns::Conformance::Transitional
    );
    let parse_options = strict_ooxml_wml::ParseOptions {
        conformance: strict_ooxml_core::opc::ConformancePolicy::Permissive,
        ..Default::default()
    };
    strict_ooxml_wml::parse_document(&package, &parse_options)
        .expect("non-strict conformance document must still parse");
}

#[test]
fn foreign_namespace_root_in_strict_package_is_rejected() {
    // The shared `.rels` are Strict, so the package is detected as Strict; a
    // root element in an unrelated namespace must still be rejected even though
    // its local name matches.
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<w:document xmlns:w=\"urn:custom\"><w:body/></w:document>";
    let parts = vec![("word/document.xml".to_owned(), xml.as_bytes().to_vec())];
    let package = strict_ooxml_core::opc::Package::open_reader(
        std::io::Cursor::new(common::package_entries(&parts)),
        &strict_ooxml_core::opc::OpenOptions::default(),
    )
    .expect("open package");
    assert_eq!(
        package.conformance(),
        strict_ooxml_core::ns::Conformance::Strict
    );
    assert!(parse_parts(&parts).is_err());
}
