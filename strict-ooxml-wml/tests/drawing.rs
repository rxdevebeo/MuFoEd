#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for inline DrawingML pictures and the media index.

mod common;

use strict_ooxml_wml::model::drawing::{DrawingKind, MediaKind};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::support::SupportStatus;

use common::{document_parts, parse_parts, rels};

const IMAGE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/image";

const INLINE_DRAWING: &str = "<w:p><w:r><w:drawing><wp:inline>\
<wp:extent cx=\"914400\" cy=\"457200\"/>\
<wp:docPr id=\"1\" name=\"Picture 1\" descr=\"a picture\"/>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"image1.png\" descr=\"\"/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg1\"/></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"457200\"/></a:xfrm></pic:spPr>\
</pic:pic></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>";

#[test]
fn resolves_inline_picture_to_media_part() {
    let rels = rels(&[("rIdImg1", IMAGE, "media/image1.png")]);
    let parts = document_parts(
        INLINE_DRAWING,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/media/image1.png", vec![0x89, b'P', b'N', b'G']),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let Inline::Run(run) = &document.body.blocks[0].as_paragraph().unwrap().inlines[0] else {
        panic!("expected run");
    };
    let strict_ooxml_wml::model::inline::RunContent::Drawing(drawing) = &run.content[0] else {
        panic!("expected drawing");
    };
    let DrawingKind::Inline(inline) = &drawing.kind else {
        panic!("expected inline drawing");
    };
    let picture = inline.picture().expect("picture");
    let blip = picture.blip.as_ref().expect("blip");
    assert_eq!(blip.embed.as_ref().unwrap().as_str(), "rIdImg1");
    assert_eq!(
        blip.resolved.as_ref().unwrap().as_str(),
        "/word/media/image1.png"
    );
    assert_eq!(inline.extent.unwrap().cx.value(), 914400);
    assert_eq!(document.media.len(), 1);
    let item = document.media.iter().next().unwrap();
    assert_eq!(item.kind, MediaKind::Png);
}

#[test]
fn floating_anchor_is_parsed() {
    let parts = document_parts(
        "<w:p><w:r><w:drawing><wp:anchor behindDoc=\"1\" relativeHeight=\"2\" distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" allowOverlap=\"1\" locked=\"0\" layoutInCell=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:posOffset>114300</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:align>top</wp:align></wp:positionV>\
<wp:extent cx=\"914400\" cy=\"457200\"/>\
<wp:wrapSquare wrapText=\"bothSides\" distL=\"114300\"/>\
<wp:docPr id=\"2\" name=\"float\"/>\
</wp:anchor></w:drawing></w:r></w:p>",
        &[],
    );
    let document = parse_parts(&parts).expect("parse");
    let Inline::Run(run) = &document.body.blocks[0].as_paragraph().unwrap().inlines[0] else {
        panic!("expected run");
    };
    let strict_ooxml_wml::model::inline::RunContent::Drawing(drawing) = &run.content[0] else {
        panic!("expected drawing");
    };
    let DrawingKind::Anchor(anchor) = &drawing.kind else {
        panic!("expected anchor");
    };
    assert!(anchor.behind_doc);
    assert_eq!(anchor.relative_height, Some(2));
    assert_eq!(anchor.dist_left, Some(114_300));
    assert_eq!(
        anchor.position_h.as_ref().unwrap().relative_from.as_deref(),
        Some("column")
    );
    assert_eq!(
        anchor.position_h.as_ref().unwrap().offset.unwrap().value(),
        114_300
    );
    assert_eq!(
        anchor.position_v.as_ref().unwrap().align.as_deref(),
        Some("top")
    );
    assert_eq!(
        anchor.wrap.as_ref().unwrap().kind,
        strict_ooxml_wml::model::WrapKind::Square
    );
    assert_eq!(
        document.support.get("wp:anchor").unwrap().status,
        SupportStatus::Supported
    );
}
