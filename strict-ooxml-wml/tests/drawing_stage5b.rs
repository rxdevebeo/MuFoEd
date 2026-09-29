#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Stage-5B DrawingML parsing breadth: shapes, fills, outlines, custom
//! geometry, groups, text boxes, anchor positioning/wrapping, page borders and
//! Microsoft namespace compatibility.

mod common;

use strict_ooxml_wml::model::drawing::{
    Graphic, PathCommand, ShapeFill, ShapeGeometry, TextAnchor,
};
use strict_ooxml_wml::model::props::{BorderOffsetFrom, BorderZOrder};
use strict_ooxml_wml::model::values::BorderStyle;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::model::{
    AnchorDrawing, Block, DrawingKind, Inline, Paragraph, RunContent, WrapKind,
};

use common::{document_parts, parse_parts};

fn anchor_of(body: &str) -> (Document, AnchorDrawing) {
    let parts = document_parts(body, &[]);
    let document = parse_parts(&parts).expect("parse");
    let owned = {
        let Block::Paragraph(Paragraph { inlines, .. }) = &document.body.blocks[0] else {
            panic!("expected paragraph");
        };
        let Inline::Run(run) = &inlines[0] else {
            panic!("expected run");
        };
        let RunContent::Drawing(drawing) = &run.content[0] else {
            panic!("expected drawing");
        };
        let DrawingKind::Anchor(anchor) = &drawing.kind else {
            panic!("expected anchor");
        };
        anchor.clone()
    };
    (document, owned)
}

fn wrap_anchor(graphic: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:anchor distT=\"1\" distB=\"2\" distL=\"3\" distR=\"4\" simplePos=\"0\" relativeHeight=\"9\" behindDoc=\"1\" locked=\"1\" layoutInCell=\"0\" allowOverlap=\"0\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"margin\"><wp:align>center</wp:align></wp:positionH>\
<wp:positionV relativeFrom=\"line\"><wp:posOffset>100</wp:posOffset></wp:positionV>\
<wp:extent cx=\"914400\" cy=\"914400\"/>\
<wp:effectExtent l=\"1\" t=\"2\" r=\"3\" b=\"4\"/>\
<wp:wrapSquare wrapText=\"left\" distL=\"5\" distR=\"6\" distT=\"7\" distB=\"8\"/>\
<wp:docPr id=\"1\" name=\"n\"/>\
<a:graphic><a:graphicData uri=\"x\">{graphic}</a:graphicData></a:graphic>\
</wp:anchor></w:drawing></w:r></w:p>"
    )
}

#[test]
fn anchor_positioning_and_wrap_are_parsed() {
    let body = wrap_anchor("<wps:wsp><wps:cNvPr id=\"1\" name=\"x\"/><wps:spPr/></wps:wsp>");
    let (_document, anchor) = anchor_of(&body);
    assert!(anchor.behind_doc && anchor.locked);
    assert!(!anchor.allow_overlap && !anchor.layout_in_cell);
    assert_eq!(anchor.relative_height, Some(9));
    assert_eq!(
        (
            anchor.dist_top,
            anchor.dist_bottom,
            anchor.dist_left,
            anchor.dist_right
        ),
        (Some(1), Some(2), Some(3), Some(4))
    );
    assert_eq!(
        anchor
            .effect_extent
            .map(|e| (e.left.value(), e.bottom.value())),
        Some((1, 4))
    );
    assert_eq!(
        anchor.position_h.as_ref().unwrap().align.as_deref(),
        Some("center")
    );
    assert_eq!(
        anchor.position_v.as_ref().unwrap().offset.unwrap().value(),
        100
    );
    let wrap = anchor.wrap.as_ref().unwrap();
    assert_eq!(wrap.kind, WrapKind::Square);
    assert_eq!(wrap.wrap_text.as_deref(), Some("left"));
    assert_eq!(wrap.dist_top, Some(7));
    // The shape is the graphic; the WPS namespace is Strict and thus supported.
    assert!(matches!(anchor.graphic.as_ref(), Graphic::Shape(_)));
}

