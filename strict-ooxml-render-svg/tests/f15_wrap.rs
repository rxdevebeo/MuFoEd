//! A21: square wrap changes line intervals and keeps text out of the image box.

#![allow(clippy::expect_used, clippy::format_push_string)]

mod common;

use common::{
    build_docx, content_types, document, open_bytes, root_rels, tiny_png, IMAGE_REL, PIC,
};
use strict_ooxml_render_svg::{render, RenderOptions};

fn render_wrap(element: &str, wrap_text: &str) -> String {
    let wrap = if wrap_text.is_empty() {
        format!("<wp:{element}/>")
    } else {
        format!("<wp:{element} wrapText=\"{wrap_text}\"/>")
    };
    let body = format!(
        "<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" \
simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:posOffset>914400</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
<wp:extent cx=\"1609725\" cy=\"1609725\"/>{wrap}<wp:docPr id=\"1\" name=\"portrait\"/>\
<a:graphic><a:graphicData uri=\"{PIC}\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"portrait\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1609725\" cy=\"1609725\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>\
<w:r><w:t>Words flow beside the portrait and must leave the wrap rectangle.</w:t></w:r></w:p>"
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

fn object_box(svg: &str) -> (f64, f64, f64, f64) {
    let document = roxmltree::Document::parse(svg).expect("svg");
    let node = document
        .descendants()
        .find(|node| {
            node.is_element()
                && matches!(node.tag_name().name(), "rect" | "image")
                && node.attribute("width").is_some_and(|width| {
                    width
                        .parse::<f64>()
                        .is_ok_and(|value| (value - 169.0).abs() < 1.0)
                })
        })
        .expect("portrait");
    let attr = |name: &str| {
        node.attribute(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0)
    };
    (attr("x"), attr("y"), attr("width"), attr("height"))
}

fn text_items(svg: &str) -> Vec<(f64, f64, String)> {
    let document = roxmltree::Document::parse(svg).expect("svg");
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .filter_map(|node| {
            // F07 emits a space-separated cluster `x` list; the first value is
            // the fragment origin used for wrap / collision oracles.
            let x = node
                .attribute("x")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            let y = node.attribute("y")?.parse().ok()?;
            Some((x, y, node.text().unwrap_or("").to_owned()))
        })
        .collect()
}

#[test]
fn f15_square_changes_line_intervals() {
    let square = render_wrap("wrapSquare", "bothSides");
    let none = render_wrap("wrapNone", "");
    let square_text = text_items(&square);
    let none_text = text_items(&none);
    assert_ne!(
        square_text, none_text,
        "square wrap must place the line differently from wrapNone"
    );
    let (x, y, w, h) = object_box(&square);
    // distL/distR are 114300 EMU = 12 px. distT/distB are 0.
    let left = x - 12.0;
    let right = x + w + 12.0;
    let bottom = y + h;
    for (text_x, text_y, text) in text_items(&square) {
        if text_y < y - 2.0 || text_y > bottom + 2.0 {
            continue;
        }
        let size = 14.67_f64;
        let chars = u32::try_from(text.chars().count()).unwrap_or(0);
        let ink = size * 0.45 * f64::from(chars);
        let overlaps = text_x < right - 0.25 && text_x + ink > left + 0.25;
        assert!(
            !overlaps,
            "text {text:?} at {text_x},{text_y} overlaps the expanded box {left}..{right}"
        );
    }
}
