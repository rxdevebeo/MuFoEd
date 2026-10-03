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

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{render, Page, RenderOptions};

const PAGE_SETUP: &str = "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>";
const FOOTER_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/footer";

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

/// Builds a package with a footer part and renders it.
fn build_with_footer(body: &str, footer_xml: &str) -> Vec<Page> {
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdF\" Type=\"{FOOTER_REL}\" Target=\"footer1.xml\"/></Relationships>"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/footer1.xml", footer_xml.as_bytes().to_vec()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default()).expect("render")
}

/// Three body paragraphs separated by page breaks.
fn three_page_body(sect_pr: &str) -> String {
    format!(
        "<w:p><w:r><w:t>One</w:t></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>Two</w:t></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>Three</w:t></w:r></w:p>\
<w:sectPr>{sect_pr}<w:footerReference w:type=\"default\" r:id=\"rIdF\"/></w:sectPr>"
    )
}

/// Footer containing a single `fldSimple` field.
fn footer_field(instruction: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><w:ftr xmlns:w=\"{W}\">\
<w:p><w:fldSimple w:instr=\"{instruction}\"><w:r><w:t>9</w:t></w:r></w:fldSimple></w:p>\
</w:ftr>"
    )
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

#[test]
fn page_in_footer_updates_per_page() {
    // AUD-70: a cached footer must not freeze PAGE at "1".
    let pages = build_with_footer(&three_page_body(PAGE_SETUP), &footer_field(" PAGE "));
    assert_eq!(pages.len(), 3);
    assert!(pages[0].svg.contains(">1</text>"), "page 1 footer");
    assert!(pages[1].svg.contains(">2</text>"), "page 2 footer");
    assert!(pages[2].svg.contains(">3</text>"), "page 3 footer");
    for page in &pages {
        assert!(
            !page.svg.contains(">9</text>"),
            "stale cache must not survive on page {}",
            page.index
        );
    }
}

#[test]
fn page_in_footer_honours_pg_num_type_start_and_fmt() {
    // AUD-70: pgNumType start=5 fmt=upperRoman → V, VI, VII.
    let sect = format!("{PAGE_SETUP}<w:pgNumType w:start=\"5\" w:fmt=\"upperRoman\"/>");
    let pages = build_with_footer(&three_page_body(&sect), &footer_field(" PAGE "));
    assert_eq!(pages.len(), 3);
    assert!(pages[0].svg.contains(">V</text>"), "start+fmt page 1");
    assert!(pages[1].svg.contains(">VI</text>"), "start+fmt page 2");
    assert!(pages[2].svg.contains(">VII</text>"), "start+fmt page 3");
}

#[test]
fn numpages_in_footer_is_total_on_every_page() {
    // AUD-70: NUMPAGES in a footer needs the second pagination pass.
    let pages = build_with_footer(&three_page_body(PAGE_SETUP), &footer_field(" NUMPAGES "));
    assert_eq!(pages.len(), 3);
    for page in &pages {
        assert!(
            page.svg.contains(">3</text>"),
            "NUMPAGES on page {}",
            page.index
        );
        assert!(!page.svg.contains(">9</text>"));
    }
}