#[test]
fn shape_fills_geometry_outline_and_style_are_parsed() {
    let shape = "<wps:wsp><wps:cNvPr id=\"1\" name=\"s\" descr=\"d\"/><wps:spPr>\
<a:xfrm rot=\"5400000\" flipH=\"1\"><a:off x=\"12\" y=\"13\"/><a:ext cx=\"100\" cy=\"200\"/></a:xfrm>\
<a:prstGeom prst=\"hexagon\"/>\
<a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FF0000\"/></a:gs><a:gs pos=\"100000\"><a:schemeClr val=\"accent1\"/></a:gs></a:gsLst><a:lin ang=\"5400000\"/></a:gradFill>\
<a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill><a:prstDash val=\"dash\"/><a:headEnd type=\"triangle\"/><a:tailEnd type=\"oval\"/></a:ln>\
</wps:spPr>\
<wps:style><a:lnRef idx=\"1\"/><a:fillRef idx=\"2\"/><a:effectRef idx=\"3\"/><a:fontRef idx=\"4\"/></wps:style>\
</wps:wsp>";
    let (document, anchor) = anchor_of(&wrap_anchor(shape));
    let Graphic::Shape(parsed) = anchor.graphic.as_ref() else {
        panic!("shape");
    };
    assert_eq!(parsed.name.as_deref(), Some("s"));
    assert!(matches!(parsed.geometry, ShapeGeometry::Preset(ref p) if p.as_ref() == "hexagon"));
    assert_eq!(parsed.offset.map(|(x, _)| x.value()), Some(12));
    assert_eq!(parsed.extent.map(|e| e.cy.value()), Some(200));
    let xfrm = parsed.xfrm.unwrap();
    assert_eq!(xfrm.rot, Some(5_400_000));
    assert!(xfrm.flip_h);
    let ShapeFill::Gradient { stops, angle } = parsed.fill.as_ref().unwrap() else {
        panic!("gradient");
    };
    assert_eq!(stops.len(), 2);
    assert_eq!(*angle, Some(5_400_000));
    let stroke = parsed.stroke.as_ref().unwrap();
    assert_eq!(stroke.dash.as_deref(), Some("dash"));
    assert_eq!(stroke.head_end.as_deref(), Some("triangle"));
    assert_eq!(stroke.tail_end.as_deref(), Some("oval"));
    assert_eq!(stroke.width.unwrap().value(), 12700);
    let style = parsed.style.unwrap();
    assert_eq!(style.line_ref, Some(1));
    assert_eq!(style.font_ref, Some(4));
    let _ = &document;
}

#[test]
fn pattern_and_no_fill_and_custom_geometry() {
    let shape = "<wps:wsp><wps:cNvPr id=\"1\" name=\"c\"/><wps:spPr>\
<a:custGeom><a:pathLst><a:path w=\"100\" h=\"100\">\
<a:moveTo><a:pt x=\"0\" y=\"0\"/></a:moveTo>\
<a:lnTo><a:pt x=\"100\" y=\"0\"/></a:lnTo>\
<a:cubicBezTo><a:pt x=\"100\" y=\"0\"/><a:pt x=\"100\" y=\"100\"/><a:pt x=\"0\" y=\"100\"/></a:cubicBezTo>\
<a:close/></a:path></a:pathLst></a:custGeom>\
<a:pattFill prst=\"pct20\"><a:fgClr><a:srgbClr val=\"112233\"/></a:fgClr><a:bgClr><a:srgbClr val=\"FFFFFF\"/></a:bgClr></a:pattFill>\
</wps:spPr></wps:wsp>";
    let (document, anchor) = anchor_of(&wrap_anchor(shape));
    let Graphic::Shape(parsed) = anchor.graphic.as_ref() else {
        panic!("shape");
    };
    let ShapeGeometry::Custom(custom) = &parsed.geometry else {
        panic!("custom geometry");
    };
    assert_eq!(custom.width, 100);
    assert_eq!(custom.commands.len(), 4);
    assert!(matches!(custom.commands[0], PathCommand::MoveTo { .. }));
    assert!(matches!(custom.commands[3], PathCommand::Close));
    assert!(matches!(
        parsed.fill.as_ref().unwrap(),
        ShapeFill::Pattern { .. }
    ));
    assert!(
        document.support.get("a:custGeom").unwrap().status
            == strict_ooxml_wml::model::SupportStatus::Partial
    );
}

