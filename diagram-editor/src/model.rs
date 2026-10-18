#![allow(clippy::many_single_char_names, clippy::struct_excessive_bools)]
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
pub(crate) fn rotate(p: Point, center: Point, degrees: f32) -> Point {
    let (s, c) = degrees.to_radians().sin_cos();
    let (x, y) = (p.x - center.x, p.y - center.y);
    Point::new(center.x + c * x - s * y, center.y + s * x + c * y)
}

/// Logical pixel coordinates, independent of canvas zoom and pan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}
impl Point {
    /// Constructs a point.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Supported MVP shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    /// Plain rectangle.
    Rectangle,
    /// Rounded rectangle.
    Rounded,
    /// Ellipse inside the frame.
    Ellipse,
    /// Four-point decision diamond.
    Diamond,
    /// Text without an outline or fill.
    Text,
    /// Triangle.
    Triangle,
    /// Trapezoid.
    Trapezoid,
    /// Hexagon.
    Hexagon,
    /// Flowchart document with a wavy bottom.
    Document,
    /// Solid right-pointing arrow.
    RightArrow,
}

/// Horizontal label alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    /// Left aligned.
    Left,
    /// Centered.
    #[default]
    Center,
    /// Right aligned.
    Right,
}
/// Outline dash preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dash {
    /// Continuous line.
    #[default]
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
}
/// Label and outline properties introduced after the initial MVP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodeStyle {
    /// Clockwise rotation about the node center, in degrees.
    pub rotation: f32,
    /// No fill behind the label.
    pub no_fill: bool,
    /// No outline.
    pub no_stroke: bool,
    /// Outline width in logical pixels.
    pub width: f32,
    /// Outline dash.
    pub dash: Dash,
    /// Whole-label bold formatting.
    pub bold: bool,
    /// Whole-label italic formatting.
    pub italic: bool,
    /// Horizontal alignment.
    pub align: TextAlign,
    /// Internal margins, in logical pixels.
    pub padding: f32,
    /// Automatic word wrapping.
    pub wrap: bool,
}
impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            rotation: 0.0,
            no_fill: false,
            no_stroke: false,
            width: 1.7,
            dash: Dash::Solid,
            bold: false,
            italic: false,
            align: TextAlign::Center,
            padding: 8.0,
            wrap: true,
        }
    }
}

