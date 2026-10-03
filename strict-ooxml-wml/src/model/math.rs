//! Office Math Markup Language (OMML) model — `STAGE-5C-TASK.md` §3.1.1–§3.1.2.
//!
//! OMML is a native part of ISO/IEC 29500-1 (namespace
//! `http://purl.oclc.org/ooxml/officeDocument/math`), so a Strict-first model is
//! exact here (unlike the Transitional-era `wps`/`wpg` extensions of echelon 5B).
//!
//! The model is pure data (ADR-0004): every node carries a
//! [`SourceLocation`], constructs the renderer cannot draw are still represented
//! ([`MathNode::Unknown`]) and the parser records their status in the
//! [`SupportModel`](super::support::SupportModel) — nothing is lost silently.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

use super::props::RunProperties;

/// `m:sty` — the character style of a math run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathStyle {
    /// `p` — plain (upright).
    Plain,
    /// `i` — italic.
    #[default]
    Italic,
    /// `b` — bold.
    Bold,
    /// `bi` — bold italic.
    BoldItalic,
}

impl MathStyle {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"p" | b"plain" => Some(Self::Plain),
            b"i" | b"italic" => Some(Self::Italic),
            b"b" | b"bold" => Some(Self::Bold),
            b"bi" | b"bold-italic" | b"boldItalic" => Some(Self::BoldItalic),
            _ => None,
        }
    }

    /// The schema lexical form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "p",
            Self::Italic => "i",
            Self::Bold => "b",
            Self::BoldItalic => "bi",
        }
    }
}

/// `m:scr` — the alphanumeric script of a math run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathScript {
    /// `doubleStruck` (𝔸).
    DoubleStruck,
    /// `fraktur` (𝔄).
    Fraktur,
    /// `roman` — upright.
    #[default]
    Roman,
    /// `sansSerif`.
    SansSerif,
    /// `monospace` (𝔸).
    Monospace,
    /// `script` (𝒜).
    Script,
}

impl MathScript {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            // Strict `ST_Script` is
            // `roman|script|fraktur|double-struck|sans-serif|monospace`
            // (ECMA-376 Part 1, `shared-math.xsd`). The spellings this
            // table used to carry - `doubleStruck`, `sansSerif` - are the
            // **Transitional** ones, so a Strict document writing
            // `double-struck` was reported `Partial` and rewritten to a
            // default. Both are accepted, because this parser also runs
            // over Transitional input, but the Strict spelling comes first.
            b"double-struck" | b"doubleStruck" => Some(Self::DoubleStruck),
            b"fraktur" => Some(Self::Fraktur),
            b"roman" => Some(Self::Roman),
            b"sans-serif" | b"sansSerif" => Some(Self::SansSerif),
            b"monospace" => Some(Self::Monospace),
            b"script" => Some(Self::Script),
            _ => None,
        }
    }
}

/// `m:aln` (first edition: `m:defJc`) — the alignment of an argument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathAlignment {
    /// Left.
    Left,
    /// Centre.
    Center,
    /// Right.
    Right,
    /// `inline` — the argument is laid out like ordinary text.
    Inline,
    #[default]
    /// Unspecified: the renderer picks the default for the construct.
    Unset,
}

impl MathAlignment {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"l" | b"left" => Some(Self::Left),
            b"ctr" | b"center" => Some(Self::Center),
            b"r" | b"right" => Some(Self::Right),
            b"inline" => Some(Self::Inline),
            _ => None,
        }
    }
}

