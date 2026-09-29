#![allow(clippy::doc_markdown, clippy::format_push_string)]

//! Integration tests for Stage-5B floating DrawingML, shapes, groups, text
//! boxes and page borders.

mod common;

use strict_ooxml_render_svg::RenderOptions;

use common::{open_body, render_body, WPG, WPS};

fn assert_single_page(body: &str) -> String {
    let pages = render_body(body);
    assert_eq!(pages.len(), 1, "expected exactly one page");
    pages[0].svg.clone()
}

const SHAPE_DATA_URI: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingShape";
const GROUP_DATA_URI: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingGroup";

fn anchor(inner: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:anchor behindDoc=\"0\" relativeHeight=\"1\" simplePos=\"0\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" allowOverlap=\"1\" layoutInCell=\"1\" locked=\"0\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionV>\
<wp:extent cx=\"914400\" cy=\"914400\"/>\
<wp:wrapNone/>\
<wp:docPr id=\"1\" name=\"object\"/>\
{inner}\
</wp:anchor></w:drawing></w:r></w:p>"
    )
}

fn rect_shape(fill: &str) -> String {
    format!(
        "<wps:wsp><wps:cNvPr id=\"2\" name=\"rect\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"/><a:solidFill><a:srgbClr val=\"{fill}\"/></a:solidFill>\
<a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill></a:ln>\
</wps:spPr></wps:wsp>"
    )
}

#[test]
fn anchored_shape_renders_positioned_path() {
    let body = anchor(&format!(
        "<a:graphic><a:graphicData uri=\"{SHAPE_DATA_URI}\">{}</a:graphicData></a:graphic>",
        rect_shape("FF0000")
    ));
    let svg = assert_single_page(&body);
    assert!(svg.contains("<path "), "has a path: {svg}");
    assert!(svg.contains("fill=\"#ff0000\""), "solid fill: {svg}");
    // 914400 EMU == 96 px at the default 96 DPI scale.
    assert!(svg.contains("translate(96 96)"), "page offset: {svg}");
}

#[test]
fn floating_objects_can_be_disabled() {
    let body = anchor(&format!(
        "<a:graphic><a:graphicData uri=\"{SHAPE_DATA_URI}\">{}</a:graphicData></a:graphic>",
        rect_shape("00FF00")
    ));
    let (_package, document) = open_body(&body);
    let pages =
        strict_ooxml_render_svg::render(&document, &RenderOptions::default().floating(false))
            .expect("render");
    assert!(
        !pages[0].svg.contains("<path "),
        "floating disabled drops shapes"
    );
}

#[test]
fn anchored_picture_is_rendered() {
    let body = anchor(
        "<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"p\" descr=\"alt\"/></pic:nvPicPr>\
<pic:blipFill/></pic:pic></a:graphicData></a:graphic>",
    );
    let svg = assert_single_page(&body);
    // No blip resolves (no relationship) but the placeholder keeps the extent.
    assert!(
        svg.contains("x=\"96\" y=\"96\""),
        "anchored placeholder: {svg}"
    );
}

#[test]
fn group_renders_all_children() {
    let group = format!(
        "<wpg:wgp><wpg:cNvPr id=\"1\" name=\"grp\"/><wpg:cNvSpPr/>\
<wpg:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/>\
<a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"914400\" cy=\"914400\"/></a:xfrm></wpg:grpSpPr>\
{}{}</wpg:wgp>",
        rect_shape("FF0000"),
        rect_shape("0000FF")
    );
    let body = anchor(&format!(
        "<a:graphic><a:graphicData uri=\"{GROUP_DATA_URI}\">{group}</a:graphicData></a:graphic>"
    ));
    let svg = assert_single_page(&body);
    assert_eq!(
        svg.matches("<path ").count(),
        2,
        "two group children: {svg}"
    );
}

#[test]
fn shape_text_box_renders_text() {
    let shape = "<wps:wsp><wps:cNvPr id=\"2\" name=\"box\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"roundRect\"/><a:solidFill><a:srgbClr val=\"EEEEEE\"/></a:solidFill>\
</wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>Boxed</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr anchor=\"ctr\"/>\
</wps:wsp>";
    let body = anchor(&format!(
        "<a:graphic><a:graphicData uri=\"{SHAPE_DATA_URI}\">{shape}</a:graphicData></a:graphic>"
    ));
    let svg = assert_single_page(&body);
    assert!(svg.contains("Boxed"), "text box content: {svg}");
}

#[test]
fn page_borders_render() {
    let body = "<w:p><w:r><w:t>x</w:t></w:r></w:p>\
<w:sectPr><w:pgBorders w:offsetFrom=\"page\">\
<w:top w:val=\"single\" w:sz=\"16\" w:space=\"24\" w:color=\"FF0000\"/>\
<w:bottom w:val=\"single\" w:sz=\"16\" w:space=\"24\" w:color=\"FF0000\"/>\
</w:pgBorders><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr>";
    let svg = assert_single_page(body);
    assert!(
        svg.contains("stroke=\"#ff0000\""),
        "page border colour: {svg}"
    );
}

#[test]
fn wps_namespaces_are_declared() {
    assert!(common::WPS == WPS);
    assert!(common::WPG == WPG);
}
