//! Mathematical formula layout — `STAGE-5C-TASK.md` §5.2 (S5C.3, S5C.4).
//!
//! [`layout_inline`] and [`layout_display`] turn a formula into a [`MathBox`]:
//! a size triple plus the paint items of the formula, expressed in the
//! formula's own coordinate system (origin at the left end of the baseline,
//! `+y` downwards, as in SVG).
//!
//! The algorithm is a deterministic TeX-style layout: every construct measures
//! its children first, then places them at explicit offsets, so the output is a
//! pure function of the input (ADR-0006). All constants are fractions of the
//! current font size, so a formula scales exactly with the surrounding
//! paragraph.
//!
//! Glyphs come from the bundled OFL math face (STIX Two Math, §9.1); every
//! *stretchy* construct — delimiters and the radical sign — is drawn as a
//! deterministic vector path, so a document still renders with the correct
//! structure when the font has no glyph for it (§9.1/§12 fallback).

use std::cell::Cell;

use strict_ooxml_wml::model::math::{
    Accent, Bar, BorderBox, Boxed, Delimiter, EquationArray, Fraction, Function, GroupCharacter,
    LimitLocation, MathAlignment, MathArgument, MathExpression, MathJustification, MathLimit,
    MathNode, MathParagraph, MathPosition, MathRun, MathRunProperties, MathStyle,
    MathVerticalAlign, Matrix, MatrixColumn, NaryOperator, Phantom, PreScript, Radical,
    SubSuperscript, Subscript, Superscript,
};
use strict_ooxml_wml::model::props::RunProperties;

use super::shapes;
use crate::layout::{Item, LayoutContext, LineItem, PathItem, RectItem, TextItem};
use crate::style::ComputedRun;

/// The math family requested by Word documents, mapped to the bundled face.
pub(crate) const MATH_FAMILY: &str = "Cambria Math";

/// Nominal ascent of the math face Word uses for OMML, in em.
///
/// `map_family` substitutes STIX Two Math for Cambria Math: it covers the OMML
/// glyph repertoire but is *not* metric-compatible with it
/// (`assets/fonts/ATTRIBUTION.md`). Block layout therefore follows the
/// **nominal** font's line metrics — the same substitution principle the text
/// cascade uses when it puts Carlito's advances behind Calibri — so that a
/// formula occupies the same number of lines and the same row pitch as in
/// Word/WPS. Cambria Math's `hhea` table: ascender 1901, descender -483, no
/// line gap, 2048 units/em.
const MATH_ASCENT_EM: f64 = 1901.0 / 2048.0;
/// Nominal descent of the math face Word uses for OMML, in em (positive).
const MATH_DESCENT_EM: f64 = 483.0 / 2048.0;
/// Nominal line height of one formula row, in em.
const MATH_LINE_EM: f64 = MATH_ASCENT_EM + MATH_DESCENT_EM;

/// The display size of a large operator, as `(height, aspect)` in em.
///
/// N-ary operators are set with a fixed display size rather than the text size,
/// which is what makes `∑` and `∫` span their limits.
fn nary_display(character: char) -> (f64, f64) {
    NARY_DISPLAY
        .iter()
        .find(|(candidate, _, _)| *candidate == character)
        .map_or(NARY_DISPLAY_DEFAULT, |(_, height, aspect)| {
            (*height, *aspect)
        })
}
/// Math axis height above the baseline, in em (Word's default).
const AXIS: f64 = 0.25;
/// Rule thickness (fraction bar, overlines, bars), in em.
const RULE: f64 = 0.045;
/// Minimum rule thickness in px so a hairline stays visible.
const MIN_RULE_PX: f64 = 0.6;
/// Horizontal padding inside a fraction, in em.
const FRACTION_PAD: f64 = 0.12;
/// The white a stacked fraction's rule must leave between itself and each part's
/// box, in em.
///
/// The invariant behind the fraction's vertical layout: a part must not reach the
/// rule. It is stated on the **box** (ascent and descent, which are taller than
/// the ink) because the substituted face's ink is not the reference's ink, and a
/// rule about ink would be a rule about the substitution.
///
/// **0.15**, measured: on `06-strict-math-display` the reference leaves 6 px of
/// white between the numerator's last ink row and the rule and 4 px below it, and
/// 0.15 em reproduces both with the bundled face (5 and 4) as well as the
/// fraction's total height, 34 px. TeX's equivalent is `num1 − axis` = 0.282 em,
/// which is **not** usable here: that number assumes Cambria Math's own ascent and
/// descent, and the face standing in for it does not have them, so TeX's constant
/// makes the fraction 8 px taller than the reference on this fixture. The
/// substituted font is the confound, so the constant is measured against the
/// reference rather than inherited from a font we do not have.
const FRACTION_RULE_CLEARANCE: f64 = 0.15;
/// The smallest gap, in px, that a stacked fraction's rule must leave between
/// itself and each part's **box**. The gate for the layout, checked in the tests below.
///
/// 2 px is a hair more than the rule's own thickness at the sizes these fixtures
/// use, which is the smallest separation a reader can still see the rule against — and
/// it is deliberately independent of FRACTION_RULE_CLEARANCE, so that a constant
/// which drifts towards zero fails here instead of quietly shrinking the formula.
const MIN_FRACTION_PART_GAP_PX: f64 = 2.0;
const LINEAR_NUMERATOR_SHIFT: f64 = 0.30;
/// Baseline shift of a linear fraction's denominator, in em.
const LINEAR_DENOMINATOR_SHIFT: f64 = 0.20;
/// Horizontal room the solidus of a linear fraction takes, in em.
const LINEAR_SLASH_GAP: f64 = 0.16;
/// Baseline shift of a first-order superscript, in em.
const SUP_SHIFT: f64 = 0.38;
/// Baseline shift of a first-order subscript, in em.
const SUB_SHIFT: f64 = 0.18;
/// Size ratio of one script level (Word's "professional" style is ≈ 0.62).
const SCRIPT_SCALE: f64 = 0.62;
/// Horizontal gap between a base and its script, in em.
const SCRIPT_GAP: f64 = 0.05;
/// Gap between the radicand and the radical overline, in em.
const RADICAL_GAP: f64 = 0.08;
/// Gap between a delimiter and its content, in em.
const DELIMITER_GAP: f64 = 0.14;
/// Width of a stretchy delimiter relative to its height.
const DELIMITER_WIDTH: f64 = 0.26;
/// Minimum delimiter width in px.
const MIN_DELIMITER_PX: f64 = 3.0;
/// Minimum delimiter height in px.
const MIN_DELIMITER_HEIGHT_PX: f64 = 8.0;
/// Gap between a limit and its base, in em.
const LIMIT_GAP: f64 = 0.12;
/// Gap between an n-ary operator and its operand, in em.
const NARY_GAP: f64 = 0.16;
/// The display size of the large operators Word sets in display math: the
/// height in em and the width/height ratio of the glyph.
///
/// Word picks a *display* cut of the math font for `m:oMathPara`, which is
/// larger than the text cut and keeps each operator's own proportions. The
/// values come from the two reproducible WPS references: `\u{2211}` with
/// `i=1`/`n` limits measures 1.98 × 0.66 em and `\u{222B}` with `a`/`b` limits
/// 2.18 × 0.28 em; the sum-like and integral-like families are kept apart.
const NARY_DISPLAY: &[(char, f64, f64)] = &[
    ('\u{2211}', 1.98, 0.66),
    ('\u{220F}', 1.98, 0.90),
    ('\u{2210}', 1.98, 0.90),
    ('\u{222B}', 2.18, 0.28),
    ('\u{222C}', 2.18, 0.55),
    ('\u{222D}', 2.18, 0.80),
    ('\u{222E}', 2.18, 0.28),
    ('\u{22C3}', 1.98, 0.90),
    ('\u{22C2}', 1.98, 0.90),
    ('\u{22C1}', 1.98, 0.90),
    ('\u{22C0}', 1.98, 0.90),
];

/// Fallback display height (in em) and aspect of a large operator whose family
/// is not tabulated above.
const NARY_DISPLAY_DEFAULT: (f64, f64) = (1.98, 0.66);
/// Gap between a function name and its argument, in em.
const FUNCTION_GAP: f64 = 0.12;
/// Gap between matrix columns, in em.
const COLUMN_GAP: f64 = 0.55;
/// Gap between an accent and its base, in em.
const ACCENT_GAP: f64 = 0.02;
/// Gap between a group character and its base, in em.
const GROUP_GAP: f64 = 0.06;
/// Gap between a bar and its base, in em.
const BAR_GAP: f64 = 0.08;
/// Inner padding of a box, in em.
const BOX_PAD: f64 = 0.15;
/// Gap between the arguments of a bordered box, in em.
const BORDER_BOX_GAP: f64 = 0.25;
/// Gap between the arguments of a delimiter, in em.
const SEPARATOR_GAP: f64 = 0.12;

// ---------------------------------------------------------------------------
// Inter-atom spacing (ECMA-376 Part 1 §22.1.2, the math spacing table).
//
// Adjacent children of an `m:oMath` used to be joined with no space at all:
// `out.append(child)` puts each one exactly at the previous one's advance
// width. For two text runs that happens to look acceptable, because a glyph
// sits inside its advance, but any construct with real extent has ink at that
// boundary. In a real document `H_q = (1/(1-q)) ln(Σ p_i^q_i)` put the `ln`
// flush against the end of the fraction rule — measured 139.832 against
// 139.832, a zero-pixel gap, which is what the visual viewer was built to
// find and did find.
//
// # `verified = false`
//
// The *widths* below are Word's (TeX's) values, but they are **not confirmed
// against a reference here**, and the reason is worth stating rather than
// hiding: the WPS references are Cambria Math and this renderer draws STIX Two
// Math, so a gap measured in a reference is the sum of the layout's spacing and
// the difference between two typefaces. The same confounded measurement is
// what makes the Stage-5C display-block height unfixable (docs/
// stage-5c-report.md §4.2).
//
// This mirrors the `verified` flag on `normalize::tables::RENAMES`: applied
// because the specification requires it, flagged because the evidence for the
// constant is the specification and not a measurement. Confirming it needs a
// metric-compatible reference, which is the same prerequisite.
// ---------------------------------------------------------------------------

