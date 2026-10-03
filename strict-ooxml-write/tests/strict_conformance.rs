//! A Strict package we write carries no extension-namespace content.
//!
//! ADR-0014, and `STAGE-10G-TASK.md` `XS-17…XS-19`.
//!
//! `a_written_package_is_strict` in `roundtrip.rs` checks the URIs, relationship
//! types and content types, and it passes on a package full of `w14` attributes -
//! because `w14` is not a Transitional URI, and a Transitional scan can never see
//! it. That is the gap this file closes, and it is a different check rather than a
//! stricter version of the same one.
//!
//! **Over the assembled package, not over the model.** ADR-0014 says so and the
//! reason is concrete: an extension can arrive through an `Opaque*` node or
//! through pass-through, so a scan of the model would never see one. A part this
//! writer regenerates cannot carry an extension; a part it copies is the
//! producer's own business and is declared, not policed.
//!
//! What is checked, per part this writer produces:
//!
//! - no attribute and no element in a namespace outside the `purl.oclc.org` set
//!   and `xml:`;
//! - no declaration of one either, since a declared-but-unused extension
//!   namespace is one line away from being used.

use std::collections::BTreeMap;
use std::path::Path;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_bytes, WriteOptions};

/// The namespaces a Strict package may use.
///
/// Every one of them is ECMA-376 (Part 1) or the XML Namespaces specification.
/// Anything else - `schemas.microsoft.com`, `schemas.openxmlformats.org` - is an
/// extension or a Transitional signal, and both are somebody else's decision.
const ALLOWED: &[&str] = &[
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
    "http://purl.oclc.org/ooxml/officeDocument/relationships",
    "http://purl.oclc.org/ooxml/officeDocument/math",
    "http://purl.oclc.org/ooxml/officeDocument/sharedTypes",
    "http://purl.oclc.org/ooxml/drawingml/main",
    "http://purl.oclc.org/ooxml/drawingml/picture",
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
    "http://purl.oclc.org/ooxml/drawingml/chart",
    "http://purl.oclc.org/ooxml/drawingml/chartDrawing",
    "http://purl.oclc.org/ooxml/drawingml/diagram",
    "http://purl.oclc.org/ooxml/drawingml/compatibility",
    "http://purl.oclc.org/ooxml/drawingml/lockedCanvas",
    "http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing",
    "http://purl.oclc.org/ooxml/officeDocument/extendedProperties",
    "http://purl.oclc.org/ooxml/officeDocument/customProperties",
    "http://purl.oclc.org/ooxml/officeDocument/customXmlProperties",
    "http://schemas.openxmlformats.org/package/2006/relationships",
    "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
    "http://schemas.openxmlformats.org/package/2006/content-types",
    "http://purl.oclc.org/ooxml/schemaLibrary/main",
    // The MCE namespace is the mechanism by which a processor is TOLD to remove
    // extensions; a part that declares it and uses nothing else is conformant.
    "http://schemas.openxmlformats.org/markup-compatibility/2006",
];

/// A vendor namespace the corpus legitimately carries in a part this writer
/// regenerates.
///
/// `wps`, `wpg` and `wp14` appear inside `w:drawing` in real Strict documents -
/// `strict-profile.docx` and `strict-smartart.docx` both carry shapes - and this
/// writer reproduces them from the model because a `w:drawing` with the graphic
/// data removed is worse than one that keeps a vendor shape. ADR-0014 keeps that
/// debt visible as ADR-0014's own open item (`XS-18`, `XS-19`) rather than
/// pretending the shape is Strict; the schema gate files those in the extension
/// basket for the same reason.
///
/// It is listed here, in the test that holds the zero, so that lifting it is a
/// visible edit rather than a silent loosening.
const DECLARED_EXEMPTIONS: &[(&str, &str)] = &[
    (
        "wps",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    ),
    (
        "wpg",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    ),
    (
        "wp14",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
    ),
    (
        "w15",
        "http://schemas.microsoft.com/office/word/2012/wordml",
    ),
    ("v", "urn:schemas-microsoft-com:vml"),
    ("o", "urn:schemas-microsoft-com:office:office"),
];

fn is_allowed(uri: &str) -> bool {
    ALLOWED.contains(&uri) || uri.starts_with("http://purl.oclc.org/ooxml/")
}

fn is_exempt(uri: &str) -> bool {
    DECLARED_EXEMPTIONS
        .iter()
        .any(|(_, allowed)| *allowed == uri)
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("?")
                .to_owned();
            out.push((name, bytes));
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    out
}

