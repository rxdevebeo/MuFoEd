//! OMML (Office Math Markup Language) parser — `STAGE-5C-TASK.md` §5.1.
//!
//! Strict-first: only `http://purl.oclc.org/ooxml/officeDocument/math` is
//! accepted. Every node carries a
//! [`SourceLocation`](strict_ooxml_core::error::SourceLocation), recursion is
//! bounded by the shared XML depth limit and by the per-formula budgets
//! [`ResourceLimits::max_math_nodes`] and
//! [`ResourceLimits::max_math_depth`], and unmodelled constructs are recorded in
//! the [`SupportModel`](crate::model::support::SupportModel) as
//! `Partial`/`Unsupported` instead of being dropped.
//!
//! A formula past either budget is skipped and reported, not refused: one hostile
//! formula must not cost the reader the document around it (AUD-06).

use std::sync::Arc;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::math::{
    Accent, ArgumentProperties, Bar, BorderBox, Boxed, Delimiter, EquationArray, Fraction,
    Function, GroupCharacter, LimitLocation, MathAlignment, MathArgument, MathExpression,
    MathJustification, MathLimit, MathNode, MathParagraph, MathParagraphProperties, MathPosition,
    MathRun, MathRunProperties, MathScript, MathStyle, MathVerticalAlign, MathVerticalJc, Matrix,
    MatrixColumn, NaryOperator, Phantom, PreScript, Radical, SubSuperscript, Subscript,
    Superscript, UnknownMathNode,
};
use crate::model::support::SupportStatus;

use super::{attr_in_ns, is_math, parse_i32, plain_attr, PartParser};

/// The per-formula budgets, taken from `ResourceLimits`.
///
/// `exhausted` is why these are a struct and not two `u32`s: once a formula is
/// past either budget the rest of it is skipped rather than refused, so the
/// reason has to survive the rest of the subtree for the report to name.
struct MathScope {
    depth: u32,
    nodes: u32,
    exhausted: Option<&'static str>,
}

impl MathScope {
    fn new() -> Self {
        Self {
            depth: 0,
            nodes: 0,
            exhausted: None,
        }
    }

    /// Whether the formula is already over a budget.
    fn is_exhausted(&self) -> bool {
        self.exhausted.is_some()
    }

    /// Spends one node, reporting whether the budget is gone.
    fn charge_node(&mut self, limit: u32) -> bool {
        self.nodes = self.nodes.saturating_add(1);
        self.nodes > limit
    }

