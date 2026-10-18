//! Off-page text: the layout marks it (`Page::off_page`), the output can tag it
//! (`data-off-page`), and `OffPageInk::Clamp` pulls a tab stop past its box and
//! a floating frame off the page back in.

#![allow(clippy::expect_used, clippy::doc_markdown)]

mod common;

use strict_ooxml_render_svg::{render, OffPageInk, OffPageReason, RenderOptions};

const SHAPE_DATA_URI: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingShape";

/// A right tab far past the page: `12` lands off the right edge.
fn tab_body() -> String {
    "<w:p><w:pPr><w:tabs><w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"20000\"/></w:tabs></w:pPr>\
<w:r><w:t>Title</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>12</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
        .to_owned()
}

/// A text box anchored far left of the page.
fn frame_body() -> String {
    let shape = "<wps:wsp><wps:cNvPr id=\"2\" name=\"box\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"/><a:noFill/></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>Off</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
</wps:wsp>";
    format!(
        "<w:p><w:r><w:drawing><wp:anchor behindDoc=\"0\" relativeHeight=\"1\" simplePos=\"0\" \
distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" allowOverlap=\"1\" layoutInCell=\"1\" locked=\"0\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:posOffset>-2000000</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionV>\
<wp:extent cx=\"914400\" cy=\"914400\"/><wp:wrapNone/><wp:docPr id=\"1\" name=\"tb\"/>\
<a:graphic><a:graphicData uri=\"{SHAPE_DATA_URI}\">{shape}</a:graphicData></a:graphic>\
</wp:anchor></w:drawing></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    )
}

fn render_body_with(body: &str, options: &RenderOptions) -> Vec<strict_ooxml_render_svg::Page> {
    let (_package, document) = common::open_body(body);
    render(&document, options).expect("render")
}

#[test]
fn a_tab_past_the_page_is_marked_tab_stop() {
    let pages = render_body_with(&tab_body(), &RenderOptions::default());
    let page = &pages[0];
    let mark = page
        .off_page
        .iter()
        .find(|mark| mark.reason == OffPageReason::TabStop)
        .expect("a tab-stop mark");
    assert!(
        mark.rect[2] > page.width_px,
        "the number is past the right edge: {:?} of {}",
        mark.rect,
        page.width_px
    );
}

#[test]
fn clamping_pulls_the_tab_back_onto_the_page() {
    let pages = render_body_with(
        &tab_body(),
        &RenderOptions::default().off_page_ink(OffPageInk::Clamp),
    );
    let page = &pages[0];
    assert!(
        page.off_page.is_empty(),
        "nothing is off the page: {:?}",
        page.off_page
    );
    let end = number_right(page).expect("the number");
    assert!(
        end <= page.width_px + 0.5,
        "the number ends on the page: {end} of {}",
        page.width_px
    );
}

#[test]
fn an_off_page_frame_is_marked_floating() {
    let pages = render_body_with(&frame_body(), &RenderOptions::default());
    let page = &pages[0];
    assert!(
        page.off_page
            .iter()
            .any(|mark| mark.reason == OffPageReason::Floating),
        "a floating mark: {:?}",
        page.off_page
    );
}

#[test]
fn clamping_pulls_the_frame_onto_the_page() {
    let pages = render_body_with(
        &frame_body(),
        &RenderOptions::default().off_page_ink(OffPageInk::Clamp),
    );
    let page = &pages[0];
    assert!(
        page.off_page.is_empty(),
        "the frame is on the page: {:?}",
        page.off_page
    );
}

#[test]
fn marks_reach_the_svg_when_requested() {
    let pages = render_body_with(&tab_body(), &RenderOptions::default().off_page_marks(true));
    assert!(
        pages[0].svg.contains("data-off-page=\"tab-stop\""),
        "the number carries its reason"
    );
    let plain = render_body_with(&tab_body(), &RenderOptions::default());
    assert!(
        !plain[0].svg.contains("data-off-page"),
        "no mark unless asked"
    );
}

/// Right edge of the `12` fragment.
fn number_right(page: &strict_ooxml_render_svg::Page) -> Option<f64> {
    // The SVG carries the rendered x; the placed items are not exposed on a
    // `Page`, so read the last x of the `12` `<text>` run instead.
    let svg = &page.svg;
    for tag in svg.split("<text").skip(1) {
        let Some((attrs, body)) = tag.split_once('>') else {
            continue;
        };
        let Some(text) = body.split('<').next() else {
            continue;
        };
        if text.trim() != "12" {
            continue;
        }
        let xs = attrs
            .split_once("x=\"")
            .and_then(|(_, tail)| tail.split('"').next())?;
        return xs
            .split_whitespace()
            .filter_map(|value| value.parse::<f64>().ok())
            .next_back();
    }
    None
}
