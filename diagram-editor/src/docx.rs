//! Adapter uses the public model and shared package writer, without patching OOXML.
#![allow(clippy::many_single_char_names)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]
use crate::{Connection, Dash, Diagram, Group, Node, NodeStyle, Point, Port, ShapeKind, TextAlign};
use std::sync::Arc;
use strict_ooxml::{model as w, StrictDocument, WriteOptions};
use strict_ooxml_core::error::SourceLocation;

/// A standard editable drawing package, with explicit interoperability limits.
pub struct DocxExport {
    /// Complete Strict DOCX package.
    pub bytes: Vec<u8>,
    /// Named conversion limitations; in-memory/JSON connector bindings remain intact.
    pub issues: Vec<String>,
}
/// Imported diagram. Unsupported input returns an error instead of replacing a scene.
pub struct DocxImport {
    /// Editable diagram model.
    pub diagram: Diagram,
    /// Reconstruction and fidelity notes.
    pub issues: Vec<String>,
}
fn loc() -> SourceLocation {
    SourceLocation::unknown()
}
fn emu(px: f32) -> w::Emu {
    w::Emu((f64::from(px) * 9525.0).round() as i64)
}
fn px(value: w::Emu) -> f32 {
    (value.0 as f64 / 9525.0) as f32
}
fn extent(size: Point) -> w::Extent {
    w::Extent {
        cx: emu(size.x),
        cy: emu(size.y),
    }
}
fn rgb(value: [u8; 3]) -> w::drawing::ShapeColor {
    w::drawing::ShapeColor {
        value: Some(w::Color::new(format!(
            "{:02X}{:02X}{:02X}",
            value[0], value[1], value[2]
        ))),
        ..Default::default()
    }
}
fn dash_name(d: Dash) -> Option<Arc<str>> {
    match d {
        Dash::Solid => None,
        Dash::Dashed => Some("dash".into()),
        Dash::Dotted => Some("dot".into()),
    }
}
fn preset(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Rectangle | ShapeKind::Text => "rect",
        ShapeKind::Rounded => "roundRect",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Diamond => "diamond",
        ShapeKind::Triangle => "triangle",
        ShapeKind::Trapezoid => "trapezoid",
        ShapeKind::Hexagon => "hexagon",
        ShapeKind::Document => "flowChartDocument",
        ShapeKind::RightArrow => "rightArrow",
    }
}
fn paragraph(text: &str, node: Option<&Node>) -> w::Paragraph {
    let mut props = w::props::ParagraphProperties::default();
    let mut run = w::props::RunProperties::default();
    if let Some(n) = node {
        props.alignment = Some(match n.style.align {
            TextAlign::Left => w::Justification::Start,
            TextAlign::Center => w::Justification::Center,
            TextAlign::Right => w::Justification::End,
        });
        run.size = Some(w::HalfPoints((n.font_size * 1.5).round() as i32));
        run.bold = if n.style.bold {
            w::TriState::On
        } else {
            w::TriState::Off
        };
        run.italic = if n.style.italic {
            w::TriState::On
        } else {
            w::TriState::Off
        };
        run.fonts = Some(w::Fonts {
            ascii: Some("Calibri".into()),
            h_ansi: Some("Calibri".into()),
            ..Default::default()
        });
    }
    w::Paragraph {
        props,
        inlines: vec![w::Inline::Run(w::Run {
            props: run,
            content: vec![w::RunContent::Text(w::TextNode {
                text: text.into(),
                space: w::Space::Preserve,
            })],
            revision: None,
            location: loc(),
        })],
        rsids: w::Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: loc(),
    }
}
fn base_shape(id: u64, position: Point, size: Point) -> w::drawing::Shape {
    w::drawing::Shape {
        name: Some(format!("Shape {id}").into()),
        descr: None,
        nv_id: Some(id as u32),
        bw_mode: None,
        tx_box: None,
        sp_locks: None,
        connector: false,
        effects: None,
        geometry: w::drawing::ShapeGeometry::None,
        xfrm: None,
        offset: Some((emu(position.x), emu(position.y))),
        extent: Some(extent(size)),
        fill: None,
        stroke: None,
        text: None,
        style: None,
        location: loc(),
    }
}
fn node_graphic(n: &Node) -> w::drawing::Graphic {
    let mut s = base_shape(n.id, n.position, n.size);
    s.name = Some(if n.kind == ShapeKind::Text {
        "Text".into()
    } else {
        format!("Shape {}", n.id).into()
    });
    s.geometry = w::drawing::ShapeGeometry::Preset(preset(n.kind).into());
    s.tx_box = Some(n.kind == ShapeKind::Text);
    if n.style.rotation.abs() > 0.001 {
        s.xfrm = Some(w::drawing::Xfrm {
            offset: s.offset,
            rot: Some((f64::from(n.style.rotation) * 60000.0).round() as i32),
            flip_h: false,
            flip_v: false,
        });
    }
    s.fill = Some(if n.style.no_fill || n.kind == ShapeKind::Text {
        w::drawing::ShapeFill::None
    } else {
        w::drawing::ShapeFill::Solid { color: rgb(n.fill) }
    });
    s.stroke = Some(w::drawing::ShapeStroke {
        none: n.style.no_stroke || n.kind == ShapeKind::Text,
        color: Some(rgb(n.stroke)),
        width: Some(emu(n.style.width)),
        dash: dash_name(n.style.dash),
        ..Default::default()
    });
    let body = w::drawing::TextBoxBody {
        anchor: Some(w::drawing::TextAnchor::Center),
        left_inset: Some(emu(n.style.padding)),
        right_inset: Some(emu(n.style.padding)),
        top_inset: Some(emu(n.style.padding)),
        bottom_inset: Some(emu(n.style.padding)),
        wrap: Some(if n.style.wrap { "square" } else { "none" }.into()),
        ..Default::default()
    };
    s.text = Some(w::drawing::TextBox {
        body: Some(body),
        blocks: n
            .text
            .split('\n')
            .map(|t| w::Block::Paragraph(paragraph(t, Some(n))))
            .collect(),
        location: loc(),
    });
    w::drawing::Graphic::Shape(s)
}
fn group_graphic(
    id: u64,
    children: Vec<w::drawing::Graphic>,
    min: Point,
    size: Point,
) -> w::drawing::Graphic {
    w::drawing::Graphic::Group(w::drawing::GroupShape {
        nv_id: Some(id as u32),
        name: Some(format!("Group {id}").into()),
        descr: None,
        xfrm: Some(w::drawing::GroupTransform {
            offset: Some((emu(min.x), emu(min.y))),
            extent: Some(extent(size)),
            child_offset: Some((emu(min.x), emu(min.y))),
            child_extent: Some(extent(size)),
            ..Default::default()
        }),
        locks: None,
        bw_mode: None,
        children,
        location: loc(),
    })
}
fn bounds<'a>(nodes: impl Iterator<Item = &'a Node>) -> (Point, Point) {
    let mut min = Point::new(0.0, 0.0);
    let mut max = Point::new(1.0, 1.0);
    for n in nodes {
        let (a, b) = n.visual_bounds();
        min.x = min.x.min(a.x);
        min.y = min.y.min(a.y);
        max.x = max.x.max(b.x);
        max.y = max.y.max(b.y);
    }
    (min, Point::new(max.x - min.x, max.y - min.y))
}
impl Diagram {
    /// Exports standard OOXML shapes and groups through `StrictLib`'s shared writer.
    /// Connector endpoint bindings cannot be represented by the current library model;
    /// the returned report names this limitation. No private XML is injected.
    /// # Errors
    /// Rejects invalid scenes, oversized canvases or package failures.
    pub fn to_docx(&self) -> Result<DocxExport, String> {
        self.validate()?;
        let mut children = vec![];
        for c in &self.connections {
            let route = self.route(c);
            let mut min = route[0];
            let mut max = min;
            for p in &route {
                min.x = min.x.min(p.x);
                min.y = min.y.min(p.y);
                max.x = max.x.max(p.x);
                max.y = max.y.max(p.y);
            }
            let size = Point::new((max.x - min.x).max(1.0), (max.y - min.y).max(1.0));
            let commands = route
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let (x, y) = (emu(p.x - min.x).0, emu(p.y - min.y).0);
                    if i == 0 {
                        w::drawing::PathCommand::MoveTo { x, y }
                    } else {
                        w::drawing::PathCommand::LineTo { x, y }
                    }
                })
                .collect();
            let mut s = base_shape(c.id, min, size);
            s.connector = true;
            s.geometry = w::drawing::ShapeGeometry::Custom(w::drawing::CustomGeometry {
                paths: vec![w::drawing::GeometryPath {
                    width: emu(size.x).0,
                    height: emu(size.y).0,
                    fill: Some("none".into()),
                    stroke: Some(true),
                    commands,
                }],
                ..Default::default()
            });
            s.fill = Some(w::drawing::ShapeFill::None);
            s.stroke = Some(w::drawing::ShapeStroke {
                color: Some(rgb(c.color)),
                width: Some(emu(c.width)),
                dash: dash_name(c.dash),
                head_end: c.start_arrow.then(|| "triangle".into()),
                tail_end: c.end_arrow.then(|| "triangle".into()),
                ..Default::default()
            });
            children.push(w::drawing::Graphic::Shape(s));
        }
        let mut written = std::collections::HashSet::new();
        for n in &self.nodes {
            if !written.insert(n.id) {
                continue;
            }
            if let Some(g) = self.groups.iter().find(|g| g.members.contains(&n.id)) {
                let members: Vec<_> = self
                    .nodes
                    .iter()
                    .filter(|n| g.members.contains(&n.id))
                    .collect();
                let (min, size) = bounds(members.iter().copied());
                let group = group_graphic(
                    g.id,
                    members.iter().map(|n| node_graphic(n)).collect(),
                    min,
                    size,
                );
                children.push(group);
                written.extend(&g.members);
            } else {
                children.push(node_graphic(n));
            }
        }
        let (mut min, mut size) = bounds(self.nodes.iter());
        let mut max = Point::new(min.x + size.x, min.y + size.y);
        for c in &self.connections {
            for p in self.route(c) {
                min.x = min.x.min(p.x - 12.0);
                min.y = min.y.min(p.y - 12.0);
                max.x = max.x.max(p.x + 12.0);
                max.y = max.y.max(p.y + 12.0);
            }
        }
        size = Point::new(max.x - min.x, max.y - min.y);
        size.x += 40.0;
        size.y += 40.0;
        if size.x > 1800.0 || size.y > 2400.0 {
            return Err("Diagram is too large for DOCX: reduce it to 1800 × 2400 pixels".into());
        }
        let root_id = self
            .nodes
            .iter()
            .map(|n| n.id)
            .chain(self.connections.iter().map(|c| c.id))
            .chain(self.groups.iter().map(|g| g.id))
            .max()
            .unwrap_or(0)
            + 1;
        let mut graphic = group_graphic(root_id, children, min, size);
        if let w::drawing::Graphic::Group(g) = &mut graphic {
            g.name = Some("Diagram".into());
            if let Some(xfrm) = &mut g.xfrm {
                xfrm.offset = Some((w::Emu(0), w::Emu(0)));
            }
        }
        let drawing = w::Drawing {
            kind: w::DrawingKind::Inline(w::drawing::InlineDrawing {
                extent: Some(extent(size)),
                effect_extent: None,
                doc_pr: Some(w::drawing::DocPr {
                    id: Some(root_id as u32),
                    name: Some("Diagram".into()),
                    ..Default::default()
                }),
                dist_top: None,
                dist_bottom: None,
                dist_left: None,
                dist_right: None,
                graphic_uri: None,
                graphic: Box::new(graphic),
                location: loc(),
            }),
            location: loc(),
        };
        let page = format!(
            "{{\\rtf1\\ansi\\paperw{}\\paperh{}\\margl300\\margr300\\margt300\\margb300\\par}}",
            (size.x * 15.0 + 600.0).ceil() as i32,
            (size.y * 15.0 + 600.0).ceil() as i32
        );
        let seed =
            StrictDocument::import_rtf(page.as_bytes(), &strict_ooxml::RtfOptions::default())
                .map_err(|e| e.to_string())?;
        let mut document = seed.document.document().clone();
        let mut p = paragraph("", None);
        p.inlines = vec![w::Inline::Drawing(drawing)];
        document.body.blocks = vec![w::Block::Paragraph(p)];
        let output = strict_ooxml::write_package(&document, None, &WriteOptions::default())
            .map_err(|e| e.to_string())?;
        output.report.verify_no_silent_loss()?;
        if strict_ooxml::dropped_count(&output.report) > 0 {
            return Err(format!("DOCX writer rejected content: {}", output.report));
        }
        let mut issues = vec![];
        if !self.connections.is_empty() {
            issues.push("DOCX: connectors are editable lines without endpoint bindings; JSON preserves exact connections".into());
        }
        for n in &self.nodes {
            if n.style.no_fill || n.style.no_stroke || n.kind == ShapeKind::Text {
                issues.push(format!("Shape {}: inactive fill/outline colors are not stored in DOCX; JSON preserves them",n.id));
            }
            if ((n.font_size * 1.5).round() / 1.5 - n.font_size).abs() > 0.001 {
                issues.push(format!(
                    "Shape {} font size was rounded to half a point",
                    n.id
                ));
            }
        }
        if self.connections.iter().any(|c| !self.route_status(c).1) {
            issues.push("Route could not avoid all obstacles".into());
        }
        Ok(DocxExport {
            bytes: output.bytes,
            issues,
        })
    }
    /// Imports the supported single-canvas DOCX subset, reconstructing bindings
    /// only when each line endpoint matches exactly one cardinal shape port.
    /// # Errors
    /// Rejects unsupported geometry/styles, ambiguous attachments and non-canvas input.
    pub fn from_docx(bytes: &[u8]) -> Result<DocxImport, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("DOCX input exceeds size limit".into());
        }
        let options = strict_ooxml::OpenOptions::default()
            .conformance(strict_ooxml::ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(strict_ooxml::TransitionalNormalizer::new()));
        let opened = StrictDocument::open_reader(bytes, &options).map_err(|e| e.to_string())?;
        let mut drawings = vec![];
        for b in &opened.document().body.blocks {
            match b {
                w::Block::Paragraph(p) => {
                    for i in &p.inlines {
                        match i {
                            w::Inline::Drawing(d) => drawings.push(d),
                            w::Inline::Run(r) => {
                                for c in &r.content {
                                    match c {
                                        w::RunContent::Drawing(d) => drawings.push(d),
                                        w::RunContent::Text(t) if t.text.trim().is_empty() => {}
                                        _ => {
                                            return Err(
                                                "DOCX contains text outside the diagram".into()
                                            )
                                        }
                                    }
                                }
                            }
                            _ => return Err("DOCX contains unsupported content".into()),
                        }
                    }
                }
                _ => return Err("Expected a DOCX containing one diagram".into()),
            }
        }
        if drawings.len() != 1 {
            return Err("Expected one DOCX drawing canvas".into());
        }
        let graphic = match &drawings[0].kind {
            w::DrawingKind::Inline(i) => i.graphic.as_ref(),
            _ => return Err("Importing an anchored canvas is not supported".into()),
        };
        let w::drawing::Graphic::Group(root) = graphic else {
            return Err("Expected a group of shapes".into());
        };
        if root
            .xfrm
            .is_some_and(|x| x.flip_h || x.flip_v || x.extent != x.child_extent)
        {
            return Err("A scaled or rotated root canvas is not supported".into());
        }
        let mut diagram = Self::default();
        let mut lines = vec![];
        for child in &root.children {
            import_graphic(child, &mut diagram, &mut lines, 0)?;
        }
        for s in lines {
            let w::drawing::ShapeGeometry::Custom(geometry) = &s.geometry else {
                return Err("Unsupported connector geometry".into());
            };
            let path = geometry.paths.first().ok_or("Empty connector")?;
            if geometry.paths.len() != 1 {
                return Err("Multiple connector paths are not supported".into());
            }
            let line_extent = s.extent.ok_or("Missing connector extent")?;
            if path.width != line_extent.cx.0 || path.height != line_extent.cy.0 {
                return Err("Scaled connector coordinates are not supported".into());
            }
            let offset = s
                .offset
                .or_else(|| s.xfrm.and_then(|x| x.offset))
                .ok_or("Missing connector position")?;
            let mut points = vec![];
            for command in &path.commands {
                match command {
                    w::drawing::PathCommand::MoveTo { x, y }
                    | w::drawing::PathCommand::LineTo { x, y } => points.push(Point::new(
                        px(offset.0) + px(w::Emu(*x)),
                        px(offset.1) + px(w::Emu(*y)),
                    )),
                    _ => return Err("Curved connectors are not supported".into()),
                }
            }
            let from = match_port(&diagram, *points.first().ok_or("Empty line")?)?;
            let to = match_port(&diagram, *points.last().ok_or("Empty line")?)?;
            let stroke = s.stroke.as_ref().ok_or("Missing connector style")?;
            diagram.connections.push(Connection {
                routing: crate::Routing::Simple,
                id: u64::from(s.nv_id.ok_or("Missing connector ID")?),
                from: from.0,
                to: to.0,
                from_port: from.1,
                to_port: to.1,
                color: read_color(stroke.color.as_ref())?,
                width: stroke.width.map_or(1.7, px),
                dash: read_dash(stroke.dash.as_deref())?,
                start_arrow: read_arrow(stroke.head_end.as_deref())?,
                end_arrow: read_arrow(stroke.tail_end.as_deref())?,
            });
            let same = |route: &[Point]| {
                route.len() == points.len()
                    && route
                        .iter()
                        .zip(&points)
                        .all(|(a, b)| (a.x - b.x).hypot(a.y - b.y) <= 0.05)
            };
            let mut route = diagram.route(
                diagram
                    .connections
                    .last()
                    .ok_or("Could not restore connector")?,
            );
            if !same(&route) {
                diagram.connections.last_mut().unwrap().routing = crate::Routing::AvoidObstacles;
                route = diagram.route(diagram.connections.last().unwrap());
            }
            if route.len() != points.len()
                || route
                    .iter()
                    .zip(&points)
                    .any(|(a, b)| (a.x - b.x).hypot(a.y - b.y) > 0.05)
            {
                return Err("Connector route was changed manually; open the original JSON".into());
            }
        }
        diagram.validate()?;
        let issues = if diagram.connections.is_empty() {
            vec![]
        } else {
            vec!["Connections were inferred from matching endpoints and shape ports; the current library model does not store DOCX bindings".into()]
        };
        Ok(DocxImport { diagram, issues })
    }
}
fn match_port(d: &Diagram, p: Point) -> Result<(u64, Port), String> {
    let mut found = vec![];
    for n in &d.nodes {
        for port in [Port::Left, Port::Right, Port::Top, Port::Bottom] {
            let q = n.port(port);
            if (q.x - p.x).hypot(q.y - p.y) < 0.05 {
                found.push((n.id, port));
            }
        }
    }
    if found.len() == 1 {
        Ok(found[0])
    } else {
        Err("Cannot infer an unambiguous connector binding; open the original JSON".into())
    }
}
fn read_arrow(v: Option<&str>) -> Result<bool, String> {
    match v {
        None | Some("none") => Ok(false),
        Some("triangle") => Ok(true),
        _ => Err("Unsupported arrowhead".into()),
    }
}
fn read_dash(v: Option<&str>) -> Result<Dash, String> {
    match v {
        None | Some("solid") => Ok(Dash::Solid),
        Some("dash") => Ok(Dash::Dashed),
        Some("dot") => Ok(Dash::Dotted),
        _ => Err("Unsupported dash".into()),
    }
}
fn read_color(c: Option<&w::drawing::ShapeColor>) -> Result<[u8; 3], String> {
    let s = c
        .and_then(|c| c.value.as_ref())
        .map(w::Color::as_str)
        .ok_or("Theme or missing colors are not supported")?;
    if s.len() != 6 || !s.is_ascii() || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Unsupported color".into());
    }
    Ok([
        u8::from_str_radix(&s[0..2], 16).map_err(|e| e.to_string())?,
        u8::from_str_radix(&s[2..4], 16).map_err(|e| e.to_string())?,
        u8::from_str_radix(&s[4..6], 16).map_err(|e| e.to_string())?,
    ])
}
fn import_graphic(
    graphic: &w::drawing::Graphic,
    d: &mut Diagram,
    lines: &mut Vec<w::drawing::Shape>,
    depth: usize,
) -> Result<(), String> {
    if depth > 1 {
        return Err("Nested groups are not supported".into());
    }
    match graphic {
        w::drawing::Graphic::Group(g) => {
            if let Some(x) = g.xfrm {
                if x.rot.unwrap_or(0) != 0
                    || x.flip_h
                    || x.flip_v
                    || x.extent != x.child_extent
                    || x.offset != x.child_offset
                {
                    return Err("Transformed groups are not supported".into());
                }
            }
            let before = d.nodes.len();
            for child in &g.children {
                import_graphic(child, d, lines, depth + 1)?;
            }
            d.groups.push(Group {
                id: u64::from(g.nv_id.ok_or("Missing group ID")?),
                members: d.nodes[before..].iter().map(|n| n.id).collect(),
            });
        }
        w::drawing::Graphic::Shape(s) if s.connector => lines.push(s.clone()),
        w::drawing::Graphic::Shape(s) => {
            if s.effects.is_some() || s.style.is_some() {
                return Err("Effects and theme style references are not supported".into());
            }
            if s.xfrm.is_some_and(|x| x.flip_h || x.flip_v) {
                return Err("Flipped shapes are not supported".into());
            }
            let w::drawing::ShapeGeometry::Preset(preset) = &s.geometry else {
                return Err("Custom shape paths are not supported".into());
            };
            let kind = if s.tx_box == Some(true) {
                ShapeKind::Text
            } else {
                match preset.as_ref() {
                    "rect" => ShapeKind::Rectangle,
                    "roundRect" => ShapeKind::Rounded,
                    "ellipse" => ShapeKind::Ellipse,
                    "diamond" => ShapeKind::Diamond,
                    "triangle" => ShapeKind::Triangle,
                    "trapezoid" => ShapeKind::Trapezoid,
                    "hexagon" => ShapeKind::Hexagon,
                    "flowChartDocument" => ShapeKind::Document,
                    "rightArrow" => ShapeKind::RightArrow,
                    _ => return Err(format!("Unsupported shape {preset}")),
                }
            };
            let offset = s
                .offset
                .or_else(|| s.xfrm.and_then(|x| x.offset))
                .ok_or("Missing shape position")?;
            let size = s.extent.ok_or("Missing shape extent")?;
            let mut style = NodeStyle {
                rotation: s
                    .xfrm
                    .and_then(|x| x.rot)
                    .map_or(0.0, |r| (r as f32 / 60000.0).rem_euclid(360.0)),
                ..NodeStyle::default()
            };
            let fill = match &s.fill {
                Some(w::drawing::ShapeFill::Solid { color }) => read_color(Some(color))?,
                Some(w::drawing::ShapeFill::None) => {
                    style.no_fill = kind != ShapeKind::Text;
                    [231, 240, 252]
                }
                _ => return Err("Unsupported fill".into()),
            };
            let stroke = if let Some(line) = &s.stroke {
                style.no_stroke = line.none && kind != ShapeKind::Text;
                style.width = line.width.map_or(1.7, px);
                style.dash = read_dash(line.dash.as_deref())?;
                if line.none {
                    [43, 87, 154]
                } else {
                    read_color(line.color.as_ref())?
                }
            } else {
                return Err("Missing outline style".into());
            };
            let mut text = vec![];
            let mut font_size = 16.0;
            if let Some(t) = &s.text {
                if let Some(body) = &t.body {
                    style.padding = body.left_inset.map_or(8.0, px);
                    if body
                        .anchor
                        .is_some_and(|v| v != w::drawing::TextAnchor::Center)
                        || [body.right_inset, body.top_inset, body.bottom_inset]
                            .into_iter()
                            .flatten()
                            .any(|v| (px(v) - style.padding).abs() > 0.001)
                    {
                        return Err(
                            "Unequal padding or vertical text alignment is not supported".into(),
                        );
                    }
                    style.wrap = body.wrap.as_deref() != Some("none");
                }
                let mut formatting = None;
                for block in &t.blocks {
                    let w::Block::Paragraph(p) = block else {
                        return Err("Tables inside shapes are not supported".into());
                    };
                    style.align = match p.props.alignment {
                        Some(w::Justification::Start) | None => TextAlign::Left,
                        Some(w::Justification::Center) => TextAlign::Center,
                        Some(w::Justification::End) => TextAlign::Right,
                        _ => return Err("Unsupported text alignment".into()),
                    };
                    let mut paragraph_props = p.props.clone();
                    paragraph_props.alignment = None;
                    paragraph_props.location = None;
                    if paragraph_props != w::props::ParagraphProperties::default() {
                        return Err("Additional paragraph formatting is not supported".into());
                    }
                    let mut value = String::new();
                    for i in &p.inlines {
                        let w::Inline::Run(r) = i else {
                            return Err("Complex shape text is not supported".into());
                        };
                        let signature = (
                            r.props.size,
                            r.props.bold,
                            r.props.italic,
                            p.props.alignment,
                        );
                        let mut extra = r.props.clone();
                        extra.size = None;
                        extra.bold = w::TriState::Absent;
                        extra.italic = w::TriState::Absent;
                        extra.fonts = None;
                        extra.location = None;
                        if extra != w::props::RunProperties::default() {
                            return Err("Additional text formatting is not supported".into());
                        }
                        if r.props.fonts.as_ref().is_some_and(|f| {
                            [f.ascii.as_deref(), f.h_ansi.as_deref()]
                                .into_iter()
                                .flatten()
                                .any(|name| !name.eq_ignore_ascii_case("Calibri"))
                        }) {
                            return Err("Other fonts are not supported".into());
                        }
                        if formatting.is_some_and(|previous| previous != signature) {
                            return Err("Mixed text formatting is not supported".into());
                        }
                        formatting = Some(signature);
                        font_size = r.props.size.map_or(16.0, |h| h.0 as f32 / 1.5);
                        style.bold = r.props.bold == w::TriState::On;
                        style.italic = r.props.italic == w::TriState::On;
                        for c in &r.content {
                            match c {
                                w::RunContent::Text(t) => value.push_str(&t.text),
                                w::RunContent::Break(_) => value.push('\n'),
                                w::RunContent::Tab => value.push('\t'),
                                _ => return Err("Complex shape text is not supported".into()),
                            }
                        }
                    }
                    text.push(value);
                }
            }
            d.nodes.push(Node {
                id: u64::from(s.nv_id.ok_or("Missing shape ID")?),
                kind,
                position: Point::new(px(offset.0), px(offset.1)),
                size: Point::new(px(size.cx), px(size.cy)),
                text: text.join("\n"),
                fill,
                stroke,
                font_size,
                style,
            });
        }
        _ => return Err("Unsupported graphic object".into()),
    }
    Ok(())
}
