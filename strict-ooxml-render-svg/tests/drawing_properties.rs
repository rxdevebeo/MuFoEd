//! DrawingML properties that used to stop at the model
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md` §6): `a:srcRect`, connectors and
//! line ends, `wps:style`, `a:path/@fill` and `wp:docPr/@hidden`.

#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::doc_markdown,
    clippy::format_collect,
    clippy::needless_pass_by_value,
    clippy::similar_names
)]

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{
    place_pages, render_with_media, Item, PathItem, PlacedPage, RenderOptions,
};
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

/// A 1x1 red PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

const THEME: &str = "<a:themeElements><a:clrScheme name=\"Test\">\
    <a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1>\
    <a:dk2><a:srgbClr val=\"44546A\"/></a:dk2><a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
    <a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1><a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2>\
    <a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3><a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>\
    <a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5><a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6>\
    <a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink><a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink>\
    </a:clrScheme><a:fontScheme name=\"Test\"><a:majorFont><a:latin typeface=\"Calibri Light\"/>\
    <a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/>\
    <a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme></a:themeElements>";

fn open(builder: DocxBuilder) -> (Package, Document) {
    let bytes = builder.build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    (package, document)
}

fn shape_docx(shape: &str) -> DocxBuilder {
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1828800\" cy=\"914400\"/>\
         <wp:docPr id=\"1\" name=\"shape\"/><a:graphic>\
         <a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
         {shape}</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    DocxBuilder::strict()
        .body(&body)
        .part_xml("word/theme/theme1.xml", "a:theme", THEME)
        .content_type(
            "/word/theme/theme1.xml",
            "application/vnd.openxmlformats-officedocument.theme+xml",
        )
        .rel("rIdTheme", "theme", "theme/theme1.xml")
}

fn placed(builder: DocxBuilder) -> PlacedPage {
    let (package, document) = open(builder);
    place_pages(&document, &RenderOptions::default(), Some(&package))
        .expect("place")
        .into_iter()
        .next()
        .expect("page")
}

fn paths(page: &PlacedPage) -> Vec<PathItem> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Path(path) => Some(path.clone()),
            _ => None,
        })
        .collect()
}

fn floating_shape(simple: &str, point: &str, horizontal: &str, vertical: &str) -> DocxBuilder {
    DocxBuilder::strict().body(&format!(
        "<w:p><w:r><w:drawing><wp:anchor simplePos=\"{simple}\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
         <wp:simplePos {point}/>{horizontal}{vertical}<wp:extent cx=\"914400\" cy=\"914400\"/><wp:wrapSquare wrapText=\"bothSides\"/>\
         <wp:docPr id=\"1\" name=\"box\"/><a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
         <wps:wsp><wps:cNvSpPr/><wps:spPr><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></wps:spPr>\
         <wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r><w:r><w:t>Wrapped text</w:t></w:r></w:p>"
    ))
}

#[test]
fn simple_position_takes_precedence_over_relative_position() {
    let page = placed(floating_shape("1", "x=\"190500\" y=\"285750\"",
        "<wp:positionH relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionH>",
        "<wp:positionV relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionV>"));
    let path = paths(&page).into_iter().next().expect("shape");
    assert!((path.x - 20.0).abs() < 0.01);
    assert!((path.y - 30.0).abs() < 0.01);
}

#[test]
fn a_cropped_picture_draws_the_whole_picture_behind_a_clip() {
    let body = "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/>\
        <wp:docPr id=\"1\" name=\"picture\"/><a:graphic>\
        <a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
        <pic:nvPicPr><pic:cNvPr id=\"0\" name=\"picture\"/><pic:cNvPicPr/></pic:nvPicPr>\
        <pic:blipFill><a:blip r:embed=\"rIdImage\"/><a:srcRect l=\"25000\" r=\"25000\"/>\
        <a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
        <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
        <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
        </a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>";
    let builder = DocxBuilder::strict()
        .body(body)
        .part("word/media/image1.png", PNG.to_vec())
        .content_type("/word/media/image1.png", "image/png")
        .rel("rIdImage", "image", "media/image1.png");
    let (package, document) = open(builder);
    let options = RenderOptions::default();
    let page = place_pages(&document, &options, Some(&package))
        .expect("place")
        .into_iter()
        .next()
        .expect("page");
    let image = page
        .items
        .iter()
        .find_map(|item| match item {
            Item::Image(image) => Some(image.clone()),
            _ => None,
        })
        .expect("image");
    assert_eq!(image.crop, Some([0.25, 0.0, 0.25, 0.0]));
    let (_, _, full_w, full_h) = image.uncropped_box();
    assert!(
        (full_w - 2.0 * image.w).abs() < 1e-9,
        "half the picture is cut"
    );
    assert!((full_h - image.h).abs() < 1e-9);
    let svg = &render_with_media(&document, &options, Some(&package)).expect("svg")[0].svg;
    assert!(svg.contains("overflow=\"hidden\""), "{svg}");
}

#[test]
fn a_connector_with_an_arrow_is_a_stroked_line_and_a_filled_head() {
    let page = placed(shape_docx(
        "<wps:wsp><wps:cNvCnPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"0\"/></a:xfrm>\
         <a:prstGeom prst=\"straightConnector1\"><a:avLst/></a:prstGeom>\
         <a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill><a:tailEnd type=\"triangle\"/></a:ln>\
         </wps:spPr><wps:bodyPr/></wps:wsp>",
    ));
    let paths = paths(&page);
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert_eq!(paths[0].fill, None, "a line is not filled");
    assert_eq!(paths[0].stroke.as_deref(), Some("#ff0000"));
    assert_eq!(paths[1].fill.as_deref(), Some("#ff0000"), "the arrow head");
    assert!(
        page.items.iter().all(|item| !matches!(item, Item::Rect(_))),
        "no placeholder"
    );
}

#[test]
fn a_shape_without_its_own_fill_or_outline_takes_its_style() {
    let page = placed(shape_docx(
        "<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></wps:spPr>\
         <wps:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:lnRef>\
         <a:fillRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:fillRef>\
         <a:effectRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:effectRef>\
         <a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></wps:style>\
         <wps:txbx><w:txbxContent><w:p><w:r><w:t>Label</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
         <wps:bodyPr/></wps:wsp>",
    ));
    let shape = paths(&page).into_iter().next().expect("shape");
    assert_eq!(shape.fill.as_deref(), Some("#4472c4"));
    assert_eq!(shape.stroke.as_deref(), Some("#4472c4"));
    assert!(
        (shape.stroke_w - 4.0 / 3.0).abs() < 0.01,
        "1 pt: {}",
        shape.stroke_w
    );
    let label = page
        .items
        .iter()
        .find_map(|item| match item {
            Item::Text(text) if text.text.contains("Label") => Some(text.clone()),
            _ => None,
        })
        .expect("label");
    assert_eq!(label.run.color.as_deref(), Some("#ffffff"));
}

#[test]
fn a_path_marked_unfilled_is_only_outlined() {
    let page = placed(shape_docx(
        "<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm>\
         <a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/>\
         <a:pathLst><a:path w=\"100\" h=\"100\" fill=\"none\"><a:moveTo><a:pt x=\"0\" y=\"0\"/></a:moveTo>\
         <a:lnTo><a:pt x=\"100\" y=\"100\"/></a:lnTo><a:lnTo><a:pt x=\"0\" y=\"100\"/></a:lnTo></a:path></a:pathLst></a:custGeom>\
         <a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill>\
         <a:ln><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill></a:ln></wps:spPr><wps:bodyPr/></wps:wsp>",
    ));
    let shape = paths(&page).into_iter().next().expect("shape");
    assert_eq!(shape.fill, None);
    assert_eq!(shape.stroke.as_deref(), Some("#000000"));
}

/// `wp:anchor/@hidden`.
#[test]
fn a_hidden_floating_object_is_not_drawn() {
    let anchor = |hidden: &str| {
        format!(
            "<w:p><w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" \
             relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"{hidden}>\
             <wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"column\"><wp:posOffset>0</wp:posOffset></wp:positionH>\
             <wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
             <wp:extent cx=\"914400\" cy=\"914400\"/><wp:wrapNone/><wp:docPr id=\"1\" name=\"box\"/>\
             <a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
             <wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></wps:spPr>\
             <wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>"
        )
    };
    let shown = placed(DocxBuilder::strict().body(&anchor("")));
    let hidden = placed(DocxBuilder::strict().body(&anchor(" hidden=\"1\"")));
    assert_eq!(paths(&shown).len(), 1);
    assert!(paths(&hidden).is_empty());
}
