#![allow(clippy::doc_markdown, clippy::float_cmp)]
//! A SmartArt diagram is drawn from the drawing Word cached beside it instead
//! of a placeholder.

use std::io::Cursor;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{render_with_media, Page, RenderOptions};
use strict_ooxml_testkit::docx::Rel;
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

const DGM_NS: &str = "http://purl.oclc.org/ooxml/drawingml/diagram";
const DSP_NS: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const DIAGRAM_DRAWING_REL: &str =
    "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing";

const LABELS: [&str; 3] = ["Alpha", "Beta", "Gamma"];
/// Shape offsets in EMU: 0, 216 and 432 px.
const OFFSETS: [i64; 3] = [0, 2_057_400, 4_114_800];
/// Shape size in px (1371600 × 914400 EMU).
const SHAPE_W: f64 = 144.0;
const SHAPE_H: f64 = 96.0;

/// A paragraph holding one inline diagram, 576 × 192 px.
fn diagram_paragraph() -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"5486400\" cy=\"1828800\"/><wp:docPr id=\"1\" name=\"Diagram 1\"/>\
<a:graphic><a:graphicData uri=\"{DGM_NS}\"><dgm:relIds xmlns:dgm=\"{DGM_NS}\" \
r:dm=\"rIdDm\" r:lo=\"rIdLo\" r:qs=\"rIdQs\" r:cs=\"rIdCs\"/></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>"
    )
}

fn data_part() -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<dgm:dataModel xmlns:dgm=\"{DGM_NS}\" xmlns:a=\"{A_NS}\"><dgm:ptLst/><dgm:cxnLst/>\
<dgm:extLst><a:ext uri=\"{DSP_NS}\"><dsp:dataModelExt xmlns:dsp=\"{DSP_NS}\" relId=\"rIdDrawing\"/>\
</a:ext></dgm:extLst></dgm:dataModel>"
    )
    .into_bytes()
}

fn rounded_rect(x: i64, label: &str) -> String {
    format!(
        "<dsp:sp><dsp:nvSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvSpPr/></dsp:nvSpPr>\
<dsp:spPr><a:xfrm><a:off x=\"{x}\" y=\"457200\"/><a:ext cx=\"1371600\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"roundRect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"4472C4\"/></a:solidFill>\
<a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill></a:ln></dsp:spPr>\
<dsp:txBody><a:bodyPr lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"ctr\"/><a:lstStyle/>\
<a:p><a:pPr algn=\"ctr\"/><a:r><a:rPr lang=\"en-US\" sz=\"1800\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill></a:rPr>\
<a:t>{label}</a:t></a:r></a:p></dsp:txBody></dsp:sp>"
    )
}

fn drawing_part() -> Vec<u8> {
    let shapes: String = LABELS
        .iter()
        .zip(OFFSETS)
        .map(|(label, x)| rounded_rect(x, label))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<dsp:drawing xmlns:dsp=\"{DSP_NS}\" xmlns:a=\"{A_NS}\"><dsp:spTree>\
<dsp:nvGrpSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr><dsp:grpSpPr/>\
{shapes}</dsp:spTree></dsp:drawing>"
    )
    .into_bytes()
}

/// Renders a document holding the diagram; `with_drawing: false` leaves the
/// drawing part out.
fn render(with_drawing: bool) -> Vec<Page> {
    let mut builder = DocxBuilder::strict()
        .body(&diagram_paragraph())
        .rel("rIdDm", "diagramData", "diagrams/data1.xml")
        .rel_uri(Rel::new(
            "rIdDrawing",
            DIAGRAM_DRAWING_REL,
            "diagrams/drawing1.xml",
        ))
        .content_type(
            "/word/diagrams/data1.xml",
            "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml",
        )
        .part("word/diagrams/data1.xml", data_part());
    if with_drawing {
        builder = builder
            .content_type(
                "/word/diagrams/drawing1.xml",
                "application/vnd.ms-office.drawingml.diagramDrawing+xml",
            )
            .part("word/diagrams/drawing1.xml", drawing_part());
    }
    let package =
        Package::open_reader(Cursor::new(builder.build()), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render")
}

/// The `translate(x y)` origin of every `<path>` filled with `fill`.
fn path_origins(svg: &str, fill: &str) -> Vec<(f64, f64)> {
    let tree = roxmltree::Document::parse(svg).expect("svg");
    tree.descendants()
        .filter(|node| node.has_tag_name("path") && node.attribute("fill") == Some(fill))
        .filter_map(|node| {
            let transform = node.attribute("transform")?;
            let inner = transform.strip_prefix("translate(")?;
            let inner = &inner[..inner.find(')')?];
            let mut numbers = inner.split_whitespace().map(str::parse::<f64>);
            Some((numbers.next()?.ok()?, numbers.next()?.ok()?))
        })
        .collect()
}

/// `(first x, baseline y, text)` of every `<text>`.
fn texts(svg: &str) -> Vec<(f64, f64, String)> {
    let tree = roxmltree::Document::parse(svg).expect("svg");
    tree.descendants()
        .filter(|node| node.has_tag_name("text"))
        .filter_map(|node| {
            let x = node
                .attribute("x")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            let y = node.attribute("y")?.parse().ok()?;
            Some((x, y, node.text()?.to_owned()))
        })
        .collect()
}

/// How many `<rect>`s are filled with `fill`.
fn rects(svg: &str, fill: &str) -> usize {
    let tree = roxmltree::Document::parse(svg).expect("svg");
    tree.descendants()
        .filter(|node| node.has_tag_name("rect") && node.attribute("fill") == Some(fill))
        .count()
}

#[test]
fn a_diagram_draws_its_cached_shapes_and_texts() {
    let pages = render(true);
    let svg = &pages[0].svg;
    assert_eq!(rects(svg, "#f2f2f2"), 0, "no placeholder");
    let mut shapes = path_origins(svg, "#4472c4");
    shapes.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(shapes.len(), 3, "three filled shapes: {shapes:?}");
    let (left, top) = shapes[0];
    for ((x, y), expected) in shapes.iter().zip([0.0, 216.0, 432.0]) {
        assert!((x - left - expected).abs() < 0.01, "{shapes:?}");
        assert!((y - top).abs() < 0.01, "{shapes:?}");
    }
    let labels = texts(svg);
    for ((x, y), label) in shapes.iter().zip(LABELS) {
        let Some((text_x, text_y, _)) = labels.iter().find(|(_, _, text)| text == label) else {
            panic!("{label} is drawn: {labels:?}");
        };
        assert!(
            *text_x >= *x && *text_x <= x + SHAPE_W,
            "{label} at {text_x} lies inside its shape at {x}"
        );
        assert!(
            *text_y >= *y && *text_y <= y + SHAPE_H,
            "{label} at {text_y} lies inside its shape at {y}"
        );
    }
}

#[test]
fn a_diagram_without_its_drawing_keeps_the_placeholder() {
    let pages = render(false);
    let svg = &pages[0].svg;
    assert!(path_origins(svg, "#4472c4").is_empty());
    assert_eq!(rects(svg, "#f2f2f2"), 1, "the placeholder stays");
}