/// The widths Word's math layout uses, in em: thin, medium and thick.
const ATOM_GAP_THIN: f64 = 3.0 / 18.0;
const ATOM_GAP_MEDIUM: f64 = 4.0 / 18.0;
const ATOM_GAP_THICK: f64 = 5.0 / 18.0;

/// The spacing class of one atom in a formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Atom {
    /// Plain text and any boxed construct: fractions, scripts, radicals,
    /// delimiters, matrices, boxes.
    Ordinary,
    /// A large operator or a function name.
    Operator,
    /// A binary operator: `+`, `-`, `×`, `÷`, `∪`, …
    Binary,
    /// A relation: `=`, `<`, `>`, `≤`, `≈`, `→`, …
    Relation,
}

/// Characters that make a whole run a binary operator.
const BINARY_CHARS: &str = "+-−*×÷±∓∪∩∖⊕⊗∝≺⋃⨁";
/// Characters that make a whole run a relation.
const RELATION_CHARS: &str = "=<>≤≥≠≈≡∼≅→←↔⇒⇔∝∈∉⊂⊃⊆⊇≪≫";
/// Characters that make a whole run a large operator or a function name.
///
/// The Latin names matter as much as the symbols: `ln`, `log`, `sin` and the
/// rest are operators for spacing purposes, which is exactly why a function
/// name needs a thin space in front of it and a fraction does not.
const OPERATOR_CHARS: &str = "∑∏∐∫∬∭∮∂∇√limlnloglgmaxminsupinfargdetexpdimhomkerdegmodPr";

/// Classifies a run by the characters it consists of.
///
/// A run that mixes classes — `x=` typed as one `m:r`, or a variable named
/// with a minus sign — is `Ordinary`: the spacing table has no entry for a
/// mixed atom, and the conservative reading of "unknown" is "no extra space".
fn classify_run(text: &str) -> Atom {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Atom::Ordinary;
    }
    let all = |set: &str| trimmed.chars().all(|c| set.contains(c));
    if all(RELATION_CHARS) {
        Atom::Relation
    } else if all(BINARY_CHARS) {
        Atom::Binary
    } else if all(OPERATOR_CHARS) {
        Atom::Operator
    } else {
        Atom::Ordinary
    }
}

/// Classifies a node for spacing purposes.
fn classify(node: &MathNode) -> Atom {
    match node {
        MathNode::Run(run) => classify_run(&run.text),
        // A large operator is an operator; every other construct is boxed and
        // reads as ordinary.
        MathNode::NaryOperator(_) => Atom::Operator,
        // The class comes from the function's *name*, which is what carries the
        // operator semantics: `ln` is an operator, its argument is ordinary.
        MathNode::Function(function) => classify_argument(&function.name),
        _ => Atom::Ordinary,
    }
}

/// The class of an `m:arg` whose contents decide it, for a function name.
fn classify_argument(argument: &MathArgument) -> Atom {
    let mut nodes = argument.nodes.iter();
    let Some(MathNode::Run(run)) = nodes.next() else {
        return Atom::Ordinary;
    };
    if nodes.next().is_some() {
        return Atom::Ordinary;
    }
    classify_run(&run.text)
}

/// The space between two adjacent atoms, in em.
///
/// Asymmetric classes take the wider of the two sides — a relation wants a
/// thick space whichever side it is on — and an operator binds to its
/// neighbour on the outside only, so `a + b` is not `a + + b`.
fn atom_gap(left: Atom, right: Atom) -> f64 {
    let side = |a: Atom, b: Atom| match (a, b) {
        (Atom::Relation, _) | (_, Atom::Relation) => ATOM_GAP_THICK,
        (Atom::Binary, _) | (_, Atom::Binary) => ATOM_GAP_MEDIUM,
        (Atom::Operator, _) | (_, Atom::Operator) => ATOM_GAP_THIN,
        (Atom::Ordinary, Atom::Ordinary) => 0.0,
    };
    match (left, right) {
        // An operator binds to its neighbour on the outside only, so a run of
        // operators stays tight: `a + b` is not `a + + b`.
        (Atom::Binary, Atom::Binary) | (Atom::Operator, Atom::Operator | Atom::Binary) => 0.0,
        (Atom::Binary, Atom::Operator) => ATOM_GAP_MEDIUM,
        _ => side(right, left),
    }
}

/// Hard cap on the paint items one formula may produce (`STAGE-5C-TASK.md` §5.3).
const MAX_ITEMS: usize = 20_000;

/// Geometry of a laid-out construct, relative to its own baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Size {
    /// Advance width in px.
    pub width: f64,
    /// Extent above the baseline in px.
    pub height: f64,
    /// Extent below the baseline in px.
    pub depth: f64,
}

impl Size {
    /// The empty size.
    const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
        depth: 0.0,
    };

    /// A finite size triple (non-finite input becomes `0`).
    fn new(width: f64, height: f64, depth: f64) -> Self {
        Self {
            width: crate::units::finite(width),
            height: crate::units::finite(height),
            depth: crate::units::finite(depth),
        }
    }
}

/// A laid-out construct: its size plus the paint items of its ink.
#[derive(Clone, Debug, Default)]
pub(crate) struct MathBox {
    /// Geometry relative to the construct's baseline.
    pub size: Size,
    /// Paint items in the construct's coordinate system.
    pub items: Vec<Item>,
}

impl MathBox {
    /// The empty construct: no ink, no size.
    const fn empty() -> Self {
        Self {
            size: Size::ZERO,
            items: Vec::new(),
        }
    }

    /// A construct of an explicit size and no ink.
    fn blank(size: Size) -> Self {
        Self {
            size: Size::new(size.width, size.height, size.depth),
            items: Vec::new(),
        }
    }

    /// Places `child` at `(dx, dy)` relative to this construct's baseline.
    fn place(&mut self, child: MathBox, dx: f64, dy: f64) {
        self.size = Size::new(
            self.size.width.max(dx + child.size.width),
            self.size.height.max(child.size.height - dy),
            self.size.depth.max(child.size.depth + dy),
        );
        for item in child.items {
            self.items.push(translate(item, dx, dy));
        }
    }

    /// Returns `true` when the construct has no ink and no size.
    fn is_empty(&self) -> bool {
        self.items.is_empty() && self.size == Size::ZERO
    }

    /// Places `child` directly to the right of the current ink.
    fn append(&mut self, child: MathBox) {
        let dx = self.size.width;
        self.place(child, dx, 0.0);
    }

    /// Places `child` to the right after a `gap` px.
    fn append_gap(&mut self, child: MathBox, gap: f64) {
        let dx = self.size.width + gap;
        self.place(child, dx, 0.0);
    }

    /// Returns a copy shifted by `(dx, dy)`.
    fn shifted(self, dx: f64, dy: f64) -> Self {
        let items = self
            .items
            .into_iter()
            .map(|item| translate(item, dx, dy))
            .collect();
        Self {
            size: self.size,
            items,
        }
    }
}

/// Translates a paint item by `(dx, dy)`.
fn translate(item: Item, dx: f64, dy: f64) -> Item {
    let dx = crate::units::finite(dx);
    let dy = crate::units::finite(dy);
    match item {
        Item::Text(mut text) => {
            text.x += dx;
            text.baseline += dy;
            Item::Text(text)
        }
        Item::Rect(mut rect) => {
            rect.x += dx;
            rect.y += dy;
            Item::Rect(rect)
        }
        Item::Line(mut line) => {
            line.x1 += dx;
            line.y1 += dy;
            line.x2 += dx;
            line.y2 += dy;
            Item::Line(line)
        }
        Item::Path(mut path) => {
            path.x += dx;
            path.y += dy;
            Item::Path(path)
        }
        Item::Image(mut image) => {
            image.x += dx;
            image.y += dy;
            Item::Image(image)
        }
    }
}

/// The layout state inherited down the formula tree.
#[derive(Clone, Copy)]
struct Frame<'a, 'b> {
    ctx: &'b LayoutContext<'a>,
    /// Current font size in px.
    size_px: f64,
    /// Script level (0 = the formula's base level).
    level: u8,
    /// Run format inherited from the paragraph (colour, weight, highlight).
    run: &'b ComputedRun,
    /// Whether the formula belongs to a display (`m:oMathPara`) paragraph.
    display: bool,
    /// Remaining item budget, shared by the whole formula.
    budget: &'b Cell<usize>,
}

impl Frame<'_, '_> {
    /// The math axis height above the baseline, in px.
    fn axis(&self) -> f64 {
        AXIS * self.size_px
    }

    /// The current em, in px.
    fn em(&self) -> f64 {
        self.size_px
    }

    /// The distance between two consecutive formula-row baselines, in px.
    fn row_pitch(&self) -> f64 {
        MATH_LINE_EM * self.size_px
    }

    /// The distance from a formula row's top edge to its baseline, in px.
    fn row_ascent(&self) -> f64 {
        MATH_ASCENT_EM * self.size_px
    }