    /// Enters one level of nesting, reporting whether the budget is gone.
    fn enter(&mut self, limit: u32) -> bool {
        self.depth = self.depth.saturating_add(1);
        self.depth > limit
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Records that a budget ran out, keeping the first reason.
    fn exhaust(&mut self, reason: &'static str) {
        if self.exhausted.is_none() {
            self.exhausted = Some(reason);
        }
    }
}

/// The message a report carries when a formula is over a budget.
const OVER_BUDGET: &str = "formula exceeds max_math_nodes/max_math_depth";

/// Parses `m:oMath` (its start element has been consumed).
pub(crate) fn parse_omath(parser: &mut PartParser<'_>) -> Result<MathExpression> {
    let location = parser.location();
    let mut scope = MathScope::new();
    let nodes = parse_omath_elements(parser, &mut scope)?;
    if scope.is_exhausted() {
        // One formula past its budget costs that formula and nothing else: the
        // model keeps a node the report names, and the rest of the paragraph -
        // the text around it, the next paragraph, the rest of the document -
        // is read as if the formula had been short. Refusing the document would
        // make a hostile formula a denial of service against a file whose text
        // is perfectly readable.
        let reason = format!("{OVER_BUDGET} ({})", scope.exhausted.unwrap_or("exhausted"));
        parser.record(
            "m:oMath",
            SupportStatus::Unsupported,
            Some(reason),
            Some(location.clone()),
        );
        return Ok(MathExpression {
            nodes: vec![MathNode::Unknown(Box::new(UnknownMathNode {
                local: std::sync::Arc::from("oMath"),
                location: location.clone(),
            }))],
            location,
        });
    }
    parser.record(
        "m:oMath",
        SupportStatus::Supported,
        None,
        Some(location.clone()),
    );
    Ok(MathExpression { nodes, location })
}

/// Parses `m:oMathPara` (its start element has been consumed).
pub(crate) fn parse_omath_para(parser: &mut PartParser<'_>) -> Result<MathParagraph> {
    let location = parser.location();
    parser.enter()?;
    let mut properties = None;
    let mut expressions = Vec::new();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "oMathParaPr" => properties = Some(parse_omath_para_pr(parser)?),
                    "oMath" => expressions.push(parse_omath(parser)?),
                    other => {
                        parser.record(
                            &format!("m:{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:oMathPara".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:oMathPara")),
        }
    }
    parser.leave();

    // The schema allows one or more `m:oMath` children; several of them are
    // flattened into one display formula. A producer that omits it entirely
    // still yields an (empty) formula, recorded as `Partial`.
    let expression = if expressions.len() == 1 {
        expressions.remove(0)
    } else {
        if expressions.is_empty() {
            parser.record(
                "m:oMathPara",
                SupportStatus::Partial,
                Some("display formula without m:oMath".to_owned()),
                Some(location.clone()),
            );
        }
        let nodes = expressions
            .into_iter()
            .flat_map(|expression| expression.nodes)
            .collect();
        MathExpression {
            nodes,
            location: location.clone(),
        }
    };
    parser.record(
        "m:oMathPara",
        SupportStatus::Supported,
        None,
        Some(location.clone()),
    );
    Ok(MathParagraph {
        properties,
        expression,
        location,
    })
}

/// Parses `m:oMathParaPr` (its start element has been consumed).
fn parse_omath_para_pr(parser: &mut PartParser<'_>) -> Result<MathParagraphProperties> {
    let mut justification = None;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => {
                if name.local() == "jc" {
                    // `parse_enum` consumes the element.
                    justification = Some(parse_enum(
                        parser,
                        "m:oMathParaPr/m:jc",
                        &attrs,
                        MathJustification::from_strict,
                        MathJustification::Default,
                    )?);
                } else {
                    parser.skip_element()?;
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:oMathParaPr")),
        }
    }
    Ok(MathParagraphProperties { justification })
}

/// Parses the children of `m:oMath` / `m:e` / ... until the end element.
fn parse_omath_elements(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
) -> Result<Vec<MathNode>> {
    parser.enter()?;
    let mut nodes = Vec::new();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => {
                if scope.is_exhausted() {
                    // The formula is already over a budget: skip what is left of
                    // it, one element at a time, until its end tag. Skipping is
                    // the point - the alternative is walking a subtree that has
                    // already been found too large.
                    parser.skip_element()?;
                    continue;
                }
                if !is_math(&name) {
                    // `w:r` / `w:br` may appear inside `m:r`; anything else
                    // foreign is reported and skipped.
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                if let Some(node) = parse_math_node(parser, &name, &attrs, scope)? {
                    nodes.push(node);
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of math content")),
        }
    }
    parser.leave();
    Ok(nodes)
}

/// Parses one math element into a [`MathNode`].
#[allow(clippy::too_many_lines)]
fn parse_math_node(
    parser: &mut PartParser<'_>,
    name: &strict_ooxml_core::xml::qname::QName,
    _attrs: &[Attr],
    scope: &mut MathScope,
) -> Result<Option<MathNode>> {
    let location = parser.location();
    if scope.enter(parser.max_math_depth) {
        scope.exhaust("max_math_depth");
        parser.skip_element()?;
        return Ok(None);
    }
    if scope.charge_node(parser.max_math_nodes) {
        scope.exhaust("max_math_nodes");
        parser.skip_element()?;
        return Ok(None);
    }

    // `<m:r>` is the only construct whose text we read directly.
    let node = match name.local() {
        "r" => Some(MathNode::Run(parse_math_run(parser)?)),
        "f" => Some(MathNode::Fraction(Box::new(parse_fraction(
            parser, scope, location,
        )?))),
        "rad" => Some(MathNode::Radical(Box::new(parse_radical(
            parser, scope, location,
        )?))),
        "sSup" => Some(MathNode::Superscript(Box::new(parse_superscript(
            parser, scope, location,
        )?))),
        "sSub" => Some(MathNode::Subscript(Box::new(parse_subscript(
            parser, scope, location,
        )?))),
        "sSubSup" => Some(MathNode::SubSuperscript(Box::new(parse_sub_superscript(
            parser, scope, location,
        )?))),
        "sPre" => Some(MathNode::PreScript(Box::new(parse_pre_script(
            parser, scope, location,
        )?))),
        "nary" => Some(MathNode::NaryOperator(Box::new(parse_nary(
            parser, scope, location,
        )?))),
        "d" => Some(MathNode::Delimiter(Box::new(parse_delimiter(
            parser, scope, location,
        )?))),
        "func" => Some(MathNode::Function(Box::new(parse_function(
            parser, scope, location,
        )?))),
        "limLow" => Some(MathNode::Limit(Box::new(parse_limit(
            parser, false, scope, location,
        )?))),
        "limUpp" => Some(MathNode::Limit(Box::new(parse_limit(
            parser, true, scope, location,
        )?))),
        "m" => Some(MathNode::Matrix(Box::new(parse_matrix(
            parser, scope, location,
        )?))),
        "eqArr" => Some(MathNode::EquationArray(Box::new(parse_eq_array(
            parser, scope, location,
        )?))),
        "acc" => Some(MathNode::Accent(Box::new(parse_accent(
            parser, scope, location,
        )?))),
        "bar" => Some(MathNode::Bar(Box::new(parse_bar(parser, scope, location)?))),
        "groupChr" => Some(MathNode::GroupCharacter(Box::new(parse_group_chr(
            parser, scope, location,
        )?))),
        "box" => Some(MathNode::Boxed(Box::new(parse_box(
            parser, scope, location,
        )?))),
        "borderBox" => Some(MathNode::BorderBox(Box::new(parse_border_box(
            parser, scope, location,
        )?))),
        "phant" => Some(MathNode::Phantom(Box::new(parse_phantom(
            parser, scope, location,
        )?))),
        other => {
            let local = parser.intern(other);
            parser.record(
                &format!("m:{other}"),
                SupportStatus::Partial,
                Some("unmodelled OMML construct preserved verbatim".to_owned()),
                Some(location.clone()),
            );
            parser.skip_element()?;
            Some(MathNode::Unknown(Box::new(UnknownMathNode {
                local,
                location,
            })))
        }
    };
    scope.leave();
    Ok(node)
}

/// Parses `m:r` (its start element has been consumed).
fn parse_math_run(parser: &mut PartParser<'_>) -> Result<MathRun> {
    let location = parser.location();
    parser.enter()?;
    let mut properties = MathRunProperties::default();
    let mut run_properties: Option<Box<crate::model::props::RunProperties>> = None;
    let mut text = String::new();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    // `w:rPr` is a legitimate child of `m:r`
                    // (ISO/IEC 29500-1 §22.1.2.79) and carries the character
                    // formatting Word applies to a math run. It used to fall
                    // through to `record_foreign` and be reported
                    // unsupported, which a real document trips on most of its
                    // formula runs.
                    if name.local() == "rPr" && crate::parse::is_wml(&name) {
                        // `parse_run_properties` consumes the element and its
                        // end tag, so the shared `skip_element` below must
                        // not run for it — doing so ate the *next* sibling,
                        // and the damage only showed up as a truncated
                        // document several constructs later.
                        run_properties = Some(Box::new(parser.parse_run_properties()?));
                        continue;
                    }
                    // `w:br` inside a math run is a line break: represent it
                    // as a newline so the renderer honours it.
                    if name.local() == "br" && crate::parse::is_wml(&name) {
                        text.push('\n');
                    } else {
                        parser.record_foreign(&name);
                    }
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "rPr" => properties = parse_math_run_properties(parser)?,
                    "t" => text.push_str(&parser.parse_math_text()?),
                    other => {
                        parser.record(
                            &format!("m:{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:r".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:r")),
        }
    }
    parser.leave();
    Ok(MathRun {
        properties,
        run_properties,
        text,
        location,
    })
}

/// Reads the character data of `m:t` (its start element has been consumed).
trait MathText {
    fn parse_math_text(&mut self) -> Result<String>;
}

impl MathText for PartParser<'_> {
    fn parse_math_text(&mut self) -> Result<String> {
        self.nested(|parser| {
            let mut text = String::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::Text(chunk) | XmlEvent::CData(chunk) => text.push_str(&chunk),
                    XmlEvent::StartElement { name, .. } => {
                        parser.record_foreign(&name);
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:t")),
                }
            }
            Ok(text)
        })
    }
}

/// Parses `m:rPr` (its start element has been consumed).
fn parse_math_run_properties(parser: &mut PartParser<'_>) -> Result<MathRunProperties> {
    let mut properties = MathRunProperties::default();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "lit" => {
                    properties.literal = math_leaf(parser, &attrs, math_on_off)?;
                }
                "nor" => {
                    properties.normal = math_leaf(parser, &attrs, math_on_off)?;
                }
                "scr" => {
                    properties.script = Some(parse_enum(
                        parser,
                        "m:rPr/m:scr",
                        &attrs,
                        MathScript::from_strict,
                        MathScript::Roman,
                    )?);
                }
                "sty" => {
                    properties.style = Some(parse_enum(
                        parser,
                        "m:rPr/m:sty",
                        &attrs,
                        MathStyle::from_strict,
                        MathStyle::Italic,
                    )?);
                }
                "aln" | "defJc" => {
                    properties.alignment = Some(parse_enum(
                        parser,
                        "m:rPr/m:aln",
                        &attrs,
                        MathAlignment::from_strict,
                        MathAlignment::Unset,
                    )?);
                }
                "brk" => {
                    properties.r#break = math_leaf(parser, &attrs, math_int)?;
                }
                "ctrlPr" => properties.control = parse_ctrl_pr(parser)?,
                other => {
                    parser.record(
                        &format!("m:rPr/{other}"),
                        SupportStatus::Partial,
                        Some("unmodelled math run property".to_owned()),
                        Some(parser.location()),
                    );
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:rPr")),
        }
    }
    Ok(properties)
}

/// Parses `m:argPr` (its start element has been consumed).
fn parse_arg_properties(parser: &mut PartParser<'_>) -> Result<ArgumentProperties> {
    let mut properties = ArgumentProperties::default();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "aln" | "defJc" => {
                    properties.alignment = Some(parse_enum(
                        parser,
                        "m:argPr/m:aln",
                        &attrs,
                        MathAlignment::from_strict,
                        MathAlignment::Unset,
                    )?);
                }
                "brk" => properties.r#break = math_leaf(parser, &attrs, math_int)?,
                "lit" => properties.literal = Some(math_leaf(parser, &attrs, math_on_off)?),
                "nor" => properties.normal = Some(math_leaf(parser, &attrs, math_on_off)?),
                "scr" => {
                    properties.script = Some(parse_enum(
                        parser,
                        "m:argPr/m:scr",
                        &attrs,
                        MathScript::from_strict,
                        MathScript::Roman,
                    )?);
                }
                "sty" => {
                    properties.style = Some(parse_enum(
                        parser,
                        "m:argPr/m:sty",
                        &attrs,
                        MathStyle::from_strict,
                        MathStyle::Italic,
                    )?);
                }
                "ctrlPr" => properties.control = parse_ctrl_pr(parser)?,
                other => {
                    parser.record(
                        &format!("m:argPr/{other}"),
                        SupportStatus::Partial,
                        Some("unmodelled math argument property".to_owned()),
                        Some(parser.location()),
                    );
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:argPr")),
        }
    }
    Ok(properties)
}

