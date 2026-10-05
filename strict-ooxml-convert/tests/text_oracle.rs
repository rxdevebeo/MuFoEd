//! A04: the preservation score must reject a total substitution.

#![allow(clippy::float_cmp)]

mod common;

use common::text::{body_text, flawed_recall, header_footer_text, score};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn recall(want: &str, have: &str) -> f64 {
    score(want, have)
        .recall()
        .unwrap_or_else(|| panic!("recall of {want:?} against {have:?} has a zero denominator"))
}

fn precision(want: &str, have: &str) -> f64 {
    score(want, have)
        .precision()
        .unwrap_or_else(|| panic!("precision of {want:?} against {have:?} has a zero denominator"))
}

#[test]
fn f01_text_metric_rejects_total_substitution() {
    assert_eq!(flawed_recall("abc", "Z"), 1.0);
    assert_eq!(recall("abc", "Z"), 0.0);
}

#[test]
fn f01_text_metric_matrix() {
    assert!((recall("abc", "c") - 1.0 / 3.0).abs() < 1e-12);
    assert!((recall("abc", "ab") - 2.0 / 3.0).abs() < 1e-12);
    assert!((recall("abc", "ac") - 2.0 / 3.0).abs() < 1e-12);
    assert!((recall("abc", "bc") - 2.0 / 3.0).abs() < 1e-12);
    assert!(recall("abc", "cba") < 1.0);
    assert!(precision("abc", "abcZZ") < 1.0);
    assert!((recall("a b", "ab") - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(recall("a\r\nb", "a\nb"), 1.0);
    assert_eq!(recall("Привет", "Привет"), 1.0);
    assert!((recall("Привет", "П") - 1.0 / 6.0).abs() < 1e-12);
}

#[test]
fn f01_table_text_is_in_the_body_and_headers_stay_separate() {
    let bytes = DocxBuilder::strict()
        .body(
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/>\
</w:sectPr>",
        )
        .rel("rIdHeader", "header", "header1.xml")
        .content_type(
            "/word/header1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        )
        .part_xml(
            "word/header1.xml",
            "w:hdr",
            "<w:p><w:r><w:t>HEADER</w:t></w:r></w:p>",
        )
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let body = body_text(&document);
    let headers = header_footer_text(&document);
    assert!(body.contains("cell"), "{body}");
    assert!(!body.contains("HEADER"), "{body}");
    assert!(headers.contains("HEADER"), "{headers}");
}