/// A shape with editable text and an axis-aligned frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Stable scene ID.
    pub id: u64,
    /// Shape geometry.
    pub kind: ShapeKind,
    /// Top-left frame corner.
    pub position: Point,
    /// Frame width and height.
    pub size: Point,
    /// Plain multiline label.
    pub text: String,
    /// Opaque sRGB fill.
    pub fill: [u8; 3],
    /// Opaque sRGB outline.
    pub stroke: [u8; 3],
    /// Label size in logical pixels.
    pub font_size: f32,
    /// Additional formatting; older JSON scenes use the defaults.
    #[serde(default)]
    pub style: NodeStyle,
}
impl Node {
    /// Axis-aligned visual bounds including rotation (minimum and maximum corners).
    #[must_use]
    pub fn visual_bounds(&self) -> (Point, Point) {
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in [
            self.position,
            Point::new(self.position.x + self.size.x, self.position.y),
            Point::new(self.position.x + self.size.x, self.position.y + self.size.y),
            Point::new(self.position.x, self.position.y + self.size.y),
        ] {
            let p = rotate(p, self.center(), self.style.rotation);
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        (min, max)
    }
    /// Centre of the node frame.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            self.position.x + self.size.x / 2.0,
            self.position.y + self.size.y / 2.0,
        )
    }
    /// Connection attachment point, including the ellipse/diamond boundary.
    #[must_use]
    pub fn port(&self, port: Port) -> Point {
        let c = self.center();
        let fallback = match port {
            Port::Left => Point::new(self.position.x, c.y),
            Port::Right => Point::new(self.position.x + self.size.x, c.y),
            Port::Top => Point::new(c.x, self.position.y),
            Port::Bottom => Point::new(c.x, self.position.y + self.size.y),
        };
        if matches!(
            self.kind,
            ShapeKind::Rectangle
                | ShapeKind::Rounded
                | ShapeKind::Ellipse
                | ShapeKind::Diamond
                | ShapeKind::Text
        ) {
            return rotate(fallback, c, self.style.rotation);
        }
        let direction = Point::new(fallback.x - c.x, fallback.y - c.y);
        let points = self.local_outline();
        let mut best = 1.0_f32;
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let edge = Point::new(b.x - a.x, b.y - a.y);
            let cross = |a: Point, b: Point| a.x * b.y - a.y * b.x;
            let denom = cross(direction, edge);
            if denom.abs() > 0.0001 {
                let relative = Point::new(a.x - c.x, a.y - c.y);
                let t = cross(relative, edge) / denom;
                let u = cross(relative, direction) / denom;
                if t >= 0.0 && (0.0..=1.0).contains(&u) {
                    best = best.min(t);
                }
            }
        }
        rotate(
            Point::new(c.x + direction.x * best, c.y + direction.y * best),
            c,
            self.style.rotation,
        )
    }
    /// Geometric hit test rather than just the bounding rectangle.
    #[must_use]
    pub fn contains(&self, p: Point) -> bool {
        let p = rotate(p, self.center(), -self.style.rotation);
        let c = self.center();
        let x = (p.x - c.x).abs() / (self.size.x / 2.0);
        let y = (p.y - c.y).abs() / (self.size.y / 2.0);
        match self.kind {
            ShapeKind::Ellipse => x * x + y * y <= 1.0,
            ShapeKind::Diamond => x + y <= 1.0,
            ShapeKind::Triangle
            | ShapeKind::Trapezoid
            | ShapeKind::Hexagon
            | ShapeKind::RightArrow
            | ShapeKind::Document => {
                let vertices = self.local_outline();
                let mut inside = false;
                for i in 0..vertices.len() {
                    let a = vertices[i];
                    let b = vertices[(i + 1) % vertices.len()];
                    if (a.y > p.y) != (b.y > p.y)
                        && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
                    {
                        inside = !inside;
                    }
                }
                inside
            }
            _ => x <= 1.0 && y <= 1.0,
        }
    }
    /// Polygon for the non-elliptic preset outline in scene coordinates.
    #[must_use]
    pub fn outline(&self) -> Vec<Point> {
        self.local_outline()
            .into_iter()
            .map(|p| rotate(p, self.center(), self.style.rotation))
            .collect()
    }
    fn local_outline(&self) -> Vec<Point> {
        if self.kind == ShapeKind::Rounded {
            let r = self.size.x.min(self.size.y) / 6.0;
            let (x, y, w, h) = (self.position.x, self.position.y, self.size.x, self.size.y);
            let corners = [
                (x + w - r, y + r, -std::f32::consts::FRAC_PI_2),
                (x + w - r, y + h - r, 0.0),
                (x + r, y + h - r, std::f32::consts::FRAC_PI_2),
                (x + r, y + r, std::f32::consts::PI),
            ];
            return corners
                .into_iter()
                .flat_map(|(cx, cy, start)| {
                    (0u16..=8).map(move |i| {
                        let a = start + f32::from(i) * std::f32::consts::FRAC_PI_2 / 8.0;
                        Point::new(cx + r * a.cos(), cy + r * a.sin())
                    })
                })
                .collect();
        }
        if self.kind == ShapeKind::Ellipse {
            return (0u16..64)
                .map(|i| {
                    let angle = f32::from(i) * std::f32::consts::TAU / 64.0;
                    let c = self.center();
                    Point::new(
                        c.x + angle.cos() * self.size.x / 2.0,
                        c.y + angle.sin() * self.size.y / 2.0,
                    )
                })
                .collect();
        }
        let normalized: &[(f32, f32)] = match self.kind {
            ShapeKind::Diamond => &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
            ShapeKind::Triangle => &[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)],
            ShapeKind::Trapezoid => &[(0.25, 0.0), (0.75, 0.0), (1.0, 1.0), (0.0, 1.0)],
            ShapeKind::Hexagon => &[
                (0.25, 0.0),
                (0.75, 0.0),
                (1.0, 0.5),
                (0.75, 1.0),
                (0.25, 1.0),
                (0.0, 0.5),
            ],
            ShapeKind::RightArrow => &[
                (0.0, 0.25),
                (0.6, 0.25),
                (0.6, 0.0),
                (1.0, 0.5),
                (0.6, 1.0),
                (0.6, 0.75),
                (0.0, 0.75),
            ],
            _ => &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        };
        let mut points: Vec<_> = normalized
            .iter()
            .map(|(x, y)| {
                Point::new(
                    self.position.x + x * self.size.x,
                    self.position.y + y * self.size.y,
                )
            })
            .collect();
        if self.kind == ShapeKind::Document {
            points = vec![
                self.position,
                Point::new(self.position.x + self.size.x, self.position.y),
            ];
            for i in 0u16..=24 {
                let t = f32::from(i) / 24.0;
                let u = 1.0 - t;
                let x = u * u * u + 3.0 * u * u * t * 0.75 + 3.0 * u * t * t * 0.25;
                let y =
                    u * u * u * 0.85 + 3.0 * u * u * t + 3.0 * u * t * t * 0.7 + t * t * t * 0.85;
                points.push(Point::new(
                    self.position.x + x * self.size.x,
                    self.position.y + y * self.size.y,
                ));
            }
        }
        points
    }
    /// Preview/SVG wrapping. DOCX stores the original text and uses the host's typesetter.
    #[must_use]
    pub fn label_lines(&self) -> Vec<String> {
        if !self.style.wrap {
            return self.text.split('\n').map(str::to_owned).collect();
        }
        let max = (self.size.x - self.style.padding * 2.0).max(8.0);
        let mut lines = vec![];
        for paragraph in self.text.split('\n') {
            let mut line = String::new();
            let mut width = 0.0;
            for word in paragraph.split_whitespace() {
                let word_width = f32::from(u16::try_from(word.chars().count()).unwrap_or(u16::MAX))
                    * self.font_size
                    * 0.56;
                if !line.is_empty() && width + word_width + self.font_size * 0.3 > max {
                    lines.push(std::mem::take(&mut line));
                    width = 0.0;
                }
                if !line.is_empty() {
                    line.push(' ');
                    width += self.font_size * 0.3;
                }
                line.push_str(word);
                width += word_width;
            }
            lines.push(line);
        }
        lines
    }
}