/// `ST_YAlign` (ECMA-376 Part 1, `shared-commonSimpleTypes.xsd`):
/// `inline | top | center | bottom | inside | outside`.
///
/// **This is not [`MathAlignment`].** That is the *justification* vocabulary
/// (`left|center|right|inline`); this one places a matrix, equation array or
/// group vertically. The two overlap in exactly two values, which is what let
/// `m:mPr/m:baseJc` be read through the wrong table: `bottom`, `top`, `inside`
/// and `outside` were all rejected as malformed and replaced by "unspecified",
/// and the writer then emitted justification values through a mapping that sent
/// `Left` to `inline` and `Right` to `bottom`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MathVerticalAlign {
    /// `inline` - laid out with the surrounding text.
    Inline,
    /// `top` - aligned to the top of the construct.
    Top,
    /// `center`.
    Center,
    /// `bottom`.
    Bottom,
    /// `inside` - vertically centred inside the construct.
    Inside,
    /// `outside` - outside the construct.
    Outside,
    #[default]
    /// Unspecified: the renderer picks the default for the construct.
    Unset,
}

impl MathVerticalAlign {
    /// Every value the schema allows, in schema order.
    pub const VALUES: [&'static str; 6] =
        ["inline", "top", "center", "bottom", "inside", "outside"];

    /// Parses the `m:val` lexical space of `ST_YAlign`; `None` when unknown.
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "inline" => Some(Self::Inline),
            "top" => Some(Self::Top),
            "center" => Some(Self::Center),
            "bottom" => Some(Self::Bottom),
            "inside" => Some(Self::Inside),
            "outside" => Some(Self::Outside),
            _ => None,
        }
    }
}

impl std::fmt::Display for MathVerticalAlign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            // `Unset` writes the schema's own default, so a written package is a
            // fixed point of the reader. It shares an arm with `Inline` because
            // on the wire they are the same value; the alternative - writing
            // nothing for `Unset` - would be a second behaviour to remember.
            Self::Inline | Self::Unset => "inline",
            Self::Top => "top",
            Self::Center => "center",
            Self::Bottom => "bottom",
            Self::Inside => "inside",
            Self::Outside => "outside",
        };
        f.write_str(text)
    }
}

/// `m:pos` of a bar or a group character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathPosition {
    /// Above the base.
    #[default]
    Top,
    /// Below the base.
    Bottom,
    /// To the left of the base.
    Left,
    /// To the right of the base.
    Right,
}

impl MathPosition {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"top" => Some(Self::Top),
            b"bot" | b"bottom" => Some(Self::Bottom),
            b"left" => Some(Self::Left),
            b"right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// `m:vertJc` of a group character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathVerticalJc {
    /// Align the top of the character with the top of the base.
    Top,
    /// Align the bottom with the bottom of the base.
    #[default]
    Bottom,
    /// Centre on the base.
    Center,
}

impl MathVerticalJc {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"top" => Some(Self::Top),
            b"bot" | b"bottom" => Some(Self::Bottom),
            b"center" => Some(Self::Center),
            _ => None,
        }
    }
}

/// `m:limLoc` — where the limits of an n-ary operator are placed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LimitLocation {
    /// `undOvr` — under and over the operator.
    #[default]
    UnderOver,
    /// `subSup` — as a subscript/superscript to the operator.
    SubSup,
}

impl LimitLocation {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"undOvr" => Some(Self::UnderOver),
            b"subSup" => Some(Self::SubSup),
            _ => None,
        }
    }
}

/// Justification of a display formula (`m:oMathParaPr/m:jc`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathJustification {
    /// Left aligned.
    Left,
    /// `centerGroup` — centred as a group.
    CenterGroup,
    /// `center` — each line centred individually.
    Center,
    /// Right aligned.
    Right,
    /// The schema default: centred.
    #[default]
    Default,
}

impl MathJustification {
    /// Parses the `m:val` lexical space; `None` for an unknown value.
    #[must_use]
    pub const fn from_strict(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"left" => Some(Self::Left),
            b"centerGroup" => Some(Self::CenterGroup),
            b"center" => Some(Self::Center),
            b"right" => Some(Self::Right),
            b"default" => Some(Self::Default),
            _ => None,
        }
    }
}

