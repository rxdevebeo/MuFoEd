//! Tables, read from the ruling lines of a page (`O-9a`).
//!
//! A PDF says where the ink is and nothing about what it means. A grid of
//! ruling lines is the closest thing it has to a table, and this module turns
//! one into a `w:tbl` — by rules that can be read, because the alternative is a
//! document that looks like a table and is not one.
//!
//! # The chain of judgements
//!
//! ```text
//! paths -> rules -> components -> a grid of boundaries -> cells -> paragraphs
//! ```
//!
//! 1. **paths to rules.** A ruling line is a path that is long and thin: stroked
//!    with a thin pen, or filled as a thin box. The test is stated in
//!    [`TableRules::min_rule_length`] and [`TableRules::max_rule_thickness`].
//! 2. **rules to components.** Two rules that meet belong to the same figure,
//!    which is what keeps a table beside a chart from becoming one grid. Ends
//!    closer than [`TableRules::tolerance`] are the same point.
//! 3. **components to a grid.** A component is a table when it bounds at least
//!    [`TableRules::min_columns`] columns and [`TableRules::min_rows`] rows, its
//!    outer frame runs all the way round, and at least
//!    [`TableRules::min_filled_cells`] cells carry text. A frame round one
//!    paragraph is a frame; a grid with no text in it is a drawing.
//! 4. **a grid to cells.** A boundary that is *missing* is a merge: no rule
//!    between two columns inside a row's band means one cell spans them
//!    (`w:gridSpan`), and no rule under a cell means it continues into the row
//!    below (`w:vMerge`). The ink says so directly — a merged cell has no rule in
//!    it — and both are recorded as inferences.
//! 5. **cells to paragraphs.** A cell's lines become paragraphs by exactly the
//!    rules the page uses ([`crate::ParagraphRules`]), with one exception: a cell
//!    is never a heading, however large its text.
//!
//! A line only joins a table when the whole of it is inside the grid: a line
//! that runs across a grid is more likely to be a paragraph beside it than a
//! cell that overflows. Every other line stays in the page flow, and a grid that
//! fails any of the tests above is reported with the rule that rejected it.
//!
//! # What this does not do
//!
//! - **A grid that spans a page break.** A table continued over a page is two
//!   figures, each with an open side, and neither is a table. The report says so
//!   once per page and the text stays in the flow; joining the halves is a
//!   decision about where a table ends, not a reading of the ink.
//! - **Nested tables.** A grid inside a cell is found as its own figure, and the
//!   two become sibling tables rather than one inside the other, because the
//!   position that says "inside" is the same position that says "beside".
//! - **Per-cell borders that differ.** A rule that covers part of a row's width
//!   is applied to the whole row: the grid gives the boundaries, and a border
//!   that stops halfway is a border this increment does not model.
//! - **A header row.** `w:tblHeader` is not written: nothing in the ink says the
//!   first row repeats, and guessing it would change how the table paginates.

use std::collections::BTreeMap;

use strict_ooxml_pdf::content::{Glyph, Item, Rgb, SubPath, Vector};
use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_pdf::PdfPage;
use strict_ooxml_wml::model::block::{Block, GridCol, Paragraph, Table, TableCell, TableRow};
use strict_ooxml_wml::model::props::{
    CellProperties, ParagraphProperties, RowProperties, TableProperties,
};
use strict_ooxml_wml::model::values::{
    Border, BorderStyle, Borders, Color, EighthsPoint, HeightRule, RowHeight, Rsids, TableLayout,
    Twips, VerticalMerge, Width, WidthKind,
};

use crate::report::{ConversionReport, Severity};
use crate::to_twips;

/// The thresholds that turn ruling lines into a table.
///
/// Public, for the same reason [`crate::ParagraphRules`] is: each number is a
/// claim about what a table looks like, and a claim nobody can change is a
/// claim nobody can check.
#[derive(Clone, Debug, PartialEq)]
pub struct TableRules {
    /// Two measurements this far apart, in points, are the same one: the ends of
    /// two ruling lines that meet, and a rule's position against its neighbour's.
    /// Default 1 pt — far below the distance between two columns, far above a
    /// rounding error in a matrix product.
    pub tolerance: f64,
    /// The thickest thing that is still a line rather than a box, in points.
    /// Default 2 pt: a table rule is a hairline and a filled cell is not.
    pub max_rule_thickness: f64,
    /// The shortest thing that is still a rule and not a mark, in points.
    /// Default 6 pt; below it a shape is a bullet, a minus sign or the bar of a
    /// fraction. It is deliberately small: a cell edge can be short — a table set
    /// in 8 pt type has edges of 8 pt — and the frame test is what separates a
    /// table from a scatter of lines, not this number.
    pub min_rule_length: f64,
    /// How much of a cell's edge a rule must cover before the cell counts as
    /// bounded on that side, as a fraction of the cell. Default 0.8: a rule that
    /// stops short is a rule beside the cell, not around it.
    pub min_cover_ratio: f64,
    /// Columns a grid needs before it is called a table. Default 2: one column
    /// bounded by rules is a frame.
    pub min_columns: usize,
    /// Rows a grid needs before it is called a table. Default 2: one row bounded
    /// by rules is a frame, and a boxed figure is a common shape.
    pub min_rows: usize,
    /// Cells that must carry text before a grid is called a table. Default 2: a
    /// grid with nothing written in it is a drawing, and turning a drawing into
    /// a table loses the drawing.
    pub min_filled_cells: usize,
}

impl Default for TableRules {
    fn default() -> Self {
        Self {
            tolerance: 1.0,
            max_rule_thickness: 2.0,
            min_rule_length: 6.0,
            min_cover_ratio: 0.8,
            min_columns: 2,
            min_rows: 2,
            min_filled_cells: 2,
        }
    }
}

/// One ruling line, in points from the page's top-left corner.
///
/// Normalised on construction: a horizontal rule has `y0 == y1` and `x0 < x1`, a
/// vertical one has `x0 == x1` and `y0 < y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rule {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    thickness: f64,
    color: [u8; 3],
}

impl Rule {
    /// Whether the rule runs across the page.
    fn horizontal(&self) -> bool {
        (self.y0 - self.y1).abs() < f64::EPSILON
    }
}

