//! Shared helpers for Stage-2 integration tests.
//!
//! Builds minimal Strict `.docx` packages in memory (no binary fixtures) and
//! parses them through the public API.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::unreadable_literal
)]

use std::io::Cursor;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

/// WML Strict namespace.
pub const W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
/// Relationships Strict namespace.
pub const R_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// DrawingML main Strict namespace.
pub const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
/// wordprocessingDrawing Strict namespace.
pub const WP_NS: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
/// DrawingML picture Strict namespace.
pub const PIC_NS: &str = "http://purl.oclc.org/ooxml/drawingml/picture";

/// Default `[Content_Types].xml` for the synthetic packages.
pub const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Default Extension=\"gif\" ContentType=\"image/gif\"/>\
<Default Extension=\"emf\" ContentType=\"image/x-emf\"/>\
<Default Extension=\"wmf\" ContentType=\"image/x-wmf\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.ms-word.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.ms-word.styles+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.ms-word.numbering+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.ms-word.settings+xml\"/>\
</Types>";

/// Root relationships declaring the main document part.
pub const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

/// Wraps a body fragment in a `w:document` with the common namespace prefixes.
pub fn document_xml(body: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" xmlns:wp=\"{WP_NS}\" xmlns:a=\"{A_NS}\" xmlns:pic=\"{PIC_NS}\">\
<w:body>{body}</w:body></w:document>"
    )
    .into_bytes()
}

/// Builds a document-part relationships string from `(id, type, target)` triples.
pub fn rels(entries: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (id, type_uri, target) in entries {
        xml.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{type_uri}\" Target=\"{target}\"/>"
        ));
    }
    xml.push_str("</Relationships>");
    xml.into_bytes()
}

/// Builds a package from `(part_name, bytes)` entries plus the shared defaults.
pub fn package_entries(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
    ];
    for (name, bytes) in parts {
        entries.push((name.as_str(), bytes.as_slice()));
    }
    zip(&entries)
}

/// Parses a package built from the given parts with the default (Strict) policy.
pub fn parse_parts(parts: &[(String, Vec<u8>)]) -> Result<Document> {
    let bytes = package_entries(parts);
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default())?;
    parse_document(&package, &ParseOptions::default())
}

/// Parses a package with the permissive core policy (for Transitional input).
pub fn parse_parts_permissive(parts: &[(String, Vec<u8>)]) -> (Package, Result<Document>) {
    let bytes = package_entries(parts);
    let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    let package = Package::open_reader(Cursor::new(bytes), &options).expect("open package");
    let parse_options = ParseOptions {
        conformance: ConformancePolicy::Permissive,
        limits: strict_ooxml_core::limits::ResourceLimits::default(),
    };
    let document = parse_document(&package, &parse_options);
    (package, document)
}

/// Builds a document package with the given body and optional extra parts.
pub fn document_parts(body: &str, extra: &[(&str, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
    let mut parts: Vec<(String, Vec<u8>)> =
        vec![("word/document.xml".to_owned(), document_xml(body))];
    for (name, bytes) in extra {
        parts.push(((*name).to_owned(), bytes.clone()));
    }
    parts
}

/// Builds a stored (uncompressed) ZIP archive from `(name, bytes)` entries.
pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        push_local(&mut local, name, crc32(content), content.len(), content);
    }
    let cd_offset = local.len() as u32;
    for ((name, content), offset) in entries.iter().zip(offsets) {
        push_central(&mut central, name, crc32(content), content.len(), offset);
    }
    let cd_size = central.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn push_local(out: &mut Vec<u8>, name: &str, crc: u32, size: usize, content: &[u8]) {
    out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(content);
}

fn push_central(out: &mut Vec<u8>, name: &str, crc: u32, size: usize, offset: u32) {
    out.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
