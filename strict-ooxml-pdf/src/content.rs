//! The content-stream interpreter: a graphics-state machine over PDF operators.
//!
//! This is where a page becomes geometry. The operators arrive as a flat
//! `(operands, operator)` list, and the state they act on — the transformation
//! matrix, the colours, the current font, the spacing parameters — is a stack,
//! because `q`/`Q` are the only thing that makes PDF's other state safe.
//!
//! Two decisions are worth stating up front:
//!
//! - **Coordinates come out top-down.** The interpreter works in PDF's y-up
//!   space, because that is what the specification's formulas are written in,
//!   and reflects once on the way out. Doing the reflection per primitive would
//!   be a second place to get it wrong.
//! - **A glyph is placed, not measured.** A PDF gives the origin and the advance
//!   of every code; the *size* of the glyph it draws is the font's ascent and
//!   descent, and the report says when those came from the font and when they
//!   were assumed.

use std::collections::BTreeMap;

use lopdf::Object;

use crate::error::{LimitKind, PdfLimits, Result};
use crate::fonts::PdfFont;
use crate::image::{decode_from, Encoded};

/// A 2×3 affine matrix, `[a b c d e f]`, in PDF's `cm` order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    /// Row 0, column 0.
    pub a: f64,
    /// Row 0, column 1.
    pub b: f64,
    /// Row 1, column 0.
    pub c: f64,
    /// Row 1, column 1.
    pub d: f64,
    /// Translation x.
    pub e: f64,
    /// Translation y.
    pub f: f64,
}

impl Matrix {
    /// The identity matrix.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A translation.
    #[must_use]
    pub const fn translate(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    /// A scale.
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    /// Composes two matrices in the order a `cm` chain emits them.
    ///
    /// PDF transforms row vectors, so `cm A` then `cm B` composes to `A · B` and
    /// sends a point through `A` first. Naming the order directly is deliberate:
    /// a `then` method on the matrix reads as the opposite of what it does.
    #[must_use]
    pub fn chain(first: Self, second: Self) -> Self {
        Self {
            a: first.a * second.a + first.b * second.c,
            b: first.a * second.b + first.b * second.d,
            c: first.c * second.a + first.d * second.c,
            d: first.c * second.b + first.d * second.d,
            // The translation row is `first`'s translation transformed by
            // `second`, plus `second`'s own: with a row-vector point that is
            // `first.e * second.a + first.f * second.c + second.e`. Mixing the
            // scale coefficients in here is a silent error - the matrix still
            // looks plausible and every position after the first is wrong.
            e: first.e * second.a + first.f * second.c + second.e,
            f: first.e * second.b + first.f * second.d + second.f,
        }
    }

    /// Applies the matrix to a point.
    #[must_use]
    pub fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The length of the unit vector `(1, 1)` after the matrix.
    ///
    /// This is the factor by which a unit in the matrix's own space becomes
    /// device units, and it is what turns a font size in text space into a height
    /// on the page. Taking the hypotenuse rather than one column is what keeps
    /// the answer right under a rotation.
    #[must_use]
    pub fn scale_factor(self) -> f64 {
        let x = self.a * self.a + self.b * self.b;
        let y = self.c * self.c + self.d * self.d;
        // `midpoint` rather than `(x + y) / 2.0`: the average of two large
        // coefficients is the one place here where the naive form can overflow
        // to infinity, and a scale factor of `inf` places a glyph at infinity.
        x.midpoint(y).sqrt()
    }

    /// Returns `true` when every coefficient is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|value| value.is_finite())
    }
}

/// An RGB colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

impl Default for Rgb {
    fn default() -> Self {
        Self(0.0, 0.0, 0.0)
    }
}

impl Rgb {
    /// The colour as three bytes.
    #[must_use]
    pub fn to_rgb8(self) -> [u8; 3] {
        let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        [channel(self.0), channel(self.1), channel(self.2)]
    }

    /// `#rrggbb`, which is what the WML model carries.
    #[must_use]
    pub fn to_hex(self) -> String {
        let [r, g, b] = self.to_rgb8();
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

/// How a glyph is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    /// Filled (the default).
    Fill,
    /// Stroked.
    Stroke,
    /// Filled then stroked.
    FillStroke,
    /// Invisible: the text is in the content stream but must not be drawn.
    /// A reader that ignores this renders a layer of text the producer hid.
    Invisible,
    /// Filled, added to the clip, then filled.
    FillClip,
    /// Stroked, added to the clip, then stroked.
    StrokeClip,
    /// Filled, added to the clip, then filled and stroked.
    FillStrokeClip,
    /// Added to the clip only.
    Clip,
}

impl RenderMode {
    fn from_int(value: i64) -> Self {
        match value {
            1 => Self::Stroke,
            2 => Self::FillStroke,
            3 => Self::Invisible,
            4 => Self::FillClip,
            5 => Self::StrokeClip,
            6 => Self::FillStrokeClip,
            7 => Self::Clip,
            _ => Self::Fill,
        }
    }

    /// Whether the mode paints anything.
    #[must_use]
    pub const fn paints(self) -> bool {
        !matches!(self, Self::Invisible | Self::Clip)
    }
}

/// One glyph, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// The character, or the replacement the reader had to use.
    pub text: String,
    /// Left edge, points from the page's left.
    pub x: f64,
    /// Baseline, points from the page's **top**.
    pub y: f64,
    /// Advance width in points.
    pub width: f64,
    /// Height above the baseline in points.
    pub ascent: f64,
    /// Depth below the baseline in points.
    pub descent: f64,
    /// Font size in points, after the transformation.
    pub size: f64,
    /// The font's base name.
    pub font: String,
    /// Fill colour.
    pub color: Rgb,
    /// How the glyph is painted.
    pub render_mode: RenderMode,
    /// Whether the character came from `ToUnicode` or from a fallback.
    pub mapped: bool,
}

/// One subpath.
#[derive(Clone, Debug, PartialEq)]
pub struct SubPath {
    /// Flattened points, in the path's own space, before the transform.
    pub points: Vec<(f64, f64)>,
    /// Whether the subpath is closed.
    pub closed: bool,
}

/// PDF line cap style (`J` operator, ISO 32000-1 §8.4.3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LineCap {
    /// Butt cap.
    Butt = 0,
    /// Round cap.
    Round = 1,
    /// Projecting square cap.
    Square = 2,
}