/// What stopped a component of ruling lines from being a table.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rejected {
    /// A single line, or an angle of two: a rule, not a figure, and nothing to
    /// report. A header separator is a rule, and calling it a table on every page
    /// is a report nobody reads.
    NotAFigure,
    /// Too few rules to bound anything.
    TooFewRules {
        verticals: usize,
        horizontals: usize,
    },
    /// A side of the outer frame does not run the whole way.
    OpenFrame,
    /// No text at all falls inside the grid.
    NoText,
    /// Not enough of the grid holds text to be a table.
    NotEnoughText { filled: usize, total: usize },
    /// A rule is missing and the cell it would have merged into is not there.
    DanglingSpan,
    /// The grid would have more cells than the page budget allows (AUD-15).
    TooManyCells { cells: usize, limit: usize },
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAFigure => f.write_str("a rule, not a figure"),
            Self::TooFewRules { verticals, horizontals } => write!(
                f,
                "{verticals} vertical and {horizontals} horizontal ruling lines cannot bound a table"
            ),
            Self::OpenFrame => f.write_str("the outer frame of the grid is open on one of its sides"),
            Self::NoText => f.write_str("no text falls inside the grid, so it is a drawing"),
            Self::NotEnoughText { filled, total } => write!(
                f,
                "only {filled} of {total} cells carry text, so the grid is a drawing or a frame"
            ),
            Self::DanglingSpan => f.write_str(
                "a rule is missing inside the grid and the cell it would have merged into is not there",
            ),
            Self::TooManyCells { cells, limit } => write!(
                f,
                "the grid would have {cells} cells, past the budget of {limit}"
            ),
        }
    }
}

/// A component of ruling lines that bounds a grid of cells.
#[derive(Clone, Debug)]
struct Grid {
    /// Column boundaries, ascending.
    xs: Vec<f64>,
    /// Row boundaries, ascending (points from the top of the page).
    ys: Vec<f64>,
    /// For each column boundary, the row bands it covers.
    vertical: Vec<Vec<(f64, f64)>>,
    /// For each row boundary, the column spans it covers.
    horizontal: Vec<Vec<(f64, f64)>>,
    /// The colour most of the rules are drawn in.
    color: [u8; 3],
    /// The thickness most of the rules are drawn with, in points.
    thickness: f64,
}

impl Grid {
    /// The number of columns.
    fn columns(&self) -> usize {
        self.xs.len() - 1
    }

    /// The number of rows.
    fn rows(&self) -> usize {
        self.ys.len() - 1
    }

    /// The leftmost column boundary.
    fn left(&self) -> f64 {
        self.xs.first().copied().unwrap_or(0.0)
    }

    /// The rightmost column boundary.
    fn right(&self) -> f64 {
        self.xs.last().copied().unwrap_or(0.0)
    }

    /// The topmost row boundary.
    fn top(&self) -> f64 {
        self.ys.first().copied().unwrap_or(0.0)
    }

    /// The bottommost row boundary.
    fn bottom(&self) -> f64 {
        self.ys.last().copied().unwrap_or(0.0)
    }
}

/// One cell of a planned table.
#[derive(Clone, Debug)]
pub(crate) struct PlannedCell {
    /// First column of the cell.
    column: usize,
    /// Number of columns it spans.
    span: usize,
    /// Vertical merge state, when a rule is missing under the cell.
    pub merge: Option<VerticalMerge>,
    /// Left edge, points from the page's left.
    pub left: f64,
    /// Width in points.
    pub width: f64,
    /// The cell's lines, in reading order.
    pub lines: Vec<GlyphLine>,
}

/// One row of a planned table.
#[derive(Clone, Debug)]
pub(crate) struct PlannedRow {
    /// The cells, left to right.
    pub cells: Vec<PlannedCell>,
    /// Height in points.
    pub height: f64,
}

/// A grid that was accepted as a table, with its text attached.
#[derive(Clone, Debug)]
pub(crate) struct PlannedTable {
    /// Index of the page line the table is placed at.
    pub first_line: usize,
    /// Width in points.
    pub width: f64,
    /// Column widths in twips.
    pub columns: Vec<i32>,
    /// The colour the rules are drawn in.
    pub color: [u8; 3],
    /// The thickness the rules are drawn with, in points.
    pub thickness: f64,
    /// The rows, top to bottom.
    pub rows: Vec<PlannedRow>,
    /// How many cells a missing rule merged, across or down.
    pub merged: usize,
}

/// Every table a page carries, and which of its lines each one owns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Plan {
    /// Per page line, the table it belongs to.
    owner: Vec<Option<usize>>,
    /// The tables, in the order their text appears on the page.
    pub tables: Vec<PlannedTable>,
}

impl Plan {
    /// Whether a line is the first line of a table, and which one.
    pub(crate) fn starts_table(&self, line: usize) -> Option<usize> {
        self.tables
            .iter()
            .position(|table| table.first_line == line)
    }

    /// Whether a line belongs to a table and must not become a paragraph.
    pub(crate) fn inside_table(&self, line: usize) -> bool {
        self.owner.get(line).copied().flatten().is_some()
    }
}

/// Reads every table a page carries.
///
/// Reporting happens here rather than in the caller, because a rejected grid is
/// a fact about the page and the caller has no other place to put it.
///
/// `max_table_lines` / `max_table_cells` come from [`crate::PdfOptions`] (AUD-15):
/// past either ceiling the page's text stays as paragraphs and the report
/// records `convert.table.budget`.
pub(crate) fn plan(
    page: &PdfPage,
    page_number: usize,
    lines: &[GlyphLine],
    config: &TableRules,
    max_table_lines: usize,
    max_table_cells: usize,
    report: &mut ConversionReport,
) -> Plan {
    let mut plan = Plan {
        owner: vec![None; lines.len()],
        tables: Vec::new(),
    };
    let found = rules_of(page, config);
    if found.len() > max_table_lines {
        report.record(
            "convert.table.budget",
            Severity::Unsupported,
            format!(
                "page {page_number}: {} ruling lines exceed max_table_lines ({max_table_lines}); \
                 tables were not sought",
                found.len()
            ),
        );
        return plan;
    }
    for component in components(&found, config.tolerance) {
        let grid = grid_of(&found, &component, config, max_table_cells);
        let index = plan.tables.len();
        match grid.and_then(|grid| attach(&grid, lines, config, index, &mut plan.owner)) {
            Ok(table) => {
                report.tables += 1;
                let cells = table.rows.iter().flat_map(|row| row.cells.iter()).count();
                let filled = table
                    .rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .filter(|cell| !cell.lines.is_empty())
                    .count();
                report.record(
                    "table.inferred",
                    Severity::Inferred,
                    format!(
                        "page {page_number}: {} ruling lines became a table of {} columns by {} \
                         rows, {filled} of {cells} cells with text, {} merged by a missing rule",
                        component.len(),
                        table.columns.len(),
                        table.rows.len(),
                        table.merged,
                    ),
                );
                plan.tables.push(table);
            }
            Err(Rejected::NotAFigure) => {}
            Err(Rejected::TooManyCells { cells, limit }) => report.record(
                "convert.table.budget",
                Severity::Unsupported,
                format!(
                    "page {page_number}: a grid of {cells} cells exceeds max_table_cells ({limit}); \
                     its text was left as paragraphs"
                ),
            ),
            Err(reason) => report.record(
                "table.detected",
                Severity::Unsupported,
                format!("page {page_number}: {reason}; its text was left as paragraphs"),
            ),
        }
    }
    plan
}

