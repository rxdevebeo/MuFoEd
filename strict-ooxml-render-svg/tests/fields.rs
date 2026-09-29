//! Field rendering tests: PAGE/NUMPAGES/SECTIONPAGES and cache fallback
//! (STAGE-5 S5.6).

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::unreadable_literal
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, Page, RenderOptions};

const PAGE_SETUP: &str = "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>";

/// Builds a Strict package with the given body and renders it.
fn build(body: &str) -> Vec<Page> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default()).expect("render")
}

/// A two-page body: `PAGE`/`NUMPAGES` fields on each page with a wrong cache.
fn two_page_body(instruction: &str) -> String {
    format!(
        "<w:p><w:r><w:t>A:</w:t></w:r><w:fldSimple w:instr=\"{instruction}\"><w:r><w:t>9</w:t></w:r></w:fldSimple></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>B:</w:t></w:r><w:fldSimple w:instr=\"{instruction}\"><w:r><w:t>9</w:t></w:r></w:fldSimple></w:p>\
<w:sectPr>{PAGE_SETUP}</w:sectPr>"
    )
}

#[test]
fn page_field_renders_current_page_number() {
    let pages = build(&two_page_body(" PAGE "));
    assert_eq!(pages.len(), 2);
    assert!(pages[0].svg.contains(">1</text>"), "page 1 field");
    assert!(pages[1].svg.contains(">2</text>"), "page 2 field");
    assert!(
        !pages[0].svg.contains(">9</text>"),
        "stale cache must be replaced"
    );
}

#[test]
fn numpages_field_renders_total() {
    let pages = build(&two_page_body(" NUMPAGES "));
    assert_eq!(pages.len(), 2);
    for page in &pages {
        assert!(
            page.svg.contains(">2</text>"),
            "NUMPAGES on page {}",
            page.index
        );
        assert!(!page.svg.contains(">9</text>"));
    }
}

#[test]
fn complex_field_is_computed() {
    let body = format!(
        "<w:p>\
<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
<w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r>\
<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
<w:r><w:t>9</w:t></w:r>\
<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>\
</w:p><w:sectPr>{PAGE_SETUP}</w:sectPr>"
    );
    let pages = build(&body);
    assert_eq!(pages.len(), 1);
    assert!(pages[0].svg.contains(">1</text>"));
    assert!(!pages[0].svg.contains(">9</text>"));
}

#[test]
fn non_computed_field_uses_cache() {
    let body = format!(
        "<w:p><w:fldSimple w:instr=\" AUTHOR \"><w:r><w:t>CachedAuthor</w:t></w:r></w:fldSimple></w:p>\
<w:sectPr>{PAGE_SETUP}</w:sectPr>"
    );
    let pages = build(&body);
    assert_eq!(pages.len(), 1);
    assert!(pages[0].svg.contains("CachedAuthor"));
}

#[test]
fn roman_switch_is_applied() {
    let pages = build(&two_page_body(" PAGE \\* roman "));
    assert_eq!(pages.len(), 2);
    assert!(
        pages[0].svg.contains(">i</text>"),
        "expected lowerRoman 'i'"
    );
}