    /// Returns a frame for the next script level.
    fn script(&self) -> Self {
        Self {
            size_px: self.size_px * SCRIPT_SCALE,
            level: self.level.saturating_add(1),
            ..*self
        }
    }

    /// The colour of the ink.
    fn color(&self) -> String {
        self.run
            .color
            .clone()
            .unwrap_or_else(|| "#000000".to_owned())
    }

    /// The rule thickness in px for the current level.
    fn rule_thickness(&self) -> f64 {
        (RULE * self.size_px).max(MIN_RULE_PX)
    }

    /// Converts a point-denominated measure (`ST_PointMeasure`) to px.
    ///
    /// Only `m:boxPr/m:sp` and `m:borderBoxPr/m:sp` are points; the matrix and
    /// equation-array spacings are twips — see [`Self::twips`].
    fn points(&self, points: i32) -> f64 {
        self.ctx.size_px(f64::from(points))
    }

    /// Converts a twips-denominated measure (`ST_UnsignedInteger`) to px.
    ///
    /// `m:cSp`, `m:cGp` and `m:rSp` carry a twip count, not points: Word
    /// writes `m:rSp w:val="120"` for 120 twips (8 px), and treating the
    /// value as points inflated a two-row `m:eqArr` to a 160 px row gap.
    fn twips(&self, twips: i32) -> f64 {
        crate::units::twips_to_px(twips, self.ctx.options.scale)
    }

    /// Builds a run format for a math text run.
    ///
    /// STIX Two Math has a single upright face, so `m:sty` reaches the output
    /// only as `font-style`/`font-weight` on the `<text>` element; the advances
    /// are identical across styles (§9.1).
    fn math_run(&self, style: MathStyle) -> ComputedRun {
        let mut run = self.run.clone();
        MATH_FAMILY.clone_into(&mut run.family);
        run.size_pt = crate::units::PT_PER_INCH * self.size_px / self.ctx.options.scale;
        run.highlight = None;
        run.underline = false;
        run.strike = false;
        run.caps = false;
        run.vert_align = strict_ooxml_wml::model::values::VertAlign::Baseline;
        run.italic = matches!(style, MathStyle::Italic | MathStyle::BoldItalic);
        run.bold = matches!(style, MathStyle::Bold | MathStyle::BoldItalic);
        run
    }

    /// Measures `text` in the current face and size.
    fn text_size(&self, text: &str, run: &ComputedRun) -> Size {
        let width: f64 = text
            .chars()
            .map(|ch| {
                self.ctx
                    .font
                    .advance_em(MATH_FAMILY, ch, run.bold, run.italic)
                    * self.size_px
            })
            .sum();
        let metrics = self.ctx.font.metrics(MATH_FAMILY, run.bold, run.italic);
        Size::new(
            width,
            metrics.ascent_em() * self.size_px,
            metrics.descender.max(0.0) * self.size_px,
        )
    }

    /// Charges one item against the budget, returning it only if it fits.
    fn charge(&self, item: Item) -> Option<Item> {
        let remaining = self.budget.get();
        if remaining == 0 {
            return None;
        }
        self.budget.set(remaining - 1);
        Some(item)
    }

    /// Emits a text run at `(x, y)` (the baseline of the glyphs).
    fn text(&self, x: f64, y: f64, text: &str, run: ComputedRun) -> MathBox {
        let size = self.text_size(text, &run);
        let width = size.width;
        let item = self.charge(Item::Text(TextItem {
            x: crate::units::finite(x),
            baseline: crate::units::finite(y),
            width: crate::units::finite(width),
            text: text.to_owned(),
            run,
            size_px: self.size_px,
            field: None,
        }));
        MathBox {
            size,
            items: item.into_iter().collect(),
        }
    }

    /// Emits a horizontal rule of the current thickness and colour.
    fn rule(&self, x: f64, y: f64, width: f64) -> Option<Item> {
        self.charge(Item::Line(LineItem {
            x1: crate::units::finite(x),
            y1: crate::units::finite(y),
            x2: crate::units::finite(x + width),
            y2: crate::units::finite(y),
            color: self.color(),
            width: self.rule_thickness(),
            dashed: false,
        }))
    }

    /// Emits a stroked path in a local `width × height` box placed at `(x, y)`.
    fn path(&self, x: f64, y: f64, width: f64, height: f64, d: String) -> Option<Item> {
        self.charge(Item::Path(PathItem {
            x: crate::units::finite(x),
            y: crate::units::finite(y),
            w: crate::units::finite(width),
            h: crate::units::finite(height),
            d,
            fill: None,
            stroke: Some(self.color()),
            stroke_w: self.rule_thickness(),
            dash: None,
            rotate_deg: 0.0,
            flip_h: false,
            flip_v: false,
        }))
    }

    /// Emits a stroked line from `(x1, y1)` to `(x2, y2)`.
    fn segment(&self, x1: f64, y1: f64, x2: f64, y2: f64) -> Option<Item> {
        self.charge(Item::Line(LineItem {
            x1: crate::units::finite(x1),
            y1: crate::units::finite(y1),
            x2: crate::units::finite(x2),
            y2: crate::units::finite(y2),
            color: self.color(),
            width: self.rule_thickness(),
            dashed: false,
        }))
    }
}

/// Builds the base frame for a formula.
fn base_frame<'a, 'b>(
    ctx: &'b LayoutContext<'a>,
    run: &'b ComputedRun,
    budget: &'b Cell<usize>,
    display: bool,
) -> Frame<'a, 'b> {
    Frame {
        ctx,
        size_px: ctx.size_px(run.size_pt),
        level: 0,
        run,
        display,
        budget,
    }
}

/// Lays out an inline formula (`m:oMath`) on the paragraph's baseline.
pub(crate) fn layout_inline(
    ctx: &LayoutContext<'_>,
    expression: &MathExpression,
    run: &ComputedRun,
) -> MathBox {
    let budget = Cell::new(MAX_ITEMS);
    layout_nodes(&base_frame(ctx, run, &budget, false), &expression.nodes)
}

/// Lays out a display formula (`m:oMathPara`), placing it in the content box.
///
/// `content_left`/`content_width` are the text column, so the returned items
/// are already positioned in page coordinates relative to the block's top-left
/// corner's x.
pub(crate) fn layout_display(
    ctx: &LayoutContext<'_>,
    paragraph: &MathParagraph,
    run: &ComputedRun,
    content_left: f64,
    content_width: f64,
) -> MathBox {
    let budget = Cell::new(MAX_ITEMS);
    let boxed = layout_nodes(
        &base_frame(ctx, run, &budget, true),
        &paragraph.expression.nodes,
    );
    // `m:oMathParaPr/m:jc` places the display formula in the text column.
    let justification = paragraph
        .properties
        .as_ref()
        .and_then(|properties| properties.justification)
        .unwrap_or(MathJustification::Default);
    let available = crate::units::finite(content_width);
    let shift = match justification {
        MathJustification::Left | MathJustification::Default => 0.0,
        MathJustification::Right => (available - boxed.size.width).max(0.0),
        MathJustification::Center | MathJustification::CenterGroup => {
            (available - boxed.size.width).max(0.0) / 2.0
        }
    };
    boxed.shifted(crate::units::finite(content_left) + shift, 0.0)
}

/// Lays out a list of sibling nodes as one horizontal list.
fn layout_nodes(frame: &Frame<'_, '_>, nodes: &[MathNode]) -> MathBox {
    let mut out = MathBox::empty();
    let mut previous: Option<Atom> = None;
    for node in nodes {
        // Siblings are separated by the spacing table, not by each one's ink
        // extent: a fraction's box ends at its rule, and a function name
        // starting exactly there touches it.
        if let Some(previous) = previous {
            let gap = atom_gap(previous, classify(node)) * frame.em();
            if gap > 0.0 {
                out.append_gap(MathBox::empty(), gap);
            }
        }
        previous = Some(classify(node));
        out.append(layout_node(frame, node));
    }
    out
}

/// Lays out an argument (`m:e` and its `m:argPr`).
fn layout_argument(frame: &Frame<'_, '_>, argument: &MathArgument) -> MathBox {
    let mut boxed = layout_nodes(frame, &argument.nodes);
    if let Some(control) = argument.properties.control.as_deref() {
        apply_control(&mut boxed, control);
    }
    boxed
}

/// Applies `m:ctrlPr/w:rPr` colours to the ink of a construct.
fn apply_control(boxed: &mut MathBox, control: &RunProperties) {
    let Some(color) = control
        .color
        .as_ref()
        .map(|color| format!("#{}", color.as_str()))
    else {
        return;
    };
    for item in &mut boxed.items {
        match item {
            Item::Text(text) => text.run.color = Some(color.clone()),
            Item::Line(line) => line.color.clone_from(&color),
            Item::Path(path) => path.stroke = Some(color.clone()),
            Item::Rect(rect) => rect.stroke = Some(color.clone()),
            Item::Image(_) => {}
        }
    }
}