#[test]
fn no_fill_and_unsupported_fill_are_recorded() {
    let shape = "<wps:wsp><wps:cNvPr id=\"1\" name=\"c\"/><wps:spPr><a:noFill/><a:ln><a:noFill/></a:ln></wps:spPr></wps:wsp>";
    let (_doc, anchor) = anchor_of(&wrap_anchor(shape));
    let Graphic::Shape(parsed) = anchor.graphic.as_ref() else {
        panic!("shape");
    };
    assert!(matches!(parsed.fill, Some(ShapeFill::None)));
    assert!(parsed.stroke.as_ref().unwrap().none);
}

#[test]
fn text_box_body_and_content_are_parsed() {
    let shape = "<wps:wsp><wps:cNvPr id=\"1\" name=\"box\"/><wps:spPr><a:prstGeom prst=\"rect\"/></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr anchor=\"b\" anchorCtr=\"1\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\"/></wps:wsp>";
    let (document, anchor) = anchor_of(&wrap_anchor(shape));
    let Graphic::Shape(parsed) = anchor.graphic.as_ref() else {
        panic!("shape");
    };
    let text = parsed.text.as_ref().expect("text box");
    assert_eq!(text.blocks.len(), 1);
    let body = text.body.unwrap();
    assert_eq!(body.anchor, Some(TextAnchor::Bottom));
    assert!(body.anchor_centered);
    assert_eq!(body.left_inset.unwrap().value(), 91440);
    assert!(document.support.get("w:txbxContent").is_some());
}

#[test]
fn group_parses_children_and_transform() {
    let child = "<wps:wsp><wps:cNvPr id=\"1\" name=\"c\"/><wps:spPr><a:prstGeom prst=\"rect\"/></wps:spPr></wps:wsp>";
    let group = format!(
        "<wpg:wgp><wpg:cNvPr id=\"9\" name=\"g\" descr=\"gd\"/><wpg:grpSpPr>\
<a:xfrm rot=\"1\" flipV=\"1\"><a:off x=\"0\" y=\"0\"/><a:ext cx=\"200\" cy=\"100\"/>\
<a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"400\" cy=\"200\"/></a:xfrm></wpg:grpSpPr>\
{child}{child}</wpg:wgp>"
    );
    let (document, anchor) = anchor_of(&wrap_anchor(&group));
    let Graphic::Group(parsed) = anchor.graphic.as_ref() else {
        panic!("group");
    };
    assert_eq!(parsed.name.as_deref(), Some("g"));
    assert_eq!(parsed.children.len(), 2);
    let xfrm = parsed.xfrm.unwrap();
    assert_eq!(xfrm.child_extent.unwrap().cx.value(), 400);
    assert!(xfrm.flip_v && xfrm.rot == Some(1));
    assert!(document.support.get("wpg:wgp").is_some());
}

#[test]
fn chart_and_diagram_graphic_are_classified() {
    let chart = "<a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/chart\"><c:chart xmlns:c=\"http://purl.oclc.org/ooxml/drawingml/chart\" r:id=\"rId1\"/></a:graphicData>";
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1\" cy=\"1\"/><wp:docPr id=\"1\" name=\"c\"/><a:graphic>{chart}</a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    let parts = document_parts(&body, &[]);
    let document = parse_parts(&parts).expect("parse");
    let Block::Paragraph(para) = &document.body.blocks[0] else {
        panic!()
    };
    let Inline::Run(run) = &para.inlines[0] else {
        panic!()
    };
    let RunContent::Drawing(drawing) = &run.content[0] else {
        panic!()
    };
    let DrawingKind::Inline(inline) = &drawing.kind else {
        panic!()
    };
    assert!(matches!(inline.graphic.as_ref(), Graphic::Chart));
}

