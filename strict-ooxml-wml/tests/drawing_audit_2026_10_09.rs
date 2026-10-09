#![allow(clippy::doc_markdown)]
//! Graphic payloads the 2026-10-09 code audit found mis-read: a locked canvas
//! whose markup named a namespace the capture has no prefix for (2.3), and a
//! `chart`/`relIds` taken by its local name alone (3.3).

mod common;

use strict_ooxml_wml::model::drawing::Graphic;
use strict_ooxml_wml::model::{
    Block, Document, DrawingKind, Inline, Paragraph, RunContent, SupportStatus,
};

use common::{document_parts, parse_parts};

const LC_NS: &str = "http://purl.oclc.org/ooxml/drawingml/lockedCanvas";
const A14_NS: &str = "http://schemas.microsoft.com/office/drawing/2010/main";
const CHARTEX_NS: &str = "http://schemas.microsoft.com/office/drawing/2014/chartex";

/// A paragraph holding one inline drawing whose `a:graphicData` is `payload`,
/// followed by a paragraph of plain text.
fn body(uri: &str, payload: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"200\"/>\
<wp:docPr id=\"1\" name=\"g\"/><a:graphic><a:graphicData uri=\"{uri}\">{payload}\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>\
<w:p><w:r><w:t>after</w:t></w:r></w:p>"
    )
}

/// A locked canvas around `content`, after its group properties.
fn canvas(content: &str) -> String {
    format!(
        "<lc:lockedCanvas xmlns:lc=\"{LC_NS}\">\
<a:nvGrpSpPr><a:cNvPr id=\"0\" name=\"\"/><a:cNvGrpSpPr/></a:nvGrpSpPr>\
<a:grpSpPr><a:xfrm><a:off x=\"10\" y=\"20\"/><a:ext cx=\"100\" cy=\"200\"/>\
<a:chOff x=\"1\" y=\"2\"/><a:chExt cx=\"100\" cy=\"200\"/></a:xfrm></a:grpSpPr>\
{content}</lc:lockedCanvas>"
    )
}

fn parse(uri: &str, payload: &str) -> Document {
    parse_parts(&document_parts(&body(uri, payload), &[])).expect("parse")
}

/// The graphic of the first paragraph's drawing.
fn graphic(document: &Document) -> Graphic {
    let Block::Paragraph(Paragraph { inlines, .. }) = &document.body.blocks[0] else {
        panic!("expected paragraph");
    };
    let Inline::Run(run) = &inlines[0] else {
        panic!("expected run");
    };
    let RunContent::Drawing(drawing) = &run.content[0] else {
        panic!("expected drawing");
    };
    let DrawingKind::Inline(inline) = &drawing.kind else {
        panic!("expected inline");
    };
    inline.graphic.as_ref().clone()
}

/// The text of the paragraph after the drawing: the parser resumed past it.
fn assert_reads_on(document: &Document) {
    assert_eq!(document.body.blocks.len(), 2, "{:?}", document.body.blocks);
    let Block::Paragraph(Paragraph { inlines, .. }) = &document.body.blocks[1] else {
        panic!("expected paragraph");
    };
    let Inline::Run(run) = &inlines[0] else {
        panic!("expected run");
    };
    let RunContent::Text(text) = &run.content[0] else {
        panic!("expected text, got {:?}", run.content);
    };
    assert_eq!(text.text, "after");
}

#[test]
fn a_canvas_attribute_in_an_unknown_namespace_drops_the_canvas() {
    // `w:val` would have been written back as `val`, in no namespace.
    let document = parse(
        LC_NS,
        &canvas("<a:sp><a:nvSpPr><a:cNvPr id=\"2\" name=\"s\" w:val=\"3\"/></a:nvSpPr></a:sp>"),
    );
    assert!(
        matches!(graphic(&document), Graphic::Other),
        "{:?}",
        graphic(&document)
    );
    assert_eq!(
        document
            .support
            .get("lc:lockedCanvas")
            .map(|use_| use_.status),
        Some(SupportStatus::Unsupported)
    );
    assert_reads_on(&document);
}

#[test]
fn a_canvas_element_in_an_unknown_namespace_drops_the_canvas() {
    let document = parse(
        LC_NS,
        &canvas("<x:shape xmlns:x=\"urn:example:shape\"><x:part/></x:shape>"),
    );
    assert!(
        matches!(graphic(&document), Graphic::Other),
        "{:?}",
        graphic(&document)
    );
    assert_reads_on(&document);
}

#[test]
fn a_canvas_drops_an_office_extension_and_keeps_the_rest() {
    let extension = format!(
        "<a:extLst><a:ext uri=\"{{28A0092B-C50C-407E-A947-70E740481C1C}}\">\
<a14:useLocalDpi xmlns:a14=\"{A14_NS}\" val=\"0\"/></a:ext></a:extLst>"
    );
    let document = parse(
        LC_NS,
        &canvas(&format!(
            "<a:sp><a:nvSpPr><a:cNvPr id=\"2\" name=\"s\"/></a:nvSpPr>\
<a:spPr>{extension}</a:spPr></a:sp>"
        )),
    );
    let Graphic::LockedCanvas(kept) = graphic(&document) else {
        panic!("locked canvas, got {:?}", graphic(&document));
    };
    assert!(!kept.markup.contains("useLocalDpi"), "{}", kept.markup);
    assert!(!kept.markup.contains("extLst"), "{}", kept.markup);
    assert!(
        kept.markup.contains("<a:off x=\"10\" y=\"20\">"),
        "{}",
        kept.markup
    );
    assert!(
        kept.markup.ends_with("</lc:lockedCanvas>"),
        "{}",
        kept.markup
    );
    assert_eq!(
        document
            .support
            .get("lc:lockedCanvas")
            .map(|use_| use_.status),
        Some(SupportStatus::Supported)
    );
    assert_eq!(
        document.support.get("a:ext").map(|use_| use_.status),
        Some(SupportStatus::Unsupported)
    );
    assert_reads_on(&document);
}

#[test]
fn a_chart_outside_the_chart_namespace_is_not_a_chart() {
    // A neutral `uri`: a `graphicData` whose `uri` names a chart is a chart
    // placeholder whatever it carries, and that is not what is tested here.
    let document = parse(
        "urn:example:chartex",
        &format!("<cx:chart xmlns:cx=\"{CHARTEX_NS}\" r:id=\"rId9\"/>"),
    );
    assert!(
        matches!(graphic(&document), Graphic::None),
        "{:?}",
        graphic(&document)
    );
    assert_reads_on(&document);
}

#[test]
fn rel_ids_outside_the_diagram_namespace_are_not_a_diagram() {
    let document = parse(
        "urn:example:diagram",
        "<x:relIds xmlns:x=\"urn:example:diagram\" r:dm=\"rId1\" r:lo=\"rId2\" \
r:qs=\"rId3\" r:cs=\"rId4\"/>",
    );
    assert!(
        matches!(graphic(&document), Graphic::None),
        "{:?}",
        graphic(&document)
    );
    assert_reads_on(&document);
}
