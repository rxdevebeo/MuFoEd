//! PDF paths that the table pass does not consume.
//!
//! An axis-aligned rectangle becomes a page-anchored DrawingML rectangle; any
//! other path - a logo's curves, a diagram's arrows and circles, a rule under
//! a heading - a page-anchored custom geometry of its flattened outline. Thin
//! rules are left to the table pass on a page that has tables. A path that is
//! neither kept nor named would be a silent loss.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_pdf::content::{Item, LineCap, LineJoin, Rgb, Vector};
use strict_ooxml_pdf::PdfPage;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, CustomGeometry, DocPr, Drawing, DrawingKind, Extent, GeometryPath, Graphic,
    PathCommand, Position, Shape, ShapeColor, ShapeFill, ShapeGeometry, ShapeStroke, Wrap,
    WrapKind,
};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::props::ParagraphProperties;
use strict_ooxml_wml::model::values::{Color, Emu, Rsids};

use crate::report::{ConversionReport, Severity};

const EMU_PER_POINT: f64 = 12_700.0;
/// A path thinner than this is a ruling line, already handled by the table pass.
const RULE_THICKNESS: f64 = 2.0;
/// The most points one path keeps as a shape; a path with more is reported.
const MAX_PATH_POINTS: usize = 20_000;

/// Page-anchored shapes for the vectors this page did not turn into table rules.
pub(crate) fn blocks_for(page: &PdfPage, report: &mut ConversionReport) -> Vec<Block> {
    placed_blocks_for(page, report, true)
        .into_iter()
        .map(|(_, block)| block)
        .collect()
}

/// [`blocks_for`], each block with the top of its shape in points from the
/// page's top (for placing it in the reading order). `rules`: whether thin
/// rules are drawn as well, which a page whose table pass found tables
/// leaves to its tables' borders.
pub(crate) fn placed_blocks_for(
    page: &PdfPage,
    report: &mut ConversionReport,
    rules: bool,
) -> Vec<(f64, Block)> {
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
            Some(rect) => blocks.push((
                rect.y,
                shape_paragraph(index, &rect, ShapeGeometry::Preset(Arc::from("rect"))),
            )),
            None if !rules && is_rule(vector) => {}
            None => match outline(vector, page.geometry.height) {
                Some((rect, geometry)) => blocks.push((
                    rect.y,
                    shape_paragraph(index, &rect, ShapeGeometry::Custom(geometry)),
                )),
                None => report.record(
                    "vector.unsupported",
                    Severity::Unsupported,
                    format!("page {} vector {index}", page.number),
                ),
            },
        }
    }
    blocks
}

struct Rect {
    /// An axial shading's linear gradient, in place of the solid fill.
    gradient: Option<ShapeFill>,
    /// The fill's opacity (1 = opaque).
    fill_alpha: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: Option<Rgb>,
    stroke: Option<Rgb>,
    line_width: f64,
    line: LineStyle,
}

/// How a stroke is drawn beyond its colour and width (G5, G6 of
/// `PDF-VECTOR-GRAPHICS-2026-10-19.md`): the reader had all of it and the
/// shape got none, so a dashed rule came out solid and a 20 % hairline frame
/// opaque.
struct LineStyle {
    /// `CA`: the stroke's opacity.
    alpha: f64,
    cap: LineCap,
    join: LineJoin,
    /// The dash array in points; empty for a solid line.
    dash: Vec<f64>,
}

impl LineStyle {
    fn of(vector: &Vector) -> Self {
        Self {
            alpha: vector.stroke_alpha,
            cap: vector.line_cap,
            join: vector.line_join,
            dash: vector.dash.clone(),
        }
    }

    /// `a:ln/@cap`, always written: DrawingML's default is a square cap and
    /// PDF's a butt one.
    fn cap(&self) -> &'static str {
        match self.cap {
            LineCap::Butt => "flat",
            LineCap::Round => "rnd",
            LineCap::Square => "sq",
        }
    }

    /// The join element. A miter's limit is left at DrawingML's default: PDF
    /// states a ratio and DrawingML a percentage, and the two only meet at
    /// the corners nobody looks at.
    fn join(&self) -> &'static str {
        match self.join {
            LineJoin::Miter => "<a:miter/>",
            LineJoin::Round => "<a:round/>",
            LineJoin::Bevel => "<a:bevel/>",
        }
    }
}