/// Lays out one math node.
fn layout_node(frame: &Frame<'_, '_>, node: &MathNode) -> MathBox {
    match node {
        MathNode::Run(run) => layout_run(frame, run),
        MathNode::Fraction(node) => layout_fraction(frame, node),
        MathNode::Radical(node) => layout_radical(frame, node),
        MathNode::Superscript(node) => layout_superscript(frame, node),
        MathNode::Subscript(node) => layout_subscript(frame, node),
        MathNode::SubSuperscript(node) => layout_sub_superscript(frame, node),
        MathNode::PreScript(node) => layout_pre_script(frame, node),
        MathNode::NaryOperator(node) => layout_nary(frame, node),
        MathNode::Delimiter(node) => layout_delimiter(frame, node),
        MathNode::Function(node) => layout_function(frame, node),
        MathNode::Limit(node) => layout_limit(frame, node),
        MathNode::Matrix(node) => layout_matrix(frame, node),
        MathNode::EquationArray(node) => layout_equation_array(frame, node),
        MathNode::Accent(node) => layout_accent(frame, node),
        MathNode::Bar(node) => layout_bar(frame, node),
        MathNode::GroupCharacter(node) => layout_group_character(frame, node),
        MathNode::Boxed(node) => layout_box(frame, node),
        MathNode::BorderBox(node) => layout_border_box(frame, node),
        MathNode::Phantom(node) => layout_phantom(frame, node),
        // Unmodelled constructs draw nothing; the parser already reported them.
        MathNode::Unknown(_) => MathBox::empty(),
    }
}

/// Lays out a math run (`m:r`).
fn layout_run(frame: &Frame<'_, '_>, run: &MathRun) -> MathBox {
    if run.text.is_empty() {
        return MathBox::empty();
    }
    let style = effective_style(&run.properties);
    let mut boxed = frame.text(0.0, 0.0, &run.text, frame.math_run(style));
    if let Some(control) = run.properties.control.as_deref() {
        apply_control(&mut boxed, control);
    }
    boxed
}

/// Returns the effective `m:sty` of a run: `m:nor` and `m:lit` override the
/// default mathematical italic.
fn effective_style(properties: &MathRunProperties) -> MathStyle {
    if properties.normal || properties.literal {
        return MathStyle::Plain;
    }
    properties.style.unwrap_or_default()
}

/// Where a stacked fraction's two baselines go, and why.
///
/// Split out from [`layout_fraction`] so the **invariant** can be tested without a
/// rasterizer, a font or a document: given the parts' boxes and the em, the
/// numerator's baseline is `-(rule_room + numerator.depth)` and the denominator's
/// is `+(rule_room + denominator.height)`, where `rule_room` is the room from the
/// formula's baseline to the rule's far edge plus [`FRACTION_RULE_CLEARANCE`].
///
/// The point of carrying each part's **own** extent is the thing a symmetric shift
/// cannot do: a denominator's box rises by its *ascent*, a numerator's by its
/// *descent*, so one shift cannot clear both by the same amount, and at the old
/// 0.40 em neither cleared the rule at all.
///
/// **`axis` is not a parameter, on purpose.** `Frame::axis()` returns *pixels* and
/// `AXIS` returns *em*; taking the frame's value here and multiplying it by the em
/// again made every fraction 50 px taller, which the fidelity gate caught and this
/// function's own tests did not, because the tests pass the constant. The
/// typographic axis is a property of mathematics, not of the frame.
fn fraction_part_placements(
    numerator: &MathBox,
    denominator: &MathBox,
    em: f64,
    rule: f64,
    shrink: f64,
) -> (f64, f64) {
    let rule_room = (FRACTION_RULE_CLEARANCE + AXIS) * em * shrink + rule / 2.0;
    // The clearance never goes below `MIN_FRACTION_PART_GAP_PX`: a 6 pt
    // numerator has no room for the nominal clearance, and a subscript of a
    // subscript has less still. This was a `debug_assert!`, which said the two
    // numbers cannot happen - and they can (AUD-09).
    let rule_room = rule_room.max(MIN_FRACTION_PART_GAP_PX + rule / 2.0);
    (
        -(rule_room + numerator.size.depth),
        rule_room + denominator.size.height,
    )
}

/// Lays out `m:f`.
fn layout_fraction(frame: &Frame<'_, '_>, fraction: &Fraction) -> MathBox {
    let mut numerator = layout_argument(frame, &fraction.numerator);
    let mut denominator = layout_argument(frame, &fraction.denominator);
    if let Some(control) = fraction.control.as_deref() {
        apply_control(&mut numerator, control);
        apply_control(&mut denominator, control);
    }
    // ISO/IEC 29500-1 §22.1.2.36: `m:type` selects between a stacked fraction
    // (`bar`/`noBar`/`skew`) and the *linear* one (`lin`), whose parts stay on
    // the line and therefore never grow it.
    if fraction.bar_type.as_deref() == Some("lin") {
        return layout_linear_fraction(frame, numerator, denominator);
    }
    let bar_type = fraction.bar_type.as_deref().unwrap_or("bar");
    let thickness = frame.rule_thickness();
    let axis = frame.axis();
    let shrink = if fraction.small_fraction { 0.6 } else { 1.0 };
    let pad = FRACTION_PAD * frame.em();
    let width = numerator.size.width.max(denominator.size.width).max(0.0) + 2.0 * pad;

    let (numerator_dy, denominator_dy) =
        fraction_part_placements(&numerator, &denominator, frame.em(), thickness, shrink);
    debug_assert!(
        -(numerator_dy + numerator.size.depth) >= MIN_FRACTION_PART_GAP_PX
            && denominator_dy - denominator.size.height >= MIN_FRACTION_PART_GAP_PX,
        "a fraction's rule must clear both of its parts"
    );

    let mut out = MathBox::empty();
    match bar_type {
        "skew" => {
            // A slanted rule spanning both parts, as Word draws a skewed
            // fraction.
            let top = numerator_dy;
            let bottom = denominator_dy + denominator.size.depth;
            if let Some(rule) = frame.segment(0.0, bottom, width, top) {
                out.items.push(rule);
            }
        }
        "noBar" => {}
        _ => {
            if let Some(rule) = frame.rule(0.0, 0.0, width) {
                out.items.push(rule);
            }
        }
    }
    let numerator_x = (width - numerator.size.width) / 2.0;
    let denominator_x = (width - denominator.size.width) / 2.0;
    out.place(numerator, numerator_x, numerator_dy);
    out.place(denominator, denominator_x, denominator_dy);
    // The rule's own half-thickness extends the box around the axis.
    out.size = Size::new(
        width,
        out.size.height.max(axis + thickness / 2.0),
        out.size.depth.max(-axis + thickness / 2.0),
    );
    out
}

/// Lays out the `m:f/m:type="lin"` linear fraction: both parts stay on the
/// text line, the numerator raised and the denominator lowered, separated by a
/// slash. The construct never exceeds one line of the math font, which is what
/// makes a linear fraction inline (§22.1.2.36).
fn layout_linear_fraction(
    frame: &Frame<'_, '_>,
    numerator: MathBox,
    denominator: MathBox,
) -> MathBox {
    let em = frame.em();
    let slash = LINEAR_SLASH_GAP * em;
    let denominator_dx = numerator.size.width + 2.0 * slash;
    let up = -LINEAR_NUMERATOR_SHIFT * em;
    let down = LINEAR_DENOMINATOR_SHIFT * em;

    let mut out = MathBox::empty();
    // The slash runs from the denominator's lower left to the numerator's upper
    // right, as a printed solidus does.
    let x0 = numerator.size.width + 0.35 * slash;
    let x1 = numerator.size.width + 1.65 * slash;
    let y0 = down + denominator.size.depth.max(0.15 * em);
    let y1 = up - numerator.size.height.max(0.15 * em);
    if let Some(rule) = frame.segment(x0, y0, x1, y1) {
        out.items.push(rule);
    }
    let denominator_width = denominator.size.width;
    out.place(numerator, 0.0, up);
    out.place(denominator, denominator_dx, down);
    out.size.width = (denominator_dx + denominator_width).max(out.size.width);
    // One line of the math font bounds the construct.
    let ascent = frame.row_ascent();
    let line = frame.row_pitch();
    out.size.height = out.size.height.min(ascent);
    out.size.depth = out.size.depth.min((line - ascent).max(0.0));
    out
}

/// Lays out `m:rad`.
fn layout_radical(frame: &Frame<'_, '_>, radical: &Radical) -> MathBox {
    let mut radicand = layout_argument(frame, &radical.radicand);
    if let Some(control) = radical.control.as_deref() {
        apply_control(&mut radicand, control);
    }
    let degree_frame = frame.script();
    let degree = if radical.hide_degree {
        MathBox::empty()
    } else {
        layout_argument(&degree_frame, &radical.degree)
    };
    let gap = RADICAL_GAP * frame.em();
    let thickness = frame.rule_thickness();
    let bar_y = -radicand.size.height - gap;
    let sign_height = (-bar_y).max(MIN_DELIMITER_HEIGHT_PX);
    let sign_width = (sign_height * DELIMITER_WIDTH).max(MIN_DELIMITER_PX);
    let degree_width = if degree.is_empty() {
        0.0
    } else {
        degree.size.width + 0.04 * frame.em()
    };
    let radicand_x = sign_width + degree_width;

    let mut out = MathBox::empty();
    if let Some(sign) = frame.path(
        0.0,
        bar_y,
        sign_width,
        sign_height,
        shapes::radical_sign(sign_width, sign_height),
    ) {
        out.items.push(sign);
    }
    if !degree.is_empty() {
        // The degree sits in the notch of the sign, above the radicand.
        let dy = bar_y - thickness - degree.size.height;
        out.place(degree, sign_width * 0.25, dy);
    }
    if let Some(bar) = frame.rule(radicand_x, bar_y, radicand.size.width) {
        out.items.push(bar);
    }
    let total_width = radicand_x + radicand.size.width;
    out.place(radicand, radicand_x, 0.0);
    out.size.width = total_width.max(out.size.width);
    out
}

