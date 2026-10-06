//! A13: a positive `wp14:pctWidth` replaces the fallback `wp:extent`.

#![allow(clippy::expect_used, clippy::format_push_string)]

mod common;

use common::{
    build_docx, content_types, document, open_body, open_bytes, root_rels, tiny_png, IMAGE_REL, PIC,
};
use strict_ooxml_render_svg::{place_pages, render, Item, RenderOptions};

fn object_width(svg: &str) -> f64 {
    let document = roxmltree::Document::parse(svg).expect("svg");
    document
        .descendants()
        .find(|node| {
            node.is_element()
                && node.tag_name().name() == "rect"
                && node
                    .children()
                    .any(|child| child.tag_name().name() == "title")
        })
        .and_then(|node| node.attribute("width"))
        .and_then(|width| width.parse().ok())
        .expect("anchored object")
}

fn render_page(page_twips: i32) -> String {
    let body = format!(
        "<w:p><w:r><w:drawing \
xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\">\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" \
behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:align>left</wp:align></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:align>top</wp:align></wp:positionV>\
<wp:extent cx=\"7315200\" cy=\"952500\"/>\
<wp14:sizeRelH relativeFrom=\"page\"><wp14:pctWidth>94100</wp14:pctWidth></wp14:sizeRelH>\
<wp:wrapNone/>\
<wp:docPr id=\"1\" name=\"rel\"/>\
<a:graphic><a:graphicData uri=\"{PIC}\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"rel\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"7315200\" cy=\"952500\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"{page_twips}\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdImage\" Type=\"{IMAGE_REL}\" Target=\"media/image1.png\"/></Relationships>"
    );
    let entries = [
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/media/image1.png", tiny_png()),
    ];
    let (_package, parsed) = open_bytes(build_docx(&entries));
    render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

#[test]
fn f14_relative_width_replaces_fallback_extent() {
    // 9633 twips = 642.2 px. 94.1% is 604.3102 px. The extent is 768 px and must lose.
    let width = object_width(&render_page(9633));
    assert!(
        (width - 604.3102).abs() <= 0.25,
        "width {width} must be 94.1% of 642.2 px, not the 768 px extent"
    );
    let wider = object_width(&render_page(19_266));
    let ratio = wider / width;
    assert!(
        (ratio - 2.0).abs() <= 0.01,
        "doubling the page ({wider} / {width}) must double the relative width"
    );
}

/// An image that is the only content of a page frame starts on the frame origin.
///
/// A paragraph mark with `w:sz="0"` used to emit a 1 px line before the picture,
/// so the box sat 15 twips below the frame.
#[test]
fn f14_image_only_frame_starts_at_the_frame_origin() {
    let body = format!(
        "<w:p><w:pPr><w:framePr w:w=\"4000\" w:h=\"3000\" w:hAnchor=\"page\" w:vAnchor=\"page\" \
w:x=\"6708\" w:y=\"1827\"/><w:rPr><w:sz w:val=\"0\"/><w:szCs w:val=\"0\"/></w:rPr></w:pPr>\
<w:r><w:drawing><wp:inline><wp:extent cx=\"1737360\" cy=\"1463040\"/>\
<wp:docPr id=\"1\" name=\"pic\"/>\
<a:graphic><a:graphicData uri=\"{PIC}\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"pic\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1737360\" cy=\"1463040\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    let (_package, parsed) = open_body(&body);
    let pages = place_pages(&parsed, &RenderOptions::default(), None).expect("place");
    let image = pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .expect("image");
    // 6708 twips = 447.2 px, 1827 twips = 121.8 px. 1737360 EMU = 182.4 px.
    assert!(
        (image.x - 447.2).abs() <= 0.25,
        "image x {} must be the frame origin",
        image.x
    );
    assert!(
        (image.y - 121.8).abs() <= 0.25,
        "image y {} must be the frame origin, not one pixel below it",
        image.y
    );
    assert!((image.w - 182.4).abs() <= 0.25, "width {}", image.w);
    assert!((image.h - 153.6).abs() <= 0.25, "height {}", image.h);
}

#[test]
fn f14_margin_align_and_offset_matrix() {
    let body = format!(
        "<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" \
behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:align>center</wp:align></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionV>\
<wp:extent cx=\"914400\" cy=\"914400\"/>\
<wp:wrapNone/>\
<wp:docPr id=\"1\" name=\"rel\"/>\
<a:graphic><a:graphicData uri=\"{PIC}\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"rel\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdImage\" Type=\"{IMAGE_REL}\" Target=\"media/image1.png\"/></Relationships>"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/media/image1.png", tiny_png()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    let svg = render(&parsed, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg;
    assert!(svg.contains("<image") || svg.contains("<rect"), "{svg}");
}
