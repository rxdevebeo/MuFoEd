//! Shared helpers for the `strict-ooxml-render-svg` integration tests.

#![allow(
    dead_code,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::doc_markdown
)]

use std::io::Cursor;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

/// Strict WML namespace.
pub(crate) const W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
/// Strict relationships namespace.
pub(crate) const R: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// Strict wordprocessingDrawing namespace.
pub(crate) const WP: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
/// Strict DrawingML namespace.
pub(crate) const A: &str = "http://purl.oclc.org/ooxml/drawingml/main";
/// Strict picture namespace.
pub(crate) const PIC: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// Strict officeDocument relationship type.
pub(crate) const DOC_REL: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
/// Strict image relationship type.
pub(crate) const IMAGE_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/image";

/// Builds a minimal Strict document wrapping `body`.
pub(crate) fn document(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{W}\" xmlns:r=\"{R}\" xmlns:wp=\"{WP}\" xmlns:a=\"{A}\" xmlns:pic=\"{PIC}\"><w:body>{body}</w:body></w:document>"
    )
}

/// Returns the `[Content_Types].xml` for a document (with PNG media support).
pub(crate) fn content_types() -> String {
    "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".to_owned()
}

/// Returns the root relationships part.
pub(crate) fn root_rels() -> String {
    format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{DOC_REL}\" Target=\"word/document.xml\"/></Relationships>"
    )
}

/// Builds and parses a Strict `.docx` from a body, returning the package too.
pub(crate) fn open_body(body: &str) -> (Package, Document) {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    open_bytes(bytes)
}

/// Builds and parses a Strict `.docx` that contains one inline PNG image.
pub(crate) fn open_with_image() -> (Package, Document) {
    let png = tiny_png();
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/><wp:docPr id=\"1\" name=\"pic\" descr=\"a description\"/><a:graphic><a:graphicData uri=\"{PIC}\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"pic\" descr=\"a description\"/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId1\"/></pic:blipFill><pic:spPr><a:ext cx=\"914400\" cy=\"914400\"/></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    let doc_rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{IMAGE_REL}\" Target=\"media/image1.png\"/></Relationships>"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.into_bytes()),
        ("word/media/image1.png", png),
    ]);
    open_bytes(bytes)
}

/// Parses Strict bytes into a package and document.
pub(crate) fn open_bytes(bytes: Vec<u8>) -> (Package, Document) {
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default())
        .expect("open strict package");
    let document =
        parse_document(&package, &ParseOptions::default()).expect("parse strict document");
    (package, document)
}

/// Renders `body` with default options.
pub(crate) fn render_body(body: &str) -> Vec<strict_ooxml_render_svg::Page> {
    let (_package, document) = open_body(body);
    strict_ooxml_render_svg::render(&document, &Default::default()).expect("render")
}

/// A tiny valid PNG (1x1, 8-bit RGBA).
pub(crate) fn tiny_png() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I',
        b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ]
}

/// Builds a ZIP archive with stored (uncompressed) entries.
pub(crate) fn build_docx(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        let crc = crc32(content);
        push_local(&mut local, name, crc, content);
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

fn push_local(out: &mut Vec<u8>, name: &str, crc: u32, content: &[u8]) {
    out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
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