/// Lays out `m:sSup`.
fn layout_superscript(frame: &Frame<'_, '_>, script: &Superscript) -> MathBox {
    let mut base = layout_argument(frame, &script.base);
    if let Some(control) = script.control.as_deref() {
        apply_control(&mut base, control);
    }
    let script_frame = frame.script();
    let mut superscript = layout_argument(&script_frame, &script.superscript);
    if let Some(control) = script.control.as_deref() {
        apply_control(&mut superscript, control);
    }
    let base_width = base.size.width;
    let script_x = base_width + SCRIPT_GAP * frame.em();
    let mut out = MathBox::empty();
    out.place(base, 0.0, 0.0);
    out.place(superscript, script_x, -SUP_SHIFT * frame.em());
    out
}

/// Lays out `m:sSub`.
fn layout_subscript(frame: &Frame<'_, '_>, script: &Subscript) -> MathBox {
    let mut base = layout_argument(frame, &script.base);
    if let Some(control) = script.control.as_deref() {
        apply_control(&mut base, control);
    }
    let script_frame = frame.script();
    let mut subscript = layout_argument(&script_frame, &script.subscript);
    if let Some(control) = script.control.as_deref() {
        apply_control(&mut subscript, control);
    }
    let script_x = base.size.width + SCRIPT_GAP * frame.em();
    let mut out = MathBox::empty();
    out.place(base, 0.0, 0.0);
    out.place(subscript, script_x, SUB_SHIFT * frame.em());
    out
}

/// Lays out `m:sSubSup`.
fn layout_sub_superscript(frame: &Frame<'_, '_>, script: &SubSuperscript) -> MathBox {
    let mut base = layout_argument(frame, &script.base);
    let script_frame = frame.script();
    let mut subscript = layout_argument(&script_frame, &script.subscript);
    let mut superscript = layout_argument(&script_frame, &script.superscript);
    if let Some(control) = script.control.as_deref() {
        apply_control(&mut base, control);
        apply_control(&mut subscript, control);
        apply_control(&mut superscript, control);
    }
    let (subscript, superscript) = separate_scripts(frame, subscript, superscript);
    let script_x = base.size.width + SCRIPT_GAP * frame.em();
    let mut out = MathBox::empty();
    out.place(base, 0.0, 0.0);
    out.place(superscript, script_x, -SUP_SHIFT * frame.em());
    out.place(subscript, script_x, SUB_SHIFT * frame.em());
    out
}

/// Lays out `m:sPre`: sub- and superscript stacked to the left of the base.
fn layout_pre_script(frame: &Frame<'_, '_>, script: &PreScript) -> MathBox {
    let mut base = layout_argument(frame, &script.base);
    let script_frame = frame.script();
    let mut subscript = layout_argument(&script_frame, &script.subscript);
    let mut superscript = layout_argument(&script_frame, &script.superscript);
    if let Some(control) = script.control.as_deref() {
        apply_control(&mut base, control);
        apply_control(&mut subscript, control);
        apply_control(&mut superscript, control);
    }
    let (subscript, superscript) = separate_scripts(frame, subscript, superscript);
    let script_width = subscript.size.width.max(superscript.size.width);
    let mut out = MathBox::blank(Size::new(script_width, 0.0, 0.0));
    out.place(subscript, 0.0, SUB_SHIFT * frame.em() / 2.0);
    out.place(superscript, 0.0, -SUP_SHIFT * frame.em() / 2.0);
    out.size.width = script_width;
    out.place(base, script_width + SCRIPT_GAP * frame.em(), 0.0);
    out
}

/// Pushes a subscript and a superscript apart so they never overlap.
fn separate_scripts(
    frame: &Frame<'_, '_>,
    subscript: MathBox,
    superscript: MathBox,
) -> (MathBox, MathBox) {
    let superscript_bottom = -SUP_SHIFT * frame.em() + superscript.size.depth;
    let subscript_top = SUB_SHIFT * frame.em() - subscript.size.height;
    let overlap = subscript_top - superscript_bottom - LIMIT_GAP * frame.em();
    if overlap <= 0.0 {
        return (subscript, superscript);
    }
    let half = overlap / 2.0;
    (
        subscript.shifted(0.0, half),
        superscript.shifted(0.0, -half),
    )
}

/// Lays out `m:nary`.
fn layout_nary(frame: &Frame<'_, '_>, nary: &NaryOperator) -> MathBox {
    let character = nary.chr.unwrap_or('\u{222B}');
    let mut operand = layout_argument(frame, &nary.operand);
    if let Some(control) = nary.control.as_deref() {
        apply_control(&mut operand, control);
    }
    let script_frame = frame.script();
    let lower = if nary.hide_sub {
        MathBox::empty()
    } else {
        layout_argument(&script_frame, &nary.subscript)
    };
    let upper = if nary.hide_sup {
        MathBox::empty()
    } else {
        layout_argument(&script_frame, &nary.superscript)
    };
    let (lower, upper) = separate_scripts(frame, lower, upper);
    let under_over = nary.limit_location == Some(LimitLocation::UnderOver);
    // The operator has to cover its limits as well as its operand.
    let limit_extent = if under_over {
        upper.size.height + LIMIT_GAP * frame.em() + lower.size.depth
    } else {
        0.0
    } + 2.0 * LIMIT_GAP * frame.em();
    let mut operator = nary_operator(
        frame,
        character,
        nary.grow != Some(false),
        operand.size.height + operand.size.depth,
        limit_extent,
    );
    if let Some(control) = nary.control.as_deref() {
        apply_control(&mut operator, control);
    }

    let mut head = MathBox::empty();
    if under_over {
        // The limits sit over/under the operator; `m:grow` widens the
        // operator's box to reach them, as Word does for a stretched glyph.
        let limits = lower.size.width.max(upper.size.width);
        let width = if nary.grow == Some(true) {
            operator.size.width.max(limits)
        } else {
            operator.size.width
        };
        let operator_x = (width - operator.size.width) / 2.0;
        let lower_x = (width - lower.size.width) / 2.0;
        let upper_x = (width - upper.size.width) / 2.0;
        head = MathBox::blank(Size::new(width, 0.0, 0.0));
        head.place(operator, operator_x, 0.0);
        // The limits clear the operator's ink, not just its baseline: a
        // stretched operator reaches well past the limit's own ascent.
        let lower_y = head.size.depth + LIMIT_GAP * frame.em() + lower.size.height;
        head.place(lower, lower_x, lower_y);
        let upper_y = -(head.size.height + LIMIT_GAP * frame.em()) - upper.size.depth;
        head.place(upper, upper_x, upper_y);
    } else {
        let script_x = operator.size.width + SCRIPT_GAP * frame.em();
        // A large operator carries its limits at its own extremes; a plain one
        // keeps them at the ordinary script positions.
        if operator.size.height > frame.row_ascent() {
            let lower_y = operator.size.depth + LIMIT_GAP * frame.em() + lower.size.height;
            let upper_y = -(operator.size.height + LIMIT_GAP * frame.em()) - upper.size.depth;
            head.place(operator, 0.0, 0.0);
            head.place(lower, script_x, lower_y);
            head.place(upper, script_x, upper_y);
        } else {
            head.place(operator, 0.0, 0.0);
            head.place(lower, script_x, SUB_SHIFT * frame.em());
            head.place(upper, script_x, -SUP_SHIFT * frame.em());
        }
    }

    let mut out = MathBox::empty();
    out.append(head);
    out.append_gap(operand, NARY_GAP * frame.em());
    out
}

/// Builds the `m:nary` operator glyph, stretched as Word draws it.
///
/// ISO/IEC 29500-1 §22.1.2.35: `m:grow` (on by default) makes the operator grow
/// to cover its limits and operand. Word only takes that decision in *display*
/// math — the inline style keeps the text cut of the math font, which is what
/// the two WPS references show (`∑` measures ≈ 0.9 em inline and ≈ 1.98 em
/// inside `m:oMathPara`). STIX Two Math has no display cut, so a grown operator
/// is stroked as a deterministic path of the tabulated height and proportions.
fn nary_operator(
    frame: &Frame<'_, '_>,
    character: char,
    grow: bool,
    operand_extent: f64,
    limit_extent: f64,
) -> MathBox {
    let run = frame.math_run(MathStyle::Plain);
    let text = character.to_string();
    let plain = frame.text_size(&text, &run);
    let em = frame.em();
    if !(grow && frame.display) {
        return frame.text(0.0, 0.0, &text, run);
    }
    let (display_height, aspect) = nary_display(character);
    let height = (plain.height + plain.depth)
        .max(operand_extent + 2.0 * NARY_GAP * em)
        .max(limit_extent)
        .max(display_height * em);
    if height <= plain.height + plain.depth {
        return frame.text(0.0, 0.0, &text, run);
    }
    let box_width = height * aspect;
    let Some(d) = shapes::nary_operator(character, box_width, height) else {
        return frame.text(0.0, 0.0, &text, run);
    };
    let width = box_width.max(plain.width);
    // A large operator is centred on the math axis, as Word draws it.
    let axis = frame.axis();
    let item = frame.charge(Item::Path(PathItem {
        x: crate::units::finite((width - box_width) / 2.0),
        y: crate::units::finite(-height / 2.0 - axis),
        w: crate::units::finite(box_width),
        h: crate::units::finite(height),
        d,
        fill: None,
        stroke: Some(frame.color()),
        stroke_w: frame.rule_thickness(),
        dash: None,
        rotate_deg: 0.0,
        flip_h: false,
        flip_v: false,
    }));
    MathBox {
        size: Size::new(width, height / 2.0 + axis, (height / 2.0 - axis).max(0.0)),
        items: item.into_iter().collect(),
    }
}