/// Builds the model table of a planned one.
///
/// `paragraphs` is `row -> cell`: the paragraphs of each cell, built by the
/// caller with the page's own rules. The geometry of a cell is not the geometry
/// of the page, and only the text is the text.
pub(crate) fn table_of(planned: &PlannedTable, paragraphs: Vec<Vec<Vec<Paragraph>>>) -> Table {
    let border = Border {
        style: Some(BorderStyle::Single),
        size: Some(eighths_of(planned.thickness)),
        color: Some(color(planned.color)),
        theme_color: None,
        space: None,
        shadow: false,
        frame: false,
    };
    let borders = Borders {
        top: Some(border.clone()),
        bottom: Some(border.clone()),
        start: Some(border.clone()),
        end: Some(border.clone()),
        inside_horizontal: Some(border.clone()),
        inside_vertical: Some(border),
    };
    let mut rows = Vec::with_capacity(planned.rows.len());
    for (row, cell_paragraphs) in planned.rows.iter().zip(paragraphs) {
        let mut cells = Vec::with_capacity(row.cells.len());
        for (cell, blocks) in row.cells.iter().zip(cell_paragraphs) {
            let mut blocks = blocks;
            // `w:tc` ends with a paragraph, so an empty cell is still a cell.
            if blocks.is_empty() {
                blocks.push(Paragraph {
                    props: ParagraphProperties::default(),
                    inlines: Vec::new(),
                    rsids: Rsids::default(),
                    para_id: None,
                    text_id: None,
                    revision: None,
                    location: strict_ooxml_core::error::SourceLocation::unknown(),
                });
            }
            cells.push(TableCell {
                props: CellProperties {
                    width: Some(Width {
                        kind: WidthKind::Dxa,
                        value: Some(to_twips(cell.width)),
                    }),
                    grid_span: (cell.span > 1).then_some(cell.span as u16),
                    vertical_merge: cell.merge,
                    ..CellProperties::default()
                },
                blocks: blocks.into_iter().map(Block::Paragraph).collect(),
                sdt: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            });
        }
        rows.push(TableRow {
            props: RowProperties {
                // The height the rules give, as a lower bound: a cell whose text
                // needs more room must be allowed to have it, or the converter
                // would be choosing a pagination the PDF never had.
                height: Some(RowHeight {
                    value: Some(Twips(to_twips(row.height))),
                    rule: Some(HeightRule::AtLeast),
                }),
                ..RowProperties::default()
            },
            cells,
            sdt: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        });
    }
    Table {
        props: TableProperties {
            width: Some(Width {
                kind: WidthKind::Dxa,
                value: Some(to_twips(planned.width)),
            }),
            // The columns have the widths the rules gave them; letting an engine
            // re-fit them would throw that away.
            layout: Some(TableLayout::Fixed),
            borders,
            ..TableProperties::default()
        },
        grid: planned
            .columns
            .iter()
            .map(|width| GridCol {
                width: Some(Twips(*width)),
            })
            .collect(),
        grid_change: None,
        rows,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    }
}

/// The page's ruling lines.
fn rules_of(page: &PdfPage, config: &TableRules) -> Vec<Rule> {
    let height = page.geometry.height;
    let mut out = Vec::new();
    for item in page.items() {
        let Item::Vector(vector) = item else {
            continue;
        };
        out.extend(vector_rules(vector, height, config));
    }
    out
}

/// The ruling lines one path draws.
fn vector_rules(vector: &Vector, height: f64, config: &TableRules) -> Vec<Rule> {
    let color = vector
        .stroke
        .or(vector.fill)
        .map_or([0, 0, 0], Rgb::to_rgb8);
    // A stroked path is as thin as its pen; a filled one is as thin as its box.
    let pen = vector.stroke.is_some() && vector.line_width <= config.max_rule_thickness;
    let mut out = Vec::new();
    for subpath in &vector.subpaths {
        if subpath.points.len() < 2 {
            continue;
        }
        let Some((left, top, right, bottom)) = bounds_of(vector, subpath, height) else {
            continue;
        };
        let (dx, dy) = (right - left, bottom - top);
        if !(dx.is_finite() && dy.is_finite()) || dx < 0.0 || dy < 0.0 {
            continue;
        }
        if dx.max(dy) < config.min_rule_length {
            continue;
        }
        // A rule is straight within the tolerance, and thin within the
        // thickness. Anything else - a filled cell, a rounded corner drawn as a
        // path - is not a boundary, and treating it as one is how a picture
        // becomes a table.
        let (horizontal, thickness) = if pen {
            if dy <= config.tolerance {
                (true, vector.line_width)
            } else if dx <= config.tolerance {
                (false, vector.line_width)
            } else {
                continue;
            }
        } else if dy <= config.max_rule_thickness {
            (true, dy)
        } else if dx <= config.max_rule_thickness {
            (false, dx)
        } else {
            continue;
        };
        out.push(if horizontal {
            Rule {
                x0: left,
                y0: top,
                x1: right,
                y1: top,
                thickness,
                color,
            }
        } else {
            Rule {
                x0: left,
                y0: top,
                x1: left,
                y1: bottom,
                thickness,
                color,
            }
        });
    }
    out
}

/// A subpath's box in page coordinates, y measured from the top.
///
/// The reader's glyphs measure y from the top of the page; a path arrives in
/// PDF's own space, where y grows upwards. Mixing the two would put every table
/// in the wrong half of the page, silently.
fn bounds_of(vector: &Vector, subpath: &SubPath, height: f64) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in &subpath.points {
        let (x, y) = vector.ctm.apply(*x, *y);
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    Some((min_x, height - max_y, max_x, height - min_y))
}