#[test]
fn microsoft_namespaces_are_accepted_as_partial() {
    let ms_ns = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
    let ms_wne = "http://schemas.microsoft.com/office/word/2006/wordml";
    let body = format!(
        "<w:p><w:r><w:drawing><wp:anchor relativeHeight=\"1\" behindDoc=\"0\" simplePos=\"0\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" allowOverlap=\"1\" layoutInCell=\"1\" locked=\"0\">\
<wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>0</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
<wp:extent cx=\"100\" cy=\"100\"/><wp:wrapNone/><wp:docPr id=\"1\" name=\"s\"/>\
<a:graphic><a:graphicData uri=\"{ms_ns}\">\
<wps:wsp xmlns:wps=\"{ms_ns}\"><wps:cNvPr id=\"1\" name=\"s\"/><wps:spPr><a:prstGeom prst=\"rect\"/></wps:spPr>\
<wps:txbx><wne:txbxContent xmlns:wne=\"{ms_wne}\"><w:p><w:r><w:t>MS</w:t></w:r></w:p></wne:txbxContent></wps:txbx>\
</wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>"
    );
    let (document, anchor) = anchor_of(&body);
    let Graphic::Shape(shape) = anchor.graphic.as_ref() else {
        panic!("shape");
    };
    assert!(shape.text.as_ref().is_some_and(|t| t.blocks.len() == 1));
    assert_eq!(
        document.support.get("wps:wsp").unwrap().status,
        strict_ooxml_wml::model::SupportStatus::Partial
    );
}

#[test]
fn page_borders_parse_edges_offsets_and_theme() {
    let body = "<w:p/><w:sectPr>\
<w:pgBorders w:offsetFrom=\"text\" w:zOrder=\"back\">\
<w:top w:val=\"double\" w:sz=\"24\" w:space=\"10\" w:color=\"112233\" w:shadow=\"1\"/>\
<w:left w:val=\"dashed\" w:sz=\"8\"/>\
<w:bottom w:val=\"nil\"/>\
<w:right w:val=\"single\" w:themeColor=\"accent1\" w:themeTint=\"AB\"/>\
</w:pgBorders></w:sectPr>";
    let parts = document_parts(body, &[]);
    let document = parse_parts(&parts).expect("parse");
    let borders = document.sections[0]
        .properties
        .page_borders
        .as_ref()
        .expect("borders");
    assert_eq!(borders.offset_from, Some(BorderOffsetFrom::Text));
    assert_eq!(borders.z_order, Some(BorderZOrder::Back));
    let top = borders.top.as_ref().unwrap();
    assert_eq!(top.style, Some(BorderStyle::Double));
    assert_eq!(top.size.unwrap().value(), 24);
    assert_eq!(top.space, Some(10));
    assert!(top.shadow);
    assert_eq!(
        borders.left.as_ref().unwrap().style,
        Some(BorderStyle::Dashed)
    );
    let theme = borders
        .right
        .as_ref()
        .unwrap()
        .theme_color
        .as_ref()
        .unwrap();
    assert_eq!(theme.color.as_str(), "accent1");
    assert_eq!(theme.tint.as_deref(), Some("AB"));
    // Lexical helpers are covered.
    assert_eq!(
        BorderOffsetFrom::from_strict("page").unwrap().as_str(),
        "page"
    );
    assert_eq!(
        BorderZOrder::from_strict("front").unwrap().as_str(),
        "front"
    );
    assert!(BorderOffsetFrom::from_strict("x").is_none());
}

#[test]
fn inline_shape_parse_and_unknown_preset() {
    let body = "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/><wp:docPr id=\"1\" name=\"s\"/>\
<a:graphic><a:graphicData uri=\"x\">\
<wps:wsp><wps:cNvPr id=\"1\" name=\"s\"/><wps:spPr><a:prstGeom prst=\"unknownPreset\"/><a:blipFill><a:blip/></a:blipFill></wps:spPr></wps:wsp>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>";
    let parts = document_parts(body, &[]);
    let document = parse_parts(&parts).expect("parse");
    let Block::Paragraph(para) = &document.body.blocks[0] else {
        panic!()
    };
    let Inline::Run(run) = &para.inlines[0] else {
        panic!()
    };
    let RunContent::Drawing(drawing) = &run.content[0] else {
        panic!()
    };
    let DrawingKind::Inline(inline) = &drawing.kind else {
        panic!()
    };
    let Graphic::Shape(shape) = inline.graphic.as_ref() else {
        panic!("shape")
    };
    assert!(
        matches!(shape.geometry, ShapeGeometry::Preset(ref p) if p.as_ref() == "unknownPreset")
    );
    // blipFill falls back to a recorded Partial no-fill.
    assert!(document.support.get("a:fill").is_some());
}