/// Lays out `m:d`.
fn layout_delimiter(frame: &Frame<'_, '_>, delimiter: &Delimiter) -> MathBox {
    let separator_gap = SEPARATOR_GAP * frame.em();
    let mut content = MathBox::empty();
    for (index, argument) in delimiter.arguments.iter().enumerate() {
        if index > 0 {
            if let Some(separator) = delimiter.separator {
                let run_box = frame.text(
                    0.0,
                    0.0,
                    &separator.to_string(),
                    frame.math_run(MathStyle::Plain),
                );
                content.append_gap(run_box, separator_gap);
            }
        }
        content.append(layout_argument(frame, argument));
    }
    if let Some(control) = delimiter.control.as_deref() {
        apply_control(&mut content, control);
    }

    // `m:grow` (the default) stretches the delimiters over the content; with
    // `m:grow="0"` they keep the height of the plain glyph.
    let plain = frame.text_size("(", &frame.math_run(MathStyle::Plain));
    let extent = content.size.height.max(content.size.depth);
    let height = match delimiter.grow {
        Some(false) => (plain.height + plain.depth).max(MIN_DELIMITER_HEIGHT_PX),
        _ => (extent + 2.0 * DELIMITER_GAP * frame.em()).max(MIN_DELIMITER_HEIGHT_PX),
    };
    let width = (height * DELIMITER_WIDTH).max(MIN_DELIMITER_PX);
    let gap = DELIMITER_GAP * frame.em();

    // The delimiters are centred on the content's vertical extent.
    let middle = (content.size.height - content.size.depth) / 2.0;
    let top = middle - height / 2.0;
    let content_width = content.size.width;
    let mut out = MathBox::empty();
    if let Some(begin) = delimiter.begin {
        if let Some(item) = delimiter_item(frame, begin, 0.0, top, width, height) {
            out.items.push(item);
        }
    }
    if let Some(end) = delimiter.end {
        let x = width + gap + content_width + gap;
        if let Some(item) = delimiter_item(frame, end, x, top, width, height) {
            out.items.push(item);
        }
    }
    out.place(content, width + gap, 0.0);
    out.size.width = (2.0 * (width + gap) + content_width).max(out.size.width);
    out.size.height = out.size.height.max(0.5 * height);
    out.size.depth = out.size.depth.max(0.5 * height);
    out
}

/// Emits one stretchy delimiter.
///
/// Characters the bundled face cannot stretch (and every geometric delimiter)
/// are drawn as a deterministic vector path; a plain glyph such as `|` keeps
/// its typographic width instead.
fn delimiter_item(
    frame: &Frame<'_, '_>,
    character: char,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Option<Item> {
    if character == '|' || character == '\u{2016}' {
        let run = frame.math_run(MathStyle::Plain);
        let text = character.to_string();
        let size = frame.text_size(&text, &run);
        return frame.charge(Item::Text(TextItem {
            x: crate::units::finite(x),
            baseline: crate::units::finite(y + height / 2.0),
            width: crate::units::finite(size.width),
            text,
            run,
            size_px: frame.size_px,
            field: None,
        }));
    }
    let d = shapes::delimiter(character, width, height)?;
    frame.path(x, y, width, height, d)
}

/// Lays out `m:func`.
fn layout_function(frame: &Frame<'_, '_>, function: &Function) -> MathBox {
    let mut name = layout_argument(frame, &function.name);
    let mut argument = layout_argument(frame, &function.argument);
    if let Some(control) = function.control.as_deref() {
        apply_control(&mut name, control);
        apply_control(&mut argument, control);
    }
    let mut out = MathBox::empty();
    out.append(name);
    out.append_gap(argument, FUNCTION_GAP * frame.em());
    out
}

/// Lays out `m:limLow`/`m:limUpp`.
fn layout_limit(frame: &Frame<'_, '_>, limit: &MathLimit) -> MathBox {
    let mut base = layout_argument(frame, &limit.base);
    let script_frame = frame.script();
    let mut script = layout_argument(&script_frame, &limit.limit);
    if let Some(control) = limit.control.as_deref() {
        apply_control(&mut base, control);
        apply_control(&mut script, control);
    }
    let gap = LIMIT_GAP * frame.em();
    let mut out = MathBox::empty();
    if limit.above {
        out.append(script);
        let dy = out.size.depth + gap + base.size.height;
        out.place(base, 0.0, dy);
    } else {
        out.append(base);
        let dy = out.size.depth + gap + script.size.height;
        out.place(script, 0.0, dy);
    }
    out
}

/// A grid of laid-out cells shared by `m:m` and `m:eqArr`.
struct Grid {
    rows: Vec<Vec<MathBox>>,
    column_widths: Vec<f64>,
    row_heights: Vec<f64>,
    row_depths: Vec<f64>,
    column_gap: f64,
    /// Distance between consecutive row baselines, in px.
    row_pitch: f64,
    /// `m:baseJc`, read and carried.
    ///
    /// Not consulted yet, and deliberately left in place rather than deleted: it
    /// places the block vertically among its neighbours, which is a rendering
    /// feature this stage did not build. Removing it would have thrown away the
    /// information, and keeping it silently unused would be a lie in the other
    /// direction. Its one previous use was a bug - it was being read as a
    /// *horizontal* column alignment - and that is what E1 fixed.
    #[allow(dead_code)]
    base: Option<MathVerticalAlign>,
    per_column: Vec<MatrixColumn>,
}

impl Grid {
    /// Computes the grid metrics from already laid-out cells.
    fn new(
        rows: Vec<Vec<MathBox>>,
        column_count: usize,
        column_gap: f64,
        row_pitch: f64,
        row_ascent: f64,
        base: Option<MathVerticalAlign>,
        per_column: Vec<MatrixColumn>,
    ) -> Self {
        let mut column_widths = vec![0.0f64; column_count];
        let mut row_heights = Vec::with_capacity(rows.len());
        let mut row_depths = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut height: f64 = 0.0;
            let mut depth: f64 = 0.0;
            for (index, cell) in row.iter().enumerate() {
                // `get_mut` and not an index: a matrix row may carry more cells
                // than the matrix declares columns, and the acceptance
                // criterion for AUD-08 is that no indexed write on a
                // layout-sized vector is left in this crate. The cell is still
                // laid out - it is the *column* that has nowhere to go.
                if let Some(width) = column_widths.get_mut(index) {
                    *width = width.max(cell.size.width);
                }
                height = height.max(cell.size.height);
                depth = depth.max(cell.size.depth);
            }
            // A row is never shorter than one line of the math font, and its
            // baseline sits at the nominal ascent within it: Word lays matrix
            // and equation-array rows out on a line grid, not on the raw ink
            // extents of the cells.
            row_heights.push(height.max(row_ascent));
            row_depths.push(depth);
        }
        Self {
            rows,
            column_widths,
            row_heights,
            row_depths,
            column_gap,
            row_pitch,
            base,
            per_column,
        }
    }

    /// The horizontal alignment of one column: `m:mcJc`, then centred.
    ///
    /// `m:baseJc` is deliberately **not** a fallback here. It is a `CT_YAlign`:
    /// it places the matrix or equation array vertically among its neighbours,
    /// and it has nothing to say about how a column's cells line up across.
    /// Using it as a horizontal alignment meant `Left` rendered as `inline` and
    /// `Right` as `bottom`, and then only `Center` and `Right` were honoured at
    /// all - so a document asking for a column at the right got one centred.
    ///
    /// `base` is still carried, and correctly typed; making it move the block
    /// vertically is a rendering feature rather than a fix, and is recorded as
    /// such instead of being approximated here.
    fn column_alignment(&self, column: usize) -> MathAlignment {
        self.per_column
            .get(column)
            .and_then(|spec| spec.justification)
            .unwrap_or(MathAlignment::Center)
    }

    /// Places every cell and returns the laid-out grid.
    fn place(&self) -> MathBox {
        let mut out = MathBox::empty();
        let mut top = 0.0f64;
        let mut widest: f64 = 0.0;
        for (row_index, row) in self.rows.iter().enumerate() {
            let mut x = 0.0f64;
            for (column_index, cell) in row.iter().enumerate() {
                let column = self.column_widths.get(column_index).copied().unwrap_or(0.0);
                let align = match self.column_alignment(column_index) {
                    MathAlignment::Center => (column - cell.size.width) / 2.0,
                    MathAlignment::Right => column - cell.size.width,
                    _ => 0.0,
                };
                // Cells are top-aligned within their row, as Word does: the
                // baseline of a cell sits `cell.height` below the row's top, and
                // the row's top is `top` below the block's baseline (which is
                // the first row's baseline).
                let dy = top + cell.size.height - self.row_heights[row_index];
                out.place(cell.clone(), x + align, dy);
                x += column + self.column_gap;
            }
            widest = widest.max(x - self.column_gap);
            // Rows advance by a whole line of the math font, or by their own
            // extent when a cell is taller than one line.
            top += self.row_heights[row_index]
                .max(self.row_pitch)
                .max(self.row_heights[row_index] + self.row_depths[row_index]);
        }
        let total = top.max(0.0);
        let first_baseline = self.row_heights.first().copied().unwrap_or(0.0);
        out.size.width = widest.max(out.size.width);
        out.size.height = out.size.height.max(first_baseline);
        out.size.depth = out.size.depth.max((total - first_baseline).max(0.0));
        out
    }
}