/// The DrawingML preset dash nearest a PDF dash array, measured in line
/// widths as the presets are. DrawingML has a custom dash too, but this model
/// carries the preset only, and a near preset is a dashed line where none was.
fn preset_dash(dash: &[f64], width: f64) -> Option<&'static str> {
    if dash.is_empty() || dash.iter().all(|value| *value <= 0.0) {
        return None;
    }
    // An odd array repeats to an even one (ISO 32000-1 §8.4.3.6).
    let pattern: Vec<f64> = if dash.len() % 2 == 1 {
        dash.iter().chain(dash.iter()).copied().collect()
    } else {
        dash.to_vec()
    };
    let width = width.max(0.25);
    let units: Vec<f64> = pattern.iter().map(|value| value / width).collect();
    let candidates: &[(&'static str, &[f64])] = match units.len() {
        2 => &[
            ("sysDot", &[1.0, 1.0]),
            ("sysDash", &[3.0, 1.0]),
            ("dot", &[1.0, 3.0]),
            ("dash", &[4.0, 3.0]),
            ("lgDash", &[8.0, 3.0]),
        ],
        4 => &[
            ("sysDashDot", &[3.0, 1.0, 1.0, 1.0]),
            ("dashDot", &[4.0, 3.0, 1.0, 3.0]),
            ("lgDashDot", &[8.0, 3.0, 1.0, 3.0]),
        ],
        _ => &[
            ("sysDashDotDot", &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
            ("lgDashDotDot", &[8.0, 3.0, 1.0, 3.0, 1.0, 3.0]),
        ],
    };
    // Distance on a log scale, so 2:1 and 1:2 are equally far from 1:1; a
    // zero-length dash (a dot drawn by a round cap) counts as one width.
    let distance = |preset: &[f64]| -> f64 {
        units
            .iter()
            .zip(preset.iter().cycle())
            .map(|(have, want)| (have.max(0.5) / want).ln().abs())
            .sum()
    };
    candidates
        .iter()
        .map(|(name, preset)| (*name, distance(preset)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(name, _)| name)
}

fn rectangle(vector: &Vector, page_height: f64) -> Option<Rect> {
    let [subpath] = vector.subpaths.as_slice() else {
        return None;
    };
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
        gradient: gradient_fill(vector),
        fill_alpha: vector.fill_alpha,
        fill: vector.fill,
        stroke: vector.stroke,
        line_width: vector.line_width,
        line: LineStyle::of(vector),
    })
}

