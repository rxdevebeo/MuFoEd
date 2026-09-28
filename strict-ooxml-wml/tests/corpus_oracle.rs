#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal,
    clippy::cast_possible_truncation,
    clippy::case_sensitive_file_extension_comparisons
)]
//! Independent WML oracle (STAGE-2-WORK-ORDER P-1).
//!
//! The Stage-2 attribute defects (D-1 `w:tab`, D-2 fractional measurements) were
//! invisible to self-authored fixtures. This test derives the *lexical forms*
//! from the real public corpus (`strict-ooxml-core/tests/samples/`) using an
//! **independent** ZIP reader (`zip`), never our own parser, and then checks that
//! the parser applies those real values. On the pre-fix code it is red:
//! `w:tab` positions read from `w:val` come back as `None`, and `parse_i32` drops
//! `1872.0000000000002`.
//!
//! Sources of truth:
//! - element/attribute names: ISO/IEC 29500-1 `CT_TabStop`, `CT_TblWidth`,
//!   `CT_PageMar`, `CT_TblGridCol` (transcribed in the schema cases below);
//! - lexical values: the real `.docx` corpus, read independently.

mod common;

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::values::TabAlignment;
use strict_ooxml_wml::model::Document;

use common::{document_parts, parse_parts, W_NS};

/// Messages written to stderr by the differential scan (visible with --nocapture).
fn note(message: &str) {
    eprintln!("[wml-oracle] {message}");
}

fn samples() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/samples");
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

/// Reads `word/document.xml` from a sample with the independent `zip` crate.
fn document_xml(path: &Path) -> String {
    let file = std::fs::File::open(path).expect("open sample");
    let mut archive = zip::ZipArchive::new(file).expect("independent zip open");
    let mut entry = archive
        .by_name("word/document.xml")
        .expect("word/document.xml present");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("read document.xml");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Extracts `name="value"` from a tag substring (independent of the parser).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// Independent scan for raw `<w:tab ...>` tags (excludes `<w:tabs>`).
fn scan_tab_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find("<w:tab") {
        let after = &rest[index..];
        let boundary = after.as_bytes().get(6).copied();
        if matches!(boundary, Some(b' ' | b'/' | b'>')) {
            let end = after.find('>').map_or(after.len(), |end| end + 1);
            tags.push(after[..end].to_owned());
        }
        rest = &rest[index + 6..];
    }
    tags
}

/// Independent scan for decimal measurement values (`w:w`, margins, `w:pos`).
fn scan_decimal_measures(text: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    for name in [
        "w:w", "w:left", "w:right", "w:top", "w:bottom", "w:h", "w:pos", "w:space",
    ] {
        let key = format!(" {name}=\"");
        let mut rest = text;
        while let Some(index) = rest.find(&key) {
            let after = &rest[index + key.len()..];
            if let Some(end) = after.find('"') {
                let value = &after[..end];
                if value.contains('.') && value.parse::<f64>().is_ok() {
                    values.insert(value.to_owned());
                }
                rest = &after[end..];
            } else {
                break;
            }
        }
    }
    values
}

fn parse_body(body: &str) -> Document {
    parse_parts(&document_parts(body, &[])).expect("parse strict body")
}

/// Independent expected alignment for an `ST_TabJc` lexical value.
fn expected_alignment(value: &str) -> Option<TabAlignment> {
    Some(match value {
        "start" | "left" => TabAlignment::Start,
        "end" | "right" => TabAlignment::End,
        "center" => TabAlignment::Center,
        "decimal" => TabAlignment::Decimal,
        "bar" => TabAlignment::Bar,
        "num" => TabAlignment::Num,
        "clear" => TabAlignment::Clear,
        _ => return None,
    })
}