/// Groups rules that touch into figures.
///
/// A table's cells share their edges, so a table is one component and a chart
/// drawn beside it is another. Comparing every pair would be quadratic in a
/// document that draws a thousand rules, so the rules are bucketed by the
/// tolerance grid and only rules that share a bucket are compared.
///
/// Touching means two things here, and both are needed: two rules that meet at
/// their **ends**, which is what a producer that draws each cell's four edges
/// gives, and two rules that **cross**, which is what a producer that draws a
/// whole column rule beside per-row rules gives. Missing the second kind loses a
/// table silently on a perfectly ordinary document.
fn components(rules: &[Rule], tolerance: f64) -> Vec<Vec<usize>> {
    let cell = tolerance.max(0.1);
    let mut parent: Vec<usize> = (0..rules.len()).collect();
    let mut buckets: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (index, rule) in rules.iter().enumerate() {
        for (x, y) in [(rule.x0, rule.y0), (rule.x1, rule.y1)] {
            let (cx, cy) = ((x / cell).floor() as i64, (y / cell).floor() as i64);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(others) = buckets.get(&(cx + dx, cy + dy)) else {
                        continue;
                    };
                    for &other in others {
                        if touches(&rules[other], x, y, tolerance) {
                            union(&mut parent, index, other);
                        }
                    }
                }
            }
            buckets.entry((cx, cy)).or_default().push(index);
        }
    }
    union_crossings(rules, &mut parent, tolerance);
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..rules.len() {
        groups
            .entry(root(&mut parent, index))
            .or_default()
            .push(index);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    for group in &mut out {
        group.sort_unstable();
    }
    // The figures in the order their rules were drawn, so two runs of the
    // converter agree on the order of two tables on one page.
    out.sort_by_key(|group| group.first().copied().unwrap_or(usize::MAX));
    out
}