/// A path's flattened outline as a custom geometry, with its box on the page.
fn outline(vector: &Vector, page_height: f64) -> Option<(Rect, CustomGeometry)> {
    let total: usize = vector
        .subpaths
        .iter()
        .map(|subpath| subpath.points.len())
        .sum();
    if !(2..=MAX_PATH_POINTS).contains(&total) {
        return None;
    }
    let mut placed: Vec<(Vec<(f64, f64)>, bool)> = Vec::with_capacity(vector.subpaths.len());
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for subpath in &vector.subpaths {
        let mut points = Vec::with_capacity(subpath.points.len());
        for (x, y) in &subpath.points {
            let (x, y) = vector.ctm.apply(*x, *y);
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            points.push((x, y));
        }
        placed.push((points, subpath.closed));
    }
    if !(min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite()) {
        return None;
    }
    let emu = |points: f64| (points * EMU_PER_POINT).round() as i64;
    let mut commands = Vec::with_capacity(total.saturating_add(placed.len()));
    for (points, closed) in &placed {
        let mut iter = points.iter();
        let Some(&(x, y)) = iter.next() else {
            continue;
        };
        commands.push(PathCommand::MoveTo {
            x: emu(x - min_x),
            y: emu(max_y - y),
        });
        for &(x, y) in iter {
            commands.push(PathCommand::LineTo {
                x: emu(x - min_x),
                y: emu(max_y - y),
            });
        }
        if *closed {
            commands.push(PathCommand::Close);
        }
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    let geometry = CustomGeometry {
        paths: vec![GeometryPath {
            width: emu(width).max(1),
            height: emu(height).max(1),
            fill: (vector.fill.is_none() && vector.gradient.is_none()).then(|| Arc::from("none")),
            stroke: Some(vector.stroke.is_some()),
            commands,
        }],
        ..CustomGeometry::default()
    };
    Some((
        Rect {
            x: min_x,
            y: page_height - max_y,
            width,
            height,
            gradient: gradient_fill(vector),
            fill_alpha: vector.fill_alpha,
            fill: vector.fill,
            stroke: vector.stroke,
            line_width: vector.line_width,
            line: LineStyle::of(vector),
        },
        geometry,
    ))
}

/// An axial shading as a DrawingML linear gradient: the same stops, along
/// the axis's direction on the page (DrawingML measures the angle clockwise
/// from the x axis, y down, in 60 000ths of a degree).
fn gradient_fill(vector: &Vector) -> Option<ShapeFill> {
    let gradient = vector.gradient.as_ref()?;
    let (sx, sy) = vector.ctm.apply(gradient.start.0, gradient.start.1);
    let (ex, ey) = vector.ctm.apply(gradient.end.0, gradient.end.1);
    let degrees = (sy - ey).atan2(ex - sx).to_degrees();
    let degrees = if degrees.is_finite() {
        degrees.rem_euclid(360.0)
    } else {
        0.0
    };
    let stops = gradient
        .stops
        .iter()
        .map(|(offset, rgb)| {
            let [r, g, b] = rgb.to_rgb8();
            strict_ooxml_wml::model::drawing::GradientStop {
                position: (offset.clamp(0.0, 1.0) * 100_000.0).round() as i32,
                color: ShapeColor {
                    value: Some(Color::new(format!("{r:02X}{g:02X}{b:02X}"))),
                    theme: None,
                    markup: None,
                },
            }
        })
        .collect::<Vec<_>>();
    (stops.len() >= 2).then(|| ShapeFill::Gradient {
        stops,
        angle: Some((degrees * 60_000.0).round() as i32),
        scaled: Some(false),
        rotate_with_shape: Some(true),
        path_shading: None,
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
    covers && white && rect.stroke.is_none() && rect.gradient.is_none()
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

/// A colour with its opacity as `a:alpha`, in Strict's percentage form.
fn alpha_markup(rgb: Rgb, alpha: f64) -> Arc<str> {
    let [r, g, b] = rgb.to_rgb8();
    let percent = (alpha.clamp(0.0, 1.0) * 100_000.0).round() / 1000.0;
    Arc::from(format!(
        "<a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"><a:alpha val=\"{percent}%\"/></a:srgbClr>"
    ))
}

fn shape_paragraph(index: usize, rect: &Rect, geometry: ShapeGeometry) -> Block {
    let emu = |points: f64| Emu((points * EMU_PER_POINT).round() as i64);
    // A rule is a line: its box is a point wide or tall, never nothing.
    let extent = Extent {
        cx: emu(rect.width.max(0.1)),
        cy: emu(rect.height.max(0.1)),
    };
    let color = |rgb: Rgb| {
        let [r, g, b] = rgb.to_rgb8();
        ShapeColor {
            value: Some(Color::new(format!("{r:02X}{g:02X}{b:02X}"))),
            theme: None,
            markup: None,
        }
    };
    // A translucent fill (the graphics state's `ca`) keeps its opacity as
    // the colour's `a:alpha`, in Strict's percentage form.
    let fill = rect.gradient.clone().or_else(|| {
        rect.fill.map(|rgb| {
            let mut fill_color = color(rgb);
            if rect.fill_alpha < 0.999 {
                fill_color.markup = Some(alpha_markup(rgb, rect.fill_alpha));
            }
            ShapeFill::Solid { color: fill_color }
        })
    });
    let stroke = rect.stroke.map(|rgb| {
        let mut line_color = color(rgb);
        if rect.line.alpha < 0.999 {
            line_color.markup = Some(alpha_markup(rgb, rect.line.alpha));
        }
        ShapeStroke {
            color: Some(line_color),
            width: Some(emu(rect.line_width.max(0.25))),
            dash: preset_dash(&rect.line.dash, rect.line_width).map(Arc::from),
            attributes: vec![(Arc::from("cap"), Arc::from(rect.line.cap()))],
            join: Some(Arc::from(rect.line.join())),
            ..ShapeStroke::default()
        }
    });
    let name: Arc<str> = Arc::from(format!("vector-{index}"));
    let shape = Shape {
        name: Some(Arc::clone(&name)),
        descr: None,
        nv_id: None,
        bw_mode: None,
        tx_box: None,
        sp_locks: None,
        connector: false,
        effects: None,
        geometry,
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
            hidden: None,
            id: None,
            name: Some(name),
            descr: None,
            title: None,
        }),
        simple_pos: false,
        simple_pos_point: None,
        hidden: None,
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
        // A filled box is painted before the text over it, and among the
        // page's pictures and shapes where the page painted it.
        behind_doc: true,
        relative_height: Some(crate::semantic::paint_height(index)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use strict_ooxml_pdf::content::{Matrix, SubPath};

    fn stroked(dash: Vec<f64>, alpha: f64, cap: LineCap, join: LineJoin) -> Vector {
        Vector {
            subpaths: vec![SubPath {
                points: vec![(10.0, 10.0), (110.0, 10.0), (110.0, 60.0), (10.0, 60.0)],
                closed: true,
            }],
            fill: None,
            gradient: None,
            fill_alpha: 1.0,
            stroke_alpha: alpha,
            stroke: Some(Rgb(1.0, 0.0, 0.0)),
            line_width: 1.0,
            line_cap: cap,
            line_join: join,
            miter_limit: 10.0,
            even_odd: false,
            ctm: Matrix::IDENTITY,
            dash,
        }
    }

    fn line_of(vector: &Vector) -> ShapeStroke {
        let rect = rectangle(vector, 800.0).expect("a rectangle");
        let Block::Paragraph(paragraph) =
            shape_paragraph(0, &rect, ShapeGeometry::Preset(Arc::from("rect")))
        else {
            panic!("a paragraph");
        };
        let Some(Inline::Drawing(Drawing {
            kind: DrawingKind::Anchor(anchor),
            ..
        })) = paragraph.inlines.into_iter().next()
        else {
            panic!("an anchored drawing");
        };
        let Graphic::Shape(shape) = *anchor.graphic else {
            panic!("a shape");
        };
        shape.stroke.expect("a stroke")
    }

    #[test]
    fn a_dash_array_becomes_the_nearest_preset() {
        assert_eq!(preset_dash(&[], 1.0), None);
        assert_eq!(preset_dash(&[4.0, 3.0], 1.0), Some("dash"));
        assert_eq!(preset_dash(&[1.0, 1.0], 1.0), Some("sysDot"));
        // Scaled with the line: a 2 pt line's 8/6 is still "dash".
        assert_eq!(preset_dash(&[8.0, 6.0], 2.0), Some("dash"));
        // A dot drawn by a round cap: no dash length at all.
        assert_eq!(preset_dash(&[0.0, 3.0], 1.0), Some("dot"));
        assert_eq!(preset_dash(&[4.0, 3.0, 1.0, 3.0], 1.0), Some("dashDot"));
        // An odd array repeats.
        assert_eq!(preset_dash(&[3.0], 1.0), preset_dash(&[3.0, 3.0], 1.0));
    }

    #[test]
    fn a_stroke_keeps_its_opacity_cap_join_and_dash() {
        let line = line_of(&stroked(
            vec![4.0, 3.0],
            0.2,
            LineCap::Round,
            LineJoin::Bevel,
        ));
        assert_eq!(line.dash.as_deref(), Some("dash"));
        assert_eq!(line.join.as_deref(), Some("<a:bevel/>"));
        assert_eq!(line.attributes, vec![(Arc::from("cap"), Arc::from("rnd"))]);
        let markup = line.color.and_then(|color| color.markup).expect("an alpha");
        assert!(markup.contains("<a:alpha val=\"20%\"/>"), "{markup}");
    }

    #[test]
    fn an_opaque_solid_stroke_has_no_alpha_and_no_dash() {
        let line = line_of(&stroked(Vec::new(), 1.0, LineCap::Butt, LineJoin::Miter));
        assert_eq!(line.dash, None);
        assert_eq!(line.attributes, vec![(Arc::from("cap"), Arc::from("flat"))]);
        assert_eq!(line.color.and_then(|color| color.markup), None);
    }
}