/// Parses `m:ctrlPr` (its start element has been consumed).
///
/// `m:ctrlPr` carries an optional `w:ins`/`w:del` revision marker and a
/// `w:rPr`; the revision marker is recorded and dropped.
fn parse_ctrl_pr(
    parser: &mut PartParser<'_>,
) -> Result<Option<Box<crate::model::props::RunProperties>>> {
    let mut control = None;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if name.local() == "rPr" && crate::parse::is_wml(&name) {
                    control = Some(Box::new(parser.parse_run_properties()?));
                } else if matches!(name.local(), "ins" | "del") && crate::parse::is_wml(&name) {
                    parser.record(
                        "m:ctrlPr/w:ins|w:del",
                        SupportStatus::Partial,
                        Some("revision marker on math control properties".to_owned()),
                        Some(parser.location()),
                    );
                    parser.skip_element()?;
                } else {
                    parser.skip_element()?;
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:ctrlPr")),
        }
    }
    Ok(control)
}

/// Parses a `CT_OMathArg` argument (`m:num`, `m:den`, `m:e`, `m:deg`, `m:sub`,
/// `m:sup`, `m:lim`, `m:fName`, ...).
///
/// Every one of these elements *is* the argument: its start element has been
/// consumed by the caller, so the loop reads `m:argPr?` and the math elements
/// up to that element's end tag.
fn parse_argument(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    optional: bool,
) -> Result<Option<MathArgument>> {
    let location = parser.location();
    parser.enter()?;
    // Entering is *not* an early return here: the argument's start element has
    // already been consumed by the caller, so bailing out now would leave the
    // reader inside it and the caller's loop would read this element's end tag as
    // its own. The loop below skips forward to the argument's end instead, which
    // keeps every reader position where the shape of the markup says it is.
    if scope.enter(parser.max_math_depth) {
        scope.exhaust("max_math_depth");
    }
    let mut properties = ArgumentProperties::default();
    let mut nodes = Vec::new();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => {
                if scope.is_exhausted() {
                    parser.skip_element()?;
                    continue;
                }
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                if name.local() == "argPr" {
                    properties = parse_arg_properties(parser)?;
                    continue;
                }
                if let Some(node) = parse_math_node(parser, &name, &attrs, scope)? {
                    nodes.push(node);
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of a math argument")),
        }
    }
    parser.leave();
    scope.leave();
    if optional && nodes.is_empty() {
        return Ok(None);
    }
    Ok(Some(MathArgument {
        properties,
        nodes,
        location,
    }))
}

/// Parses one required `m:e` argument; an empty `m:e` is kept.
fn required_argument(parser: &mut PartParser<'_>, scope: &mut MathScope) -> Result<MathArgument> {
    Ok(
        parse_argument(parser, scope, false)?.unwrap_or_else(|| MathArgument {
            properties: ArgumentProperties::default(),
            nodes: Vec::new(),
            location: parser.location(),
        }),
    )
}

/// Parses one optional `m:e` argument.
fn optional_argument(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
) -> Result<Option<MathArgument>> {
    parse_argument(parser, scope, true)
}

/// Parses the arguments of a repeated-argument construct (`m:mr`).
fn argument_list(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    feature: &str,
) -> Result<Vec<MathArgument>> {
    let mut arguments = Vec::new();
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                if name.local() == "e" {
                    if let Some(argument) = parse_argument(parser, scope, false)? {
                        arguments.push(argument);
                    }
                    continue;
                }
                parser.record(
                    &format!("{feature}/{}", name.local()),
                    SupportStatus::Partial,
                    Some("unexpected child in a repeated math argument".to_owned()),
                    Some(parser.location()),
                );
                parser.skip_element()?;
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of a math construct")),
        }
    }
    Ok(arguments)
}

/// Parses the single optional property element (`m:*Pr`) of a construct.
///
/// The property element is consumed *completely* (its start element, its
/// children and its end tag), so the caller's loop resumes at the next sibling
/// of the enclosing construct. A producer that repeats the element gets the
/// first one parsed and the rest recorded as `Partial`.
fn parse_properties(
    parser: &mut PartParser<'_>,
    element: &str,
    mut skip_children: impl FnMut(&mut PartParser<'_>) -> Result<()>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if name.local() == element {
                    return skip_children(parser);
                }
                parser.skip_element()?;
            }
            // The property element is optional: the enclosing construct ends
            // here, so there is nothing to parse.
            XmlEvent::EndElement { .. } => return Ok(()),
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid(format!("unexpected end of m:{element}"))),
        }
    }
}