/// Resolves the `m:cSp` inter-column spacing of a math grid, in px.
///
/// ISO/IEC 29500-1 §22.1.2.16: `m:cSp` is `ST_UnsignedTwipsMeasure` when
/// `m:cGpRule` is `exact`/`atLeast` and a hundredth of the current font size
/// otherwise.
fn grid_column_gap(
    frame: &Frame<'_, '_>,
    rule: Option<&str>,
    spacing: Option<i32>,
    default_em: f64,
) -> f64 {
    match (rule, spacing) {
        (Some("exact" | "atLeast"), Some(value)) => default_em * frame.em() + frame.twips(value),
        (_, Some(value)) => (f64::from(value) / 100.0 * frame.em()).max(default_em * frame.em()),
        _ => default_em * frame.em(),
    }
}

/// Resolves the `m:rSp` row spacing of a math grid, in px.
///
/// ISO/IEC 29500-1 §22.1.2.79: with `m:rSpRule` `exact`/`atLeast` the value is
/// `ST_UnsignedTwipsMeasure` added to the row pitch; with the rule `1` it is a
/// hundredth of the current font size that sets the pitch itself. A row is
/// never compressed below one line of the math font.
fn grid_row_pitch(frame: &Frame<'_, '_>, rule: Option<&str>, spacing: Option<i32>) -> f64 {
    let natural = frame.row_pitch();
    match (rule, spacing) {
        (Some("exact" | "atLeast"), Some(value)) => natural + frame.twips(value).max(0.0),
        (_, Some(value)) => (f64::from(value) / 100.0 * frame.em()).max(natural),
        _ => natural,
    }
}

/// Lays out `m:m`.
fn layout_matrix(frame: &Frame<'_, '_>, matrix: &Matrix) -> MathBox {
    let column_gap = grid_column_gap(
        frame,
        matrix.column_group_rule.as_deref(),
        matrix.column_spacing,
        COLUMN_GAP,
    );
    let row_pitch = grid_row_pitch(
        frame,
        matrix.row_spacing_rule.as_deref(),
        matrix.row_spacing,
    );
    let rows: Vec<Vec<MathBox>> = matrix
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|argument| layout_argument(frame, argument))
                .collect()
        })
        .collect();
    let columns = matrix.rows.iter().map(Vec::len).max().unwrap_or(0);
    let grid = Grid::new(
        rows,
        columns,
        column_gap,
        row_pitch,
        frame.row_ascent(),
        matrix.base_justification,
        matrix.columns.clone(),
    );
    let mut out = grid.place();
    if let Some(control) = matrix.control.as_deref() {
        apply_control(&mut out, control);
    }
    out
}

/// Lays out `m:eqArr`.
fn layout_equation_array(frame: &Frame<'_, '_>, array: &EquationArray) -> MathBox {
    let row_pitch = grid_row_pitch(frame, array.row_spacing_rule.as_deref(), array.row_spacing);
    let rows: Vec<Vec<MathBox>> = array
        .rows
        .iter()
        .map(|row| vec![layout_argument(frame, row)])
        .collect();
    let grid = Grid::new(
        rows,
        1,
        0.0,
        row_pitch,
        frame.row_ascent(),
        array.base_justification,
        Vec::new(),
    );
    let mut out = grid.place();

    if let Some(control) = array.control.as_deref() {
        apply_control(&mut out, control);
    }
    out
}

/// Lays out `m:acc`.
fn layout_accent(frame: &Frame<'_, '_>, accent: &Accent) -> MathBox {
    let character = accent.chr.unwrap_or('\u{0302}');
    let mut base = layout_argument(frame, &accent.base);
    if let Some(control) = accent.control.as_deref() {
        apply_control(&mut base, control);
    }
    let run = frame.math_run(MathStyle::Plain);
    let text = character.to_string();
    let accent_size = frame.text_size(&text, &run);
    // The accent is centred on the base and sits directly on top of it, as a
    // combining mark does.
    let dx = ((base.size.width - accent_size.width) / 2.0).max(0.0);
    let dy = -(base.size.height + ACCENT_GAP * frame.em() - accent_size.depth);
    let mut out = MathBox::empty();
    out.place(frame.text(dx, dy, &text, run), 0.0, 0.0);
    out.place(base, 0.0, 0.0);
    out
}

/// Lays out `m:bar`.
fn layout_bar(frame: &Frame<'_, '_>, bar: &Bar) -> MathBox {
    let mut base = layout_argument(frame, &bar.base);
    if let Some(control) = bar.control.as_deref() {
        apply_control(&mut base, control);
    }
    let gap = BAR_GAP * frame.em();
    let width = base.size.width;
    let thickness = frame.rule_thickness();
    let mut out = MathBox::empty();
    if bar.position == Some(MathPosition::Bottom) {
        out.append(base);
        let y = out.size.depth + gap;
        if let Some(rule) = frame.rule(0.0, y, width) {
            out.items.push(rule);
        }
        out.size.depth = out.size.depth.max(y + thickness / 2.0);
    } else {
        let y = -(base.size.height + gap);
        if let Some(rule) = frame.rule(0.0, y, width) {
            out.items.push(rule);
        }
        out.size = Size::new(width, -y + thickness / 2.0, 0.0);
        out.append(base);
    }
    out
}

/// Lays out `m:groupChr`.
fn layout_group_character(frame: &Frame<'_, '_>, group: &GroupCharacter) -> MathBox {
    let position = group.position.unwrap_or(MathPosition::Top);
    let mut base = layout_argument(frame, &group.base);
    if let Some(control) = group.control.as_deref() {
        apply_control(&mut base, control);
    }
    let character = group.chr.unwrap_or('\u{23DF}');
    let run = frame.math_run(MathStyle::Plain);
    let marker = frame.text(0.0, 0.0, &character.to_string(), run);
    let gap = GROUP_GAP * frame.em();
    let side_gap = SCRIPT_GAP * frame.em();
    let mut out = MathBox::empty();
    match position {
        MathPosition::Left | MathPosition::Right => {
            // `m:vertJc` decides the side character's vertical alignment; Word
            // centres it, which is the only value with a defined width.
            let (left, right) = if position == MathPosition::Left {
                (marker, base)
            } else {
                (base, marker)
            };
            let dy = (left.size.height - right.size.height) / 2.0
                + (left.size.depth - right.size.depth) / 2.0;
            out.append(left);
            out.append_gap(right.shifted(0.0, dy), side_gap);
        }
        MathPosition::Top | MathPosition::Bottom => {
            let width = base.size.width.max(marker.size.width);
            let dx = (width - marker.size.width) / 2.0;
            out = MathBox::blank(Size::new(width, 0.0, 0.0));
            if position == MathPosition::Top {
                out.place(marker, dx, 0.0);
                let dy = out.size.depth + gap + base.size.height;
                out.place(base, 0.0, dy);
            } else {
                out.append(base);
                let dy = out.size.depth + gap + marker.size.height;
                out.place(marker, dx, dy);
            }
            out.size.width = width;
        }
    }
    out
}

/// Lays out `m:box`.
fn layout_box(frame: &Frame<'_, '_>, boxed: &Boxed) -> MathBox {
    let mut inner = layout_argument(frame, &boxed.argument);
    if let Some(control) = boxed.control.as_deref() {
        apply_control(&mut inner, control);
    }
    let pad = BOX_PAD * frame.em() + boxed.spacing.map_or(0.0, |p| frame.points(p));
    boxed_rectangle(frame, inner, pad)
}

/// Lays out `m:borderBox`.
fn layout_border_box(frame: &Frame<'_, '_>, border: &BorderBox) -> MathBox {
    let mut inner = MathBox::empty();
    for (index, argument) in border.arguments.iter().enumerate() {
        let cell = layout_argument(frame, argument);
        if index > 0 {
            inner.append_gap(cell, BORDER_BOX_GAP * frame.em());
        } else {
            inner.append(cell);
        }
    }
    if let Some(control) = border.control.as_deref() {
        apply_control(&mut inner, control);
    }
    let pad = BOX_PAD * frame.em() + border.spacing.map_or(0.0, |p| frame.points(p));
    let mut out = boxed_rectangle(frame, inner, pad);
    if border.shadow {
        out.items.insert(0, shadow_rectangle(frame, &out));
    }
    out
}

/// Returns a translucent-looking offset copy of a box's frame (`m:shadow`).
fn shadow_rectangle(frame: &Frame<'_, '_>, boxed: &MathBox) -> Item {
    let offset = 0.06 * frame.em();
    let Some(Item::Rect(rect)) = boxed.items.first() else {
        return Item::Rect(RectItem {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            fill: None,
            stroke: None,
            stroke_w: 0.0,
        });
    };
    Item::Rect(RectItem {
        x: rect.x + offset,
        y: rect.y + offset,
        w: rect.w,
        h: rect.h,
        fill: Some("#000000".to_owned()),
        stroke: None,
        stroke_w: 0.0,
    })
}

/// Draws a padded frame around `inner`.
fn boxed_rectangle(frame: &Frame<'_, '_>, inner: MathBox, pad: f64) -> MathBox {
    let width = inner.size.width + 2.0 * pad;
    let height = inner.size.height + 2.0 * pad;
    let depth = inner.size.depth + 2.0 * pad;
    let mut out = MathBox {
        size: Size::ZERO,
        items: vec![Item::Rect(RectItem {
            x: 0.0,
            y: -height,
            w: crate::units::finite(width),
            h: crate::units::finite(height + depth),
            fill: None,
            stroke: Some(frame.color()),
            stroke_w: frame.rule_thickness(),
        })],
    };
    out.place(inner, pad, 0.0);
    out.size = Size::new(width, height, depth);
    out
}