/// Unions the rules that cross rather than meet.
///
/// Verticals are sorted by `x`; each horizontal walks only the sliding window of
/// verticals whose `x` falls in `[x0 − tolerance, x1 + tolerance]` (AUD-15).
/// That is O(n log n + k) rather than a pairwise scan of every horizontal against
/// every vertical.
fn union_crossings(rules: &[Rule], parent: &mut [usize], tolerance: f64) {
    let mut verticals: Vec<usize> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| !rule.horizontal())
        .map(|(index, _)| index)
        .collect();
    verticals.sort_by(|&left, &right| {
        rules[left]
            .x0
            .partial_cmp(&rules[right].x0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (index, rule) in rules.iter().enumerate() {
        if !rule.horizontal() {
            continue;
        }
        let lo = verticals.partition_point(|&other| rules[other].x0 < rule.x0 - tolerance);
        for &other in &verticals[lo..] {
            if rules[other].x0 > rule.x1 + tolerance {
                break;
            }
            if crosses(rule, &rules[other], tolerance) {
                union(parent, index, other);
            }
        }
    }
}

/// Whether a horizontal and a vertical rule cross.
///
/// The horizontal stands at `y0` and runs from `x0` to `x1`; the vertical
/// stands at `x0` and runs from `y0` to `y1`. They cross when the point
/// `(vertical.x0, horizontal.y0)` is inside both spans - and *only* then.
/// Comparing the two spans against each other looks like the same test, is not,
/// and links two tables that happen to sit at the same distance from the top and
/// the left.
fn crosses(horizontal: &Rule, vertical: &Rule, tolerance: f64) -> bool {
    horizontal.x0 - tolerance <= vertical.x0
        && vertical.x0 <= horizontal.x1 + tolerance
        && vertical.y0 - tolerance <= horizontal.y0
        && horizontal.y0 <= vertical.y1 + tolerance
}

/// Whether either end of a rule is within `tolerance` of a point.
fn touches(rule: &Rule, x: f64, y: f64, tolerance: f64) -> bool {
    [(rule.x0, rule.y0), (rule.x1, rule.y1)]
        .iter()
        .any(|(rx, ry)| (rx - x).abs() <= tolerance && (ry - y).abs() <= tolerance)
}

fn root(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let (left, right) = (root(parent, left), root(parent, right));
    if left != right {
        // The lower index wins, so the representative does not depend on the
        // order the pairs were compared in.
        parent[left.max(right)] = left.min(right);
    }
}

/// Turns one component of rules into a grid of boundaries.
fn grid_of(
    rules: &[Rule],
    component: &[usize],
    config: &TableRules,
    max_table_cells: usize,
) -> Result<Grid, Rejected> {
    let member: Vec<&Rule> = component.iter().map(|index| &rules[*index]).collect();
    let verticals: Vec<&Rule> = member.iter().copied().filter(|r| !r.horizontal()).collect();
    let horizontals: Vec<&Rule> = member.iter().copied().filter(|r| r.horizontal()).collect();
    if verticals.len() < 2 || horizontals.len() < 2 {
        // One line, or an angle of two, bounds nothing: a rule under a header, a
        // fraction bar, an axis. There is no table here to have failed.
        return Err(Rejected::NotAFigure);
    }
    if verticals.len() < config.min_columns + 1 || horizontals.len() < config.min_rows + 1 {
        return Err(Rejected::TooFewRules {
            verticals: verticals.len(),
            horizontals: horizontals.len(),
        });
    }
    let xs = cluster(
        verticals.iter().map(|rule| rule.x0).collect(),
        config.tolerance,
    );
    let ys = cluster(
        horizontals.iter().map(|rule| rule.y0).collect(),
        config.tolerance,
    );
    if xs.len() < config.min_columns + 1 || ys.len() < config.min_rows + 1 {
        return Err(Rejected::TooFewRules {
            verticals: verticals.len(),
            horizontals: horizontals.len(),
        });
    }
    let columns = xs.len() - 1;
    let rows = ys.len() - 1;
    let cells = columns.saturating_mul(rows);
    if cells > max_table_cells {
        return Err(Rejected::TooManyCells {
            cells,
            limit: max_table_cells,
        });
    }
    let vertical: Vec<Vec<(f64, f64)>> = xs
        .iter()
        .map(|x| {
            merge_intervals(
                verticals
                    .iter()
                    .filter(|rule| (rule.x0 - x).abs() <= config.tolerance)
                    .map(|rule| (rule.y0, rule.y1))
                    .collect(),
                config.tolerance,
            )
        })
        .collect();
    let horizontal: Vec<Vec<(f64, f64)>> = ys
        .iter()
        .map(|y| {
            merge_intervals(
                horizontals
                    .iter()
                    .filter(|rule| (rule.y0 - y).abs() <= config.tolerance)
                    .map(|rule| (rule.x0, rule.x1))
                    .collect(),
                config.tolerance,
            )
        })
        .collect();
    let colors: Vec<[u8; 3]> = member.iter().map(|rule| rule.color).collect();
    let thicknesses: Vec<i64> = member
        .iter()
        .map(|rule| (rule.thickness * 100.0).round() as i64)
        .collect();
    let grid = Grid {
        xs,
        ys,
        vertical,
        horizontal,
        color: common(&colors),
        thickness: mode(&thicknesses).map_or(1.0, hundredths),
    };
    let (last_x, last_y) = (grid.xs.len() - 1, grid.ys.len() - 1);
    if last_x == 0 || last_y == 0 {
        // Every rule of this figure landed on one boundary, so it bounds nothing.
        // Only a caller who asked for no columns and no rows at all can get here,
        // and they get a refusal rather than an empty table.
        return Err(Rejected::TooFewRules {
            verticals: grid.xs.len(),
            horizontals: grid.ys.len(),
        });
    }
    // The outer frame is what makes a grid a region rather than a set of lines.
    let closed = [
        grid.vertical[0].as_slice(),
        grid.vertical[last_x].as_slice(),
    ]
    .iter()
    .all(|intervals| {
        covers(
            intervals,
            grid.ys[0],
            grid.ys[last_y],
            config.min_cover_ratio,
        )
    }) && [
        grid.horizontal[0].as_slice(),
        grid.horizontal[last_y].as_slice(),
    ]
    .iter()
    .all(|intervals| {
        covers(
            intervals,
            grid.xs[0],
            grid.xs[last_x],
            config.min_cover_ratio,
        )
    });
    if !closed {
        return Err(Rejected::OpenFrame);
    }
    Ok(grid)
}

/// Whether a rule runs along a band from `from` to `to` closely enough.
fn covers(intervals: &[(f64, f64)], from: f64, to: f64, ratio: f64) -> bool {
    if to <= from {
        return true;
    }
    let mut total = 0.0;
    for (start, end) in intervals {
        total += (end.min(to) - start.max(from)).max(0.0);
    }
    total >= (to - from) * ratio - f64::EPSILON
}

/// The cells of a grid, with the page's text attached to them.
fn attach(
    grid: &Grid,
    lines: &[GlyphLine],
    config: &TableRules,
    table: usize,
    owner: &mut [Option<usize>],
) -> Result<PlannedTable, Rejected> {
    // Which lines the table owns, and which row band each one falls in. A line
    // on the last rule belongs to the last row: dropping it would lose text,
    // and a lost character is the one failure this whole crate exists to avoid.
    let mut row_of: Vec<Option<usize>> = vec![None; lines.len()];
    let mut first_line = None;
    for (index, line) in lines.iter().enumerate() {
        if line.baseline < grid.top() - config.tolerance
            || line.baseline > grid.bottom() + config.tolerance
        {
            continue;
        }
        // A line that runs across the grid is a paragraph beside it far more
        // often than a cell that overflows, and moving it would be a guess.
        if !within(line, grid.left(), grid.right(), config.tolerance) {
            continue;
        }
        let row = (0..grid.rows())
            .find(|row| line.baseline < grid.ys[row + 1])
            .unwrap_or(grid.rows() - 1);
        row_of[index] = Some(row);
        first_line.get_or_insert(index);
    }
    let first_line = first_line.ok_or(Rejected::NoText)?;
    let mut rows: Vec<PlannedRow> = Vec::with_capacity(grid.rows());
    let mut merged = 0;
    for row in 0..grid.rows() {
        let band = (grid.ys[row], grid.ys[row + 1]);
        let mut cells: Vec<PlannedCell> = Vec::new();
        for column in 0..grid.columns() {
            if column > 0
                && !covers(
                    &grid.vertical[column],
                    band.0,
                    band.1,
                    config.min_cover_ratio,
                )
            {
                // No rule between this column and the last one: they are one
                // cell, and the ink says so.
                if let Some(previous) = cells.last_mut() {
                    previous.span += 1;
                    previous.width = grid.xs[column + 1] - previous.left;
                    merged += 1;
                    continue;
                }
            }
            cells.push(PlannedCell {
                column,
                span: 1,
                merge: None,
                left: grid.xs[column],
                width: grid.xs[column + 1] - grid.xs[column],
                lines: Vec::new(),
            });
        }
        let glyphs = row_glyphs(lines, &row_of, Some(row));
        for cell in &mut cells {
            // Every glyph goes to exactly one cell - the first whose right edge it
            // is left of. Splitting a line that runs across a rule is what keeps
            // a straddling line from losing half of itself.
            let items: Vec<Item> = glyphs
                .iter()
                .filter(|glyph| {
                    cell.left - config.tolerance <= glyph.x && glyph.x < cell.left + cell.width
                })
                .cloned()
                .map(Item::Glyph)
                .collect();
            // The reader's own grouping rule, rather than a second one written
            // here: a cell's text is grouped the way the page's text was.
            cell.lines = strict_ooxml_pdf::text::lines(&items);
        }
        rows.push(PlannedRow {
            height: band.1 - band.0,
            cells,
        });
    }
    // A missing rule under a cell means the cell continues into the row below,
    // and the row below has to agree: the same columns, one cell. Where it does
    // not, the rules and the text describe two different things, and choosing
    // between them is worse than saying the grid was not understood.
    for row in 0..rows.len().saturating_sub(1) {
        for index in 0..rows[row].cells.len() {
            let (column, span, width) = {
                let cell = &rows[row].cells[index];
                (cell.column, cell.span, cell.width)
            };
            if covers(
                &grid.horizontal[row + 1],
                grid.xs[column],
                grid.xs[column] + width,
                config.min_cover_ratio,
            ) {
                continue;
            }
            let continues = rows[row + 1]
                .cells
                .iter()
                .position(|cell| cell.column == column && cell.span == span);
            let Some(next) = continues else {
                return Err(Rejected::DanglingSpan);
            };
            let above = row
                .checked_sub(1)
                .and_then(|above| {
                    rows[above]
                        .cells
                        .iter()
                        .find(|cell| cell.column == column && cell.span == span)
                })
                .is_some_and(|cell| cell.merge.is_some());
            // The cell the rule is missing under is the top of the merged region
            // and the one below it is its continuation; the first cell of a run
            // is the one that says `restart`, exactly as WML spells it.
            rows[row].cells[index].merge = Some(if above {
                VerticalMerge::Continue
            } else {
                VerticalMerge::Restart
            });
            rows[row + 1].cells[next].merge = Some(VerticalMerge::Continue);
            merged += 1;
        }
    }
    let total = rows.iter().flat_map(|row| row.cells.iter()).count();
    let filled = rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| !cell.lines.is_empty())
        .count();
    if filled < config.min_filled_cells {
        return Err(Rejected::NotEnoughText { filled, total });
    }
    for (index, row) in row_of.iter().enumerate() {
        if row.is_some() {
            owner[index] = Some(table);
        }
    }
    Ok(PlannedTable {
        first_line,
        width: grid.right() - grid.left(),
        columns: grid
            .xs
            .windows(2)
            .map(|pair| to_twips(pair[1] - pair[0]))
            .collect(),
        color: grid.color,
        thickness: grid.thickness,
        rows,
        merged,
    })
}

/// The glyphs of one row band, in reading order.
fn row_glyphs(lines: &[GlyphLine], row_of: &[Option<usize>], row: Option<usize>) -> Vec<Glyph> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if row_of.get(index).copied().flatten() != row {
            continue;
        }
        out.extend(line.glyphs.iter().cloned());
    }
    out
}

