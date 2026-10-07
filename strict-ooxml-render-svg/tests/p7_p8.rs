//! P7–P8 paint colour samples: theme resolve and schemeClr → sRGB in SVG.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, Page, RenderOptions};

const THEME_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/theme";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";

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

fn doc_rels() -> String {
    format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdTheme\" Type=\"{THEME_REL}\" Target=\"theme/theme1.xml\"/></Relationships>"
    )
}

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

/// T-P7-1 paint: resolved themeColour equals the theme slot sRGB.
#[test]
fn t_p7_1_paint_theme_color_matches_resolved_srgb() {
    let pages = build(
        "<w:p><w:r><w:rPr><w:color w:val=\"4472C4\" w:themeColor=\"accent1\"/></w:rPr>\
<w:t>theme</w:t></w:r></w:p>",
    );
    assert!(
        pages[0].svg.contains("#4472c4"),
        "accent1 must paint as theme sRGB: {}",
        pages[0].svg
    );
}

/// T-P7-3 paint: accent2 is a different paint from accent1.
#[test]
fn t_p7_3_paint_accent_slot_change_is_visible() {
    let accent1 = build(
        "<w:p><w:r><w:rPr><w:color w:themeColor=\"accent1\"/></w:rPr><w:t>a</w:t></w:r></w:p>",
    );
    let accent2 = build(
        "<w:p><w:r><w:rPr><w:color w:themeColor=\"accent2\"/></w:rPr><w:t>a</w:t></w:r></w:p>",
    );
    assert!(accent1[0].svg.contains("#4472c4"));
    assert!(accent2[0].svg.contains("#ed7d31"));
    assert_ne!(accent1[0].svg, accent2[0].svg);
}

/// T-P7-1 paint: themeFill on paragraph shading resolves through the theme.
#[test]
fn t_p7_1_paint_theme_fill_shading() {
    let pages = build(
        "<w:p><w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"D6DCE5\" \
w:themeFill=\"accent1\"/></w:pPr>\
<w:r><w:t>shaded</w:t></w:r></w:p>",
    );
    assert!(
        pages[0].svg.contains("#4472c4"),
        "themeFill accent1 must paint: {}",
        pages[0].svg
    );
}

/// T-P8-3 paint: DrawingML schemeClr aliases resolve (`bg1` → lt1).
#[test]
fn t_p8_3_paint_scheme_clr_alias_resolves() {
    let pages = build(
        "<w:p><w:r><w:drawing><wp:inline>\
<wp:extent cx=\"914400\" cy=\"914400\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/wordprocessingml/drawingml/wordprocessingShape\">\
<wps:wsp><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>\
<a:solidFill><a:schemeClr val=\"bg1\"/></a:solidFill>\
</wps:spPr><wps:bodyPr/></wps:wsp>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
    );
    assert!(
        pages[0].svg.contains("#ffffff") || pages[0].svg.to_ascii_lowercase().contains("ffffff"),
        "bg1 must resolve to lt1 white: {}",
        pages[0].svg
    );
}