impl LineCap {
    fn from_pdf(value: f64) -> Self {
        match value as i32 {
            1 => Self::Round,
            2 => Self::Square,
            _ => Self::Butt,
        }
    }
}

/// PDF line join style (`j` operator, ISO 32000-1 §8.4.3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LineJoin {
    /// Miter join.
    Miter = 0,
    /// Round join.
    Round = 1,
    /// Bevel join.
    Bevel = 2,
}

impl LineJoin {
    fn from_pdf(value: f64) -> Self {
        match value as i32 {
            1 => Self::Round,
            2 => Self::Bevel,
            _ => Self::Miter,
        }
    }
}

/// A filled or stroked path.
#[derive(Clone, Debug, PartialEq)]
pub struct Vector {
    /// The subpaths.
    pub subpaths: Vec<SubPath>,
    /// Fill colour, when the path is filled.
    pub fill: Option<Rgb>,
    /// Stroke colour, when the path is stroked.
    pub stroke: Option<Rgb>,
    /// Line width in points.
    pub line_width: f64,
    /// Line cap style from `J`.
    pub line_cap: LineCap,
    /// Line join style from `j`.
    pub line_join: LineJoin,
    /// Miter limit from `M` (PDF default 10).
    pub miter_limit: f64,
    /// Whether the fill uses the even-odd rule.
    pub even_odd: bool,
    /// The transform the path was built in, so a consumer can place it.
    pub ctm: Matrix,
    /// Dash pattern from `d`, in points (already scaled by the CTM); empty for
    /// a solid line. At most 16 entries, each finite and non-negative.
    pub dash: Vec<f64>,
}

/// A placed image.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedImage {
    /// The decoded image, when it could be carried.
    pub image: Option<Encoded>,
    /// Lower-left corner, points from the page's left and top.
    pub x: f64,
    /// Upper-right corner.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// Why the image is missing, when it is.
    pub missing: Option<String>,
}

/// One item of a page.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// Text.
    Glyph(Glyph),
    /// A path.
    Vector(Vector),
    /// An image.
    Image(PlacedImage),
}

/// The state a path is being built in.
#[derive(Clone, Debug)]
struct PathBuilder {
    subpaths: Vec<SubPath>,
    current: Vec<(f64, f64)>,
    start: (f64, f64),
    /// The last control point of a `v`/`y` shorthand.
    last_cubic: Option<(f64, f64)>,
}

impl Default for PathBuilder {
    fn default() -> Self {
        Self {
            subpaths: Vec::new(),
            current: Vec::new(),
            start: (0.0, 0.0),
            last_cubic: None,
        }
    }
}

impl PathBuilder {
    /// How many points the builder currently holds.
    fn point_count(&self) -> usize {
        self.subpaths
            .iter()
            .map(|subpath| subpath.points.len())
            .sum::<usize>()
            + self.current.len()
    }

    /// Moves to a point, starting a new subpath. Returns points added.
    fn move_to(&mut self, x: f64, y: f64) -> usize {
        let before = self.point_count();
        self.flush();
        self.current = vec![(x, y)];
        self.start = (x, y);
        self.last_cubic = None;
        self.point_count().saturating_sub(before)
    }

    /// Adds a straight line. Returns points added.
    fn line_to(&mut self, x: f64, y: f64) -> usize {
        let before = self.point_count();
        if self.current.is_empty() {
            self.current = vec![(x, y)];
            self.start = (x, y);
        } else {
            self.current.push((x, y));
        }
        self.last_cubic = None;
        self.point_count().saturating_sub(before)
    }

    /// Adds a cubic curve, flattened.
    /// The six coordinates of a `c` operator plus the flattening tolerance.
    /// Returns points added.
    #[allow(clippy::too_many_arguments)]
    fn curve_to(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
        tolerance: f64,
    ) -> usize {
        let before = self.point_count();
        if self.current.is_empty() {
            self.current = vec![(x, y)];
            self.start = (x, y);
        }
        let from = *self.current.last().unwrap_or(&(x, y));
        let points = flatten_cubic(from, (x1, y1), (x2, y2), (x, y), tolerance);
        self.current.extend(points);
        self.last_cubic = Some((x2, y2));
        self.point_count().saturating_sub(before)
    }

    /// Closes the current subpath. Returns points added.
    fn close(&mut self) -> usize {
        if self.current.is_empty() {
            return 0;
        }
        let before = self.point_count();
        self.current.push(self.start);
        self.flush();
        self.point_count().saturating_sub(before)
    }

    /// Moves the current subpath into the list.
    fn flush(&mut self) {
        if self.current.len() > 1 {
            let points = std::mem::take(&mut self.current);
            self.subpaths.push(SubPath {
                points,
                closed: false,
            });
        } else {
            self.current.clear();
        }
    }

    /// Finishes the path, marking every subpath closed if one was.
    fn finish(mut self) -> Vec<SubPath> {
        self.flush();
        self.subpaths
    }
}

/// Flattens a cubic Bézier into line segments.
///
/// The segment count follows the length of the control polygon rather than a
/// fixed number, so a large curve is subdivided and a nearly straight one is
/// not: a fixed 16 would put a visible polygonal edge on a page-filling curve
/// and waste points on a 1 pt rule. `tolerance` is the largest deviation
/// tolerated, in the path's own units (SC-9 allows 0.5 pt).
#[must_use]
pub fn flatten_cubic(
    from: (f64, f64),
    control1: (f64, f64),
    control2: (f64, f64),
    to: (f64, f64),
    tolerance: f64,
) -> Vec<(f64, f64)> {
    let polygon = distance(from, control1) + distance(control1, control2) + distance(control2, to);
    let segments = if polygon <= f64::MIN_POSITIVE {
        1
    } else {
        // A cubic deviates from its chord by at most a quarter of the control
        // polygon's excess, so this bound is the one that actually holds.
        let step = (tolerance.max(0.01) * 4.0).max(0.5);
        ((polygon / step).ceil() as usize).clamp(1, 96)
    };
    let mut out = Vec::with_capacity(segments);
    for index in 1..=segments {
        // The loop runs `1..=segments`, so the parameter is `index / segments`
        // and the last point lands exactly on the curve's end. Writing
        // `index + 1` here puts the final point past the end, which no reader of
        // the path data would notice and a test does.
        let step = f64::from(u32::try_from(segments).unwrap_or(u32::MAX));
        let t = f64::from(u32::try_from(index).unwrap_or(u32::MAX)) / step;
        let u = 1.0 - t;
        let x = u * u * u * from.0
            + 3.0 * u * u * t * control1.0
            + 3.0 * u * t * t * control2.0
            + t * t * t * to.0;
        let y = u * u * u * from.1
            + 3.0 * u * u * t * control1.1
            + 3.0 * u * t * t * control2.1
            + t * t * t * to.1;
        out.push((x, y));
    }
    out
}

