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
    let page = placed(floating_shape(
        "1",
        "x=\"190500\" y=\"285750\"",
        "<wp:positionH relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionH>",
        "<wp:positionV relativeFrom=\"page\"><wp:posOffset>914400</wp:posOffset></wp:positionV>",
    ));
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

/// All text items of a page, groups included, with their translation applied.
fn all_texts(items: &[Item], dx: f64, dy: f64, out: &mut Vec<(f64, f64, String)>) {
    for item in items {
        match item {
            Item::Text(text) => out.push((text.x + dx, text.baseline + dy, text.text.clone())),
            Item::Group(group) => all_texts(
                &group.items,
                dx + group.matrix[4],
                dy + group.matrix[5],
                out,
            ),
            _ => {}
        }
    }
}

/// `bodyPr/@vertOverflow="ellipsis"` (R4): the lines that do not fit are
/// dropped and the last visible one ends in an ellipsis.
#[test]
fn vertical_overflow_ellipsis_ends_the_last_visible_line() {
    let paragraphs = (1..=8)
        .map(|index| format!("<w:p><w:r><w:t>Line{index} words</w:t></w:r></w:p>"))
        .collect::<String>();
    let shape = |overflow: &str| {
        format!(
            "<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"457200\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></wps:spPr>\
             <wps:txbx><w:txbxContent>{paragraphs}</w:txbxContent></wps:txbx>\
             <wps:bodyPr vertOverflow=\"{overflow}\"/></wps:wsp>"
        )
    };
    let texts = |overflow: &str| {
        let page = placed(shape_docx(&shape(overflow)));
        let mut out = Vec::new();
        all_texts(&page.items, 0.0, 0.0, &mut out);
        out
    };
    let overflowing = texts("overflow");
    assert!(overflowing
        .iter()
        .any(|(_, _, text)| text.contains("Line8")));
    let ellipsis = texts("ellipsis");
    assert!(
        !ellipsis.iter().any(|(_, _, text)| text.contains("Line8")),
        "{ellipsis:?}"
    );
    let last = ellipsis
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)))
        .expect("a visible line");
    assert!(last.2.ends_with('\u{2026}'), "{ellipsis:?}");
}

/// Every group on the page, nested ones included.
fn groups(items: &[Item]) -> Vec<&strict_ooxml_render_svg::layout::GroupItem> {
    let mut out = Vec::new();
    for item in items {
        if let Item::Group(group) = item {
            out.push(group);
            out.extend(groups(&group.items));
        }
    }
    out
}

/// A filled, outlined rectangle with `effects` in its `a:effectLst`.
fn shape_with_effects(effects: &str, extra: &str) -> PlacedPage {
    placed(shape_docx(&format!(
        "<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>\
         <a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill>\
         <a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"0000FF\"/></a:solidFill></a:ln>\
         <a:effectLst>{effects}</a:effectLst></wps:spPr>{extra}<wps:bodyPr/></wps:wsp>"
    )))
}

/// A blurred outer shadow is nine offset copies in the shadow colour, each
/// carrying its share of the alpha, beneath the shape; the shape stays on top
/// in its own colours.
#[test]
fn an_outer_shadow_is_blurred_copies_beneath_the_shape() {
    let page = shape_with_effects(
        "<a:outerShdw blurRad=\"50800\" dist=\"38100\" dir=\"2700000\" sx=\"90000\" sy=\"90000\">\
         <a:srgbClr val=\"FF0000\"><a:alpha val=\"45000\"/></a:srgbClr></a:outerShdw>\
         <a:blur rad=\"1000\"/><a:reflection/>",
        "",
    );
    let shadows: Vec<_> = groups(&page.items)
        .into_iter()
        .filter(|group| (group.opacity - 0.05).abs() < 1e-9)
        .collect();
    assert_eq!(shadows.len(), 9, "{:?}", page.items);
    for shadow in &shadows {
        let leaves: Vec<_> = shadow.items.iter().flat_map(Item::leaves).collect();
        assert!(leaves.iter().any(|leaf| matches!(
            leaf,
            Item::Path(path) if path.fill.as_deref() == Some("#FF0000")
                && path.stroke.as_deref() == Some("#FF0000")
        )));
        // Scaled to 90% about the centre and moved down-right.
        assert!((shadow.matrix[0] - 0.9).abs() < 1e-9 && (shadow.matrix[3] - 0.9).abs() < 1e-9);
        assert!(
            shadow.matrix[4] > 0.0 && shadow.matrix[5] > 0.0,
            "{:?}",
            shadow.matrix
        );
    }
    let shape = paths(&page).into_iter().last().expect("the shape");
    assert_eq!(shape.fill.as_deref(), Some("#00ff00"));
}