/// Flat group of shapes; nesting is deliberately excluded in this version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Stable group ID.
    pub id: u64,
    /// Member node IDs, each node belongs to at most one group.
    pub members: Vec<u64>,
}

/// Cardinal attachment ports. Connections retain node IDs when nodes move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Port {
    /// Left boundary midpoint.
    Left,
    /// Right boundary midpoint.
    Right,
    /// Top boundary midpoint.
    Top,
    /// Bottom boundary midpoint.
    Bottom,
}
/// Directed orthogonal connector; endpoints are never duplicated coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Routing {
    /// Original short orthogonal route.
    #[default]
    Simple,
    /// Bounded orthogonal search around visual shape bounds.
    AvoidObstacles,
}
/// Directed connector with stable shape and port references.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    /// Routing policy; older scenes preserve their original route.
    #[serde(default)]
    pub routing: Routing,
    /// Stable scene ID.
    pub id: u64,
    /// Source node ID.
    pub from: u64,
    /// Destination node ID.
    pub to: u64,
    /// Source attachment.
    pub from_port: Port,
    /// Destination attachment.
    pub to_port: Port,
    /// Opaque sRGB line and arrow colour.
    pub color: [u8; 3],
    /// Line width in logical pixels.
    #[serde(default = "default_width")]
    pub width: f32,
    /// Line dash.
    #[serde(default)]
    pub dash: Dash,
    /// Arrowhead on the source end.
    #[serde(default)]
    pub start_arrow: bool,
    /// Arrowhead on the destination end.
    #[serde(default = "yes")]
    pub end_arrow: bool,
}
fn default_width() -> f32 {
    1.7
}
fn yes() -> bool {
    true
}