/// Lays out `m:phant`: reserves space, draws only when `m:show` is set.
fn layout_phantom(frame: &Frame<'_, '_>, phantom: &Phantom) -> MathBox {
    let mut inner = layout_argument(frame, &phantom.argument);
    if let Some(control) = phantom.control.as_deref() {
        apply_control(&mut inner, control);
    }
    let mut size = inner.size;
    if phantom.zero_width {
        size.width = 0.0;
    }
    if phantom.zero_ascent {
        size.height = 0.0;
    }
    if phantom.zero_descent {
        size.depth = 0.0;
    }
    if !phantom.show {
        // The space is reserved but nothing is drawn.
        return MathBox::blank(size);
    }
    let dx = ((size.width - inner.size.width) / 2.0).max(0.0);
    let dy = (size.height - inner.size.height) / 2.0 - size.depth + inner.size.depth;
    let mut out = inner.shifted(dx, dy);
    out.size = size;
    out
}

/// Returns `true` when the item budget still allows more paint items.
#[cfg(test)]
fn has_budget(budget: &Cell<usize>) -> bool {
    budget.get() > 0
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{
        fraction_part_placements, translate, Item, MathBox, Size, MAX_ITEMS,
        MIN_FRACTION_PART_GAP_PX,
    };
    #[test]
    fn the_rule_always_clears_both_parts_however_small_they_are() {
        // The `debug_assert!` this replaces claimed the clearance could never go
        // below `MIN_FRACTION_PART_GAP_PX`. It can: a 6 pt numerator has no room
        // for the nominal clearance, and a subscript of a subscript has less
        // still. In debug that ended the process; in release the rule drew
        // through its own parts and nothing said so (AUD-09). The rule is now a
        // floor rather than a claim.
        let tiny = part(2.0, 2.0, 0.0);
        for shrink in [0.05f64, 0.2, 0.5, 1.0] {
            let (numerator_dy, denominator_dy) =
                fraction_part_placements(&tiny, &tiny, 8.0, 1.0, shrink);
            assert!(
                -(numerator_dy + tiny.size.depth) >= MIN_FRACTION_PART_GAP_PX,
                "shrink {shrink}: the rule does not clear the numerator"
            );
            assert!(
                denominator_dy - tiny.size.height >= MIN_FRACTION_PART_GAP_PX,
                "shrink {shrink}: the rule does not clear the denominator"
            );
        }
    }

    #[test]
    fn a_fraction_with_room_keeps_the_nominal_clearance() {
        // The floor is a floor, not a replacement: a fraction with room to spare
        // still gets the nominal clearance, or every formula would gain a fixed
        // gap and the pinned WPS references would stop matching.
        let big = part(20.0, 20.0, 4.0);
        let (numerator_dy, denominator_dy) = fraction_part_placements(&big, &big, 24.0, 1.5, 1.0);
        let clearance = -(numerator_dy + big.size.depth);
        assert!(
            clearance > MIN_FRACTION_PART_GAP_PX,
            "a big fraction lost its clearance: {clearance}"
        );
        assert!((denominator_dy - big.size.height - clearance).abs() < 1e-9);
    }
    use crate::layout::{LineItem, RectItem};

    /// A part box of the given extents, in px.
    fn part(width: f64, height: f64, depth: f64) -> MathBox {
        MathBox {
            size: Size::new(width, height, depth),
            items: Vec::new(),
        }
    }

    /// **The invariant: a fraction's rule always sits clear of both parts.**
    ///
    /// This is stated on the parts' **boxes** and is therefore a statement about
    /// the placement, not about the ink: the substituted face's ink is not the
    /// reference's ink, and a layout test that asserted on ink would be a test of
    /// the substitution. It still bites — the parts are separated by
    /// `2 x (clearance + axis) x em`, which a zero or negative clearance closes —
    /// and the rendered half of it is measured by the fidelity gate, where this
    /// change moved `06-strict-math-display` from 0.9496 (below the threshold, the
    /// rule through the numerator) to 0.9768.
    #[test]
    fn a_fractions_parts_are_always_separated_by_its_rule() {
        // (numerator height, depth, denominator height, depth): a plain run, a part
        // with a descender, a part with a tall script over it, and a part one whole
        // line deep in both directions.
        let shapes = [
            (12.0, 3.0, 12.0, 3.0),
            (12.0, 5.0, 12.0, 0.0),
            (20.0, 6.0, 18.0, 2.0),
            (24.0, 8.0, 24.0, 8.0),
            (9.0, 2.0, 9.0, 2.0),
        ];
        for em in [8.0, 11.0, 14.667, 24.0] {
            for shrink in [1.0, 0.6] {
                for (numerator_h, numerator_d, denominator_h, denominator_d) in shapes {
                    let numerator = part(40.0, numerator_h, numerator_d);
                    let denominator = part(30.0, denominator_h, denominator_d);
                    let (numerator_dy, denominator_dy) =
                        fraction_part_placements(&numerator, &denominator, em, 0.66, shrink);
                    let above = -(numerator_dy + numerator.size.depth);
                    let below = denominator_dy - denominator.size.height;
                    assert!(
                        above >= MIN_FRACTION_PART_GAP_PX && below >= MIN_FRACTION_PART_GAP_PX,
                        "em {em}, shrink {shrink}: the rule must keep at least \
                         {MIN_FRACTION_PART_GAP_PX} px of each part's box (got {above:+.2} above, \
                         {below:+.2} below)"
                    );
                }
            }
        }
    }

    /// The placement is asymmetric, and that is the point: a denominator's box
    /// rises by its ascent, a numerator's by its descent.
    #[test]
    fn a_denominator_is_further_from_the_rule_than_a_numerator_of_the_same_size() {
        let numerator = part(40.0, 12.0, 3.0);
        let denominator = part(40.0, 12.0, 3.0);
        let (numerator_dy, denominator_dy) =
            fraction_part_placements(&numerator, &denominator, 14.667, 0.66, 1.0);
        assert!(
            denominator_dy > -numerator_dy,
            "one shift cannot clear both: a symmetric one leaves the denominator on the rule \
             ({numerator_dy:+.2} / {denominator_dy:+.2})"
        );
    }

    /// A small fraction keeps its clearance: `m:smallFraction` shrinks the shift,
    /// and a clearance folded into the shift would shrink with it.
    #[test]
    fn a_small_fraction_keeps_a_visible_gap() {
        let numerator = part(20.0, 8.0, 2.0);
        let denominator = part(20.0, 8.0, 2.0);
        let (_, denominator_dy) =
            fraction_part_placements(&numerator, &denominator, 11.0, 0.66, 0.6);
        assert!(
            denominator_dy - denominator.size.height >= MIN_FRACTION_PART_GAP_PX,
            "a small fraction still needs room: {denominator_dy:+.2}"
        );
    }

    #[test]
    fn place_grows_the_box_in_both_directions() {
        let child = MathBox {
            size: Size::new(4.0, 3.0, 1.0),
            items: Vec::new(),
        };
        let mut out = MathBox::empty();
        // The child sits 1 px right and 2 px *below* this baseline, so it
        // contributes 1 px of ascent and 3 px of descent.
        out.place(child, 1.0, 2.0);
        assert_eq!(out.size.width, 5.0);
        assert_eq!(out.size.height, 1.0);
        assert_eq!(out.size.depth, 3.0);
    }

    #[test]
    fn place_above_the_baseline_grows_the_ascent() {
        let child = MathBox {
            size: Size::new(2.0, 8.0, 0.0),
            items: Vec::new(),
        };
        let mut out = MathBox::empty();
        out.place(child, 0.0, -6.0);
        assert_eq!(out.size.width, 2.0);
        assert_eq!(out.size.height, 14.0);
        assert_eq!(out.size.depth, 0.0);
    }

    #[test]
    fn a_placed_child_never_shrinks_the_box() {
        let mut out = MathBox::empty();
        out.place(MathBox::empty(), 10.0, 0.0);
        assert_eq!(out.size.width, 10.0);
        assert_eq!(out.size.height, 0.0);
    }

    #[test]
    fn size_is_always_finite() {
        let size = Size::new(f64::NAN, f64::INFINITY, -f64::INFINITY);
        assert!(size.width.is_finite());
        assert!(size.height.is_finite());
        assert!(size.depth.is_finite());
    }

    #[test]
    fn translate_moves_every_item_kind() {
        let rect = Item::Rect(RectItem {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            fill: None,
            stroke: None,
            stroke_w: 0.0,
        });
        let Item::Rect(rect) = translate(rect, 10.0, -5.0) else {
            panic!("expected a rectangle");
        };
        assert_eq!((rect.x, rect.y), (11.0, -3.0));

        let line = Item::Line(LineItem {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
            color: "#000000".to_owned(),
            width: 1.0,
            dashed: false,
        });
        let Item::Line(line) = translate(line, 0.5, 0.5) else {
            panic!("expected a line");
        };
        assert_eq!((line.x1, line.y1, line.x2, line.y2), (1.5, 2.5, 3.5, 4.5));
    }

    #[test]
    fn translate_normalises_non_finite_offsets() {
        let rect = Item::Rect(RectItem {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            fill: None,
            stroke: None,
            stroke_w: 0.0,
        });
        let Item::Rect(rect) = translate(rect, f64::NAN, f64::INFINITY) else {
            panic!("expected a rectangle");
        };
        assert!(rect.x.is_finite() && rect.y.is_finite());
    }

    #[test]
    fn the_budget_bounds_the_output() {
        let budget = std::cell::Cell::new(MAX_ITEMS);
        assert!(super::has_budget(&budget));
        budget.set(0);
        assert!(!super::has_budget(&budget));
    }
}
