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
use strict_ooxml_wml::model::values::{DocGridType, PageOrientation, SectionType};

use common::{document_parts, parse_parts, rels, W_NS};

const HEADER: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/header";

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
        ("rIdFooter", HEADER, "footer1.xml"),
    ]);
    let parts = document_parts(&body, &[("word/_rels/document.xml.rels", rels)]);
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
    assert_eq!(second.footers[0].kind, HeaderFooterKind::Even);
    assert!(second.title_page);
    assert_eq!(second.doc_grid.unwrap().grid_type, Some(DocGridType::Lines));
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