/// Properties of a math run (`m:rPr`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathRunProperties {
    /// `m:lit` — the run is literal text (not folded to a different script).
    pub literal: bool,
    /// `m:nor` — normal text (upright, not italic).
    pub normal: bool,
    /// `m:scr` — the script.
    pub script: Option<MathScript>,
    /// `m:sty` — the character style.
    pub style: Option<MathStyle>,
    /// `m:aln` — the argument alignment override.
    pub alignment: Option<MathAlignment>,
    /// `m:brk` — the break requested before this run (kept verbatim; the
    /// renderer applies its own line breaking).
    pub r#break: Option<i32>,
    /// `m:ctrlPr/w:rPr` — the formatting applied to the whole construct.
    pub control: Option<Box<RunProperties>>,
}

/// A math run (`m:r`) with its text (`m:t`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathRun {
    /// Run properties (`m:rPr`).
    pub properties: MathRunProperties,
    /// `w:rPr` — the character formatting Word puts on a math run.
    ///
    /// ISO/IEC 29500-1 §22.1.2.79 gives `m:r` an optional `w:rPr` alongside
    /// `m:rPr`, and a Word document that styles a formula through the normal
    /// character properties writes one on most of its runs. Treating it as a
    /// foreign element made a third of a real document's runs report as
    /// unsupported.
    pub run_properties: Option<Box<RunProperties>>,
    /// The concatenated `m:t` text of the run.
    pub text: String,
    /// Source location.
    pub location: SourceLocation,
}

/// Properties of a math argument (`m:argPr`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArgumentProperties {
    /// `m:aln` (first edition `m:defJc`).
    pub alignment: Option<MathAlignment>,
    /// `m:brk`.
    pub r#break: Option<i32>,
    /// `m:lit`.
    pub literal: Option<bool>,
    /// `m:nor`.
    pub normal: Option<bool>,
    /// `m:scr`.
    pub script: Option<MathScript>,
    /// `m:sty`.
    pub style: Option<MathStyle>,
    /// `m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
}

/// A math argument (`m:e`, `m:num`, `m:den`, `m:deg`, `m:lim`, `m:fName`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathArgument {
    /// Argument properties.
    pub properties: ArgumentProperties,
    /// The argument's children.
    pub nodes: Vec<MathNode>,
    /// Source location.
    pub location: SourceLocation,
}

impl MathArgument {
    /// Returns the concatenated text of every run in the argument.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            out.push_str(&node.text());
        }
        out
    }
}

