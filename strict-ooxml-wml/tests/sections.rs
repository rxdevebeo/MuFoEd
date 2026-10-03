#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for sections (`w:sectPr`), page setup and columns.

mod common;

use strict_ooxml_wml::model::props::HeaderFooterKind;
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::{DocGridType, PageOrientation, SectionType, Twips};

use common::{document_parts, parse_parts, rels, W_NS};

const HEADER: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/header";
const FOOTER: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footer";

/// A minimal Strict header part.
fn header_xml(text: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:hdr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>"
    )
    .into_bytes()
}

/// A minimal Strict footer part.
fn footer_xml(text: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:ftr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:ftr>"
    )
    .into_bytes()
}

#[test]
fn parses_sections_page_setup_and_columns() {
    let body = format!(
        "<w:p><w:pPr><w:sectPr>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:type w:val=\"continuous\"/>\
</w:sectPr></w:pPr><w:r><w:t>first</w:t></w:r></w:p>\
<w:p><w:r><w:t>second</w:t></w:r></w:p>\
<w:sectPr>\
<w:pgSz w:w=\"11906\" w:h=\"16838\" w:orient=\"landscape\"/>\
<w:cols w:num=\"2\" w:space=\"425\"><w:col w:w=\"4000\"/><w:col w:w=\"4000\" w:space=\"200\"/></w:cols>\
<w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>\
<w:footerReference w:type=\"even\" r:id=\"rIdFooter\"/>\
<w:titlePg/><w:docGrid w:type=\"lines\" w:linePitch=\"360\"/>\
</w:sectPr>"
    );
    let rels = rels(&[
        ("rIdHeader", HEADER, "header1.xml"),
        ("rIdFooter", FOOTER, "footer1.xml"),
    ]);
    let parts = document_parts(
        &body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", header_xml("Head")),
            ("word/footer1.xml", footer_xml("Foot")),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    assert_eq!(document.sections.len(), 2);

    let first = &document.sections[0].properties;
    assert_eq!(first.section_type, Some(SectionType::Continuous));
    let size = first.page_size.unwrap();
    assert_eq!(size.width.unwrap().value(), 12240);
    assert_eq!(first.page_margins.unwrap().top.unwrap().value(), 1440);

    let second = &document.sections[1].properties;
    let size = second.page_size.unwrap();
    assert_eq!(size.orientation, Some(PageOrientation::Landscape));
    let columns = second.columns.as_ref().unwrap();
    assert_eq!(columns.count, Some(2));
    assert_eq!(columns.columns.len(), 2);
    assert_eq!(columns.columns[1].space.unwrap().value(), 200);
    assert_eq!(second.headers.len(), 1);
    assert_eq!(second.headers[0].kind, HeaderFooterKind::Default);
    assert!(second.headers[0].part.is_some());
    assert_eq!(second.footers[0].kind, HeaderFooterKind::Even);
    assert!(second.footers[0].part.is_some());
    assert!(second.title_page);
    assert_eq!(second.doc_grid.unwrap().grid_type, Some(DocGridType::Lines));

    // The referenced parts are parsed once each.
    assert_eq!(document.headers_footers.len(), 2);
    assert!(document.headers_footers.iter().any(|hf| hf.is_header));
    assert!(document.headers_footers.iter().any(|hf| !hf.is_header));
}

#[test]
fn writes_unresolved_header_reference_report() {
    // No document rels: the header reference cannot be resolved.
    let body = format!(
        "<w:p/><w:sectPr xmlns:w=\"{W_NS}\"><w:headerReference w:type=\"default\" r:id=\"rIdMissing\"/></w:sectPr>"
    );
    let parts = document_parts(&body, &[]);
    let document = parse_parts(&parts).expect("parse");
    assert!(
        document.support.get("w:headerReference").is_some(),
        "expected a headerReference support entry"
    );
    assert_eq!(
        document.support.get("w:headerReference").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn parses_header_and_footer_block_content() {
    let body = "<w:sectPr>\
<w:headerReference w:type=\"default\" r:id=\"rIdH\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdF\"/>\
</w:sectPr>";
    let rels = rels(&[
        ("rIdH", HEADER, "header1.xml"),
        ("rIdF", FOOTER, "footer1.xml"),
    ]);
    let parts = document_parts(
        body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", header_xml("Header text")),
            ("word/footer1.xml", footer_xml("Footer text")),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let header = document
        .headers_footers
        .iter()
        .find(|hf| hf.is_header)
        .expect("header parsed");
    let footer = document
        .headers_footers
        .iter()
        .find(|hf| !hf.is_header)
        .expect("footer parsed");
    assert_eq!(header.blocks.len(), 1);
    assert_eq!(footer.blocks.len(), 1);
    assert!(document.support.get("w:hdr").is_some());
    assert!(document.support.get("w:ftr").is_some());
    assert_eq!(
        document.support.get("w:headerReference").unwrap().status,
        SupportStatus::Supported
    );
}

#[test]
fn missing_referenced_part_is_an_error() {
    let body = "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdH\"/></w:sectPr>";
    let rels = rels(&[("rIdH", HEADER, "header1.xml")]);
    // The relationship exists but the header part is absent.
    let parts = document_parts(body, &[("word/_rels/document.xml.rels", rels)]);
    let error = parse_parts(&parts).expect_err("missing header part must fail");
    assert!(
        matches!(
            error,
            strict_ooxml_core::error::StrictError::MissingReferencedPart { .. }
        ),
        "unexpected error: {error:?}"
    );
}

/// AUD-40: `w:sectPr` inside `w:sdt` and a trailing body `w:sectPr` stay in order.
#[test]
fn sections_from_sdt_and_body_stay_aligned() {
    // p, sdt{ p[sectPr A] }, p, p[sectPr B], body/sectPr C → [A, B, C]
    let body = "\
<w:p><w:r><w:t>before</w:t></w:r></w:p>\
<w:sdt><w:sdtContent>\
<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"10000\" w:h=\"15840\"/></w:sectPr></w:pPr>\
<w:r><w:t>in-sdt</w:t></w:r></w:p>\
</w:sdtContent></w:sdt>\
<w:p><w:r><w:t>middle</w:t></w:r></w:p>\
<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"11000\" w:h=\"15840\"/></w:sectPr></w:pPr>\
<w:r><w:t>end-sect</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12000\" w:h=\"15840\"/></w:sectPr>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    assert_eq!(document.sections.len(), 3);
    let widths: Vec<i32> = document
        .sections
        .iter()
        .map(|s| s.properties.page_size.unwrap().width.unwrap().value())
        .collect();
    assert_eq!(widths, vec![10000, 11000, 12000]);

    // Paragraph copies stay aligned with `sections` (sync uses the same walk).
    let seen = paragraph_section_widths(&document.body.blocks);
    assert_eq!(seen, vec![10000, 11000]);
}

fn paragraph_section_widths(blocks: &[strict_ooxml_wml::model::block::Block]) -> Vec<i32> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            strict_ooxml_wml::model::block::Block::Paragraph(p) => {
                if let Some(sect) = &p.props.section {
                    out.push(sect.page_size.unwrap().width.unwrap().value());
                }
            }
            strict_ooxml_wml::model::block::Block::SdtBlock(sdt) => {
                out.extend(paragraph_section_widths(&sdt.blocks));
            }
            strict_ooxml_wml::model::block::Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        out.extend(paragraph_section_widths(&cell.blocks));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// AUD-40: a section break wrapped in `w:ins` is collected in document order.
#[test]
fn sections_inside_revision_markers_are_collected() {
    let body = "\
<w:p><w:r><w:t>a</w:t></w:r></w:p>\
<w:ins w:id=\"0\" w:author=\"x\">\
<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"9000\" w:h=\"15840\"/></w:sectPr></w:pPr>\
<w:r><w:t>inserted</w:t></w:r></w:p>\
</w:ins>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    assert_eq!(document.sections.len(), 2);
    assert_eq!(
        document.sections[0]
            .properties
            .page_size
            .unwrap()
            .width
            .unwrap()
            .value(),
        9000
    );
    assert_eq!(
        document.sections[1]
            .properties
            .page_size
            .unwrap()
            .width
            .unwrap()
            .value(),
        12240
    );
}

/// AUD-40: `w:sectPr` inside a table cell is kept and reported as partial.
#[test]
fn section_break_inside_table_cell_is_recorded() {
    let body = "\
<w:tbl><w:tr><w:tc>\
<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"8000\" w:h=\"15840\"/></w:sectPr></w:pPr>\
<w:r><w:t>cell</w:t></w:r></w:p>\
</w:tc></w:tr></w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    assert_eq!(document.sections.len(), 2);
    assert_eq!(
        document.sections[0]
            .properties
            .page_size
            .unwrap()
            .width
            .unwrap()
            .value(),
        8000
    );
    let entry = document.support.get("w:sectPr").expect("partial record");
    assert_eq!(entry.status, SupportStatus::Partial);
    assert!(
        entry
            .message
            .as_deref()
            .unwrap_or("")
            .contains("table cell"),
        "message={:?}",
        entry.message
    );
}

#[test]
fn header_reference_with_wrong_relationship_type_is_partial() {
    let body = "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdF\"/></w:sectPr>";
    // The reference points at a footer relationship.
    let rels = rels(&[("rIdF", FOOTER, "footer1.xml")]);
    let parts = document_parts(
        body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/footer1.xml", footer_xml("Footer")),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let reference = &document.sections[0].properties.headers[0];
    assert!(reference.part.is_none());
    assert!(document.headers_footers.is_empty());
    assert_eq!(
        document.support.get("w:headerReference").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn zero_page_size_is_partial() {
    // AUD-73: pgSz w="0" h="0" stays in the model but is recorded as Partial.
    let body = "<w:sectPr><w:pgSz w:w=\"0\" w:h=\"0\"/></w:sectPr>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    let size = document.sections[0]
        .properties
        .page_size
        .expect("page_size");
    assert_eq!(size.width.map(Twips::value), Some(0));
    assert_eq!(size.height.map(Twips::value), Some(0));
    let entry = document.support.get("w:pgSz").expect("w:pgSz");
    assert_eq!(entry.status, SupportStatus::Partial);
    assert!(
        entry.message.as_deref().unwrap_or("").contains("Letter"),
        "message={:?}",
        entry.message
    );
}
