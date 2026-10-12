//! Theme resolution in the cascade (STAGE-5 S5.9): theme fonts and colours.

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

const THEME_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/theme";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";

/// A minimal DrawingML Strict theme (major = Cambria, minor = Calibri).
fn theme_xml() -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><a:theme xmlns:a=\"{A_NS}\" name=\"Office\"><a:themeElements>\
<a:clrScheme name=\"Office\">\
<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"44546A\"/></a:dk2>\
<a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1>\
<a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2>\
<a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3>\
<a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>\
<a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5>\
<a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink>\
<a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink>\
</a:clrScheme>\
<a:fontScheme name=\"Office\">\
<a:majorFont><a:latin typeface=\"Cambria\"/></a:majorFont>\
<a:minorFont><a:latin typeface=\"Calibri\"/></a:minorFont>\
</a:fontScheme>\
<a:fmtScheme name=\"Office\"/>\
</a:themeElements></a:theme>"
    )
    .into_bytes()
}

/// Relationships part for the document, declaring the theme.
fn doc_rels() -> String {
    format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdTheme\" Type=\"{THEME_REL}\" Target=\"theme/theme1.xml\"/></Relationships>"
    )
}

/// Builds a Strict package with a theme and renders the given body.
fn build(body: &str) -> Vec<Page> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels().into_bytes()),
        ("word/theme/theme1.xml", theme_xml()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    render(&parsed, &RenderOptions::default()).expect("render")
}

#[test]
fn theme_font_is_resolved_to_the_bundled_face() {
    // Minor theme font (Calibri → Carlito), major theme font (Cambria → Caladea).
    let body =
        "<w:p><w:r><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\"/></w:rPr><w:t>minor</w:t></w:r>\
<w:r><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\"/></w:rPr><w:t>major</w:t></w:r></w:p>";
    let pages = build(body);
    let svg = &pages[0].svg;
    assert!(svg.contains("Carlito"), "minor theme font not applied");
    assert!(svg.contains("Caladea"), "major theme font not applied");
}

#[test]
fn direct_font_overrides_theme_font() {
    let body = "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Arial\" w:asciiTheme=\"minorHAnsi\"/></w:rPr><w:t>A</w:t></w:r></w:p>";
    let pages = build(body);
    assert!(pages[0].svg.contains("Arimo"));
    assert!(!pages[0].svg.contains("Carlito"));
}

#[test]
fn theme_color_is_resolved() {
    let body =
        "<w:p><w:r><w:rPr><w:color w:themeColor=\"accent1\"/></w:rPr><w:t>tinted</w:t></w:r></w:p>";
    let pages = build(body);
    assert!(pages[0].svg.contains("#4472c4"), "accent1 not resolved");
}

#[test]
fn theme_shade_darkens_the_color() {
    let body = "<w:p><w:r><w:rPr><w:color w:themeColor=\"accent1\" w:themeShade=\"80\"/></w:rPr><w:t>shaded</w:t></w:r></w:p>";
    let pages = build(body);
    // #4472c4 * 0x80/0xff = #223962.
    assert!(pages[0].svg.contains("#223962"), "theme shade not applied");
}