/// Parses `m:f` (its start element has been consumed).
fn parse_fraction(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Fraction> {
    let mut bar_type = None;
    let mut small_fraction = false;
    let mut control = None;
    let mut numerator = empty_argument(&location);
    let mut denominator = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "fPr" => parse_fraction_properties(
                        parser,
                        &mut bar_type,
                        &mut small_fraction,
                        &mut control,
                    )?,
                    "num" => numerator = required_argument(parser, scope)?,
                    "den" => denominator = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:f/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:f".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:f")),
        }
    }
    parser.leave();
    Ok(Fraction {
        bar_type: bar_type.map(Arc::from),
        small_fraction,
        control,
        numerator,
        denominator,
        location,
    })
}

/// An empty argument at a fixed location.
fn empty_argument(location: &strict_ooxml_core::error::SourceLocation) -> MathArgument {
    MathArgument {
        properties: ArgumentProperties::default(),
        nodes: Vec::new(),
        location: location.clone(),
    }
}

/// Parses the children of `m:fPr` (its start element has been consumed).
fn parse_fraction_properties(
    parser: &mut PartParser<'_>,
    bar_type: &mut Option<String>,
    small_fraction: &mut bool,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "type" => {
                    let raw = math_val(&attrs);
                    if let Some(value) = raw {
                        if matches!(
                            // Strict `ST_FType` is `bar|skw|lin|noBar` (ECMA-376 Part 1,
                            // `shared-math.xsd`); `skew` is the Transitional spelling. Accepting
                            // only the Transitional one made a Strict document carrying `skw`
                            // - the spelling the schema actually defines - report `Partial`
                            // and fall back to `bar`.
                            value.as_str(),
                            "bar" | "skw" | "skew" | "lin" | "noBar",
                        ) {
                            *bar_type = Some(value);
                        } else {
                            parser.record_enum("m:fPr/m:type", &value, &parser.location());
                            *bar_type = Some("bar".to_owned());
                        }
                    }
                    parser.skip_element()?;
                }
                "smallFrac" => {
                    *small_fraction = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:fPr")),
        }
    }
    Ok(())
}

/// Parses `m:rad` (its start element has been consumed).
fn parse_radical(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Radical> {
    let mut hide_degree = false;
    let mut control = None;
    let mut degree = empty_argument(&location);
    let mut radicand = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "radPr" => parse_radical_properties(parser, &mut hide_degree, &mut control)?,
                    "deg" => degree = required_argument(parser, scope)?,
                    "e" => radicand = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:rad/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:rad".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:rad")),
        }
    }
    parser.leave();
    Ok(Radical {
        hide_degree,
        control,
        degree,
        radicand,
        location,
    })
}

/// Parses the children of `m:radPr` (its start element has been consumed).
fn parse_radical_properties(
    parser: &mut PartParser<'_>,
    hide_degree: &mut bool,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "degHide" => {
                    *hide_degree = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:radPr")),
        }
    }
    Ok(())
}

/// Parses a `m:sSup`/`m:sSub`/`m:sSubSup`-shaped construct whose property
/// element is `property` and which carries `(property, name, property, name)`.
fn parse_script(
    parser: &mut PartParser<'_>,
    property: &str,
    scope: &mut MathScope,
    first: &str,
    second: &str,
    with_base: bool,
) -> Result<(
    Option<Box<crate::model::props::RunProperties>>,
    Vec<MathArgument>,
)> {
    let mut control = None;
    let mut arguments = Vec::new();
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                let local = name.local();
                if local == property {
                    parse_properties(parser, property, |parser| {
                        parse_control_only(parser, &mut control)
                    })?;
                } else if with_base && local == "e" {
                    arguments.push(required_argument(parser, scope)?);
                } else if local == first || local == second {
                    if let Some(argument) = optional_argument(parser, scope)? {
                        arguments.push(argument);
                    }
                } else {
                    parser.record(
                        &format!("m:{local}"),
                        SupportStatus::Partial,
                        Some(format!("unexpected child of m:{property}")),
                        Some(parser.location()),
                    );
                    parser.skip_element()?;
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid(format!("unexpected end of m:{property}"))),
        }
    }
    parser.leave();
    Ok((control, arguments))
}

/// Parses the children of a property element that only carries `m:ctrlPr`.
fn parse_control_only(
    parser: &mut PartParser<'_>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if name.local() == "ctrlPr" {
                    *control = parse_ctrl_pr(parser)?;
                } else {
                    parser.skip_element()?;
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of math properties")),
        }
    }
    Ok(())
}

/// Parses `m:sSup` (its start element has been consumed).
fn parse_superscript(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Superscript> {
    let (control, mut arguments) = parse_script(parser, "sSupPr", scope, "sup", "", true)?;
    let base = take_argument(parser, &mut arguments, "m:sSup", "base", &location);
    let superscript = take_argument(parser, &mut arguments, "m:sSup", "superscript", &location);
    Ok(Superscript {
        control,
        base,
        superscript,
        location,
    })
}

/// Takes the `index`-th argument of a script construct, or records the
/// construct as partial and substitutes an empty one.
/// The script constructs have a fixed arity in the schema, so a producer that
/// writes `m:sSub` without its subscript is malformed. It used to panic:
/// `arguments.remove(0)` on a short vector, reached by a real document in the
/// Transitional corpus. A malformed document is a `Partial` feature and a
/// rendered page, not a crash — which is what SC-5 asks for.
fn take_argument(
    parser: &mut PartParser<'_>,
    arguments: &mut Vec<MathArgument>,
    construct: &str,
    part: &str,
    location: &strict_ooxml_core::error::SourceLocation,
) -> MathArgument {
    if !arguments.is_empty() {
        return arguments.remove(0);
    }
    parser.record(
        construct,
        SupportStatus::Partial,
        Some(format!("{construct} has no {part} argument")),
        Some(location.clone()),
    );
    MathArgument {
        properties: crate::model::math::ArgumentProperties::default(),
        nodes: Vec::new(),
        location: location.clone(),
    }
}

/// Parses `m:sSub` (its start element has been consumed).
fn parse_subscript(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Subscript> {
    let (control, mut arguments) = parse_script(parser, "sSubPr", scope, "sub", "", true)?;
    let base = take_argument(parser, &mut arguments, "m:sSub", "base", &location);
    let subscript = take_argument(parser, &mut arguments, "m:sSub", "subscript", &location);
    Ok(Subscript {
        control,
        base,
        subscript,
        location,
    })
}

/// Parses `m:sSubSup` (its start element has been consumed).
fn parse_sub_superscript(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<SubSuperscript> {
    let (control, mut arguments) = parse_script(parser, "sSubSupPr", scope, "sub", "sup", true)?;
    let base = take_argument(parser, &mut arguments, "m:sSubSup", "base", &location);
    let subscript = take_argument(parser, &mut arguments, "m:sSubSup", "subscript", &location);
    let superscript = take_argument(
        parser,
        &mut arguments,
        "m:sSubSup",
        "superscript",
        &location,
    );
    Ok(SubSuperscript {
        control,
        base,
        subscript,
        superscript,
        location,
    })
}

/// Parses `m:sPre` (its start element has been consumed).
fn parse_pre_script(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<PreScript> {
    let mut control = None;
    let mut subscript = empty_argument(&location);
    let mut superscript = empty_argument(&location);
    let mut base = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "sPrePr" => parse_control_only(parser, &mut control)?,
                    "sub" => subscript = required_argument(parser, scope)?,
                    "sup" => superscript = required_argument(parser, scope)?,
                    "e" => base = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:sPre/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:sPre".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:sPre")),
        }
    }
    parser.leave();
    Ok(PreScript {
        control,
        subscript,
        superscript,
        base,
        location,
    })
}