/// Portable editable scene. Versioned separately from any document format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagram {
    /// Scene schema version, currently 3; versions 1/2 migrate on JSON load.
    pub version: u32,
    /// Paint order: later nodes cover earlier ones.
    pub nodes: Vec<Node>,
    /// Directed links, painted behind nodes.
    pub connections: Vec<Connection>,
    /// Flat editable groups.
    #[serde(default)]
    pub groups: Vec<Group>,
}
impl Default for Diagram {
    fn default() -> Self {
        Self {
            version: 3,
            nodes: vec![],
            connections: vec![],
            groups: vec![],
        }
    }
}
impl Diagram {
    /// Validates IDs, references and resource/coordinate limits.
    /// # Errors
    /// Returns a diagnostic for unsupported versions or invalid scene data.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 3 {
            return Err("Unsupported diagram version".into());
        }
        if self.nodes.len() > 1000 || self.connections.len() > 2000 {
            return Err("Diagram object limit exceeded".into());
        }
        let mut ids = HashSet::new();
        for n in &self.nodes {
            if n.id == 0 || n.id > 1_000_000_000 || !ids.insert(n.id) {
                return Err("Invalid or duplicate node ID".into());
            }
            if ![n.position.x, n.position.y]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100_000.0)
                || ![n.size.x, n.size.y]
                    .iter()
                    .all(|v| v.is_finite() && (24.0..=10000.0).contains(v))
                || !n.font_size.is_finite()
                || !(8.0..=96.0).contains(&n.font_size)
                || n.text.len() > 8192
                || !n.style.width.is_finite()
                || !n.style.rotation.is_finite()
                || !(0.0..360.0).contains(&n.style.rotation)
                || !(0.5..=20.0).contains(&n.style.width)
                || !n.style.padding.is_finite()
                || !(0.0..=100.0).contains(&n.style.padding)
                || n.text
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err("Invalid shape geometry or text".into());
            }
        }
        let node_ids = ids.clone();
        let mut grouped = HashSet::new();
        for g in &self.groups {
            if g.id == 0
                || g.id > 1_000_000_000
                || !ids.insert(g.id)
                || g.members.len() < 2
                || g.members
                    .iter()
                    .any(|id| !node_ids.contains(id) || !grouped.insert(*id))
            {
                return Err("Invalid group or duplicate membership".into());
            }
            let indices: Vec<_> = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| g.members.contains(&n.id))
                .map(|(i, _)| i)
                .collect();
            if indices.windows(2).any(|pair| pair[1] != pair[0] + 1) {
                return Err("Grouped shapes must be consecutive in paint order".into());
            }
        }
        for c in &self.connections {
            if c.id == 0
                || c.id > 1_000_000_000
                || !ids.insert(c.id)
                || c.from == c.to
                || !node_ids.contains(&c.from)
                || !node_ids.contains(&c.to)
                || !c.width.is_finite()
                || !(0.5..=20.0).contains(&c.width)
            {
                return Err("Invalid connector ID or endpoint".into());
            }
        }
        Ok(())
    }
    /// Loads bounded JSON and validates the complete scene before returning it.
    /// # Errors
    /// Returns an encoding, schema, geometry or resource-limit diagnostic.
    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > 8 * 1024 * 1024 {
            return Err("Diagram file is too large".into());
        }
        let mut diagram: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if diagram.version == 1 || diagram.version == 2 {
            diagram.version = 3;
        }
        diagram.validate()?;
        Ok(diagram)
    }
    /// Serializes editable source after validation.
    /// # Errors
    /// Returns a validation or serialization error.
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }
    /// Looks up a node by stable ID.
    #[must_use]
    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    /// Current connector path, recalculated from its attached shapes.
    #[must_use]
    pub fn route(&self, c: &Connection) -> Vec<Point> {
        self.route_status(c).0
    }
    /// Route plus whether requested obstacle avoidance succeeded. A bounded
    /// search may fall back to the original route; callers can show a warning.
    #[must_use]
    pub fn route_status(&self, c: &Connection) -> (Vec<Point>, bool) {
        if c.routing == Routing::AvoidObstacles {
            if let Some(path) = crate::routing::avoid(self, c) {
                return (path, true);
            }
            return (self.simple_route(c), false);
        }
        (self.simple_route(c), true)
    }
    fn simple_route(&self, c: &Connection) -> Vec<Point> {
        let (Some(a), Some(b)) = (self.node(c.from), self.node(c.to)) else {
            return vec![];
        };
        let start = a.port(c.from_port);
        let end = b.port(c.to_port);
        let outward = |p: Point, port: Port| match port {
            Port::Left => Point::new(p.x - 24.0, p.y),
            Port::Right => Point::new(p.x + 24.0, p.y),
            Port::Top => Point::new(p.x, p.y - 24.0),
            Port::Bottom => Point::new(p.x, p.y + 24.0),
        };
        let a = rotate(
            outward(rotate(start, a.center(), -a.style.rotation), c.from_port),
            a.center(),
            a.style.rotation,
        );
        let b = rotate(
            outward(rotate(end, b.center(), -b.style.rotation), c.to_port),
            b.center(),
            b.style.rotation,
        );
        let horizontal = |p| matches!(p, Port::Left | Port::Right);
        let mut path = vec![start, a];
        match (horizontal(c.from_port), horizontal(c.to_port)) {
            (true, true) => {
                let mid = a.x.midpoint(b.x);
                path.extend([Point::new(mid, a.y), Point::new(mid, b.y)]);
            }
            (false, false) => {
                let mid = a.y.midpoint(b.y);
                path.extend([Point::new(a.x, mid), Point::new(b.x, mid)]);
            }
            (true, false) => path.push(Point::new(b.x, a.y)),
            (false, true) => path.push(Point::new(a.x, b.y)),
        }
        path.extend([b, end]);
        path.dedup();
        path
    }
}
