//! Reading the cached data of a DrawingML chart part (`c:chartSpace`).
//!
//! A `c:chart r:id` in a drawing names a chart part through the relationships
//! of the part being parsed. `PartParser::chart_for` resolves that id, reads
//! the part through the package (so the package's part-size limits and its
//! normalizer apply) and parses it into a [`ChartData`].
//!
//! The chart part is small and its interesting content sits at fixed paths, so
//! it is first read into a bounded element tree ([`MAX_CHART_NODES`] elements,
//! built without recursion) and then queried. A part that is not a chart, is
//! malformed or exceeds a bound leaves the drawing without data and is recorded
//! in the support report; it never fails the document.

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

use crate::model::chart::{
    BarGrouping, ChartData, ChartKind, ChartSeries, LegendPosition, MAX_CHART_POINTS,
    MAX_CHART_SERIES,
};
use crate::model::support::SupportStatus;

use super::PartParser;

/// Most elements read from one chart part before it is given up on.
pub const MAX_CHART_NODES: usize = 200_000;

/// Most characters kept from one `c:v` / `a:t` text node.
const MAX_CHART_TEXT: usize = 1024;

/// Transitional chart namespace (a normalized part carries the Strict one;
/// both are read).
const CHART_TRANSITIONAL_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// Transitional DrawingML main namespace.
const DRAWINGML_TRANSITIONAL_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// The Office 2013+ default theme accents, `accent1`..`accent6`.
const OFFICE_ACCENTS: [&str; 6] = ["4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47"];

/// Charts already read by one part parser, by chart part.
pub(crate) struct ChartCache {
    limits: ResourceLimits,
    parsed: HashMap<PartId, Option<Arc<ChartData>>>,
}

impl ChartCache {
    /// An empty cache reading parts under `limits`.
    pub(crate) fn new(limits: &ResourceLimits) -> Self {
        Self {
            limits: *limits,
            parsed: HashMap::new(),
        }
    }
}

impl PartParser<'_> {
    /// The cached data of the chart part `rel_id` names, if it can be read.
    ///
    /// Every failure is a support record, never an error: a chart that cannot
    /// be drawn from its cache is drawn as a placeholder.
    pub(crate) fn chart_for(
        &mut self,
        rel_id: &str,
        location: &SourceLocation,
    ) -> Option<Arc<ChartData>> {
        let Some(part) = self.resolve_relationship_target(rel_id) else {
            self.record(
                "c:chart",
                SupportStatus::Partial,
                Some(format!(
                    "r:id '{rel_id}' does not resolve to a chart part; not drawn"
                )),
                Some(location.clone()),
            );
            return None;
        };
        if let Some(cached) = self.charts.parsed.get(&part) {
            return cached.clone();
        }
        let limits = self.charts.limits;
        let result = self
            .package
            .read_part(&part)
            .map_err(|error| error.to_string())
            .and_then(|bytes| parse_chart_xml(bytes, part.clone(), &limits));
        let chart = match result {
            Ok(chart) => {
                for feature in &chart.unsupported {
                    self.record(
                        feature,
                        SupportStatus::Partial,
                        Some("chart content not drawn from the cache".to_owned()),
                        Some(location.clone()),
                    );
                }
                self.record(
                    "c:chartSpace",
                    SupportStatus::Supported,
                    None,
                    Some(location.clone()),
                );
                Some(Arc::new(chart))
            }
            Err(message) => {
                self.record(
                    "c:chartSpace",
                    SupportStatus::Partial,
                    Some(format!(
                        "chart part {part} could not be read ({message}); drawn as a placeholder"
                    )),
                    Some(location.clone()),
                );
                None
            }
        };
        self.charts.parsed.insert(part, chart.clone());
        chart
    }
}

/// Which vocabulary an element belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Space {
    /// DrawingML chart (`c:`).
    Chart,
    /// DrawingML main (`a:`).
    Drawing,
    /// Anything else.
    Other,
}

fn space_of(name: &QName) -> Space {
    match name.ns.as_ref() {
        Some(ns) if ns == crate::CHART_STRICT_NS || ns == CHART_TRANSITIONAL_NS => Space::Chart,
        Some(ns) if ns == crate::DRAWINGML_STRICT_NS || ns == DRAWINGML_TRANSITIONAL_NS => {
            Space::Drawing
        }
        _ => Space::Other,
    }
}