/// Parses `m:nary` (its start element has been consumed).
fn parse_nary(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<NaryOperator> {
    let mut chr = None;
    let mut limit_location = None;
    let mut grow = None;
    let mut no_lower_limit = false;
    let mut no_upper_limit = false;
    let mut control = None;
    let mut subscript = empty_argument(&location);
    let mut superscript = empty_argument(&location);
    let mut operand = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "naryPr" => parse_nary_properties(
                        parser,
                        &mut chr,
                        &mut limit_location,
                        &mut grow,
                        &mut no_lower_limit,
                        &mut no_upper_limit,
                        &mut control,
                    )?,
                    "sub" => subscript = required_argument(parser, scope)?,
                    "sup" => superscript = required_argument(parser, scope)?,
                    "e" => operand = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:nary/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:nary".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:nary")),
        }
    }
    parser.leave();
    Ok(NaryOperator {
        chr,
        limit_location,
        grow,
        hide_sub: no_lower_limit,
        hide_sup: no_upper_limit,
        control,
        subscript,
        superscript,
        operand,
        location,
    })
}

/// Parses the children of `m:naryPr` (its start element has been consumed).
#[allow(clippy::too_many_arguments)]
fn parse_nary_properties(
    parser: &mut PartParser<'_>,
    chr: &mut Option<char>,
    limit_location: &mut Option<LimitLocation>,
    grow: &mut Option<bool>,
    no_lower_limit: &mut bool,
    no_upper_limit: &mut bool,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "chr" => {
                    *chr = math_char(parser, &attrs)?;
                }
                "limLoc" => {
                    // `parse_enum` consumes the element.
                    *limit_location = Some(parse_enum(
                        parser,
                        "m:naryPr/m:limLoc",
                        &attrs,
                        LimitLocation::from_strict,
                        LimitLocation::UnderOver,
                    )?);
                }
                "grow" => {
                    *grow = Some(math_on_off(&attrs));
                    parser.skip_element()?;
                }
                "subHide" => {
                    *no_lower_limit = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "supHide" => {
                    *no_upper_limit = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:naryPr")),
        }
    }
    Ok(())
}

/// Parses `m:d` (its start element has been consumed).
fn parse_delimiter(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Delimiter> {
    let mut begin = Some('(');
    let mut separator = Some('|');
    let mut end = Some(')');
    let mut grow = None;
    let mut shape = None;
    let mut control = None;
    let mut arguments = Vec::new();
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "dPr" => parse_delimiter_properties(
                        parser,
                        &mut begin,
                        &mut separator,
                        &mut end,
                        &mut grow,
                        &mut shape,
                        &mut control,
                    )?,
                    "e" => {
                        if let Some(argument) = parse_argument(parser, scope, false)? {
                            arguments.push(argument);
                        }
                    }
                    other => {
                        parser.record(
                            &format!("m:d/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:d".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:d")),
        }
    }
    parser.leave();
    Ok(Delimiter {
        begin,
        separator,
        end,
        grow,
        shape: shape.map(Arc::from),
        control,
        arguments,
        location,
    })
}

/// Parses the children of `m:dPr` (its start element has been consumed).
///
/// The three character elements are tri-state: an absent element keeps the
/// schema default, `<m:begChr m:val=""/>` asks for *no* delimiter, and any other
/// value is the character to draw.
#[allow(clippy::too_many_arguments)]
fn parse_delimiter_properties(
    parser: &mut PartParser<'_>,
    begin: &mut Option<char>,
    separator: &mut Option<char>,
    end: &mut Option<char>,
    grow: &mut Option<bool>,
    shape: &mut Option<String>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "begChr" => {
                    let attrs = attrs.clone();
                    *begin = delimiter_char(parser, &attrs, *begin)?;
                }
                "sepChr" => {
                    let attrs = attrs.clone();
                    *separator = delimiter_char(parser, &attrs, *separator)?;
                }
                "endChr" => {
                    let attrs = attrs.clone();
                    *end = delimiter_char(parser, &attrs, *end)?;
                }
                "grow" => {
                    *grow = Some(math_on_off(&attrs));
                    parser.skip_element()?;
                }
                "shp" => {
                    *shape = math_val(&attrs);
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:dPr")),
        }
    }
    Ok(())
}

/// Reads a `ST_Char` attribute of a delimiter element, consuming the element.
///
/// §22.1.2.36: `m:val` on `CT_Char` is optional and defaults to the delimiter's
/// own character, so `<m:begChr/>` means the default `(`, **not** "no
/// delimiter". Only an explicitly empty `m:val` suppresses it — that is how
/// Word writes a delimiter with no bracket at all.
///
/// The distinction matters beyond fidelity: `default` is what keeps the written
/// form stable. Reading a valueless `m:begChr` as "no delimiter" turned the
/// first write into `<m:dPr>` with no `m:begChr`; re-reading *that* restored
/// the seeded default and the second write spelled it out, so a document with a
/// valueless delimiter never reached a fixed point.
fn delimiter_char(
    parser: &mut PartParser<'_>,
    attrs: &[Attr],
    default: Option<char>,
) -> Result<Option<char>> {
    if math_val(attrs).is_none() {
        parser.record(
            "m:dPr/char",
            SupportStatus::Partial,
            Some("delimiter character element without m:val".to_owned()),
            Some(parser.location()),
        );
        parser.skip_element()?;
        return Ok(default);
    }
    // An empty `m:val` means "draw nothing"; the element is consumed once.
    math_char(parser, attrs)
}

/// Parses `m:func` (its start element has been consumed).
fn parse_function(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Function> {
    let mut control = None;
    let mut name = empty_argument(&location);
    let mut argument = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name: element, .. } => {
                if !is_math(&element) {
                    parser.record_foreign(&element);
                    parser.skip_element()?;
                    continue;
                }
                match element.local() {
                    "funcPr" => parse_control_only(parser, &mut control)?,
                    "fName" => {
                        // `m:fName` is itself a `CT_OMathArg`.
                        let value = parse_argument(parser, scope, false)?;
                        name = value.unwrap_or_else(|| empty_argument(&location));
                    }
                    "e" => argument = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:func/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:func".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:func")),
        }
    }
    parser.leave();
    Ok(Function {
        control,
        name,
        argument,
        location,
    })
}

