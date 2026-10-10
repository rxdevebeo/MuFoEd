//! A picture bullet is drawn as the list marker (`w:lvlPicBulletId`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::io::Cursor;

use strict_ooxml::{OpenOptions, RenderOptions, StrictDocument};
use strict_ooxml_testkit::docx::{rels_xml, Family, Rel};
use strict_ooxml_testkit::DocxBuilder;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nbullet";
const NUMBERING_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";

const PICTURE_BULLET_LEVEL: &str = "<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/>\
<w:lvlText w:val=\"\u{f0b7}\"/><w:lvlPicBulletId w:val=\"0\"/><w:lvlJc w:val=\"start\"/>\
</w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";

const STRICT_BULLET: &str = "<w:numPicBullet w:numPicBulletId=\"0\"><w:drawing>\
<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"138430\" cy=\"138430\"/>\
<wp:docPr id=\"1\" name=\"bullet\"/><a:graphic>\
<a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"bullet\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"138430\" cy=\"138430\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:numPicBullet>";

#[test]
fn a_picture_bullet_is_drawn_in_place_of_the_marker_text() {
    let bytes = DocxBuilder::strict()
        .body(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
<w:r><w:t>item</w:t></w:r></w:p>",
        )
        .rel("rIdNumbering", "numbering", "numbering.xml")
        .content_type("/word/numbering.xml", NUMBERING_TYPE)
        .part_xml(
            "word/numbering.xml",
            "w:numbering",
            &format!("{STRICT_BULLET}{PICTURE_BULLET_LEVEL}"),
        )
        .part(
            "word/_rels/numbering.xml.rels",
            rels_xml(&[Rel::new(
                "rId1",
                Family::Strict.rel_type("image"),
                "media/image1.png",
            )]),
        )
        .part("word/media/image1.png", PNG.to_vec())
        .build();
    let document =
        StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let pages = document
        .render_svg(&RenderOptions::default())
        .expect("render");
    let svg = &pages[0].svg;
    assert_eq!(
        svg.matches("<image").count(),
        1,
        "the bullet picture: {svg}"
    );
    assert!(!svg.contains('\u{f0b7}'), "no text marker beside it: {svg}");
    assert!(svg.contains("item"), "{svg}");
}