/// One element of the chart tree.
#[derive(Debug)]
struct Node {
    space: Space,
    local: String,
    /// Unqualified attributes (`val`, `idx`...), the only ones a chart uses.
    attrs: Vec<(String, String)>,
    /// Character data, kept only for `c:v` and `a:t`.
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn new(name: &QName, attrs: Vec<strict_ooxml_core::xml::Attr>) -> Self {
        Self {
            space: space_of(name),
            local: name.local().to_owned(),
            attrs: attrs
                .into_iter()
                .filter(|attr| attr.name.ns.is_none())
                .map(|attr| (attr.name.local().to_owned(), attr.value))
                .collect(),
            text: String::new(),
            children: Vec::new(),
        }
    }

    fn is(&self, space: Space, local: &str) -> bool {
        self.space == space && self.local == local
    }

    fn keeps_text(&self) -> bool {
        self.is(Space::Chart, "v") || self.is(Space::Drawing, "t")
    }

    fn child(&self, space: Space, local: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.is(space, local))
    }

    fn c(&self, local: &str) -> Option<&Node> {
        self.child(Space::Chart, local)
    }

    fn a(&self, local: &str) -> Option<&Node> {
        self.child(Space::Drawing, local)
    }

    fn all_c<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children
            .iter()
            .filter(move |child| child.is(Space::Chart, local))
    }

    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn val(&self) -> Option<&str> {
        self.attr("val")
    }

    /// `CT_Boolean`: an absent `val` means true.
    fn flag(&self) -> bool {
        !matches!(self.val().map(str::trim), Some("0" | "false"))
    }
}

/// A chart tree and the number of `c:pt` elements in it.
struct Tree {
    root: Node,
    points: usize,
}

/// Reads a whole part into a [`Tree`], iteratively.
fn build_tree(reader: &mut XmlReader) -> Result<Tree, String> {
    let mut stack: Vec<Node> = Vec::new();
    let mut nodes = 0usize;
    let mut points = 0usize;
    loop {
        match reader.next_event().map_err(|error| error.to_string())? {
            XmlEvent::StartElement { name, attrs } => {
                nodes += 1;
                if nodes > MAX_CHART_NODES {
                    return Err(format!("more than {MAX_CHART_NODES} elements"));
                }
                let node = Node::new(&name, attrs);
                if node.is(Space::Chart, "pt") {
                    points += 1;
                }
                stack.push(node);
            }
            XmlEvent::EndElement { .. } => {
                let Some(node) = stack.pop() else {
                    return Err("unbalanced end element".to_owned());
                };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Ok(Tree { root: node, points }),
                }
            }
            XmlEvent::Text(text) | XmlEvent::CData(text) => {
                if let Some(node) = stack.last_mut() {
                    if node.keeps_text() {
                        let room = MAX_CHART_TEXT.saturating_sub(node.text.chars().count());
                        node.text.extend(text.chars().take(room));
                    }
                }
            }
            XmlEvent::Eof => return Err("unexpected end of chart part".to_owned()),
        }
    }
}

/// Confirms nothing but whitespace follows the root element.
///
/// lint-eof: this arm is the success case. Reaching the end of the part is
/// what this function is waiting for.
fn drain(reader: &mut XmlReader) -> Result<(), String> {
    loop {
        match reader.next_event().map_err(|error| error.to_string())? {
            XmlEvent::Eof => return Ok(()),
            XmlEvent::Text(text) | XmlEvent::CData(text) if text.trim().is_empty() => {}
            _ => return Err("content after the root element".to_owned()),
        }
    }
}