/// Parses `m:limLow`/`m:limUpp` (its start element has been consumed).
fn parse_limit(
    parser: &mut PartParser<'_>,
    above: bool,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<MathLimit> {
    let property = if above { "limUppPr" } else { "limLowPr" };
    let mut control = None;
    let mut base = empty_argument(&location);
    let mut limit = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    local if local == property => {
                        parse_properties(parser, property, |parser| {
                            parse_control_only(parser, &mut control)
                        })?;
                    }
                    "e" => base = required_argument(parser, scope)?,
                    "lim" => limit = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:{property}/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of a math limit".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of a math limit")),
        }
    }
    parser.leave();
    Ok(MathLimit {
        above,
        control,
        base,
        limit,
        location,
    })
}

/// Parses `m:m` (its start element has been consumed).
fn parse_matrix(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Matrix> {
    let mut base_justification: Option<MathVerticalAlign> = None;
    let mut hide_placeholders = false;
    let mut column_group_rule = None;
    let mut column_spacing = None;
    let mut column_group_spacing = None;
    let mut row_spacing_rule = None;
    let mut row_spacing = None;
    let mut columns = Vec::new();
    let mut control = None;
    let mut rows: Vec<Vec<MathArgument>> = Vec::new();
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "mPr" => parse_matrix_properties(
                        parser,
                        &mut base_justification,
                        &mut hide_placeholders,
                        &mut column_group_rule,
                        &mut column_spacing,
                        &mut column_group_spacing,
                        &mut row_spacing_rule,
                        &mut row_spacing,
                        &mut columns,
                        &mut control,
                    )?,
                    "mr" => {
                        let cells = argument_list(parser, scope, "m:mr")?;
                        rows.push(cells);
                    }
                    other => {
                        parser.record(
                            &format!("m:m/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:m".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:m")),
        }
    }
    parser.leave();
    Ok(Matrix {
        base_justification,
        hide_placeholders,
        column_group_rule: column_group_rule.map(Arc::from),
        column_spacing,
        column_group_spacing,
        row_spacing_rule: row_spacing_rule.map(Arc::from),
        row_spacing,
        columns,
        control,
        rows,
        location,
    })
}

/// Parses the children of `m:mPr` (its start element has been consumed).
#[allow(clippy::too_many_arguments)]
fn parse_matrix_properties(
    parser: &mut PartParser<'_>,
    base_justification: &mut Option<MathVerticalAlign>,
    hide_placeholders: &mut bool,
    column_group_rule: &mut Option<String>,
    column_spacing: &mut Option<i32>,
    column_group_spacing: &mut Option<i32>,
    row_spacing_rule: &mut Option<String>,
    row_spacing: &mut Option<i32>,
    columns: &mut Vec<MatrixColumn>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "baseJc" => {
                    *base_justification = Some(parse_enum(
                        parser,
                        "m:mPr/m:baseJc",
                        &attrs,
                        MathVerticalAlign::from_strict,
                        MathVerticalAlign::Unset,
                    )?);
                }
                "plcHide" => {
                    *hide_placeholders = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "cGpRule" => {
                    *column_group_rule = math_val(&attrs);
                    parser.skip_element()?;
                }
                "cSp" => *column_spacing = math_leaf(parser, &attrs, math_int)?,
                "cGp" => *column_group_spacing = math_leaf(parser, &attrs, math_int)?,
                "rSpRule" => {
                    *row_spacing_rule = math_val(&attrs);
                    parser.skip_element()?;
                }
                "rSp" => *row_spacing = math_leaf(parser, &attrs, math_int)?,
                "mcs" => parse_matrix_columns(parser, columns)?,
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:mPr")),
        }
    }
    Ok(())
}

/// Parses the children of `m:mcs` (its start element has been consumed).
fn parse_matrix_columns(
    parser: &mut PartParser<'_>,
    columns: &mut Vec<MatrixColumn>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if name.local() == "mc" {
                    columns.push(parse_matrix_column(parser)?);
                } else {
                    parser.skip_element()?;
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:mcs")),
        }
    }
    Ok(())
}

/// Parses `m:mc` (its start element has been consumed).
fn parse_matrix_column(parser: &mut PartParser<'_>) -> Result<MatrixColumn> {
    let mut justification = None;
    let mut count = None;
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => match name.local() {
                "mcPr" => parse_matrix_column_properties(parser, &mut justification, &mut count)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:mc")),
        }
    }
    parser.leave();
    Ok(MatrixColumn {
        justification,
        count,
    })
}

/// Parses the children of `m:mcPr` (its start element has been consumed).
fn parse_matrix_column_properties(
    parser: &mut PartParser<'_>,
    justification: &mut Option<MathAlignment>,
    count: &mut Option<i32>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "mcJc" => {
                    *justification = Some(parse_enum(
                        parser,
                        "m:mcPr/m:mcJc",
                        &attrs,
                        MathAlignment::from_strict,
                        MathAlignment::Unset,
                    )?);
                }
                "count" => {
                    *count = math_int(&attrs);
                    parser.skip_element()?;
                }
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:mcPr")),
        }
    }
    Ok(())
}

/// Parses `m:eqArr` (its start element has been consumed).
fn parse_eq_array(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<EquationArray> {
    let mut base_justification: Option<MathVerticalAlign> = None;
    let mut max_distance = None;
    let mut object_distance = None;
    let mut row_spacing_rule = None;
    let mut row_spacing = None;
    let mut control = None;
    let mut rows = Vec::new();
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "eqArrPr" => parse_eq_array_properties(
                        parser,
                        &mut base_justification,
                        &mut max_distance,
                        &mut object_distance,
                        &mut row_spacing_rule,
                        &mut row_spacing,
                        &mut control,
                    )?,
                    "e" => rows.push(required_argument(parser, scope)?),
                    other => {
                        parser.record(
                            &format!("m:eqArr/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:eqArr".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:eqArr")),
        }
    }
    parser.leave();
    Ok(EquationArray {
        base_justification,
        max_distance,
        object_distance,
        row_spacing_rule: row_spacing_rule.map(Arc::from),
        row_spacing,
        control,
        rows,
        location,
    })
}

/// Parses the children of `m:eqArrPr` (its start element has been consumed).
#[allow(clippy::too_many_arguments)]
fn parse_eq_array_properties(
    parser: &mut PartParser<'_>,
    base_justification: &mut Option<MathVerticalAlign>,
    max_distance: &mut Option<i32>,
    object_distance: &mut Option<i32>,
    row_spacing_rule: &mut Option<String>,
    row_spacing: &mut Option<i32>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "baseJc" => {
                    *base_justification = Some(parse_enum(
                        parser,
                        "m:eqArrPr/m:baseJc",
                        &attrs,
                        MathVerticalAlign::from_strict,
                        MathVerticalAlign::Unset,
                    )?);
                }
                "maxDist" => *max_distance = math_leaf(parser, &attrs, math_int)?,
                "objDist" => *object_distance = math_leaf(parser, &attrs, math_int)?,
                "rSpRule" => {
                    *row_spacing_rule = math_val(&attrs);
                    parser.skip_element()?;
                }
                "rSp" => *row_spacing = math_leaf(parser, &attrs, math_int)?,
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:eqArrPr")),
        }
    }
    Ok(())
}

