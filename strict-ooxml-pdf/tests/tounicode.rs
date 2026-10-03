//! AUD-82: two characters that share a glyph both survive text extraction.
//!
//! Space and NBSP map to the same outline in most faces. Before the fix the
//! writer kept only the first character in `ToUnicode`, so «a b\u{A0}c» came
//! back without U+00A0.

#![allow(clippy::doc_markdown)]

use std::io::Cursor;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

#[test]
fn space_and_nbsp_both_survive_text_extraction() {
    let body = "<w:p><w:r><w:t xml:space=\"preserve\">a b\u{A0}c</w:t></w:r></w:p>";
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let pages = place_pages(&document, &options, Some(&package)).expect("place");
    let pdf = render_with_source(&pages, &options, Some(&package)).expect("render");
    let mut reader = PdfDocument::open(&pdf.bytes, PdfLimits::default()).expect("pdf open");
    let page = reader.page(1).expect("page");
    // `page.text()` inserts spaces between glyphs; collect raw mapped characters.
    let text: String = page
        .items()
        .iter()
        .filter_map(|item| match item {
            strict_ooxml_pdf::Item::Glyph(glyph) if glyph.mapped => Some(glyph.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        text.contains('\u{A0}'),
        "extracted text must keep U+00A0, got {text:?}"
    );
    assert!(
        text.contains(' ') && text.contains('a') && text.contains('b') && text.contains('c'),
        "extracted text must keep the rest of the run, got {text:?}"
    );
}