/// Parses the bytes of a chart part.
///
/// # Errors
///
/// A message for malformed XML, a root other than `c:chartSpace`, a missing
/// `c:chart`/`c:plotArea` or a part past [`MAX_CHART_NODES`].
pub(crate) fn parse_chart_xml(
    bytes: Vec<u8>,
    part: PartId,
    limits: &ResourceLimits,
) -> Result<ChartData, String> {
    let mut reader = XmlReader::from_vec(bytes, part, limits).map_err(|error| error.to_string())?;
    let tree = build_tree(&mut reader)?;
    drain(&mut reader)?;
    if !tree.root.is(Space::Chart, "chartSpace") {
        return Err(format!("root is '{}', not c:chartSpace", tree.root.local));
    }
    let chart = tree.root.c("chart").ok_or("no c:chart element")?;
    let plot = chart.c("plotArea").ok_or("no c:plotArea element")?;
    // Memory follows the input: a series is at most as long as its highest
    // cached index, and all of them together get 16 slots per cached point.
    let budget = tree
        .points
        .saturating_mul(16)
        .saturating_add(MAX_CHART_POINTS)
        .min(MAX_CHART_SERIES * MAX_CHART_POINTS);
    let mut reader = ChartReader {
        notes: Vec::new(),
        cells_left: budget,
    };
    Ok(reader.chart(chart, plot))
}

/// The state of one chart's conversion: the notes and the point budget.
struct ChartReader {
    notes: Vec<String>,
    cells_left: usize,
}

impl ChartReader {
    fn note(&mut self, feature: &str) {
        if !self.notes.iter().any(|note| note == feature) {
            self.notes.push(feature.to_owned());
        }
    }

    fn chart(&mut self, chart: &Node, plot: &Node) -> ChartData {
        let mut types = plot
            .children
            .iter()
            .filter(|child| child.space == Space::Chart && child.local.ends_with("Chart"));
        let (kind, series, categories) = match types.next() {
            Some(first) => {
                let kind = self.kind(first);
                let (series, categories) = self.all_series(first, &kind);
                (kind, series, categories)
            }
            None => (
                ChartKind::Other("plotArea".to_owned()),
                Vec::new(),
                Vec::new(),
            ),
        };
        for other in types {
            self.note(&format!("c:{}", other.local));
        }
        let title = title(chart, &series);
        let legend_node = chart.c("legend");
        let legend_position = match legend_node.and_then(|legend| legend.c("legendPos")) {
            Some(position) => match position.val() {
                Some("l") => LegendPosition::Left,
                Some("t") => LegendPosition::Top,
                Some("b") => LegendPosition::Bottom,
                _ => LegendPosition::Right,
            },
            None => LegendPosition::Right,
        };
        ChartData {
            kind,
            title,
            categories,
            series,
            legend: legend_node.is_some(),
            legend_position,
            unsupported: std::mem::take(&mut self.notes),
        }
    }

    /// The chart kind of a chart-type element, noting what it approximates.
    fn kind(&mut self, node: &Node) -> ChartKind {
        let local = node.local.as_str();
        match local {
            "barChart" | "bar3DChart" => {
                if local == "bar3DChart" {
                    self.note("c:bar3DChart");
                }
                let horizontal = node.c("barDir").and_then(Node::val) == Some("bar");
                let grouping = match node.c("grouping").and_then(Node::val) {
                    Some("stacked") => BarGrouping::Stacked,
                    Some("percentStacked") => BarGrouping::PercentStacked,
                    _ => BarGrouping::Clustered,
                };
                ChartKind::Bar {
                    horizontal,
                    grouping,
                }
            }
            "lineChart" | "line3DChart" | "areaChart" | "area3DChart" => {
                if local.contains("3D") {
                    self.note(&format!("c:{local}"));
                }
                if matches!(
                    node.c("grouping").and_then(Node::val),
                    Some("stacked" | "percentStacked")
                ) {
                    self.note(&format!("c:{local}/c:grouping"));
                }
                if local.starts_with("line") {
                    ChartKind::Line
                } else {
                    ChartKind::Area
                }
            }
            "pieChart" | "pie3DChart" | "doughnutChart" | "ofPieChart" => {
                if local != "pieChart" && local != "doughnutChart" {
                    self.note(&format!("c:{local}"));
                }
                ChartKind::Pie {
                    doughnut: local == "doughnutChart",
                }
            }
            "scatterChart" | "bubbleChart" => {
                if local == "bubbleChart" {
                    self.note("c:bubbleChart");
                }
                ChartKind::Scatter
            }
            _ => {
                self.note(&format!("c:{local}"));
                ChartKind::Other(local.to_owned())
            }
        }
    }

