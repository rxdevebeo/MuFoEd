//! Off-page text: the layout marks it (`Page::off_page`), the output can tag it
//! (`data-off-page`), and `OffPageInk::Clamp` pulls a tab stop past its box and
//! a floating frame off the page back in.

#![allow(clippy::expect_used, clippy::doc_markdown)]

mod common;

use strict_ooxml_render_svg::{render, OffPageInk, OffPageReason, RenderOptions};

const NUMBERING_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";

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

/// A text box wider than the sheet, anchored off the left edge and set not to
/// wrap (`wrap="none"`), so no origin can bring its far edge on and it cannot
/// reflow: `Clamp` has to clip its ink to the sheet.
fn wide_frame_body() -> String {
    let shape = "<wps:wsp><wps:cNvPr id=\"2\" name=\"box\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"18288000\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"/><a:noFill/></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>Wide</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr wrap=\"none\"/></wps:wsp>";
    format!(
        "<w:p><w:r><w:drawing><wp:anchor behindDoc=\"0\" relativeHeight=\"1\" simplePos=\"0\" \
distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" allowOverlap=\"1\" layoutInCell=\"1\" locked=\"0\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:posOffset>-2000000</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionV>\
<wp:extent cx=\"18288000\" cy=\"914400\"/><wp:wrapNone/><wp:docPr id=\"1\" name=\"tb\"/>\
<a:graphic><a:graphicData uri=\"{SHAPE_DATA_URI}\">{shape}</a:graphicData></a:graphic>\
</wp:anchor></w:drawing></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    )
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
fn clamping_clips_a_frame_wider_than_the_page() {
    let options = RenderOptions::default().off_page_ink(OffPageInk::Clamp);
    let pages = render_body_with(&wide_frame_body(), &options);
    let page = &pages[0];
    assert!(
        page.svg.contains("overflow=\"hidden\""),
        "the frame's ink is clipped to the page"
    );
    assert!(
        page.svg.contains("Wide"),
        "the text is clipped, not dropped"
    );
    let kept = render_body_with(&wide_frame_body(), &RenderOptions::default());
    assert!(
        !kept[0].svg.contains("overflow=\"hidden\""),
        "no clip unless clamp is asked for"
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

/// A numbered item whose suffix tab is a stop past the page, the same stop a
/// body tab would take.
fn numbered_tab_body() -> (String, String) {
    let numbering = format!(
        "<?xml version=\"1.0\"?><w:numbering xmlns:w=\"{}\">\
<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\">\
<w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
<w:suff w:val=\"tab\"/></w:lvl></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>",
        common::W
    );
    let body = "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>\
<w:tabs><w:tab w:val=\"left\" w:pos=\"20000\"/></w:tabs></w:pPr>\
<w:r><w:t>Q</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
        .to_owned();
    (body, numbering)
}

fn render_numbered(options: &RenderOptions) -> Vec<strict_ooxml_render_svg::Page> {
    let (body, numbering) = numbered_tab_body();
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdN\" Type=\"{NUMBERING_REL}\" Target=\"numbering.xml\"/></Relationships>"
    );
    let bytes = common::build_docx(&[
        ("[Content_Types].xml", common::content_types().into_bytes()),
        ("_rels/.rels", common::root_rels().into_bytes()),
        ("word/document.xml", common::document(&body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
    ]);
    let (_package, document) = common::open_bytes(bytes);
    render(&document, options).expect("render")
}

#[test]
fn a_list_suffix_tab_past_the_page_is_marked_tab_stop() {
    let pages = render_numbered(&RenderOptions::default());
    let page = &pages[0];
    assert!(
        page.off_page
            .iter()
            .any(|mark| mark.reason == OffPageReason::TabStop),
        "the suffix tab is a tab stop: {:?}",
        page.off_page
    );
}

#[test]
fn clamping_pulls_a_list_suffix_tab_back_to_the_box() {
    let kept = render_numbered(&RenderOptions::default());
    let clamped = render_numbered(&RenderOptions::default().off_page_ink(OffPageInk::Clamp));
    let kept_x = text_origin(&kept[0], "Q").expect("the kept stop");
    let clamped_x = text_origin(&clamped[0], "Q").expect("the clamped stop");
    assert!(
        kept_x > kept[0].width_px,
        "an unclamped stop stays past the page: {kept_x} of {}",
        kept[0].width_px
    );
    assert!(
        clamped_x <= clamped[0].width_px + 0.5,
        "the clamped stop is the box edge: {clamped_x} of {}",
        clamped[0].width_px
    );
}

/// Right edge of the `12` fragment.
fn number_right(page: &strict_ooxml_render_svg::Page) -> Option<f64> {
    text_x(page, "12", true)
}

/// Origin of one `<text>` run.
fn text_origin(page: &strict_ooxml_render_svg::Page, needle: &str) -> Option<f64> {
    text_x(page, needle, false)
}

/// An `x` of one `<text>` run. The placed items are not exposed on a `Page`,
/// so this reads the attribute: the first value is the origin, the last is
/// the right edge.
fn text_x(page: &strict_ooxml_render_svg::Page, needle: &str, last: bool) -> Option<f64> {
    let svg = &page.svg;
    for tag in svg.split("<text").skip(1) {
        let Some((attrs, body)) = tag.split_once('>') else {
            continue;
        };
        let Some(text) = body.split('<').next() else {
            continue;
        };
        if text.trim() != needle {
            continue;
        }
        let xs = attrs
            .split_once("x=\"")
            .and_then(|(_, tail)| tail.split('"').next())?;
        let mut values = xs
            .split_whitespace()
            .filter_map(|value| value.parse::<f64>().ok());
        return if last {
            values.next_back()
        } else {
            values.next()
        };
    }
    None
}