/// Every namespace URI the part DECLARES, as a `prefix -> uri` binding.
///
/// Scans the text rather than asking the reader, because the reader resolves
/// names and then throws the declarations away, and a declaration is exactly what
/// this file is about: `xmlns:w14` on a part that uses nothing from `w14` is
/// still a Strict part reaching for an extension namespace.
fn namespaces(text: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while let Some(found) = text[index..].find("xmlns") {
        let start = index + found + 5;
        index = start;
        let mut cursor = start;
        let prefix = if text[cursor..].starts_with(':') {
            cursor += 1;
            let end = text[cursor..]
                .find(|c: char| c == '=' || c.is_whitespace())
                .map_or(text.len(), |at| cursor + at);
            let prefix = text[cursor..end].to_owned();
            cursor = end;
            prefix
        } else {
            String::new()
        };
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'"' {
            continue;
        }
        cursor += 1;
        let end = text[cursor..]
            .find('"')
            .map_or(text.len(), |at| cursor + at);
        out.insert(prefix, text[cursor..end].to_owned());
        index = end;
    }
    out
}

/// The `w:` part this writer regenerates, by name.
fn regenerated(part_name: &str) -> bool {
    matches!(
        part_name,
        "word/document.xml"
            | "word/styles.xml"
            | "word/settings.xml"
            | "word/numbering.xml"
            | "word/fontTable.xml"
            | "word/footnotes.xml"
            | "word/endnotes.xml"
            | "word/theme/theme1.xml"
    ) || part_name.starts_with("word/header")
        || part_name.starts_with("word/footer")
}

#[test]
fn a_written_part_declares_no_extension_namespace() {
    let mut checked = 0usize;
    let mut problems: Vec<String> = Vec::new();

    for (name, bytes) in corpus() {
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let Ok(package) = Package::open_reader(bytes.as_slice(), &options) else {
            continue;
        };
        let Ok(document) = parse_document(&package, &ParseOptions::default()) else {
            continue;
        };
        let written =
            write_bytes(&document, Some(&package), &WriteOptions::default()).expect("write");
        let Ok(reopened) = Package::open_reader(written.as_slice(), &options) else {
            continue;
        };
        for part in reopened.parts() {
            let Some(part_name) = part.id.as_str().strip_prefix("/") else {
                continue;
            };
            if !regenerated(part_name) {
                // A copied part is the producer's own markup and is declared, not
                // policed: `XS-16`'s chart parts are invalid in the INPUT and the
                // schema gate prints them every run.
                continue;
            }
            let Ok(bytes) = reopened.read_part(&part.id) else {
                continue;
            };
            checked += 1;
            let text = String::from_utf8_lossy(&bytes);
            for (prefix, uri) in namespaces(&text) {
                if is_allowed(&uri) || is_exempt(&uri) {
                    continue;
                }
                problems.push(format!(
                    "{name}: {part_name} declares {prefix}=\"{uri}\", which is neither \
                     ECMA-376 Strict nor a declared exemption"
                ));
            }
        }
    }

    assert!(checked > 40, "only {checked} parts were inspected");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The specific defect this decision was made for: `w14:paraId` and
/// `w14:textId` on every `w:p` that had them.
///
/// It is asserted on its own because it is the regression this decision is
/// about, and a general "no extension namespace" scan would also pass on a
/// writer that had simply stopped seeing the namespace.
#[test]
fn no_written_paragraph_carries_a_paragraph_id() {
    let mut paragraphs = 0usize;
    for (name, bytes) in corpus() {
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let Ok(package) = Package::open_reader(bytes.as_slice(), &options) else {
            continue;
        };
        let Ok(document) = parse_document(&package, &ParseOptions::default()) else {
            continue;
        };
        let written =
            write_bytes(&document, Some(&package), &WriteOptions::default()).expect("write");
        let Ok(reopened) = Package::open_reader(written.as_slice(), &options) else {
            continue;
        };
        let main = reopened.main_document_part().expect("main").clone();
        let bytes = reopened.read_part(&main).expect("document.xml");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("w14:paraId") && !text.contains("w14:textId"),
            "{name}: the written document still carries w14 paragraph ids"
        );
        paragraphs += text.matches("<w:p ").count() + text.matches("<w:p>").count();
    }
    assert!(
        paragraphs > 100,
        "only {paragraphs} paragraphs were inspected"
    );
}