/// Parses `m:acc` (its start element has been consumed).
fn parse_accent(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Accent> {
    let mut chr = None;
    let mut control = None;
    let mut base = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, .. } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "accPr" => parse_accent_properties(parser, &mut chr, &mut control)?,
                    "e" => base = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:acc/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:acc".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:acc")),
        }
    }
    parser.leave();
    Ok(Accent {
        chr,
        control,
        base,
        location,
    })
}

/// Parses the children of `m:accPr` (its start element has been consumed).
fn parse_accent_properties(
    parser: &mut PartParser<'_>,
    chr: &mut Option<char>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "chr" => {
                    *chr = math_char(parser, &attrs)?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:accPr")),
        }
    }
    Ok(())
}

/// Parses `m:bar` (its start element has been consumed).
fn parse_bar(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Bar> {
    let mut position = None;
    let mut control = None;
    let mut base = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "barPr" => {
                        parse_position_properties(parser, "m:barPr", &mut position, &mut control)?;
                    }
                    "e" => base = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:bar/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:bar".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:bar")),
        }
    }
    parser.leave();
    Ok(Bar {
        position,
        control,
        base,
        location,
    })
}

/// Parses a property element with `m:pos` and `m:ctrlPr`.
fn parse_position_properties(
    parser: &mut PartParser<'_>,
    feature: &str,
    position: &mut Option<MathPosition>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "pos" => {
                    *position = Some(parse_enum(
                        parser,
                        &format!("{feature}/m:pos"),
                        &attrs,
                        MathPosition::from_strict,
                        MathPosition::Top,
                    )?);
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of math properties")),
        }
    }
    Ok(())
}

/// Parses `m:groupChr` (its start element has been consumed).
fn parse_group_chr(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<GroupCharacter> {
    let mut chr = None;
    let mut position = None;
    let mut vertical_justification = None;
    let mut control = None;
    let mut base = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "groupChrPr" => parse_group_chr_properties(
                        parser,
                        &mut chr,
                        &mut position,
                        &mut vertical_justification,
                        &mut control,
                    )?,
                    "e" => base = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:groupChr/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:groupChr".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:groupChr")),
        }
    }
    parser.leave();
    Ok(GroupCharacter {
        chr,
        position,
        vertical_justification,
        control,
        base,
        location,
    })
}

/// Parses the children of `m:groupChrPr` (its start element has been consumed).
fn parse_group_chr_properties(
    parser: &mut PartParser<'_>,
    chr: &mut Option<char>,
    position: &mut Option<MathPosition>,
    vertical_justification: &mut Option<MathVerticalJc>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "chr" => {
                    *chr = math_char(parser, &attrs)?;
                }
                "pos" => {
                    *position = Some(parse_enum(
                        parser,
                        "m:groupChrPr/m:pos",
                        &attrs,
                        MathPosition::from_strict,
                        MathPosition::Top,
                    )?);
                }
                "vertJc" => {
                    *vertical_justification = Some(parse_enum(
                        parser,
                        "m:groupChrPr/m:vertJc",
                        &attrs,
                        MathVerticalJc::from_strict,
                        MathVerticalJc::Bottom,
                    )?);
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:groupChrPr")),
        }
    }
    Ok(())
}

/// Parses `m:box` (its start element has been consumed).
fn parse_box(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Boxed> {
    let mut alignment = None;
    let mut spacing = None;
    let mut control = None;
    let mut argument = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "boxPr" => {
                        parse_box_properties(parser, &mut alignment, &mut spacing, &mut control)?;
                    }
                    "e" => argument = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:box/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:box".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:box")),
        }
    }
    parser.leave();
    Ok(Boxed {
        alignment,
        spacing,
        control,
        argument,
        location,
    })
}

/// Parses the children of `m:boxPr` (its start element has been consumed).
fn parse_box_properties(
    parser: &mut PartParser<'_>,
    alignment: &mut Option<MathAlignment>,
    spacing: &mut Option<i32>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "aln" => {
                    *alignment = Some(parse_enum(
                        parser,
                        "m:boxPr/m:aln",
                        &attrs,
                        MathAlignment::from_strict,
                        MathAlignment::Center,
                    )?);
                }
                "sp" => {
                    *spacing = math_int(&attrs);
                    parser.skip_element()?;
                }
                "cr" | "brk" => {
                    // Line-break placement inside a box is not modelled.
                    parser.record(
                        "m:boxPr/m:brk",
                        SupportStatus::Partial,
                        Some("in-box line breaks are laid out by the renderer".to_owned()),
                        Some(parser.location()),
                    );
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:boxPr")),
        }
    }
    Ok(())
}

/// Parses `m:borderBox` (its start element has been consumed).
fn parse_border_box(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<BorderBox> {
    let mut alignment = None;
    let mut spacing = None;
    let mut shadow = false;
    let mut lines = None;
    let mut control = None;
    let mut arguments = Vec::new();
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "borderBoxPr" => parse_border_box_properties(
                        parser,
                        &mut alignment,
                        &mut spacing,
                        &mut shadow,
                        &mut lines,
                        &mut control,
                    )?,
                    "e" => {
                        let argument = parse_argument(parser, scope, false)?
                            .unwrap_or_else(|| empty_argument(&location));
                        arguments.push(argument);
                    }
                    other => {
                        parser.record(
                            &format!("m:borderBox/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:borderBox".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:borderBox")),
        }
    }
    parser.leave();
    Ok(BorderBox {
        alignment,
        spacing,
        shadow,
        lines,
        control,
        arguments,
        location,
    })
}

/// Parses the children of `m:borderBoxPr` (its start element has been consumed).
fn parse_border_box_properties(
    parser: &mut PartParser<'_>,
    alignment: &mut Option<MathAlignment>,
    spacing: &mut Option<i32>,
    shadow: &mut bool,
    lines: &mut Option<i32>,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "algn" => {
                    *alignment = Some(parse_enum(
                        parser,
                        "m:borderBoxPr/m:algn",
                        &attrs,
                        MathAlignment::from_strict,
                        MathAlignment::Center,
                    )?);
                }
                "sp" => {
                    *spacing = math_int(&attrs);
                    parser.skip_element()?;
                }
                "shadow" => {
                    *shadow = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "lines" => {
                    *lines = math_int(&attrs);
                    parser.skip_element()?;
                }
                "cr" | "brk" => {
                    parser.record(
                        "m:borderBoxPr/m:brk",
                        SupportStatus::Partial,
                        Some("in-box line breaks are laid out by the renderer".to_owned()),
                        Some(parser.location()),
                    );
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:borderBoxPr")),
        }
    }
    Ok(())
}

/// Parses `m:phant` (its start element has been consumed).
fn parse_phantom(
    parser: &mut PartParser<'_>,
    scope: &mut MathScope,
    location: strict_ooxml_core::error::SourceLocation,
) -> Result<Phantom> {
    let mut show = false;
    let mut transparent = false;
    let mut zero_width = false;
    let mut zero_ascent = false;
    let mut zero_descent = false;
    let mut show_all = false;
    let mut rtl = false;
    let mut control = None;
    let mut argument = empty_argument(&location);
    parser.enter()?;
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs: _ } => {
                if !is_math(&name) {
                    parser.record_foreign(&name);
                    parser.skip_element()?;
                    continue;
                }
                match name.local() {
                    "phantPr" => parse_phantom_properties(
                        parser,
                        &mut show,
                        &mut transparent,
                        &mut zero_width,
                        &mut zero_ascent,
                        &mut zero_descent,
                        &mut show_all,
                        &mut rtl,
                        &mut control,
                    )?,
                    "e" => argument = required_argument(parser, scope)?,
                    other => {
                        parser.record(
                            &format!("m:phant/{other}"),
                            SupportStatus::Partial,
                            Some("unexpected child of m:phant".to_owned()),
                            Some(parser.location()),
                        );
                        parser.skip_element()?;
                    }
                }
            }
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:phant")),
        }
    }
    parser.leave();
    Ok(Phantom {
        show,
        transparent,
        zero_width,
        zero_ascent,
        zero_descent,
        show_all,
        rtl,
        control,
        argument,
        location,
    })
}

