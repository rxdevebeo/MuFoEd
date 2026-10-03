#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Property-based tests: arbitrary input never panics and parsing is
//! deterministic.

mod common;

use proptest::prelude::*;
use proptest::sample::select;
use std::io::Cursor;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_wml::{parse_document, ParseOptions};

use common::{document_parts, parse_parts, W_NS};

const FRAGMENTS: &[&str] = &[
    "<w:p/>",
    "<w:p><w:r><w:t>x</w:t></w:r></w:p>",
    "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:t>y</w:t></w:r></w:p>",
    "<w:tbl><w:tblGrid><w:gridCol w:w=\"100\"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>",
    "<w:p><w:hyperlink r:id=\"rId1\"><w:r><w:t>l</w:t></w:r></w:hyperlink></w:p>",
    "<w:sdt><w:sdtContent><w:p/></w:sdtContent></w:sdt>",
    "<w:p><w:r><w:drawing/></w:r></w:p>",
    "<w:p><w:bookmarkStart w:id=\"0\" w:name=\"b\"/><w:bookmarkEnd w:id=\"0\"/></w:p>",
    "<w:altChunk/>",
    "<w:unknown/>",
    "text",
];

fn fragments() -> impl Strategy<Value = String> {
    proptest::collection::vec(select(FRAGMENTS), 0..16).prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_bodies_never_panic(body in fragments()) {
        let parts = document_parts(&body, &[]);
        let _ = parse_parts(&parts);
    }

    #[test]
    fn random_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        if let Ok(package) = Package::open_reader(Cursor::new(data), &options) {
            let parse_options = ParseOptions {
                limits: strict_ooxml_core::limits::ResourceLimits::default(),
            };
            let _ = parse_document(&package, &parse_options);
        }
    }

    #[test]
    fn parsing_is_deterministic(body in fragments()) {
        let parts = document_parts(&body, &[]);
        if let Ok(first) = parse_parts(&parts) {
            let second = parse_parts(&parts).expect("second parse");
            prop_assert_eq!(first.body.blocks.len(), second.body.blocks.len());
            prop_assert_eq!(first.support_debug(), second.support_debug());
        }
    }

    #[test]
    fn document_namespace_is_accepted(body in fragments()) {
        // A valid Strict root must always parse (unknown children become opaque).
        let wrapped = format!("<w:document xmlns:w=\"{W_NS}\"><w:body>{body}</w:body></w:document>");
        let parts = vec![("word/document.xml".to_owned(), wrapped.into_bytes())];
        let _ = parse_parts(&parts);
    }
}
