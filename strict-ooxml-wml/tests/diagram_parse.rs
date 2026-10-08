#![allow(clippy::doc_markdown, clippy::format_push_string)]
//! A `dgm:relIds` is followed through its data part to the drawing Word cached
//! beside the diagram, and that drawing's shapes become the diagram's
//! [`DiagramDrawing`].

use std::io::Cursor;
use std::sync::Arc;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_testkit::docx::Rel;
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::drawing::{DiagramDrawing, MAX_DIAGRAM_SHAPES};
use strict_ooxml_wml::model::values::{Color, HalfPoints, Justification, TriState};
use strict_ooxml_wml::model::{
    Block, Document, DrawingKind, ForeignRefs, Graphic, Inline, RunContent, Shape, ShapeFill,
    ShapeGeometry, SupportStatus,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

const DGM_NS: &str = "http://purl.oclc.org/ooxml/drawingml/diagram";
const DSP_NS: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const DIAGRAM_DRAWING_REL: &str =
    "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing";
const DATA_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml";
const DRAWING_CONTENT_TYPE: &str = "application/vnd.ms-office.drawingml.diagramDrawing+xml";

/// The three labels, one per rounded rectangle.
const LABELS: [&str; 3] = ["Alpha", "Beta", "Gamma"];

/// A paragraph holding one inline diagram, 576 × 192 px at 96 DPI.
fn diagram_paragraph() -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"5486400\" cy=\"1828800\"/><wp:docPr id=\"1\" name=\"Diagram 1\"/>\
<a:graphic><a:graphicData uri=\"{DGM_NS}\"><dgm:relIds xmlns:dgm=\"{DGM_NS}\" \
r:dm=\"rIdDm\" r:lo=\"rIdLo\" r:qs=\"rIdQs\" r:cs=\"rIdCs\"/></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>"
    )
}

/// The data part, naming the drawing part as Word does.
fn data_part(drawing_rel: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<dgm:dataModel xmlns:dgm=\"{DGM_NS}\" xmlns:a=\"{A_NS}\"><dgm:ptLst/><dgm:cxnLst/><dgm:bg/><dgm:whole/>\
<dgm:extLst><a:ext uri=\"http://schemas.microsoft.com/office/drawing/2008/diagram\">\
<dsp:dataModelExt xmlns:dsp=\"{DSP_NS}\" relId=\"{drawing_rel}\" minVer=\"http://schemas.openxmlformats.org/drawingml/2006/diagram\"/>\
</a:ext></dgm:extLst></dgm:dataModel>"
    )
    .into_bytes()
}

/// One rounded rectangle with a centred, bold 18 pt label, at `x` EMU.
fn rounded_rect(x: i64, label: &str) -> String {
    format!(
        "<dsp:sp modelId=\"{{0}}\"><dsp:nvSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvSpPr/></dsp:nvSpPr>\
<dsp:spPr><a:xfrm><a:off x=\"{x}\" y=\"457200\"/><a:ext cx=\"1371600\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"roundRect\"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val=\"4472C4\"/></a:solidFill>\
<a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill></a:ln></dsp:spPr>\
<dsp:style><a:lnRef idx=\"2\"><a:scrgbClr r=\"0%\" g=\"0%\" b=\"0%\"/></a:lnRef>\
<a:fillRef idx=\"1\"><a:scrgbClr r=\"0%\" g=\"0%\" b=\"0%\"/></a:fillRef>\
<a:effectRef idx=\"0\"><a:scrgbClr r=\"0%\" g=\"0%\" b=\"0%\"/></a:effectRef>\
<a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></dsp:style>\
<dsp:txBody><a:bodyPr spcFirstLastPara=\"0\" vert=\"horz\" wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"ctr\"/>\
<a:lstStyle/><a:p><a:pPr algn=\"ctr\"/><a:r><a:rPr lang=\"en-US\" sz=\"1800\" b=\"1\"/><a:t>{label}</a:t></a:r></a:p></dsp:txBody>\
<dsp:txXfrm><a:off x=\"{x}\" y=\"457200\"/><a:ext cx=\"1371600\" cy=\"914400\"/></dsp:txXfrm></dsp:sp>"
    )
}

/// A drawing part holding `shapes`.
fn drawing_part(shapes: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<dsp:drawing xmlns:dgm=\"{DGM_NS}\" xmlns:dsp=\"{DSP_NS}\" xmlns:a=\"{A_NS}\"><dsp:spTree>\
<dsp:nvGrpSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr><dsp:grpSpPr/>\
{shapes}</dsp:spTree></dsp:drawing>"
    )
    .into_bytes()
}

/// The three rounded rectangles.
fn three_shapes() -> String {
    LABELS
        .iter()
        .zip([0_i64, 2_057_400, 4_114_800])
        .map(|(label, x)| rounded_rect(x, label))
        .collect()
}

/// A package with one diagram. `drawing` is the drawing part's bytes; `None`
/// leaves the part out while keeping the relationship to it.
fn package(drawing: Option<Vec<u8>>) -> Package {
    let mut builder = DocxBuilder::strict()
        .body(&diagram_paragraph())
        .rel("rIdDm", "diagramData", "diagrams/data1.xml")
        .rel_uri(Rel::new(
            "rIdDrawing",
            DIAGRAM_DRAWING_REL,
            "diagrams/drawing1.xml",
        ))
        .content_type("/word/diagrams/data1.xml", DATA_CONTENT_TYPE)
        .part("word/diagrams/data1.xml", data_part("rIdDrawing"));
    if let Some(bytes) = drawing {
        builder = builder
            .content_type("/word/diagrams/drawing1.xml", DRAWING_CONTENT_TYPE)
            .part("word/diagrams/drawing1.xml", bytes);
    }
    Package::open_reader(Cursor::new(builder.build()), &OpenOptions::default()).expect("open")
}