/// Parses the children of `m:phantPr` (its start element has been consumed).
#[allow(clippy::too_many_arguments)]
fn parse_phantom_properties(
    parser: &mut PartParser<'_>,
    show: &mut bool,
    transparent: &mut bool,
    zero_width: &mut bool,
    zero_ascent: &mut bool,
    zero_descent: &mut bool,
    show_all: &mut bool,
    rtl: &mut bool,
    control: &mut Option<Box<crate::model::props::RunProperties>>,
) -> Result<()> {
    loop {
        match parser.next_event()? {
            XmlEvent::StartElement { name, attrs } => match name.local() {
                "show" => {
                    *show = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "transp" => {
                    *transparent = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "zeroWid" => {
                    *zero_width = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "zeroAsc" => {
                    *zero_ascent = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "zeroDesc" => {
                    *zero_descent = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "showAll" => {
                    *show_all = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "rtl" => {
                    *rtl = math_on_off(&attrs);
                    parser.skip_element()?;
                }
                "ctrlPr" => *control = parse_ctrl_pr(parser)?,
                _ => {
                    parser.skip_element()?;
                }
            },
            XmlEvent::EndElement { .. } => break,
            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:phantPr")),
        }
    }
    Ok(())
}

/// Reads an `m:val` attribute.
///
/// The schema qualifies the attribute with the math prefix (`m:val`); a
/// handful of producers emit it unqualified, so both spellings are accepted.
fn math_val(attrs: &[Attr]) -> Option<String> {
    attr_in_ns(attrs, crate::MATH_STRICT_NS, "val")
        .or_else(|| plain_attr(attrs, "val"))
        .map(str::to_owned)
}

/// Reads a leaf OMML property element: its attributes, then its end tag.
///
/// The XML reader emits a **synthetic** `EndElement` for every self-closing tag
/// (`<m:grow m:val="1"/>`), so a property loop that stopped at the first
/// `EndElement` would end on the property element instead of on the property
/// container. Every leaf therefore goes through this helper, which consumes the
/// element (its end tag included) before the loop continues.
fn math_leaf<T>(
    parser: &mut PartParser<'_>,
    attrs: &[Attr],
    read: impl FnOnce(&[Attr]) -> T,
) -> Result<T> {
    let value = read(attrs);
    parser.skip_element()?;
    Ok(value)
}

/// Reads an `m:val` integer attribute (`ST_SignedInteger`).
fn math_int(attrs: &[Attr]) -> Option<i32> {
    math_val(attrs).as_deref().and_then(parse_i32)
}

/// Reads an `m:val` character attribute (`ST_Char`) and consumes the element.
///
/// The lexical space is a single Unicode code point; the `U+xxxx` spelling used
/// by some producers is accepted and reported as an out-of-schema value. An
/// absent or empty `m:val` yields `None` ("draw nothing"), which is how Word
/// writes `<m:begChr m:val=""/>`.
fn math_char(parser: &mut PartParser<'_>, attrs: &[Attr]) -> Result<Option<char>> {
    let raw = math_val(attrs);
    let character = match raw {
        None => None,
        Some(ref value) if value.chars().count() == 1 => value.chars().next(),
        Some(ref value) if value.is_empty() => None,
        Some(ref value) => {
            let parsed = value
                .strip_prefix("U+")
                .or_else(|| value.strip_prefix("u+"))
                .or_else(|| value.strip_prefix('#'))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .and_then(char::from_u32);
            if parsed.is_some() {
                parser.record_enum("m:chr/@m:val", value, &parser.location());
            }
            parsed
        }
    };
    parser.skip_element()?;
    Ok(character)
}

/// Reads an OMML on/off property value, defaulting to `true` when absent.
fn math_on_off(attrs: &[Attr]) -> bool {
    match math_val(attrs) {
        None => true,
        Some(value) => matches!(value.as_str(), "true" | "1" | "on"),
    }
}

/// Records an out-of-enumeration value as `Partial`.
///
/// A missing `m:val` is also reported: the schema default then applies, which
/// is observable behaviour the report must not hide.
fn record_bad_enum(parser: &mut PartParser<'_>, element: &str, value: Option<&str>) {
    let Some(value) = value else {
        parser.record(
            element,
            SupportStatus::Partial,
            Some("missing m:val; schema default applied".to_owned()),
            Some(parser.location()),
        );
        return;
    };
    parser.record_enum(element, value, &parser.location());
}

/// Parses an `m:val` enumerated leaf element and consumes it.
///
/// Returns the parsed value, or the schema default when `m:val` is missing or
/// outside the lexical space (the failure is recorded in the support model).
/// The element is always consumed, so the caller's loop advances correctly even
/// for a self-closing tag.
fn parse_enum<T>(
    parser: &mut PartParser<'_>,
    element: &str,
    attrs: &[Attr],
    from_strict: fn(&str) -> Option<T>,
    default: T,
) -> Result<T> {
    let raw = math_val(attrs);
    // An **absent** `m:val` and an **unrecognised** `m:val` are different
    // facts. Almost every `m:val` in OMML is optional with a schema default,
    // so an element that means "inherit" arrives with no value at all, and the
    // old code recorded that as a malformed enum and reported the document as
    // damaged when it was not. Only a value that is present and outside the
    // vocabulary is a loss.
    let value = match raw.as_deref() {
        None => default,
        Some(text) => {
            if let Some(value) = from_strict(text) {
                value
            } else {
                record_bad_enum(parser, element, Some(text));
                default
            }
        }
    };
    parser.skip_element()?;
    Ok(value)
}
