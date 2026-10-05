//! The `pdf` feature renders a document when it is the only facade feature.

#![cfg(feature = "pdf")]
#![allow(missing_docs)]

use std::io::Cursor;

use strict_ooxml::{OpenOptions, RenderOptions, StrictDocument};
use strict_ooxml_testkit::DocxBuilder;

#[test]
fn f02_pdf_feature_renders() {
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>Hello</w:t></w:r></w:p>")
        .build();
    let document =
        StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let pdf = document
        .render_pdf(&RenderOptions::default())
        .expect("render");
    assert!(
        pdf.bytes.starts_with(b"%PDF-"),
        "pdf feature did not produce a PDF"
    );
    assert!(pdf.page_count >= 1, "pdf feature produced zero pages");
}