/// A fraction (`m:f`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fraction {
    /// `m:fPr/m:type` — `bar` (default), `skew`, `lin` or `noBar`.
    pub bar_type: Option<Arc<str>>,
    /// `m:fPr/m:smallFrac` (first edition) — reduce the vertical gaps.
    pub small_fraction: bool,
    /// `m:fPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The numerator.
    pub numerator: MathArgument,
    /// The denominator.
    pub denominator: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A radical (`m:rad`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Radical {
    /// `m:radPr/m:degHide` — hide the degree.
    pub hide_degree: bool,
    /// `m:radPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The degree (empty when hidden).
    pub degree: MathArgument,
    /// The radicand.
    pub radicand: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A superscript (`m:sSup`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Superscript {
    /// `m:sSupPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The base.
    pub base: MathArgument,
    /// The superscript.
    pub superscript: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A subscript (`m:sSub`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscript {
    /// `m:sSubPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The base.
    pub base: MathArgument,
    /// The subscript.
    pub subscript: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A combined subscript and superscript (`m:sSubSup`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubSuperscript {
    /// `m:sSubSupPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The base.
    pub base: MathArgument,
    /// The subscript.
    pub subscript: MathArgument,
    /// The superscript.
    pub superscript: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A pre-script base with sub- and superscript (`m:sPre`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreScript {
    /// `m:sPrePr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The subscript (left, below).
    pub subscript: MathArgument,
    /// The superscript (left, above).
    pub superscript: MathArgument,
    /// The base.
    pub base: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// An n-ary operator — sum, product, integral, union (`m:nary`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NaryOperator {
    /// `m:naryPr/m:chr/@m:val` — the operator character (default U+222B).
    pub chr: Option<char>,
    /// `m:naryPr/m:limLoc`.
    pub limit_location: Option<LimitLocation>,
    /// `m:naryPr/m:grow` — grow the operator to the operand.
    pub grow: Option<bool>,
    /// `m:naryPr/m:subHide` — hide the lower limit.
    pub hide_sub: bool,
    /// `m:naryPr/m:supHide` — hide the upper limit.
    pub hide_sup: bool,
    /// `m:naryPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The lower limit.
    pub subscript: MathArgument,
    /// The upper limit.
    pub superscript: MathArgument,
    /// The operand.
    pub operand: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A delimiter set (`m:d`).
///
/// The character fields hold the *effective* character: `None` means the
/// producer asked for no delimiter there (`<m:begChr m:val=""/>`), whereas an
/// absent `m:begChr` element yields the schema default `(` (applied by the
/// parser).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delimiter {
    /// `m:dPr/m:begChr/@m:val` (default `(`); `None` draws no opening delimiter.
    pub begin: Option<char>,
    /// `m:dPr/m:sepChr/@m:val` (default `|`); `None` draws no separator.
    pub separator: Option<char>,
    /// `m:dPr/m:endChr/@m:val` (default `)`); `None` draws no closing delimiter.
    pub end: Option<char>,
    /// `m:dPr/m:grow` — stretch the delimiters to the content.
    pub grow: Option<bool>,
    /// `m:dPr/m:shp/@m:val` — the shape, kept verbatim.
    pub shape: Option<Arc<str>>,
    /// `m:dPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The separated operands.
    pub arguments: Vec<MathArgument>,
    /// Source location.
    pub location: SourceLocation,
}

/// A function application (`m:func`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    /// `m:funcPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The function name (`m:fName`).
    pub name: MathArgument,
    /// The argument (`m:e`).
    pub argument: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A limit placed under (`m:limLow`) or over (`m:limUpp`) its base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathLimit {
    /// Whether the limit is placed over (true) or under (false) the base.
    pub above: bool,
    /// `m:limUppPr`/`m:limLowPr`/`m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The base (`m:e`).
    pub base: MathArgument,
    /// The limit (`m:lim`).
    pub limit: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// One matrix column specification (`m:mc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixColumn {
    /// `m:mcPr/m:mcJc` — the column justification.
    pub justification: Option<MathAlignment>,
    /// `m:mcPr/m:count` — how many grid columns this entry spans.
    pub count: Option<i32>,
}

/// A matrix (`m:m`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// `m:mPr/m:baseJc`.
    /// `m:mPr/m:baseJc` - a `CT_YAlign`, so the `ST_YAlign` vocabulary and not
    /// the justification one.
    pub base_justification: Option<MathVerticalAlign>,
    /// `m:mPr/m:plcHide`.
    pub hide_placeholders: bool,
    /// `m:mPr/m:cGpRule/@m:val` — the inter-column spacing rule.
    pub column_group_rule: Option<Arc<str>>,
    /// `m:mPr/m:cSp`.
    pub column_spacing: Option<i32>,
    /// `m:mPr/m:cGp`.
    pub column_group_spacing: Option<i32>,
    /// `m:mPr/m:rSpRule/@m:val`.
    pub row_spacing_rule: Option<Arc<str>>,
    /// `m:mPr/m:rSp`.
    pub row_spacing: Option<i32>,
    /// `m:mPr/m:mcs` — per-column specifications.
    pub columns: Vec<MatrixColumn>,
    /// `m:mPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The rows.
    pub rows: Vec<Vec<MathArgument>>,
    /// Source location.
    pub location: SourceLocation,
}

/// An equation array (`m:eqArr`) — a system of equations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationArray {
    /// `m:eqArrPr/m:baseJc`.
    /// `m:mPr/m:baseJc` - a `CT_YAlign`, so the `ST_YAlign` vocabulary and not
    /// the justification one.
    pub base_justification: Option<MathVerticalAlign>,
    /// `m:eqArrPr/m:maxDist`.
    pub max_distance: Option<i32>,
    /// `m:eqArrPr/m:objDist`.
    pub object_distance: Option<i32>,
    /// `m:eqArrPr/m:rSpRule/@m:val`.
    pub row_spacing_rule: Option<Arc<str>>,
    /// `m:eqArrPr/m:rSp`.
    pub row_spacing: Option<i32>,
    /// `m:eqArrPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The rows.
    pub rows: Vec<MathArgument>,
    /// Source location.
    pub location: SourceLocation,
}

/// An accent (`m:acc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accent {
    /// `m:accPr/m:chr/@m:val` (default U+0302).
    pub chr: Option<char>,
    /// `m:accPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The accented base.
    pub base: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A bar over or under its base (`m:bar`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    /// `m:barPr/m:pos`.
    pub position: Option<MathPosition>,
    /// `m:barPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The base.
    pub base: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A group character — braces, overbar, underbar (`m:groupChr`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupCharacter {
    /// `m:groupChrPr/m:chr/@m:val` (default U+23DF).
    pub chr: Option<char>,
    /// `m:groupChrPr/m:pos`.
    pub position: Option<MathPosition>,
    /// `m:groupChrPr/m:vertJc`.
    pub vertical_justification: Option<MathVerticalJc>,
    /// `m:groupChrPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The base.
    pub base: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A boxed expression (`m:box`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Boxed {
    /// `m:boxPr/m:aln`.
    pub alignment: Option<MathAlignment>,
    /// `m:boxPr/m:sp` — inner padding in points.
    pub spacing: Option<i32>,
    /// `m:boxPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The content.
    pub argument: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A bordered box (`m:borderBox`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorderBox {
    /// `m:borderBoxPr/m:algn`.
    pub alignment: Option<MathAlignment>,
    /// `m:borderBoxPr/m:sp`.
    pub spacing: Option<i32>,
    /// `m:borderBoxPr/m:shadow`.
    pub shadow: bool,
    /// `m:borderBoxPr/m:lines`.
    pub lines: Option<i32>,
    /// `m:borderBoxPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The content arguments.
    pub arguments: Vec<MathArgument>,
    /// Source location.
    pub location: SourceLocation,
}

/// A phantom — reserves space without drawing (`m:phant`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phantom {
    /// `m:phantPr/m:show`.
    pub show: bool,
    /// `m:phantPr/m:transp`.
    pub transparent: bool,
    /// `m:phantPr/m:zeroWid`.
    pub zero_width: bool,
    /// `m:phantPr/m:zeroAsc`.
    pub zero_ascent: bool,
    /// `m:phantPr/m:zeroDesc`.
    pub zero_descent: bool,
    /// `m:phantPr/m:showAll`.
    pub show_all: bool,
    /// `m:phantPr/m:rtl`.
    pub rtl: bool,
    /// `m:phantPr/m:ctrlPr/w:rPr`.
    pub control: Option<Box<RunProperties>>,
    /// The content.
    pub argument: MathArgument,
    /// Source location.
    pub location: SourceLocation,
}

/// A math construct the parser recognised but does not model in detail.
///
/// The name and location are preserved so the construct is reported with
/// `Partial`/`Unsupported` instead of being dropped silently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownMathNode {
    /// The element's local name.
    pub local: Arc<str>,
    /// Source location.
    pub location: SourceLocation,
}

/// One node of a formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathNode {
    /// A run (`m:r`).
    Run(MathRun),
    /// A fraction (`m:f`).
    Fraction(Box<Fraction>),
    /// A radical (`m:rad`).
    Radical(Box<Radical>),
    /// A superscript (`m:sSup`).
    Superscript(Box<Superscript>),
    /// A subscript (`m:sSub`).
    Subscript(Box<Subscript>),
    /// A combined subscript and superscript (`m:sSubSup`).
    SubSuperscript(Box<SubSuperscript>),
    /// A pre-script base (`m:sPre`).
    PreScript(Box<PreScript>),
    /// An n-ary operator (`m:nary`).
    NaryOperator(Box<NaryOperator>),
    /// A delimiter set (`m:d`).
    Delimiter(Box<Delimiter>),
    /// A function application (`m:func`).
    Function(Box<Function>),
    /// A limit (`m:limLow`/`m:limUpp`).
    Limit(Box<MathLimit>),
    /// A matrix (`m:m`).
    Matrix(Box<Matrix>),
    /// An equation array (`m:eqArr`).
    EquationArray(Box<EquationArray>),
    /// An accent (`m:acc`).
    Accent(Box<Accent>),
    /// A bar (`m:bar`).
    Bar(Box<Bar>),
    /// A group character (`m:groupChr`).
    GroupCharacter(Box<GroupCharacter>),
    /// A box (`m:box`).
    Boxed(Box<Boxed>),
    /// A bordered box (`m:borderBox`).
    BorderBox(Box<BorderBox>),
    /// A phantom (`m:phant`).
    Phantom(Box<Phantom>),
    /// An unmodelled construct.
    Unknown(Box<UnknownMathNode>),
}

impl MathNode {
    /// The OMML element name of the construct (for the support model).
    #[must_use]
    pub fn element_name(&self) -> &'static str {
        match self {
            Self::Run(_) => "m:r",
            Self::Fraction(_) => "m:f",
            Self::Radical(_) => "m:rad",
            Self::Superscript(_) => "m:sSup",
            Self::Subscript(_) => "m:sSub",
            Self::SubSuperscript(_) => "m:sSubSup",
            Self::PreScript(_) => "m:sPre",
            Self::NaryOperator(_) => "m:nary",
            Self::Delimiter(_) => "m:d",
            Self::Function(_) => "m:func",
            Self::Limit(limit) => {
                if limit.above {
                    "m:limUpp"
                } else {
                    "m:limLow"
                }
            }
            Self::Matrix(_) => "m:m",
            Self::EquationArray(_) => "m:eqArr",
            Self::Accent(_) => "m:acc",
            Self::Bar(_) => "m:bar",
            Self::GroupCharacter(_) => "m:groupChr",
            Self::Boxed(_) => "m:box",
            Self::BorderBox(_) => "m:borderBox",
            Self::Phantom(_) => "m:phant",
            Self::Unknown(_) => "m:unknown",
        }
    }

    /// The source location of the construct.
    #[must_use]
    pub fn location(&self) -> &SourceLocation {
        match self {
            Self::Run(run) => &run.location,
            Self::Fraction(node) => &node.location,
            Self::Radical(node) => &node.location,
            Self::Superscript(node) => &node.location,
            Self::Subscript(node) => &node.location,
            Self::SubSuperscript(node) => &node.location,
            Self::PreScript(node) => &node.location,
            Self::NaryOperator(node) => &node.location,
            Self::Delimiter(node) => &node.location,
            Self::Function(node) => &node.location,
            Self::Limit(node) => &node.location,
            Self::Matrix(node) => &node.location,
            Self::EquationArray(node) => &node.location,
            Self::Accent(node) => &node.location,
            Self::Bar(node) => &node.location,
            Self::GroupCharacter(node) => &node.location,
            Self::Boxed(node) => &node.location,
            Self::BorderBox(node) => &node.location,
            Self::Phantom(node) => &node.location,
            Self::Unknown(node) => &node.location,
        }
    }

    /// Returns the run, if this node is one.
    #[must_use]
    pub const fn as_run(&self) -> Option<&MathRun> {
        match self {
            Self::Run(run) => Some(run),
            _ => None,
        }
    }

    /// Visits this node and, recursively, every descendant argument.
    pub fn walk(&self, visit: &mut impl FnMut(&MathNode)) {
        visit(self);
        for argument in self.arguments() {
            for node in &argument.nodes {
                node.walk(visit);
            }
        }
    }

    /// Returns the arguments carried by the construct, in reading order.
    #[must_use]
    pub fn arguments(&self) -> Vec<&MathArgument> {
        match self {
            Self::Run(_) | Self::Unknown(_) => Vec::new(),
            Self::Fraction(node) => vec![&node.numerator, &node.denominator],
            Self::Radical(node) => vec![&node.degree, &node.radicand],
            Self::Superscript(node) => vec![&node.base, &node.superscript],
            Self::Subscript(node) => vec![&node.base, &node.subscript],
            Self::SubSuperscript(node) => {
                vec![&node.base, &node.subscript, &node.superscript]
            }
            Self::PreScript(node) => {
                vec![&node.subscript, &node.superscript, &node.base]
            }
            Self::NaryOperator(node) => {
                vec![&node.subscript, &node.superscript, &node.operand]
            }
            Self::Delimiter(node) => node.arguments.iter().collect(),
            Self::Function(node) => vec![&node.name, &node.argument],
            Self::Limit(node) => vec![&node.base, &node.limit],
            Self::Matrix(node) => node.rows.iter().flatten().collect(),
            Self::EquationArray(node) => node.rows.iter().collect(),
            Self::Accent(node) => vec![&node.base],
            Self::Bar(node) => vec![&node.base],
            Self::GroupCharacter(node) => vec![&node.base],
            Self::Boxed(node) => vec![&node.argument],
            Self::BorderBox(node) => node.arguments.iter().collect(),
            Self::Phantom(node) => vec![&node.argument],
        }
    }

    /// Returns the concatenated text of the subtree (the plain-text projection).
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.append_text(&mut out);
        out
    }

    fn append_text(&self, out: &mut String) {
        match self {
            Self::Run(run) => out.push_str(&run.text),
            Self::Unknown(_) => {}
            Self::Fraction(node) => {
                for child in &node.numerator.nodes {
                    child.append_text(out);
                }
                out.push('/');
                for child in &node.denominator.nodes {
                    child.append_text(out);
                }
            }
            Self::Delimiter(node) => {
                if let Some(begin) = node.begin {
                    out.push(begin);
                }
                for (index, argument) in node.arguments.iter().enumerate() {
                    if index > 0 {
                        if let Some(separator) = node.separator {
                            out.push(separator);
                        }
                    }
                    for child in &argument.nodes {
                        child.append_text(out);
                    }
                }
                if let Some(end) = node.end {
                    out.push(end);
                }
            }
            Self::Phantom(node) => {
                for child in &node.argument.nodes {
                    child.append_text(out);
                }
            }
            _ => {
                let separator = match self {
                    Self::Matrix(_) | Self::EquationArray(_) => " ; ",
                    _ => " ",
                };
                for (index, argument) in self.arguments().iter().enumerate() {
                    if index > 0 {
                        out.push_str(separator);
                    }
                    for child in &argument.nodes {
                        child.append_text(out);
                    }
                }
            }
        }
    }
}

/// A formula (`m:oMath`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathExpression {
    /// The top-level nodes.
    pub nodes: Vec<MathNode>,
    /// Source location.
    pub location: SourceLocation,
}

impl MathExpression {
    /// Returns the concatenated text of the formula.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            node.append_text(&mut out);
        }
        out
    }

    /// Visits every node of the formula, parents before children.
    pub fn walk(&self, visit: &mut impl FnMut(&MathNode)) {
        for node in &self.nodes {
            node.walk(visit);
        }
    }
}

/// Properties of a display formula (`m:oMathParaPr`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathParagraphProperties {
    /// `m:jc` — the justification of the display formula.
    pub justification: Option<MathJustification>,
}

/// A display formula (`m:oMathPara`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathParagraph {
    /// Paragraph-level math properties.
    pub properties: Option<MathParagraphProperties>,
    /// One or more `m:oMath` children (AUD-50).
    pub equations: Vec<MathExpression>,
    /// Source location.
    pub location: SourceLocation,
}

impl MathParagraph {
    /// First equation, if any (compatibility helper).
    #[must_use]
    pub fn expression(&self) -> Option<&MathExpression> {
        self.equations.first()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MathAlignment, MathExpression, MathJustification, MathNode, MathRun, MathRunProperties,
        MathScript, MathStyle, MathVerticalJc,
    };

    fn run(text: &str) -> MathNode {
        MathNode::Run(MathRun {
            properties: MathRunProperties::default(),
            run_properties: None,
            text: text.to_owned(),
            location: super::SourceLocation::default(),
        })
    }

    #[test]
    fn enumerations_parse_strict_lexical_forms() {
        assert_eq!(MathStyle::from_strict("bi"), Some(MathStyle::BoldItalic));
        assert_eq!(MathStyle::from_strict("nope"), None);
        assert_eq!(
            MathScript::from_strict("doubleStruck"),
            Some(MathScript::DoubleStruck)
        );
        assert_eq!(
            MathAlignment::from_strict("ctr"),
            Some(MathAlignment::Center)
        );
        assert_eq!(
            MathJustification::from_strict("centerGroup"),
            Some(MathJustification::CenterGroup)
        );
        assert_eq!(
            MathVerticalJc::from_strict("bot"),
            Some(MathVerticalJc::Bottom)
        );
        assert_eq!(MathStyle::Bold.as_str(), "b");
    }

    #[test]
    fn text_projection_walks_the_constructs() {
        let numerator = super::MathArgument {
            properties: super::ArgumentProperties::default(),
            nodes: vec![run("a")],
            location: super::SourceLocation::default(),
        };
        let denominator = super::MathArgument {
            properties: super::ArgumentProperties::default(),
            nodes: vec![run("b")],
            location: super::SourceLocation::default(),
        };
        let fraction = MathNode::Fraction(Box::new(super::Fraction {
            bar_type: None,
            small_fraction: false,
            control: None,
            numerator,
            denominator,
            location: super::SourceLocation::default(),
        }));
        let expression = MathExpression {
            nodes: vec![fraction],
            location: super::SourceLocation::default(),
        };
        assert_eq!(expression.text(), "a/b");
        let mut seen = Vec::new();
        expression.walk(&mut |node| seen.push(node.element_name().to_owned()));
        assert_eq!(seen, vec!["m:f", "m:r", "m:r"]);
    }

    #[test]
    fn element_names_identify_limit_direction() {
        let location = super::SourceLocation::default();
        let limit = MathNode::Limit(Box::new(super::MathLimit {
            above: true,
            control: None,
            base: super::MathArgument {
                properties: super::ArgumentProperties::default(),
                nodes: Vec::new(),
                location: location.clone(),
            },
            limit: super::MathArgument {
                properties: super::ArgumentProperties::default(),
                nodes: Vec::new(),
                location: location.clone(),
            },
            location: location.clone(),
        }));
        assert_eq!(limit.element_name(), "m:limUpp");
    }
}

#[cfg(test)]
mod y_align_tests {
    use super::MathVerticalAlign as V;

    /// The six values, and the round trip, stated as the schema states them.
    ///
    /// This is the test that would have caught the original defect, because the
    /// original table accepted `left`/`right` - the *justification* vocabulary -
    /// and rejected four of the six values this one is actually about.
    #[test]
    fn st_yalign_is_exactly_six_values_and_they_round_trip() {
        assert_eq!(
            V::VALUES,
            ["inline", "top", "center", "bottom", "inside", "outside"]
        );
        for value in V::VALUES {
            let parsed = V::from_strict(value).unwrap_or_else(|| panic!("{value}"));
            assert_eq!(parsed.to_string(), value, "{value} does not round trip");
        }
    }

    #[test]
    fn the_justification_vocabulary_is_not_this_one() {
        // `left` and `right` belong to `MathAlignment`; accepting them here is
        // how a matrix came to be laid out "inline" because someone wrote
        // `Left`.
        assert_eq!(V::from_strict("left"), None);
        assert_eq!(V::from_strict("right"), None);
        // `bot` is the spelling of `ST_TopBot`, a third vocabulary again.
        assert_eq!(V::from_strict("bot"), None);
    }
}
