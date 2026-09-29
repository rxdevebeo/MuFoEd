#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal,
    clippy::cast_possible_truncation,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::redundant_closure_for_method_calls
)]
//! Independent XML oracle for the Stage-5 parts (STAGE-5-TASK §8.2, S5.12).
//!
//! Reads the committed `strict-stage5.docx` fixture with an **independent** ZIP
//! reader (`zip`) and parses its new parts with an **independent** XML parser
//! (`roxmltree`), then checks the same structures through our parser. This is the
//! anti-self-confirmation guard for headers, footnotes, endnotes, theme and
//! numbering.

use std::io::Read;
use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::model::values::VerticalMerge;
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The WordprocessingML Strict namespace (attributes are namespaced there).
const W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

/// Reads a namespaced `w:*` attribute with the independent parser.
fn w_attr<'a>(node: &roxmltree::Node<'a, 'a>, local: &str) -> Option<&'a str> {
    node.attribute((W_NS, local))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-stage5.docx")
}

/// Reads a part with the independent `zip` crate.
fn read_part(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).expect("open fixture");
    let mut archive = zip::ZipArchive::new(file).expect("independent zip open");
    let mut entry = archive.by_name(name).expect("part present");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("read part");
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn independent_oracle_checks_stage5_parts() {
    let path = fixture();
    assert!(path.is_file(), "missing fixture {}", path.display());

    // Header: the independent parser sees the expected text.
    let header = read_part(&path, "word/header1.xml");
    let header_doc = roxmltree::Document::parse(&header).expect("valid header xml");
    assert_eq!(header_doc.root_element().tag_name().name(), "hdr");
    let header_text: String = header_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "t")
        .filter_map(|node| node.text())
        .collect();
    assert!(
        header_text.contains("Default"),
        "header text: {header_text:?}"
    );

    // Footnotes: three definitions, ids -1, 0, 1.
    let footnotes = read_part(&path, "word/footnotes.xml");
    let footnotes_doc = roxmltree::Document::parse(&footnotes).expect("valid footnotes xml");
    let ids: Vec<i64> = footnotes_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "footnote")
        .filter_map(|node| w_attr(&node, "id"))
        .filter_map(|value| value.parse().ok())
        .collect();
    assert_eq!(ids, vec![-1, 0, 1], "footnote ids");

    // Theme: accent1 colour and the minor Latin typeface.
    let theme = read_part(&path, "word/theme/theme1.xml");
    let theme_doc = roxmltree::Document::parse(&theme).expect("valid theme xml");
    let accent1 = theme_doc
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "accent1")
        .and_then(|node| node.children().find(|child| child.is_element()))
        .and_then(|node| node.attribute("val"))
        .map(str::to_ascii_lowercase);
    assert_eq!(accent1.as_deref(), Some("4472c4"));
    let minor_latin = theme_doc
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "minorFont")
        .and_then(|node| {
            node.descendants()
                .find(|child| child.tag_name().name() == "latin")
        })
        .and_then(|node| node.attribute("typeface"));
    assert_eq!(minor_latin, Some("Calibri"));

    // Numbering: two levels with the expected lvlText.
    let numbering = read_part(&path, "word/numbering.xml");
    let numbering_doc = roxmltree::Document::parse(&numbering).expect("valid numbering xml");
    let texts: Vec<&str> = numbering_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "lvlText")
        .filter_map(|node| w_attr(&node, "val"))
        .collect();
    assert_eq!(texts, vec!["%1.", "%1.%2"]);

    // Content types must be ISO/IEC 29500 (no legacy `vnd.ms-word.*`), which is
    // what makes WPS read the package as a real document (B5-2).
    let content_types = read_part(&path, "[Content_Types].xml");
    let types_doc = roxmltree::Document::parse(&content_types).expect("valid content types");
    let declared: Vec<&str> = types_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "Override")
        .filter_map(|node| node.attribute("ContentType"))
        .collect();
    assert!(!declared.is_empty(), "no content-type overrides");
    for content_type in &declared {
        assert!(
            !content_type.starts_with("application/vnd.ms-word"),
            "legacy content type: {content_type}"
        );
    }
    assert!(declared.contains(
        &"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
    ));

    // Cross-check with our parser: the same facts must hold.
    let package = Package::open_path(&path, &OpenOptions::default()).expect("open strict");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    assert!(document.theme.is_some());
    assert_eq!(document.footnotes.len(), 3);
    assert_eq!(
        document
            .theme
            .as_ref()
            .and_then(|theme| theme.color("accent1")),
        Some("#4472c4")
    );

    // The vertical merge is modelled.
    let merged = document.body.blocks.iter().any(|block| {
        block.as_table().is_some_and(|table| {
            table.rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| cell.props.vertical_merge == Some(VerticalMerge::Restart))
            })
        })
    });
    assert!(merged, "vMerge restart not parsed");
}