    /// Every `c:ser` of a chart-type element, and the category labels.
    fn all_series(&mut self, node: &Node, kind: &ChartKind) -> (Vec<ChartSeries>, Vec<String>) {
        let mut series = Vec::new();
        let mut categories: Option<Vec<String>> = None;
        for (index, ser) in node.all_c("ser").enumerate() {
            if index >= MAX_CHART_SERIES {
                self.note("limit.chart_series");
                break;
            }
            if categories.is_none() {
                categories = ser.c("cat").and_then(|cat| self.labels(cat));
            }
            series.push(self.series(ser, kind));
        }
        if node.c("dLbls").is_some_and(shows_labels) {
            self.note("c:dLbls");
        }
        (series, categories.unwrap_or_default())
    }

    fn series(&mut self, ser: &Node, kind: &ChartKind) -> ChartSeries {
        let scatter = matches!(kind, ChartKind::Scatter);
        let values_node = if scatter {
            ser.c("yVal").or_else(|| ser.c("val"))
        } else {
            ser.c("val")
        };
        let values = values_node
            .map(|node| self.numbers(node))
            .unwrap_or_default();
        let x_values = if scatter {
            ser.c("xVal")
                .map(|node| self.numbers(node))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for child in &ser.children {
            if child.space == Space::Chart
                && matches!(child.local.as_str(), "trendline" | "errBars")
            {
                self.note(&format!("c:{}", child.local));
            }
        }
        if ser.c("dLbls").is_some_and(shows_labels) {
            self.note("c:dLbls");
        }
        let line_first = matches!(kind, ChartKind::Line | ChartKind::Scatter);
        let mut color = ser.c("spPr").and_then(|sp| shape_color(sp, line_first));
        if color.is_none() && line_first {
            color = ser
                .c("marker")
                .and_then(|marker| marker.c("spPr"))
                .and_then(|sp| shape_color(sp, false));
        }
        ChartSeries {
            name: ser.c("tx").and_then(series_name),
            values,
            x_values,
            color,
            point_colors: point_colors(ser),
        }
    }

    /// The cache under a data source (`c:val`, `c:cat`...), when it has one.
    fn cache(source: &Node) -> Option<&Node> {
        source
            .c("numRef")
            .and_then(|reference| reference.c("numCache"))
            .or_else(|| source.c("numLit"))
            .or_else(|| {
                source
                    .c("strRef")
                    .and_then(|reference| reference.c("strCache"))
            })
            .or_else(|| source.c("strLit"))
            .or_else(|| {
                source
                    .c("multiLvlStrRef")
                    .and_then(|reference| reference.c("multiLvlStrCache"))
                    .and_then(|cache| cache.c("lvl"))
            })
    }

    /// The `(idx, text)` of each cached point, bounded by `c:ptCount` and by
    /// [`MAX_CHART_POINTS`] and the chart's budget, plus the length to allocate.
    fn points<'n>(&mut self, cache: &'n Node) -> (Vec<(usize, &'n str)>, usize) {
        let declared = cache
            .c("ptCount")
            .and_then(Node::val)
            .and_then(|value| value.trim().parse::<usize>().ok());
        let bound = declared.unwrap_or(MAX_CHART_POINTS).min(MAX_CHART_POINTS);
        let mut points = Vec::new();
        for pt in cache.all_c("pt") {
            let Some(index) = pt
                .attr("idx")
                .and_then(|idx| idx.trim().parse::<usize>().ok())
            else {
                continue;
            };
            if index >= bound {
                if index < declared.unwrap_or(usize::MAX) {
                    self.note("limit.chart_points");
                }
                continue;
            }
            let text = pt.c("v").map_or("", |v| v.text.as_str());
            points.push((index, text));
        }
        let wanted = points.iter().map(|(index, _)| index + 1).max().unwrap_or(0);
        let length = wanted.min(self.cells_left);
        if length < wanted {
            self.note("limit.chart_points");
        }
        self.cells_left -= length;
        points.retain(|(index, _)| *index < length);
        (points, length)
    }

    fn numbers(&mut self, source: &Node) -> Vec<Option<f64>> {
        let Some(cache) = Self::cache(source) else {
            return Vec::new();
        };
        let (points, length) = self.points(cache);
        let mut values = vec![None; length];
        for (index, text) in points {
            if let Some(slot) = values.get_mut(index) {
                *slot = super::parse_decimal(text);
            }
        }
        values
    }

    fn labels(&mut self, source: &Node) -> Option<Vec<String>> {
        let cache = Self::cache(source)?;
        let numeric = cache.is(Space::Chart, "numCache") || cache.is(Space::Chart, "numLit");
        let (points, length) = self.points(cache);
        let mut labels = vec![String::new(); length];
        for (index, text) in points {
            if let Some(slot) = labels.get_mut(index) {
                *slot = if numeric {
                    super::parse_decimal(text).map_or_else(|| text.to_owned(), format_number)
                } else {
                    text.to_owned()
                };
            }
        }
        Some(labels)
    }
}

/// The title to show: its text, or the single series' name for a title
/// with no text of its own; none when deleted or absent.
fn title(chart: &Node, series: &[ChartSeries]) -> Option<String> {
    let deleted = chart.c("autoTitleDeleted").is_some_and(Node::flag);
    let title = chart.c("title")?;
    let text = title.c("tx").and_then(|tx| {
        tx.c("rich")
            .map(rich_text)
            .or_else(|| series_name(tx))
            .filter(|text| !text.trim().is_empty())
    });
    match text {
        Some(text) => Some(text),
        None if deleted => None,
        None => match series {
            [only] => only.name.clone(),
            _ => Some("Chart Title".to_owned()),
        },
    }
}

/// Whether a `c:dLbls` shows anything.
fn shows_labels(labels: &Node) -> bool {
    ["showVal", "showPercent", "showCatName", "showSerName"]
        .iter()
        .any(|name| labels.c(name).is_some_and(Node::flag))
}

/// A series name: the first cached string of `c:strRef`, or a literal `c:v`.
fn series_name(tx: &Node) -> Option<String> {
    let cached = tx
        .c("strRef")
        .and_then(|reference| reference.c("strCache"))
        .and_then(|cache| cache.c("pt"))
        .and_then(|pt| pt.c("v"))
        .or_else(|| tx.c("v"))?;
    Some(cached.text.clone())
}

/// The text of `c:rich`: its runs, paragraphs separated by a space.
fn rich_text(rich: &Node) -> String {
    let mut paragraphs = Vec::new();
    for paragraph in rich.children.iter().filter(|p| p.is(Space::Drawing, "p")) {
        let mut text = String::new();
        for run in &paragraph.children {
            if run.is(Space::Drawing, "r") || run.is(Space::Drawing, "fld") {
                if let Some(t) = run.a("t") {
                    text.push_str(&t.text);
                }
            }
        }
        paragraphs.push(text);
    }
    paragraphs.join(" ")
}

/// Explicit per-point colours (`c:dPt`).
fn point_colors(ser: &Node) -> Vec<(usize, String)> {
    ser.all_c("dPt")
        .filter_map(|point| {
            let index = point
                .c("idx")
                .and_then(Node::val)
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|index| *index < MAX_CHART_POINTS)?;
            let color = point.c("spPr").and_then(|sp| shape_color(sp, false))?;
            Some((index, color))
        })
        .take(MAX_CHART_POINTS)
        .collect()
}

/// The colour of a `c:spPr`: its solid fill, or the solid fill of its line
/// (line first when `line_first`).
fn shape_color(sp: &Node, line_first: bool) -> Option<String> {
    let fill = || sp.a("solidFill").and_then(fill_color);
    let line = || {
        sp.a("ln")
            .and_then(|ln| ln.a("solidFill"))
            .and_then(fill_color)
    };
    if line_first {
        line().or_else(fill)
    } else {
        fill().or_else(line)
    }
}

/// `a:srgbClr` as `RRGGBB`; `a:schemeClr accentN` as the Office accent.
fn fill_color(fill: &Node) -> Option<String> {
    if let Some(rgb) = fill.a("srgbClr").and_then(Node::val) {
        let rgb = rgb.trim();
        return (rgb.len() == 6 && rgb.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| rgb.to_ascii_uppercase());
    }
    let scheme = fill.a("schemeClr").and_then(Node::val)?;
    let index = scheme
        .strip_prefix("accent")
        .and_then(|digit| digit.parse::<usize>().ok())?;
    OFFICE_ACCENTS
        .get(index.checked_sub(1)?)
        .map(|hex| (*hex).to_owned())
}

/// A numeric category label: integers without a decimal point.
fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}