fn parse(drawing: Option<Vec<u8>>) -> Document {
    parse_document(&package(drawing), &ParseOptions::default()).expect("parse")
}

/// The first diagram reference in the body.
fn diagram_refs(document: &Document) -> ForeignRefs {
    for block in &document.body.blocks {
        let Some(paragraph) = block.as_paragraph() else {
            continue;
        };
        for inline in &paragraph.inlines {
            let Inline::Run(run) = inline else { continue };
            for content in &run.content {
                let RunContent::Drawing(drawing) = content else {
                    continue;
                };
                let DrawingKind::Inline(inline) = &drawing.kind else {
                    continue;
                };
                if let Graphic::Diagram(found) = inline.graphic.as_ref() {
                    return found.clone();
                }
            }
        }
    }
    panic!("no diagram in the body");
}

/// The text of a shape's text box.
fn shape_text(shape: &Shape) -> String {
    let mut text = String::new();
    for block in &shape.text.as_ref().expect("text body").blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &paragraph.inlines {
            let Inline::Run(run) = inline else { continue };
            for content in &run.content {
                if let RunContent::Text(node) = content {
                    text.push_str(&node.text);
                }
            }
        }
    }
    text
}

fn only_drawing(document: &Document) -> Arc<DiagramDrawing> {
    diagram_refs(document).diagram.expect("diagram drawing")
}

#[test]
fn the_cached_drawing_of_a_diagram_is_read() {
    let document = parse(Some(drawing_part(&three_shapes())));
    let refs = diagram_refs(&document);
    assert_eq!(refs.ids(), vec!["rIdDm", "rIdLo", "rIdQs", "rIdCs"]);
    let drawing = only_drawing(&document);
    assert_eq!(drawing.part.as_str(), "/word/diagrams/drawing1.xml");
    assert_eq!(drawing.xfrm, None, "an empty dsp:grpSpPr is no transform");
    assert!(!drawing.truncated);
    assert_eq!(drawing.shapes.len(), 3);
    for ((graphic, label), x) in drawing
        .shapes
        .iter()
        .zip(LABELS)
        .zip([0_i64, 2_057_400, 4_114_800])
    {
        let Graphic::Shape(shape) = graphic else {
            panic!("a dsp:sp is a shape: {graphic:?}");
        };
        assert_eq!(
            shape.geometry,
            ShapeGeometry::Preset(Arc::from("roundRect"))
        );
        let (off_x, off_y) = shape.offset.expect("offset");
        assert_eq!((off_x.value(), off_y.value()), (x, 457_200));
        let extent = shape.extent.expect("extent");
        assert_eq!((extent.cx.value(), extent.cy.value()), (1_371_600, 914_400));
        let Some(ShapeFill::Solid { color }) = &shape.fill else {
            panic!("solid fill: {:?}", shape.fill);
        };
        assert_eq!(color.value.as_ref().map(Color::as_str), Some("4472C4"));
        assert!(shape.stroke.is_some());
        assert_eq!(shape_text(shape), label);

        let text = shape.text.as_ref().expect("text");
        let Block::Paragraph(paragraph) = &text.blocks[0] else {
            panic!("a paragraph");
        };
        assert_eq!(paragraph.props.alignment, Some(Justification::Center));
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!("a run");
        };
        assert_eq!(run.props.size, Some(HalfPoints(36)));
        assert_eq!(run.props.bold, TriState::On);
        assert_eq!(
            run.props
                .color_theme
                .as_ref()
                .map(|theme| theme.color.as_str()),
            Some("lt1"),
            "the text colour comes from dsp:style/a:fontRef"
        );
    }
    assert_eq!(
        document.support.get("dsp:drawing").map(|use_| use_.status),
        Some(SupportStatus::Supported)
    );
    assert_eq!(
        document.support.get("dsp:txXfrm").map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
}

#[test]
fn a_missing_drawing_part_leaves_no_drawing_and_a_record() {
    let document = parse(None);
    let refs = diagram_refs(&document);
    assert_eq!(refs.ids(), vec!["rIdDm", "rIdLo", "rIdQs", "rIdCs"]);
    assert!(refs.diagram.is_none());
    assert_eq!(
        document.support.get("dgm:relIds").map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
    assert!(document.support.get("dsp:drawing").is_none());
}

#[test]
fn a_malformed_drawing_part_leaves_no_drawing_and_a_record() {
    let broken = format!("<dsp:drawing xmlns:dsp=\"{DSP_NS}\"><dsp:spTree>").into_bytes();
    let document = parse(Some(broken));
    assert!(diagram_refs(&document).diagram.is_none());
    assert_eq!(
        document.support.get("dgm:relIds").map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
}

#[test]
fn shapes_past_the_cap_are_dropped_and_reported() {
    let shape = rounded_rect(0, "x");
    let shapes = shape.repeat(MAX_DIAGRAM_SHAPES + 5);
    let document = parse(Some(drawing_part(&shapes)));
    let drawing = only_drawing(&document);
    assert_eq!(drawing.shapes.len(), MAX_DIAGRAM_SHAPES);
    assert!(drawing.truncated);
    assert_eq!(
        document.support.get("dsp:spTree").map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
}