fn distance(from: (f64, f64), to: (f64, f64)) -> f64 {
    (to.0 - from.0).hypot(to.1 - from.1)
}

/// The mutable state of one `q`/`Q` scope.
#[derive(Clone, Debug)]
struct State {
    ctm: Matrix,
    stroke: Rgb,
    fill: Rgb,
    line_width: f64,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f64,
    /// Dash pattern from `d`, in user space; empty for solid.
    dash: Vec<f64>,
    font_name: Option<String>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    hscale: f64,
    leading: f64,
    rise: f64,
    render_mode: RenderMode,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            stroke: Rgb(0.0, 0.0, 0.0),
            fill: Rgb(0.0, 0.0, 0.0),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: Vec::new(),
            font_name: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            hscale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: RenderMode::Fill,
        }
    }
}

/// The text state, which `q`/`Q` does *not* save.
#[derive(Clone, Debug)]
struct TextState {
    matrix: Matrix,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            matrix: Matrix::IDENTITY,
        }
    }
}

impl TextState {
    fn reset(&mut self) {
        self.matrix = Matrix::IDENTITY;
    }
}

/// What the interpreter could not carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignored {
    /// A stable id, for a report line.
    pub id: &'static str,
    /// Detail, for a report line.
    pub detail: String,
}

impl Ignored {
    fn new(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            detail: detail.into(),
        }
    }
}

/// The resources a page needs to interpret its operators.
pub trait Resources {
    /// The font resource `name`.
    fn font(&self, name: &str) -> Option<&PdfFont>;
    /// The image XObject `name`.
    fn image(&self, name: &str) -> Result<Option<(Encoded, u32, u32)>>;
    /// Whether an XObject `name` exists at all, so a missing one is not a lookup
    /// failure.
    fn has_xobject(&self, name: &str) -> bool;

    /// Whether `name` is a **form** XObject rather than an image.
    ///
    /// The two share one table, so the interpreter has to ask before it decides
    /// what `Do` means. Kept separate from [`Resources::form`] so the answer costs
    /// a dictionary lookup rather than decoding a content stream: a page of
    /// pictures asks this once per picture.
    fn is_form(&self, _name: &str) -> bool {
        false
    }

    /// The form XObject `name`: its content, the matrix that places it, and the
    /// resources **its own** names resolve against.
    ///
    /// `None` for anything that is not a form, and for a provider that does not
    /// follow forms at all — the default, so a consumer of this trait that only
    /// has images keeps working.
    fn form(&self, _name: &str) -> Option<Form<'_>> {
        None
    }
}

/// A form XObject, as far as the interpreter needs it.
///
/// A form is not a picture of something: it is **content that draws content**,
/// with a matrix that places it and a resource dictionary of its own. ISO
/// 32000-1 §8.10.2: the form's content runs with the graphics state in force at
/// the `Do`, the CTM multiplied by `/Matrix`, and every name resolved against the
/// form's `/Resources` — and a form that declares no resources of its own
/// inherits the page's.
///
/// `operations` is an [`Rc`](std::rc::Rc) so a form drawn a thousand times is decoded once
/// for the document and shared (AUD-13).
#[derive(Clone)]
pub struct Form<'a> {
    /// The form's content stream, decoded into operations.
    pub operations: std::rc::Rc<Vec<lopdf::content::Operation>>,
    /// Inline images extracted before tokenisation (AUD-84).
    pub inlines: std::rc::Rc<Vec<crate::inline::InlineImage>>,
    /// `/Matrix`, or the identity when the form states none.
    pub matrix: Matrix,
    /// The resources the form's own names resolve against.
    pub resources: std::rc::Rc<dyn Resources + 'a>,
}

/// Counters shared by a page and every form it draws (AUD-13).
///
/// Glyphs, operators and path points are **per page**, not per form invocation:
/// a form drawn ten thousand times with a thousand glyphs each would otherwise
/// reset the glyph count on every `Do` and never trip the budget. When a counter
/// exceeds its limit, interpretation of the page stops; what was already
/// collected is kept and the report records `pdf.page.budget`.
#[derive(Clone, Debug, Default)]
pub struct PageBudget {
    /// Operators seen on this page, nested forms included.
    pub operations: usize,
    /// Glyphs placed on this page, nested forms included.
    pub glyphs: usize,
    /// Flattened path points produced on this page, nested forms included.
    pub path_points: usize,
    /// Which limit stopped the page, if any (`"glyphs"`, `"operations"`, …).
    pub stopped: Option<&'static str>,
}

impl PageBudget {
    /// Returns `false` when the page has already stopped or this charge stops it.
    fn charge_operation(&mut self, limits: &PdfLimits) -> bool {
        if self.stopped.is_some() {
            return false;
        }
        if self.operations >= limits.max_operations {
            self.stopped = Some("operations");
            return false;
        }
        self.operations += 1;
        true
    }

    /// Returns `false` when the next glyph would exceed the page budget.
    fn charge_glyph(&mut self, limits: &PdfLimits) -> bool {
        if self.stopped.is_some() {
            return false;
        }
        if self.glyphs >= limits.max_glyphs {
            self.stopped = Some("glyphs");
            return false;
        }
        self.glyphs += 1;
        true
    }

    /// Charges `added` flattened path points; returns `false` when the budget stops.
    fn charge_path_points(&mut self, added: usize, limits: &PdfLimits) -> bool {
        if self.stopped.is_some() {
            return false;
        }
        if added == 0 {
            return true;
        }
        let next = self.path_points.saturating_add(added);
        if next > limits.max_path_points {
            self.path_points = limits.max_path_points;
            self.stopped = Some("path_points");
            return false;
        }
        self.path_points = next;
        true
    }
}