/// A glow widens the outline of every copy instead of filling it, and takes a
/// theme colour; an effect without blur is one copy at full alpha.
#[test]
fn a_glow_strokes_wider_copies_in_its_theme_colour() {
    let page = shape_with_effects(
        "<a:glow rad=\"63500\"><a:schemeClr val=\"accent2\"/></a:glow>\
         <a:outerShdw dist=\"12700\" dir=\"5400000\"><a:srgbClr val=\"zz\"/></a:outerShdw>",
        "",
    );
    let all = groups(&page.items);
    let glow: Vec<_> = all
        .iter()
        .flat_map(|group| group.items.iter().flat_map(Item::leaves))
        .filter_map(|leaf| match leaf {
            Item::Path(path) if path.stroke.as_deref() == Some("#ed7d31") => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(glow.len(), 9, "{:?}", page.items);
    assert!(glow.iter().all(|path| path.fill.is_none()));
    let widths: Vec<f64> = glow.iter().map(|path| path.stroke_w).collect();
    assert!(
        widths.windows(2).all(|pair| pair[1] > pair[0]),
        "each sample is wider: {widths:?}"
    );
    // The unblurred shadow: one copy, full alpha, still black (a bad colour
    // value keeps the default).
    let shadow = all
        .iter()
        .find(|group| (group.opacity - 1.0).abs() < 1e-9 && group.matrix[5] > 0.0)
        .expect("one shadow copy");
    assert!(shadow
        .items
        .iter()
        .flat_map(Item::leaves)
        .any(|leaf| matches!(
            leaf,
            Item::Path(path) if path.fill.as_deref() == Some("#000000")
        )));
}

/// An inner shadow is drawn over the shape, clipped to its silhouette and
/// cut out by the shifted silhouette.
#[test]
fn an_inner_shadow_is_clipped_inside_the_shape() {
    let page = shape_with_effects(
        "<a:innerShdw blurRad=\"25400\" dist=\"25400\" dir=\"0\">\
         <a:srgbClr val=\"0000FF\"><a:alpha val=\"90000\"/></a:srgbClr></a:innerShdw>",
        "",
    );
    let all = groups(&page.items);
    let outer: Vec<_> = all
        .iter()
        .filter(|group| group.clip_path.is_some() && !group.clip_inverse)
        .collect();
    assert_eq!(outer.len(), 9, "{:?}", page.items);
    assert!(outer.iter().all(|group| (group.opacity - 0.1).abs() < 1e-9));
    let cut: Vec<_> = all.iter().filter(|group| group.clip_inverse).collect();
    assert_eq!(cut.len(), 9);
    assert!(cut.iter().all(|group| group.clip.is_some()));
    // Drawn after the shape it shades.
    let last = page.items.last().expect("items");
    assert!(matches!(last, Item::Group(_)), "{last:?}");
}

/// Text in a shape with a shadow is copied into the shadow too, in the
/// shadow's colour and without its shading.
#[test]
fn a_shadow_copies_the_shapes_text_in_its_colour() {
    let page = shape_with_effects(
        "<a:outerShdw dist=\"25400\" dir=\"5400000\"><a:srgbClr val=\"808080\"/></a:outerShdw>",
        "<wps:txbx><w:txbxContent><w:p><w:r><w:rPr><w:shd w:val=\"clear\" w:fill=\"FFFF00\"/></w:rPr>\
         <w:t>Shade</w:t></w:r></w:p></w:txbxContent></wps:txbx>",
    );
    let texts: Vec<_> = page
        .items
        .iter()
        .flat_map(Item::leaves)
        .filter_map(|leaf| match leaf {
            Item::Text(text) if text.text.contains("Shade") => Some(text),
            _ => None,
        })
        .collect();
    assert!(!texts.is_empty());
    let shadowed: Vec<_> = groups(&page.items)
        .into_iter()
        .flat_map(|group| group.items.iter())
        .filter_map(|item| match item {
            Item::Text(text) if text.text.contains("Shade") => Some(text),
            _ => None,
        })
        .collect();
    if let Some(text) = shadowed.first() {
        assert_eq!(text.run.color.as_deref(), Some("#808080"));
        assert!(text.run.shading.is_none());
    }
}

/// A connector's glow widens its line.
#[test]
fn a_glow_on_a_line_widens_it() {
    let page = placed(shape_docx(
        "<wps:wsp><wps:cNvCnPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"0\"/></a:xfrm>\
         <a:prstGeom prst=\"straightConnector1\"><a:avLst/></a:prstGeom>\
         <a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></a:ln>\
         <a:effectLst><a:glow rad=\"25400\"><a:srgbClr val=\"00FFFF\"/></a:glow></a:effectLst>\
         </wps:spPr><wps:bodyPr/></wps:wsp>",
    ));
    let glow: Vec<_> = groups(&page.items)
        .into_iter()
        .flat_map(|group| group.items.iter().flat_map(Item::leaves))
        .filter(|leaf| match leaf {
            Item::Path(path) => path.stroke.as_deref() == Some("#00FFFF"),
            Item::Line(line) => line.color == "#00FFFF",
            _ => false,
        })
        .collect();
    assert!(!glow.is_empty(), "{:?}", page.items);
}

/// A rectangle filled with `fill`, rotated by `rot` (60000ths of a degree).
fn filled_shape(fill: &str, rot: i32) -> Option<strict_ooxml_render_svg::layout::PathPaint> {
    let page = placed(shape_docx(&format!(
        "<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm rot=\"{rot}\"><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{fill}</wps:spPr><wps:bodyPr/></wps:wsp>"
    )));
    let paint = page
        .items
        .iter()
        .flat_map(Item::leaves)
        .find_map(|leaf| match leaf {
            Item::Path(path) => Some(path.paint.clone()),
            _ => None,
        })
        .expect("the shape");
    paint
}

#[test]
fn a_linear_gradient_spans_the_box_with_padded_and_merged_stops() {
    use strict_ooxml_render_svg::layout::PathPaint;
    let paint = filled_shape(
        "<a:gradFill rotWithShape=\"0\"><a:gsLst>\
         <a:gs pos=\"60%\"><a:srgbClr val=\"0000FF\"/></a:gs>\
         <a:gs pos=\"20%\"><a:srgbClr val=\"FF0000\"/></a:gs>\
         <a:gs pos=\"60%\"><a:srgbClr val=\"00FF00\"/></a:gs>\
         </a:gsLst><a:lin ang=\"5400000\" scaled=\"1\"/></a:gradFill>",
        5_400_000,
    );
    let Some(PathPaint::LinearGradient { stops, start, end }) = paint else {
        panic!("a linear gradient: {paint:?}");
    };
    let positions: Vec<f64> = stops.iter().map(|stop| stop.0).collect();
    assert_eq!(positions, [0.0, 0.2, 0.6, 1.0], "{stops:?}");
    assert_eq!(stops.first().map(|stop| stop.1.as_str()), Some("#ff0000"));
    // The shape's own 90° cancels the gradient's 90° (rotWithShape="0").
    assert!((start.1 - end.1).abs() < 1e-6, "{start:?} {end:?}");
    assert!(end.0 > start.0);

    let single = filled_shape(
        "<a:gradFill><a:gsLst><a:gs pos=\"100%\"><a:srgbClr val=\"123456\"/></a:gs></a:gsLst>\
         <a:lin ang=\"0\" scaled=\"0\"/></a:gradFill>",
        0,
    );
    let Some(PathPaint::LinearGradient { stops, .. }) = single else {
        panic!("{single:?}");
    };
    assert_eq!(stops.len(), 2);
    assert!(stops.iter().all(|stop| stop.1 == "#123456"));
}

#[test]
fn a_path_gradient_falls_back_to_a_flat_fill() {
    let paint = filled_shape(
        "<a:gradFill><a:gsLst><a:gs pos=\"0%\"><a:srgbClr val=\"FF0000\"/></a:gs>\
         <a:gs pos=\"100%\"><a:srgbClr val=\"0000FF\"/></a:gs></a:gsLst>\
         <a:path path=\"circle\"><a:fillToRect l=\"50%\" t=\"50%\" r=\"50%\" b=\"50%\"/></a:path></a:gradFill>",
        0,
    );
    assert!(paint.is_none(), "{paint:?}");
}

#[test]
fn pattern_presets_draw_their_tiles() {
    use strict_ooxml_render_svg::layout::PathPaint;
    let pattern = |preset: &str| {
        filled_shape(
            &format!(
                "<a:pattFill prst=\"{preset}\"><a:fgClr><a:srgbClr val=\"FF0000\"/></a:fgClr>\
                 <a:bgClr><a:srgbClr val=\"FFFFFF\"/></a:bgClr></a:pattFill>"
            ),
            0,
        )
    };
    for (preset, lines, rectangles) in [
        ("horz", 1, 0),
        ("dkHorz", 2, 0),
        ("narVert", 1, 0),
        ("dkVert", 2, 0),
        ("cross", 2, 0),
        ("lgGrid", 2, 0),
        ("dnDiag", 3, 0),
        ("wdUpDiag", 3, 0),
        ("dkDnDiag", 6, 0),
        ("diagCross", 2, 0),
        ("smCheck", 0, 2),
        ("pct50", 0, 32),
        ("pct0", 0, 0),
    ] {
        let Some(PathPaint::Pattern {
            foreground,
            background,
            size,
            lines: drawn,
            rectangles: cells,
        }) = pattern(preset)
        else {
            panic!("{preset}: a pattern");
        };
        assert_eq!(foreground, "#ff0000", "{preset}");
        assert_eq!(background, "#ffffff", "{preset}");
        assert_eq!(drawn.len(), lines, "{preset}");
        assert_eq!(cells.len(), rectangles, "{preset}");
        assert!(size > 0.0);
    }
    // Presets this renderer does not draw fall back to a flat fill.
    for preset in ["pct250", "trellis"] {
        assert!(pattern(preset).is_none(), "{preset}");
    }
    // No colours: black on white.
    let Some(PathPaint::Pattern {
        foreground,
        background,
        ..
    }) = filled_shape("<a:pattFill prst=\"pct25\"/>", 0)
    else {
        panic!("a pattern");
    };
    assert_eq!(
        (foreground.as_str(), background.as_str()),
        ("#000000", "#ffffff")
    );
}