/// Whether a line's glyphs are all inside a span of x.
fn within(line: &GlyphLine, left: f64, right: f64, tolerance: f64) -> bool {
    line.glyphs
        .iter()
        .all(|glyph| glyph.x >= left - tolerance && glyph.x + glyph.width <= right + tolerance)
}

/// Averages positions that are within `tolerance` of each other.
///
/// The mean, not the first value: a rule drawn twice at 71.999 and 72.001 is one
/// rule, and where it really is lies between them.
fn cluster(mut values: Vec<f64>, tolerance: f64) -> Vec<f64> {
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    // `(mean, count as f64)`: the count is a `f64` so the running mean needs no
    // cast, and a cast here would be a lint this workspace denies.
    let mut out: Vec<(f64, f64)> = Vec::new();
    for value in values {
        match out.last_mut() {
            Some(entry) if value - entry.0 <= tolerance => {
                entry.0 = (entry.0 * entry.1 + value) / (entry.1 + 1.0);
                entry.1 += 1.0;
            }
            _ => out.push((value, 1.0)),
        }
    }
    out.into_iter().map(|(value, _)| value).collect()
}

/// Merges intervals that touch or overlap.
fn merge_intervals(mut intervals: Vec<(f64, f64)>, tolerance: f64) -> Vec<(f64, f64)> {
    intervals.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (start, end) in intervals {
        match out.last_mut() {
            Some(last) if start <= last.1 + tolerance => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// The value that occurs most often.
///
/// Counting by key and breaking ties by the key itself, so the answer does not
/// depend on the order the values arrived in.
fn common(values: &[[u8; 3]]) -> [u8; 3] {
    let mut counts: BTreeMap<[u8; 3], usize> = BTreeMap::new();
    for value in values {
        *counts.entry(*value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| (*count, *value))
        .map_or([0, 0, 0], |(value, _)| value)
}

/// The most frequent of a set of numbers, `None` when there are none.
fn mode(values: &[i64]) -> Option<i64> {
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(*value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| (*count, *value))
        .map(|(value, _)| value)
}

/// A count of hundredths of a point, back in points.
///
/// The count is a `f64` produced by rounding, so it is well inside the range of
/// an `i32`; the conversion is checked rather than cast so a hostile page cannot
/// turn a width into a nonsense one.
fn hundredths(value: i64) -> f64 {
    f64::from(i32::try_from(value).unwrap_or(i32::MAX)) / 100.0
}

/// A measurement in eighths of a point, which is what `w:sz` counts in.
fn eighths_of(points: f64) -> EighthsPoint {
    let eighths = (points * 8.0).round();
    if !eighths.is_finite() {
        return EighthsPoint(2);
    }
    // ISO/IEC 29500-1 ST_EighthPointMeasure: 2..96, half a point to twelve.
    EighthsPoint(eighths.clamp(2.0, 96.0) as u16)
}

/// A colour in the form the model carries.
fn color(rgb: [u8; 3]) -> Color {
    let [red, green, blue] = rgb;
    Color::new(format!("{red:02X}{green:02X}{blue:02X}"))
}

#[cfg(test)]
mod tests {
    use super::{plan, Item, Rgb, TableRules, Vector};
    use strict_ooxml_pdf::content::{Content, Matrix, PageGeometry, RenderMode, SubPath};
    use strict_ooxml_pdf::report::ReadReport;
    use strict_ooxml_pdf::{Glyph, PdfPage};
    use strict_ooxml_wml::model::values::VerticalMerge;

    use crate::report::ConversionReport;
    use crate::Mode;

    /// The page the tests draw on, in points from its top-left corner.
    const HEIGHT: f64 = 792.0;
    const WIDTH: f64 = 612.0;

    /// Column boundaries of the test grid.
    const XS: [f64; 4] = [72.0, 200.0, 330.0, 460.0];
    /// Row boundaries of the test grid.
    const YS: [f64; 3] = [100.0, 130.0, 160.0];

    /// A stroked ruling line, top-left to bottom-right, as the reader sees it.
    fn rule(x0: f64, y0: f64, x1: f64, y1: f64) -> Item {
        Item::Vector(Vector {
            subpaths: vec![SubPath {
                points: vec![(x0, HEIGHT - y0), (x1, HEIGHT - y1)],
                closed: false,
            }],
            fill: None,
            stroke: Some(Rgb(0.0, 0.0, 0.0)),
            line_width: 0.5,
            line_cap: strict_ooxml_pdf::LineCap::Butt,
            line_join: strict_ooxml_pdf::LineJoin::Miter,
            miter_limit: 10.0,
            even_odd: false,
            ctm: Matrix::IDENTITY,
        })
    }

    /// A filled box, the way a producer draws a shaded cell.
    fn box_at(left: f64, top: f64, right: f64, bottom: f64) -> Item {
        Item::Vector(Vector {
            subpaths: vec![SubPath {
                points: vec![
                    (left, HEIGHT - top),
                    (right, HEIGHT - top),
                    (right, HEIGHT - bottom),
                    (left, HEIGHT - bottom),
                ],
                closed: true,
            }],
            fill: Some(Rgb(0.85, 0.88, 0.95)),
            stroke: None,
            line_width: 1.0,
            line_cap: strict_ooxml_pdf::LineCap::Butt,
            line_join: strict_ooxml_pdf::LineJoin::Miter,
            miter_limit: 10.0,
            even_odd: false,
            ctm: Matrix::IDENTITY,
        })
    }

    /// Text on the page, one glyph every six points.
    fn text(value: &str, x: f64, baseline: f64) -> Vec<Item> {
        let mut pen = x;
        value
            .chars()
            .map(|character| {
                let item = Item::Glyph(Glyph {
                    text: character.to_string(),
                    x: pen,
                    y: baseline,
                    width: 6.0,
                    ascent: 8.0,
                    descent: 2.0,
                    size: 11.0,
                    font: "Carlito".to_owned(),
                    color: Rgb(0.0, 0.0, 0.0),
                    render_mode: RenderMode::Fill,
                    mapped: true,
                });
                pen += 6.0;
                item
            })
            .collect()
    }

    /// Every edge of every cell of the test grid, drawn the way a producer draws
    /// a table: one segment per cell edge, so neighbouring cells share a line.
    /// `offset` moves the whole grid down the page.
    fn grid_rules_at(offset: f64) -> Vec<Item> {
        let mut items = Vec::new();
        for row in 0..YS.len() - 1 {
            let (top, bottom) = (YS[row] + offset, YS[row + 1] + offset);
            for column in 0..XS.len() - 1 {
                let (left, right) = (XS[column], XS[column + 1]);
                items.push(rule(left, top, right, top));
                items.push(rule(left, bottom, right, bottom));
                items.push(rule(left, top, left, bottom));
                items.push(rule(right, top, right, bottom));
            }
        }
        items
    }

    /// The test grid, in its place on the page.
    fn grid_rules() -> Vec<Item> {
        grid_rules_at(0.0)
    }

    /// A page of hand-placed items.
    fn page(items: Vec<Item>) -> PdfPage {
        PdfPage {
            number: 1,
            geometry: PageGeometry {
                width: WIDTH,
                height: HEIGHT,
                rotation: 0,
            },
            content: Content {
                items,
                ..Content::default()
            },
            report: ReadReport::new(),
        }
    }

    /// The text of a cell.
    fn cell_text(table: &super::PlannedTable, row: usize, cell: usize) -> String {
        table.rows[row].cells[cell]
            .lines
            .iter()
            .map(strict_ooxml_pdf::GlyphLine::text)
            .collect()
    }

    /// A word in every cell of the test grid, one row per baseline.
    fn grid_text(baselines: &[f64]) -> Vec<Item> {
        let mut items = Vec::new();
        for baseline in baselines {
            for (column, value) in ["A", "B", "C"].into_iter().enumerate() {
                items.extend(text(value, XS[column] + 4.0, *baseline));
            }
        }
        items
    }

    /// The page of the test grid, with a word in every cell.
    fn filled_grid() -> PdfPage {
        let mut items = grid_rules();
        items.extend(grid_text(&[118.0, 148.0]));
        page(items)
    }

    /// Drops every rule whose box matches a predicate, which is how a merged cell
    /// is drawn: the rules that would have been inside it are not drawn.
    fn without(items: Vec<Item>, drop: impl Fn((f64, f64, f64, f64)) -> bool) -> Vec<Item> {
        items
            .into_iter()
            .filter(|item| {
                let Item::Vector(vector) = item else {
                    return true;
                };
                vector.subpaths.first().is_none_or(|subpath| {
                    super::bounds_of(vector, subpath, HEIGHT).is_none_or(|bounds| !drop(bounds))
                })
            })
            .collect()
    }

    /// A grid with a word in every cell is a table.
    #[test]
    fn a_grid_of_rules_becomes_a_table() {
        let page = filled_grid();
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert_eq!(plan.tables.len(), 1, "{report}");
        let table = &plan.tables[0];
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.columns, vec![2560, 2600, 2600]);
        for row in 0..2 {
            for cell in 0..3 {
                assert_eq!(
                    cell_text(table, row, cell),
                    ["A", "B", "C"][cell],
                    "row {row} cell {cell}"
                );
            }
        }
        assert_eq!(report.tables(), 1);
        assert!(report.to_string().contains("table.inferred"), "{report}");
    }

    /// A frame round one paragraph is a frame, and saying so is the point.
    #[test]
    fn a_frame_around_one_paragraph_is_not_a_table() {
        let mut items = vec![
            rule(72.0, 100.0, 460.0, 100.0),
            rule(72.0, 160.0, 460.0, 160.0),
            rule(72.0, 100.0, 72.0, 160.0),
            rule(460.0, 100.0, 460.0, 160.0),
        ];
        items.extend(text("boxed", 90.0, 130.0));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert!(plan.tables.is_empty());
        assert!(!plan.inside_table(0), "the text stays in the page");
        assert!(report.to_string().contains("table.detected"), "{report}");
    }

    /// A line on its own is a rule, not a table - the separator under a running
    /// header is the common case - and a report that fires on every page of a
    /// document with no table in it is a report nobody reads.
    #[test]
    fn a_lone_rule_is_neither_a_table_nor_a_report() {
        let mut items = vec![rule(72.0, 60.0, 540.0, 60.0)];
        items.extend(text("Body", 72.0, 100.0));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert!(plan.tables.is_empty());
        assert!(report.is_lossless(), "{report}");
        assert!(!report.to_string().contains("table."), "{report}");
    }

    /// A grid with nothing written in it is a drawing, and turning it into a
    /// table would lose the drawing.
    #[test]
    fn an_empty_grid_is_not_a_table() {
        let page = page(grid_rules());
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert!(plan.tables.is_empty());
        assert!(
            report.to_string().contains("no text falls inside"),
            "{report}"
        );
    }

    /// A shaded cell is a box, not a rule: a grid drawn only out of filled boxes
    /// with cell-sized sides is not a set of boundaries.
    #[test]
    fn a_filled_box_is_not_a_rule() {
        let items = vec![
            box_at(72.0, 100.0, 200.0, 130.0),
            box_at(200.0, 100.0, 330.0, 130.0),
            box_at(72.0, 130.0, 200.0, 160.0),
            box_at(200.0, 130.0, 330.0, 160.0),
        ];
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert!(plan.tables.is_empty());
    }

    /// A rule that is missing between two columns inside one row is one cell
    /// across them: the ink says so.
    #[test]
    fn a_missing_rule_merges_the_cells_across() {
        // The header row is one cell over all three columns, so the two internal
        // verticals of that band are not drawn.
        let items = without(grid_rules(), |(left, top, right, bottom)| {
            let vertical = (left - right).abs() < f64::EPSILON;
            let band = top > YS[0] - 0.01 && bottom < YS[1] + 0.01;
            let internal = (left - XS[1]).abs() < 0.01 || (left - XS[2]).abs() < 0.01;
            vertical && band && internal
        });
        let mut items = items;
        items.extend(text("Head", XS[0] + 4.0, 118.0));
        items.extend(grid_text(&[148.0, 148.0]));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        let table = &plan.tables[0];
        assert_eq!(table.rows[0].cells.len(), 1, "one cell over three columns");
        assert_eq!(table.rows[0].cells[0].span, 3);
        assert_eq!(cell_text(table, 0, 0), "Head");
        assert_eq!(table.rows[1].cells.len(), 3, "the band below is not merged");
        assert_eq!(table.merged, 2);
    }

    /// A rule that is missing under a cell means the cell continues downwards.
    #[test]
    fn a_missing_rule_merges_the_cells_down() {
        // The first column of the second row is merged into the one above it. A
        // merged cell is drawn as one region: its left and right rules run the
        // whole way down, and the rule that would have been under it is not
        // drawn at all.
        let items = without(grid_rules(), |(left, top, right, bottom)| {
            (top - bottom).abs() < f64::EPSILON
                && (top - YS[1]).abs() < 0.01
                && (left - XS[0]).abs() < 0.01
                && (right - XS[1]).abs() < 0.01
        });
        let mut items = items;
        items.extend(grid_text(&[118.0, 118.0]));
        items.extend(text("D", XS[1] + 4.0, 148.0));
        items.extend(text("E", XS[2] + 4.0, 148.0));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        let table = &plan.tables[0];
        assert_eq!(table.rows[0].cells[0].merge, Some(VerticalMerge::Restart));
        assert_eq!(table.rows[1].cells[0].merge, Some(VerticalMerge::Continue));
        assert!(cell_text(table, 1, 0).is_empty(), "nothing is drawn in it");
        assert_eq!(cell_text(table, 1, 1), "D", "the cells beside it are not");
    }

    /// A line beside the grid is a paragraph, and a line that runs across it is
    /// not a cell that overflowed.
    #[test]
    fn text_outside_the_grid_stays_in_the_page() {
        let mut items = grid_rules();
        items.extend(grid_text(&[118.0, 148.0]));
        items.extend(text("beside", 500.0, 118.0));
        items.extend(text("above", 72.0, 80.0));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert_eq!(plan.tables.len(), 1);
        let owned: Vec<String> = lines
            .iter()
            .enumerate()
            .filter(|(index, _)| plan.inside_table(*index))
            .map(|(_, line)| line.text())
            .collect();
        assert_eq!(owned, vec!["A", "B", "C", "A", "B", "C"]);
    }

    /// A producer that draws its rules *through* each other - a column rule from
    /// above the frame to below it, a row rule from the left of it to the right,
    /// the way a producer avoids leaving a hairline gap at a corner - produces a
    /// figure whose rules cross in the middle and never meet at an end. Missing
    /// that shape loses a table on an ordinary document, and nothing else in the
    /// converter would notice.
    #[test]
    fn rules_that_cross_without_meeting_are_one_figure() {
        let (top, bottom) = (YS[0], YS[YS.len() - 1]);
        let (left, right) = (XS[0], XS[XS.len() - 1]);
        let over = 5.0;
        let mut items = Vec::new();
        for x in XS {
            items.push(rule(x, top - over, x, bottom + over));
        }
        for y in YS {
            items.push(rule(left - over, y, right + over, y));
        }
        items.extend(grid_text(&[118.0, 148.0]));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert_eq!(plan.tables.len(), 1, "{report}");
        let table = &plan.tables[0];
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 3);
        assert_eq!(cell_text(table, 1, 2), "C");
    }

    /// Two grids on one page are two tables, not one grid with a gap in it.
    #[test]
    fn two_grids_are_two_tables() {
        let mut items = grid_rules();
        // A second grid below the first, sharing no point with it.
        items.extend(grid_rules_at(200.0));
        items.extend(grid_text(&[118.0, 148.0, 318.0, 348.0]));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules::default(),
            4000,
            10_000,
            &mut report,
        );
        assert_eq!(plan.tables.len(), 2, "{report}");
        assert_eq!(report.tables(), 2);
        assert!(plan.tables[0].first_line < plan.tables[1].first_line);
    }

    /// The same page read twice gives the same plan: a converter whose output
    /// depends on iteration order is not reproducible, and SC-1 says it must be.
    #[test]
    fn the_plan_is_reproducible() {
        let page = filled_grid();

        let lines = strict_ooxml_pdf::text::lines(page.items());
        let run = || {
            let mut report = ConversionReport::new(Mode::Semantic);
            let plan = plan(
                &page,
                1,
                &lines,
                &TableRules::default(),
                4000,
                10_000,
                &mut report,
            );
            (plan, report.to_string())
        };
        let (first, first_report) = run();
        let (second, second_report) = run();
        assert_eq!(first_report, second_report);
        assert_eq!(
            format!("{first:?}"),
            format!("{second:?}"),
            "two runs must agree on the whole plan"
        );
    }

    /// A caller who asks for no columns and no rows at all still gets a refusal
    /// rather than an empty table - and never a panic, however degenerate the
    /// figure. The thresholds are public, so this is a reachable configuration.
    #[test]
    fn a_caller_who_wants_no_conditions_gets_a_refusal() {
        // A figure whose rules all land on one boundary: two verticals on the same
        // x, with a horizontal either side of them.
        let mut items = vec![
            rule(100.0, 40.0, 100.0, 60.0),
            rule(100.0, 40.0, 100.0, 60.0),
            rule(40.0, 50.0, 100.0, 50.0),
            rule(100.0, 50.0, 200.0, 50.0),
        ];
        items.extend(text("A", 60.0, 50.0));
        let page = page(items);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let plan = plan(
            &page,
            1,
            &lines,
            &TableRules {
                min_columns: 0,
                min_rows: 0,
                min_filled_cells: 0,
                ..TableRules::default()
            },
            4000,
            10_000,
            &mut report,
        );
        assert!(plan.tables.is_empty(), "{}", report);
        assert!(report.to_string().contains("table.detected"), "{report}");
    }

    /// The thresholds are the converter's claims, and a caller that raises the
    /// bar must get fewer tables.
    #[test]
    fn the_table_rules_are_load_bearing() {
        let page = filled_grid();

        let lines = strict_ooxml_pdf::text::lines(page.items());
        let mut report = ConversionReport::new(Mode::Semantic);
        let strict = plan(
            &page,
            1,
            &lines,
            &TableRules {
                min_columns: 4,
                ..TableRules::default()
            },
            4000,
            10_000,
            &mut report,
        );
        assert!(strict.tables.is_empty(), "four columns are not on the page");
        assert!(
            report.to_string().contains("cannot bound a table"),
            "{report}"
        );
    }

    /// Positions closer together than the tolerance are one boundary, and the
    /// boundary is where they average out.
    #[test]
    fn boundaries_a_tolerance_apart_are_one() {
        let clustered = super::cluster(vec![72.0, 72.5, 72.9, 200.0], 1.0);
        assert_eq!(clustered.len(), 2);
        assert!((clustered[0] - 72.466).abs() < 0.01, "{}", clustered[0]);
        assert!((clustered[1] - 200.0).abs() < 1e-9, "{}", clustered[1]);
    }

    /// A rule thinner than a hairline still has to be a legal `w:sz`.
    #[test]
    fn a_border_size_stays_inside_the_schema() {
        assert_eq!(super::eighths_of(0.0).0, 2);
        assert_eq!(super::eighths_of(0.5).0, 4);
        assert_eq!(super::eighths_of(100.0).0, 96);
        assert_eq!(super::eighths_of(f64::NAN).0, 2);
    }
}