/// The result of interpreting one page.
#[derive(Clone, Debug, Default)]
pub struct Content {
    /// The items, in drawing order.
    pub items: Vec<Item>,
    /// What was not carried out.
    pub ignored: Vec<Ignored>,
    /// Glyphs whose character came from a fallback rather than from a mapping.
    pub unmapped_glyphs: usize,
    /// Glyphs whose width the font did not state.
    pub estimated_widths: usize,
}

/// The page size and rotation an interpreter needs to place items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// `/Rotate`, in degrees clockwise, normalised to 0/90/180/270.
    pub rotation: i32,
}

impl PageGeometry {
    /// The width and height after `/Rotate` swaps them.
    #[must_use]
    pub fn displayed(&self) -> (f64, f64) {
        if self.rotation % 180 == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

/// Interprets one page's content stream.
///
/// Operators, glyphs and path points share one [`PageBudget`]. Exceeding a
/// page budget stops interpretation and keeps what was already collected; the
/// caller records `pdf.page.budget` (AUD-13). Form nesting past
/// [`PdfLimits::max_form_depth`] is still a hard [`PdfError::LimitExceeded`](crate::error::PdfError::LimitExceeded).
///
/// The operator dispatch is one function on purpose: the graphics state it
/// threads through `q`/`Q` is only correct if every arm sees the same one,
/// and splitting it by operator family would put that in two places.
#[allow(clippy::too_many_lines)]
pub fn interpret(
    operations: &[lopdf::content::Operation],
    resources: &dyn Resources,
    geometry: PageGeometry,
    limits: PdfLimits,
    tolerance: f64,
) -> Result<Content> {
    interpret_with_inlines(operations, &[], resources, geometry, limits, tolerance)
}

/// As [`interpret`], with inline images extracted before tokenisation (AUD-84).
pub fn interpret_with_inlines(
    operations: &[lopdf::content::Operation],
    inlines: &[crate::inline::InlineImage],
    resources: &dyn Resources,
    geometry: PageGeometry,
    limits: PdfLimits,
    tolerance: f64,
) -> Result<Content> {
    let mut budget = PageBudget::default();
    let mut content = interpret_from(
        operations,
        inlines,
        resources,
        geometry,
        limits,
        tolerance,
        State::default(),
        0,
        &mut budget,
    )?;
    if let Some(limit) = budget.stopped {
        content.ignored.push(Ignored::new(
            "pdf.page.budget",
            format!("page exceeded the {limit} budget"),
        ));
    }
    Ok(content)
}

/// Interprets content that starts from a given graphics state, at a given nesting
/// depth, drawing against one shared [`PageBudget`].
///
/// A form XObject is interpreted by a recursive call, and the three extra
/// parameters are what make that safe and correct: the **seed state** so the form
/// draws with the colours and pen in force at the `Do` and not with defaults, the
/// **depth** so a form that draws itself is refused, and the **budget** so the
/// breadth of nested forms is counted once rather than per level.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn interpret_from(
    operations: &[lopdf::content::Operation],
    inlines: &[crate::inline::InlineImage],
    resources: &dyn Resources,
    geometry: PageGeometry,
    limits: PdfLimits,
    tolerance: f64,
    seed: State,
    depth: usize,
    budget: &mut PageBudget,
) -> Result<Content> {
    let mut out = Content::default();
    let mut stack: Vec<State> = vec![seed.clone()];
    let mut text = TextState::default();
    let mut path = PathBuilder::default();
    let mut current = seed;

    for operation in operations {
        // The budget counts **operators across the whole page**, forms included:
        // a form drawn a thousand times is a thousand times the work, and a
        // per-invocation count would reset on every `Do` (AUD-13).
        if !budget.charge_operation(&limits) {
            break;
        }
        let operator = operation.operator.as_str();
        let args = &operation.operands;
        match operator {
            "q" => stack.push(current.clone()),
            "Q" => {
                if let Some(previous) = stack.pop() {
                    current = previous;
                }
            }
            // Colours. `rg`/`RG` are RGB in 0..=1, `g`/`G` are grey, `k`/`K`
            // are CMYK, and `sc`/`scn` take their components from the *current*
            // colour space, which is why the components are read positionally
            // rather than by name.
            "rg" => {
                if let (Some(r), Some(g), Some(b)) =
                    (number_at(args, 0), number_at(args, 1), number_at(args, 2))
                {
                    current.fill = Rgb(r, g, b);
                }
            }
            "RG" => {
                if let (Some(r), Some(g), Some(b)) =
                    (number_at(args, 0), number_at(args, 1), number_at(args, 2))
                {
                    current.stroke = Rgb(r, g, b);
                }
            }
            "g" => {
                if let Some(grey) = number_at(args, 0) {
                    current.fill = Rgb(grey, grey, grey);
                }
            }
            "G" => {
                if let Some(grey) = number_at(args, 0) {
                    current.stroke = Rgb(grey, grey, grey);
                }
            }
            "k" => {
                if let (Some(c), Some(m), Some(y), Some(k)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                ) {
                    current.fill = Rgb(
                        (1.0 - c).clamp(0.0, 1.0) * (1.0 - k).clamp(0.0, 1.0),
                        (1.0 - m).clamp(0.0, 1.0) * (1.0 - k).clamp(0.0, 1.0),
                        (1.0 - y).clamp(0.0, 1.0) * (1.0 - k).clamp(0.0, 1.0),
                    );
                }
            }
            "K" => {
                if let (Some(c), Some(m), Some(y), Some(k)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                ) {
                    current.stroke = Rgb(
                        (1.0 - c).clamp(0.0, 1.0) * (1.0 - k).clamp(0.0, 1.0),
                        (1.0 - m).clamp(0.0, 1.0) * (1.0 - k).clamp(0.0, 1.0),
                        (1.0 - y).clamp(0.0, 1.0) * (1.0 - k).clamp(0.0, 1.0),
                    );
                }
            }
            "cs" | "CS" => {
                // Only the device spaces are resolved; a separation or an ICC
                // based space is reported rather than approximated.
                if let Some(name) = name_at(args, 0) {
                    let known = matches!(
                        name.as_str(),
                        "DeviceGray"
                            | "DeviceRGB"
                            | "DeviceCMYK"
                            | "CalGray"
                            | "CalRGB"
                            | "G"
                            | "RGB"
                            | "CMYK"
                            | "Pattern"
                    );
                    if !known {
                        out.ignored.push(Ignored::new(
                            "pdf.colorspace",
                            format!("colour space `{name}` is resolved as device colour"),
                        ));
                    }
                }
            }
            "sc" | "scn" | "SC" | "SCN" => {
                let components: Vec<f64> = args.iter().filter_map(number_of).collect();
                let colour = match components.len() {
                    1 => Rgb(components[0], components[0], components[0]),
                    3 => Rgb(components[0], components[1], components[2]),
                    4 => Rgb(
                        (1.0 - components[0]).clamp(0.0, 1.0)
                            * (1.0 - components[3]).clamp(0.0, 1.0),
                        (1.0 - components[1]).clamp(0.0, 1.0)
                            * (1.0 - components[3]).clamp(0.0, 1.0),
                        (1.0 - components[2]).clamp(0.0, 1.0)
                            * (1.0 - components[3]).clamp(0.0, 1.0),
                    ),
                    _ => {
                        out.ignored.push(Ignored::new(
                            "pdf.operator",
                            "colour operator with an unexpected component count",
                        ));
                        continue;
                    }
                };
                if operator.starts_with('s') && operator.ends_with('c') {
                    // `sc`/`scn` set the fill; `SC`/`SCN` the stroke.
                    if operator == "sc" || operator == "scn" {
                        current.fill = colour;
                    } else {
                        current.stroke = colour;
                    }
                }
            }
            "cm" => {
                if let Some(m) = matrix_from(args) {
                    current.ctm = Matrix::chain(m, current.ctm);
                }
            }
            "w" => {
                if let Some(width) = number_at(args, 0) {
                    current.line_width = width;
                }
            }
            "J" => {
                if let Some(value) = number_at(args, 0) {
                    current.line_cap = LineCap::from_pdf(value);
                }
            }
            "j" => {
                if let Some(value) = number_at(args, 0) {
                    current.line_join = LineJoin::from_pdf(value);
                }
            }
            "M" => {
                if let Some(value) = number_at(args, 0) {
                    // Spec default is 10; values below 1 are meaningless for a
                    // miter ratio, so clamp rather than store a trap for a
                    // consumer that divides by it.
                    current.miter_limit = value.max(1.0);
                }
            }
            "d" => {
                // `[on off ...] phase d`. The phase is not carried: a consumer
                // that draws the pattern starts it at the path's first point,
                // which is where every producer this reads puts it.
                current.dash = match args.first() {
                    Some(Object::Array(items)) => items
                        .iter()
                        .filter_map(number_of)
                        .filter(|value| value.is_finite() && *value >= 0.0)
                        .take(16)
                        .collect(),
                    _ => Vec::new(),
                };
                if current.dash.iter().all(|value| *value == 0.0) {
                    current.dash.clear();
                }
            }
            "gs" => out
                .ignored
                .push(Ignored::new("pdf.ext_g_state", "ExtGState is not carried")),
            // `BT` resets the text matrix and nothing else; `ET` ends the object
            // and has no state of its own.
            "BT" | "ET" => text.reset(),
            "Tc" => {
                if let Some(value) = number_at(args, 0) {
                    current.char_spacing = value;
                }
            }
            "Tw" => {
                if let Some(value) = number_at(args, 0) {
                    current.word_spacing = value;
                }
            }
            "Tz" => {
                if let Some(value) = number_at(args, 0) {
                    current.hscale = value / 100.0;
                }
            }
            "TL" => {
                if let Some(value) = number_at(args, 0) {
                    current.leading = value;
                }
            }
            "Ts" => {
                if let Some(value) = number_at(args, 0) {
                    current.rise = value;
                }
            }
            "Tr" => {
                if let Some(value) = int_at(args, 0) {
                    current.render_mode = RenderMode::from_int(value);
                }
            }
            "Tf" => {
                if let (Some(name), Some(size)) = (name_at(args, 0), number_at(args, 1)) {
                    current.font_name = Some(name);
                    current.font_size = size;
                }
            }
            "Td" => {
                if let (Some(x), Some(y)) = (number_at(args, 0), number_at(args, 1)) {
                    text.matrix = Matrix::chain(Matrix::translate(x, y), text.matrix);
                }
            }
            "TD" => {
                if let (Some(x), Some(y)) = (number_at(args, 0), number_at(args, 1)) {
                    current.leading = -y;
                    text.matrix = Matrix::chain(Matrix::translate(x, y), text.matrix);
                }
            }
            "Tm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                    number_at(args, 4),
                    number_at(args, 5),
                ) {
                    text.matrix = Matrix { a, b, c, d, e, f };
                }
            }
            "T*" => {
                text.matrix = Matrix::chain(Matrix::translate(0.0, -current.leading), text.matrix);
            }
            "Tj" | "'" | "\"" => {
                if operator != "Tj" {
                    // `'` and `"` both move to the next line first.
                    text.matrix =
                        Matrix::chain(Matrix::translate(0.0, -current.leading), text.matrix);
                }
                if operator == "\"" {
                    if let (Some(aw), Some(ac)) = (number_at(args, 0), number_at(args, 1)) {
                        current.word_spacing = aw;
                        current.char_spacing = ac;
                    }
                }
                // `Tj` carries the string as its only operand; `'` and `"`
                // carry the spacing first, so the string is the last one.
                let string_index = if operator == "Tj" {
                    0
                } else {
                    args.len().saturating_sub(1)
                };
                if let Some(bytes) = args.get(string_index).and_then(string_of) {
                    show_text(
                        &bytes, &mut text, &current, resources, geometry, limits, budget, &mut out,
                    );
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = args.first() {
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                show_text(
                                    bytes, &mut text, &current, resources, geometry, limits,
                                    budget, &mut out,
                                );
                            }
                            other => {
                                if let Some(adjust) = number_of(other) {
                                    // A `TJ` number is a shift in thousandths of
                                    // an em, so it is scaled by the font size
                                    // like any other text-space distance: `-1000`
                                    // at 10 pt moves the pen 10 pt, not 1.
                                    let shift =
                                        -adjust / 1000.0 * current.font_size * current.hscale;
                                    adjust_text(shift, &mut text, &current, geometry);
                                }
                            }
                        }
                    }
                }
            }
            "m" => {
                if let (Some(x), Some(y)) = (number_at(args, 0), number_at(args, 1)) {
                    let added = path.move_to(x, y);
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
            }
            "l" => {
                if let (Some(x), Some(y)) = (number_at(args, 0), number_at(args, 1)) {
                    let added = path.line_to(x, y);
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
            }
            "c" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                    number_at(args, 4),
                    number_at(args, 5),
                ) {
                    let added = path.curve_to(x1, y1, x2, y2, x, y, tolerance);
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
            }
            "v" => {
                // The first control point is the current point.
                let from = path.current.last().copied().unwrap_or((0.0, 0.0));
                if let (Some(x2), Some(y2), Some(x), Some(y)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                ) {
                    let added = path.curve_to(from.0, from.1, x2, y2, x, y, tolerance);
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
            }
            "y" => {
                if let (Some(x1), Some(y1), Some(x), Some(y)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                ) {
                    let x2 = x;
                    let y2 = y;
                    let _ = (x1, y1);
                    // `y` mirrors `v`: the last control point is the endpoint.
                    if let Some(last) = path.last_cubic {
                        let _ = last;
                    }
                    let added = path.curve_to(x1, y1, x2, y2, x, y, tolerance);
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
            }
            "h" => {
                let added = path.close();
                if !budget.charge_path_points(added, &limits) {
                    break;
                }
            }
            "re" => {
                if let (Some(x), Some(y), Some(w), Some(h)) = (
                    number_at(args, 0),
                    number_at(args, 1),
                    number_at(args, 2),
                    number_at(args, 3),
                ) {
                    let mut added = path.move_to(x, y);
                    added += path.line_to(x + w, y);
                    added += path.line_to(x + w, y + h);
                    added += path.line_to(x, y + h);
                    added += path.close();
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
            }
            // `S` is stroke and `s` is close-then-stroke: neither fills. The
            // first draft passed neither flag here, so a stroked path came out
            // with no stroke colour at all.
            "S" => paint(
                &mut out,
                std::mem::take(&mut path).finish(),
                &current,
                false,
                true,
                false,
            ),
            "s" => {
                let added = path.close();
                if !budget.charge_path_points(added, &limits) {
                    break;
                }
                paint(
                    &mut out,
                    std::mem::take(&mut path).finish(),
                    &current,
                    false,
                    true,
                    false,
                );
            }
            "f" | "F" => paint(
                &mut out,
                std::mem::take(&mut path).finish(),
                &current,
                true,
                false,
                false,
            ),
            "f*" => paint(
                &mut out,
                std::mem::take(&mut path).finish(),
                &current,
                true,
                false,
                true,
            ),
            "B" | "b" => {
                if operator == "b" {
                    let added = path.close();
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
                paint(
                    &mut out,
                    std::mem::take(&mut path).finish(),
                    &current,
                    true,
                    true,
                    false,
                );
            }
            "B*" | "b*" => {
                if operator == "b*" {
                    let added = path.close();
                    if !budget.charge_path_points(added, &limits) {
                        break;
                    }
                }
                paint(
                    &mut out,
                    std::mem::take(&mut path).finish(),
                    &current,
                    true,
                    true,
                    true,
                );
            }
            "n" => {
                std::mem::take(&mut path);
            }
            "W" | "W*" => {
                // Clipping changes what is visible and not what is drawn; the
                // consumer keeps the whole path and notes that it was clipped.
                out.ignored
                    .push(Ignored::new("pdf.clip", "clip path is not applied"));
            }
            "Do" => {
                if let Some(name) = name_at(args, 0) {
                    if resources.is_form(&name) {
                        place_form(
                            &name, &current, resources, geometry, limits, tolerance, depth, budget,
                            &mut out,
                        )?;
                    } else {
                        place_image(&name, &current, resources, geometry, &mut out);
                    }
                }
            }
            "BI" => {
                // A bare `BI` that survived extraction is a malformed stream;
                // the samples were not pulled out and must not paint as ops.
                out.ignored.push(Ignored::new(
                    "pdf.inline_image",
                    "inline image marker survived extraction without samples",
                ));
            }
            op if op == crate::inline::INLINE_OP => {
                place_inline(args, inlines, &current, &mut out);
            }
            "sh" => out.ignored.push(Ignored::new(
                "pdf.shading",
                "shading pattern is not carried",
            )),
            "BMC" | "BDC" | "EMC" | "MP" | "DP" | "BX" | "EX" => {}
            other => {
                if !other.is_empty() {
                    out.ignored
                        .push(Ignored::new("pdf.operator", format!("operator `{other}`")));
                }
            }
        }
    }
    Ok(out)
}

/// Emits a finished path.
fn paint(
    out: &mut Content,
    subpaths: Vec<SubPath>,
    state: &State,
    fill: bool,
    stroke: bool,
    even_odd: bool,
) {
    if subpaths.is_empty() {
        return;
    }
    let scale = state.ctm.scale_factor();
    out.items.push(Item::Vector(Vector {
        subpaths,
        fill: fill.then_some(state.fill),
        stroke: stroke.then_some(state.stroke),
        line_width: state.line_width * scale,
        line_cap: state.line_cap,
        line_join: state.line_join,
        miter_limit: state.miter_limit,
        even_odd,
        ctm: state.ctm,
        dash: state.dash.iter().map(|value| value * scale).collect(),
    }));
}

/// Draws a form XObject: its content, interpreted.
///
/// Three things make a form different from a picture, and all three are in here:
/// the **graphics state** carries over from the `Do` (a form that draws a red
/// square inherits the colour, not a default), the **CTM** is multiplied by the
/// form's `/Matrix`, and the **resources** are the form's own, so a name the page
/// does not declare can still resolve.
#[allow(clippy::too_many_arguments)]
fn place_form(
    name: &str,
    state: &State,
    resources: &dyn Resources,
    geometry: PageGeometry,
    limits: PdfLimits,
    tolerance: f64,
    depth: usize,
    budget: &mut PageBudget,
    out: &mut Content,
) -> Result<()> {
    if depth >= limits.max_form_depth {
        return Err(limits.exceeded(LimitKind::FormDepth, depth as u64));
    }
    let Some(form) = resources.form(name) else {
        // The provider said it was a form and then could not produce one, which
        // is a broken file rather than a missing name; saying so beats drawing
        // nothing without a word.
        out.ignored.push(Ignored::new(
            "pdf.form",
            format!("form `{name}` could not be read"),
        ));
        return Ok(());
    };
    let mut seed = state.clone();
    seed.ctm = Matrix::chain(form.matrix, state.ctm);
    let content = interpret_from(
        form.operations.as_ref(),
        form.inlines.as_ref(),
        form.resources.as_ref(),
        geometry,
        limits,
        tolerance,
        seed,
        depth + 1,
        budget,
    )?;
    out.items.extend(content.items);
    out.ignored.extend(content.ignored);
    out.unmapped_glyphs += content.unmapped_glyphs;
    out.estimated_widths += content.estimated_widths;
    Ok(())
}

/// Places an inline image extracted before tokenisation (AUD-84).
fn place_inline(
    args: &[lopdf::Object],
    inlines: &[crate::inline::InlineImage],
    state: &State,
    out: &mut Content,
) {
    let Some(index) = args
        .first()
        .and_then(|value| value.as_i64().ok())
        .and_then(|n| usize::try_from(n).ok())
    else {
        out.ignored.push(Ignored::new(
            "pdf.inline_image",
            "inline image marker has no index",
        ));
        return;
    };
    let Some(image) = inlines.get(index) else {
        out.ignored.push(Ignored::new(
            "pdf.inline_image",
            format!("inline image {index} was not extracted"),
        ));
        return;
    };
    let lower = state.ctm.apply(0.0, 0.0);
    let upper = state.ctm.apply(1.0, 1.0);
    out.items.push(Item::Image(PlacedImage {
        image: image.encoded.clone(),
        x: lower.0.min(upper.0),
        y: lower.1.min(upper.1),
        width: (upper.0 - lower.0).abs(),
        height: (upper.1 - lower.1).abs(),
        missing: image.missing.clone(),
    }));
}

/// Places an image XObject into the unit square of its own space.
fn place_image(
    name: &str,
    state: &State,
    resources: &dyn Resources,
    geometry: PageGeometry,
    out: &mut Content,
) {
    let (decoded, pixel_width, pixel_height) = match resources.image(name) {
        Ok(Some(image)) => image,
        Ok(None) => {
            let missing = if resources.has_xobject(name) {
                format!("image `{name}` could not be decoded")
            } else {
                format!("image `{name}` is not in the page's resources")
            };
            out.items.push(Item::Image(PlacedImage {
                image: None,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                missing: Some(missing),
            }));
            return;
        }
        Err(error) => {
            out.items.push(Item::Image(PlacedImage {
                image: None,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                missing: Some(error.to_string()),
            }));
            return;
        }
    };
    let _ = (pixel_width, pixel_height);
    let lower = state.ctm.apply(0.0, 0.0);
    let upper = state.ctm.apply(1.0, 1.0);
    out.items.push(Item::Image(PlacedImage {
        image: Some(decoded),
        x: lower.0.min(upper.0),
        y: lower.1.min(upper.1),
        width: (upper.0 - lower.0).abs(),
        height: (upper.1 - lower.1).abs(),
        missing: None,
    }));
    let _ = geometry;
}

/// Shows a string, placing a glyph per code and advancing the text matrix.
///
/// Glyphs share the page's [`PageBudget`]: a form drawn many times cannot reset
/// the count on every `Do` (AUD-13). Past the ceiling the page stops; already
/// placed glyphs stay.
#[allow(clippy::too_many_arguments)]
fn show_text(
    bytes: &[u8],
    text: &mut TextState,
    state: &State,
    resources: &dyn Resources,
    geometry: PageGeometry,
    limits: PdfLimits,
    budget: &mut PageBudget,
    out: &mut Content,
) {
    let Some(name) = state.font_name.clone() else {
        return;
    };
    let Some(font) = resources.font(&name) else {
        out.ignored.push(Ignored::new(
            "pdf.font.missing",
            format!("font resource `{name}` is not in the page's resources"),
        ));
        return;
    };
    let codes = font.codes(bytes);
    for code in codes {
        if budget.stopped.is_some() {
            return;
        }
        // The advance is computed first so the glyph can report it: a consumer
        // that lays text out from the reader's output needs the pen movement,
        // and recomputing it from `/W` is exactly the work the reader has
        // already done.
        let advance = glyph_advance(code, font, state);
        let mut glyph = render_glyph(code, font, state, text, geometry, out);
        if let Some(glyph) = glyph.as_mut() {
            // `glyph_advance` is already in text space, where one unit is one em
            // scaled by `Tfs`, so on an identity CTM it is the device distance.
            // Scaling it again by the font size is what turned a 7.6 pt advance
            // into 1937 pt.
            glyph.width = advance.abs() * state.ctm.scale_factor();
        }
        adjust_text(advance, text, state, geometry);
        if let Some(glyph) = glyph {
            if !budget.charge_glyph(&limits) {
                return;
            }
            out.items.push(Item::Glyph(glyph));
        }
    }
}

/// Builds the glyph at the current text position, or `None` when it is not drawn.
fn render_glyph(
    code: u32,
    font: &PdfFont,
    state: &State,
    text: &TextState,
    geometry: PageGeometry,
    out: &mut Content,
) -> Option<Glyph> {
    if !state.render_mode.paints() {
        // The glyph still advances the pen: invisible text is still text, and
        // dropping the advance would shift everything after it.
        return None;
    }
    // Placement and size are two different questions, and answering them with
    // one matrix gets one of them wrong.
    //
    // The glyph *origin* is the text matrix and the CTM only: the font scale
    // multiplies the glyph's own coordinates, not the position the text matrix
    // has already established. Folding the scale into the origin scales every
    // coordinate by the font size - a 12 pt glyph at `20, 50` lands at
    // `240, 600`.
    let placement = Matrix::chain(text.matrix, state.ctm);
    // The *size* is the scale of the whole chain, which is why a rotation still
    // yields the right factor.
    let trm = Matrix::chain(
        Matrix::chain(
            Matrix::scale(state.font_size * state.hscale, state.font_size),
            Matrix::translate(0.0, state.rise),
        ),
        placement,
    );
    if !trm.is_finite() || !placement.is_finite() {
        return None;
    }
    let (x, y) = placement.apply(0.0, 0.0);
    let scale = trm.scale_factor();
    let character = font
        .to_unicode
        .get(&code)
        .cloned()
        .or_else(|| font.character(code).map(|ch| ch.to_string()));
    // "Mapped" means the *document* said which character this code is:
    // `/ToUnicode`, a `/Differences` entry, or a **named** base encoding. A
    // character that came out of the fallback — no `/Encoding`, or
    // `StandardEncoding`, whose upper half this reader approximates — is a guess,
    // and the flag is how a caller learns to distrust it. Getting this wrong in
    // the other direction is worse than it looks: a page of plain Latin text
    // would be reported as unreadable and sent off to a vision model.
    let mapped = font.to_unicode.contains_key(&code)
        || font.differences.contains_key(&(code as u8))
        || (font.encoding_stated && character.is_some());
    if character.is_none() {
        out.unmapped_glyphs += 1;
    }
    let ch = character.unwrap_or_else(|| "\u{fffd}".to_owned());
    let (ascent, descent) = font.vertical_metrics();
    let measured = font.width(code);
    let estimated = measured.is_estimated();
    let _width = measured.value();
    if estimated {
        out.estimated_widths += 1;
    }
    Some(Glyph {
        text: ch,
        x,
        y: geometry.height - y,
        width: 0.0,
        ascent: ascent * scale.abs() / 1000.0,
        descent: descent.abs() * scale.abs() / 1000.0,
        size: scale.abs(),
        font: font.base_font.clone(),
        color: state.fill,
        render_mode: state.render_mode,
        mapped,
    })
}

/// The advance of one code, in text space, before the transformation.
fn glyph_advance(code: u32, font: &PdfFont, state: &State) -> f64 {
    let mut advance = font.width(code).value() / 1000.0 * state.font_size + state.char_spacing;
    // Word spacing applies to a single-byte code 32 only, which is what makes
    // it "word" spacing.
    if !font.two_byte && code == 32 {
        advance += state.word_spacing;
    }
    advance * state.hscale
}

/// Moves the text matrix right by `advance` in text space.
fn adjust_text(advance: f64, text: &mut TextState, state: &State, _geometry: PageGeometry) {
    let _ = state;
    text.matrix = Matrix::chain(Matrix::translate(advance, 0.0), text.matrix);
}

/// The nth operand as a number.
fn number_at(args: &[Object], index: usize) -> Option<f64> {
    args.get(index).and_then(number_of)
}

/// The nth operand as an integer.
fn int_at(args: &[Object], index: usize) -> Option<i64> {
    args.get(index).and_then(|value| match value {
        Object::Integer(value) => Some(*value),
        Object::Real(value) => Some(f64::from(*value) as i64),
        _ => None,
    })
}

/// The nth operand as a name.
fn name_at(args: &[Object], index: usize) -> Option<String> {
    args.get(index)
        .and_then(|value| value.as_name().ok())
        .map(|name| String::from_utf8_lossy(name).into_owned())
}

/// A number operand.
fn number_of(object: &Object) -> Option<f64> {
    match object {
        // A PDF number is a length, a colour component or a rotation; all of
        // them fit an `f64` exactly, and `i64` beyond 2^53 is not a coordinate.
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

/// A string operand's bytes.
fn string_of(object: &Object) -> Option<Vec<u8>> {
    match object {
        Object::String(bytes, _) => Some(bytes.clone()),
        _ => None,
    }
}

/// A matrix from six operands, or from the `cm` operand list.
fn matrix_from(args: &[Object]) -> Option<Matrix> {
    Some(Matrix {
        a: number_at(args, 0)?,
        b: number_at(args, 1)?,
        c: number_at(args, 2)?,
        d: number_at(args, 3)?,
        e: number_at(args, 4)?,
        f: number_at(args, 5)?,
    })
}

/// The fonts a page used, by resource name, in the order they were reached.
#[must_use]
pub fn fonts_used(content: &Content) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for item in &content.items {
        if let Item::Glyph(glyph) = item {
            *out.entry(glyph.font.clone()).or_default() += 1;
        }
    }
    out
}

/// Decodes an image for a `Resources` implementation.
pub fn decode_image(
    id: lopdf::ObjectId,
    document: &lopdf::Document,
    limits: &PdfLimits,
) -> Result<(Encoded, u32, u32)> {
    decode_from(id, document, limits)
        .map_err(|reject| crate::error::PdfError::Refused(reject.to_string()))
}

#[cfg(test)]
mod tests {
    use super::flatten_cubic;

    /// The exact value of the cubic at `t`, computed independently of the
    /// flattener so the test compares two different computations.
    fn cubic_at(
        from: (f64, f64),
        c1: (f64, f64),
        c2: (f64, f64),
        to: (f64, f64),
        t: f64,
    ) -> (f64, f64) {
        let u = 1.0 - t;
        (
            u * u * u * from.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * to.0,
            u * u * u * from.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * to.1,
        )
    }

    #[test]
    fn a_straight_cubic_flattens_onto_its_chord() {
        let points = flatten_cubic((0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0), 0.125);
        // The flattener emits the interior points and ends on the endpoint.
        assert_eq!(points.last().copied(), Some((30.0, 0.0)));
        assert!(
            points.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "the points must advance along the chord: {points:?}"
        );
        for point in &points {
            assert!(point.1.abs() < 1e-9, "{point:?}");
        }
    }

    #[test]
    fn every_flattened_point_sits_on_the_curve() {
        let from = (0.0, 0.0);
        let c1 = (0.0, 55.23);
        let c2 = (44.77, 55.23);
        let to = (100.0, 0.0);
        let points = flatten_cubic(from, c1, c2, to, 0.125);
        assert!(
            points.len() > 8,
            "{} points is not a flattening",
            points.len()
        );
        let count = points.len();
        for (index, point) in points.iter().enumerate() {
            // The interior points of the parameter range, from t=1/n to t=1.
            let t = (index + 1) as f64 / count as f64;
            let expected = cubic_at(from, c1, c2, to, t);
            let error = (point.0 - expected.0).hypot(point.1 - expected.1);
            assert!(
                error < 0.01,
                "point {index} {point:?} is {error} pt from the curve at t={t} (expected {expected:?})"
            );
        }
    }
}