#[test]
fn corpus_tab_tags_are_all_parsed() {
    let mut all_tags = Vec::new();
    let mut values = BTreeSet::new();
    for path in samples() {
        let text = document_xml(&path);
        for tag in scan_tab_tags(&text) {
            if let Some(value) = attribute(&tag, "w:val") {
                values.insert(value);
            }
            all_tags.push(tag);
        }
    }
    assert!(!all_tags.is_empty(), "corpus has no <w:tab> tags");
    note(&format!(
        "corpus: {} <w:tab> tags across {} file(s)",
        all_tags.len(),
        samples().len()
    ));

    // Embed every real tab tag in one Strict paragraph and require that the
    // parser returns exactly as many stops (D-1 criterion 5).
    let tabs = all_tags.join("");
    let body = format!("<w:p><w:pPr><w:tabs>{tabs}</w:tabs></w:pPr></w:p>");
    let document = parse_body(&body);
    let parsed = &document.body.blocks[0].as_paragraph().unwrap().props.tabs;
    assert_eq!(
        parsed.len(),
        all_tags.len(),
        "parsed tab count != independent count"
    );

    // Every distinct alignment lexeme maps exactly per ST_TabJc (schema source).
    for value in &values {
        let tag = format!("<w:tab w:val=\"{value}\" w:pos=\"100\"/>");
        let body = format!("<w:p><w:pPr><w:tabs>{tag}</w:tabs></w:pPr></w:p>");
        let document = parse_body(&body);
        let stop = &document.body.blocks[0].as_paragraph().unwrap().props.tabs[0];
        match expected_alignment(value) {
            Some(expected) => {
                assert_eq!(stop.alignment, expected, "alignment for w:val=\"{value}\"");
            }
            None => assert!(
                document.support.get("w:tab").is_some(),
                "unknown w:val=\"{value}\" must be recorded"
            ),
        }
    }
    note(&format!(
        "distinct tab alignments: {:?}",
        values.iter().collect::<Vec<_>>()
    ));
}

#[test]
fn corpus_decimal_measures_are_applied() {
    let mut values = BTreeSet::new();
    for path in samples() {
        values.extend(scan_decimal_measures(&document_xml(&path)));
    }
    assert!(
        !values.is_empty(),
        "corpus has no fractional measurement values"
    );
    note(&format!(
        "{} distinct fractional measurement lexemes (e.g. {:?})",
        values.len(),
        values.iter().take(3).collect::<Vec<_>>()
    ));

    for value in values.iter().take(64) {
        let expected = value.parse::<f64>().expect("numeric").round() as i32;
        let body = format!(
            "<w:tbl><w:tblPr><w:tblInd w:w=\"{value}\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol/></w:tblGrid></w:tbl>"
        );
        let document = parse_body(&body);
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("expected table");
        };
        let applied = table
            .props
            .indent
            .unwrap_or_else(|| panic!("w:tblInd w:w=\"{value}\" was silently dropped"));
        assert_eq!(
            applied.value(),
            expected,
            "w:tblInd w:w=\"{value}\" rounded incorrectly"
        );
    }
}

#[test]
fn schema_attribute_names_are_used() {
    // Pre-fix regression guard: the historically wrong form (position in
    // `w:val`, alignment in `w:jc`) must NOT satisfy the schema contract. If the
    // parser regressed to reading those, this becomes red.
    let wrong = parse_body(
        "<w:p><w:pPr><w:tabs><w:tab w:val=\"720\" w:jc=\"center\" w:leader=\"dot\"/></w:tabs></w:pPr></w:p>",
    );
    let stop = &wrong.body.blocks[0].as_paragraph().unwrap().props.tabs[0];
    assert_ne!(
        stop.position.value(),
        720,
        "position must not come from w:val"
    );
    assert_ne!(
        stop.alignment,
        TabAlignment::Center,
        "alignment must not come from w:jc"
    );

    // Schema-correct form.
    let right = parse_body(
        "<w:p><w:pPr><w:tabs><w:tab w:val=\"right\" w:leader=\"none\" w:pos=\"9360\"/></w:tabs></w:pPr></w:p>",
    );
    let stop = &right.body.blocks[0].as_paragraph().unwrap().props.tabs[0];
    assert_eq!(stop.position.value(), 9360);
    assert_eq!(stop.alignment, TabAlignment::End);
    assert_eq!(
        stop.leader,
        Some(strict_ooxml_wml::model::values::TabLeader::None)
    );

    // D-2 schema contract: a fractional `w:pgMar`/`w:tblInd` is applied.
    let fractional = parse_body(&format!(
        "<w:p/><w:sectPr xmlns:w=\"{W_NS}\"><w:pgMar w:left=\"1872.0000000000002\"/></w:sectPr>"
    ));
    let left = fractional.sections[0]
        .properties
        .page_margins
        .unwrap()
        .left
        .unwrap();
    assert_eq!(left.value(), 1872);
}
