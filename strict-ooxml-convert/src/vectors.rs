//! PDF paths that the table pass does not consume.
//!
//! An axis-aligned rectangle becomes a page-anchored DrawingML shape. Anything
//! else is named in the report. A path that is neither kept nor named would be
//! a silent loss.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_pdf::content::{Item, Rgb, Vector};
use strict_ooxml_pdf::PdfPage;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, DocPr, Drawing, DrawingKind, Extent, Graphic, Position, Shape, ShapeColor,
    ShapeFill, ShapeGeometry, ShapeStroke, Wrap, WrapKind,
};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::props::ParagraphProperties;
use strict_ooxml_wml::model::values::{Color, Emu, Rsids};

use crate::report::{ConversionReport, Severity};

const EMU_PER_POINT: f64 = 12_700.0;
/// A path thinner than this is a ruling line, already handled by the table pass.
const RULE_THICKNESS: f64 = 2.0;

/// Page-anchored shapes for the vectors this page did not turn into table rules.
pub(crate) fn blocks_for(page: &PdfPage, report: &mut ConversionReport) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (index, item) in page.items().iter().enumerate() {
        let Item::Vector(vector) = item else {
            continue;
        };
        match rectangle(vector, page.geometry.height) {
            // The PDF renderer paints the page itself as a white rectangle.
            // That canvas is the page, not a drawing, so it is not a second
            // shape and it is not a loss.
            Some(rect) if is_page_canvas(&rect, page.geometry.width, page.geometry.height) => {}
            Some(rect) => blocks.push(shape_paragraph(index, &rect)),
            None if is_rule(vector) => {}
            None => report.record(
                "vector.unsupported",
                Severity::Unsupported,
                format!("page {} vector {index}", page.number),
            ),
        }
    }
    blocks
}

struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: Option<Rgb>,
    stroke: Option<Rgb>,
    line_width: f64,
}

fn rectangle(vector: &Vector, page_height: f64) -> Option<Rect> {
    if vector.subpaths.len() != 1 {
        return None;
    }
    let subpath = &vector.subpaths[0];
    if subpath.points.len() < 4 {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut placed = Vec::with_capacity(subpath.points.len());
    for (x, y) in &subpath.points {
        let (x, y) = vector.ctm.apply(*x, *y);
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        placed.push((x, y));
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    if width <= RULE_THICKNESS || height <= RULE_THICKNESS {
        return None;
    }
    let on_edge = placed.iter().all(|(x, y)| {
        let on_x = (x - min_x).abs() <= 0.5 || (x - max_x).abs() <= 0.5;
        let on_y = (y - min_y).abs() <= 0.5 || (y - max_y).abs() <= 0.5;
        on_x && on_y
    });
    if !on_edge {
        return None;
    }
    Some(Rect {
        x: min_x,
        y: page_height - max_y,
        width,
        height,
        fill: vector.fill,
        stroke: vector.stroke,
        line_width: vector.line_width,
    })
}

fn is_page_canvas(rect: &Rect, page_width: f64, page_height: f64) -> bool {
    let covers = rect.x.abs() <= 1.0
        && rect.y.abs() <= 1.0
        && (rect.width - page_width).abs() <= 1.0
        && (rect.height - page_height).abs() <= 1.0;
    let white = rect.fill.is_some_and(|rgb| {
        let [red, green, blue] = rgb.to_rgb8();
        red >= 250 && green >= 250 && blue >= 250
    });
    covers && white && rect.stroke.is_none()
}

fn is_rule(vector: &Vector) -> bool {
    vector.subpaths.iter().any(|subpath| {
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in &subpath.points {
            let (x, y) = vector.ctm.apply(*x, *y);
            if !(x.is_finite() && y.is_finite()) {
                return false;
            }
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        (max_x - min_x).max(max_y - min_y) > RULE_THICKNESS
            && (max_x - min_x).min(max_y - min_y) <= RULE_THICKNESS
    })
}

fn shape_paragraph(index: usize, rect: &Rect) -> Block {
    let emu = |points: f64| Emu((points * EMU_PER_POINT).round() as i64);
    let extent = Extent {
        cx: emu(rect.width),
        cy: emu(rect.height),
    };
    let color = |rgb: Rgb| {
        let [r, g, b] = rgb.to_rgb8();
        ShapeColor {
            value: Some(Color::new(format!("{r:02X}{g:02X}{b:02X}"))),
            theme: None,
        }
    };
    let fill = rect.fill.map(|rgb| ShapeFill::Solid { color: color(rgb) });
    let stroke = rect.stroke.map(|rgb| ShapeStroke {
        color: Some(color(rgb)),
        width: Some(emu(rect.line_width.max(0.25))),
        ..ShapeStroke::default()
    });
    let name: Arc<str> = Arc::from(format!("vector-{index}"));
    let shape = Shape {
        name: Some(Arc::clone(&name)),
        descr: None,
        nv_id: None,
        bw_mode: None,
        tx_box: None,
        geometry: ShapeGeometry::Preset(Arc::from("rect")),
        xfrm: None,
        offset: Some((Emu(0), Emu(0))),
        extent: Some(extent),
        fill,
        stroke,
        text: None,
        style: None,
        location: SourceLocation::unknown(),
    };
    let anchor = AnchorDrawing {
        extent: Some(extent),
        effect_extent: None,
        doc_pr: Some(DocPr {
            id: None,
            name: Some(name),
            descr: None,
            title: None,
        }),
        simple_pos: false,
        position_h: Some(Position {
            relative_from: Some(Arc::from("page")),
            align: None,
            offset: Some(emu(rect.x)),
            percent_offset: None,
        }),
        position_v: Some(Position {
            relative_from: Some(Arc::from("page")),
            align: None,
            offset: Some(emu(rect.y)),
            percent_offset: None,
        }),
        wrap: Some(Wrap {
            kind: WrapKind::None,
            wrap_text: None,
            dist_left: None,
            dist_right: None,
            dist_top: None,
            dist_bottom: None,
            polygon: Vec::new(),
            polygon_edited: None,
        }),
        behind_doc: false,
        relative_height: Some(1),
        dist_top: None,
        dist_bottom: None,
        dist_left: None,
        dist_right: None,
        allow_overlap: true,
        layout_in_cell: true,
        locked: false,
        graphic_uri: None,
        graphic: Box::new(Graphic::Shape(shape)),
        size_rel_h: None,
        size_rel_v: None,
        location: SourceLocation::unknown(),
    };
    Block::Paragraph(Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Drawing(Drawing {
            kind: DrawingKind::Anchor(anchor),
            location: SourceLocation::unknown(),
        })],
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: SourceLocation::unknown(),
    })
}
