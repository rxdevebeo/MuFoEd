//! MathML projection of a parsed formula — `STAGE-5C-TASK.md` §9.6.
//!
//! The renderer draws formulas straight to SVG; this module provides the
//! independent *structural* view of the same model, so a formula can be checked
//! against an OMML oracle without parsing pixels (§7.2 "структурный оракул",
//! §3.2 — export is the only addition to the 5C scope).
//!
//! The mapping follows Presentation MathML 3 (W3C). Output is a single
//! `<math>` element; every value that comes from the document is escaped, and
//! the result is a pure function of the model (deterministic, ADR-0006).

use std::fmt::Write as _;

use strict_ooxml_wml::model::math::{
    MathAlignment, MathArgument, MathExpression, MathJustification, MathNode, MathParagraph,
    MathPosition, MathRun, MathRunProperties,
};

/// The MathML namespace.
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Why a formula could not be projected to MathML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathMlError {
    /// An OMML construct with no MathML equivalent was found.
    ///
    /// The name is reported so the caller can point at the OMML element.
    Unmappable {
        /// The OMML element name, for example `m:nary`.
        element: String,
    },
}

impl std::fmt::Display for MathMlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unmappable { element } => {
                write!(f, "OMML element {element} has no MathML equivalent")
            }
        }
    }
}

impl std::error::Error for MathMlError {}

/// Projects a formula (`m:oMath`) to a MathML fragment.
pub fn math_expression_to_mathml(expression: &MathExpression) -> Result<String, MathMlError> {
    let mut out = String::with_capacity(256);
    out.push_str("<math xmlns=\"");
    out.push_str(MATHML_NS);
    out.push_str("\" display=\"inline\">");
    write_nodes(&mut out, &expression.nodes)?;
    out.push_str("</math>");
    Ok(out)
}

/// Projects a display formula (`m:oMathPara`) to a MathML fragment.
pub fn math_paragraph_to_mathml(paragraph: &MathParagraph) -> Result<String, MathMlError> {
    let mut out = String::with_capacity(256);
    let justification = paragraph
        .properties
        .as_ref()
        .and_then(|properties| properties.justification)
        .unwrap_or(MathJustification::Default);
    out.push_str("<math xmlns=\"");
    out.push_str(MATHML_NS);
    out.push_str("\" display=\"block\"");
    if justification != MathJustification::Default {
        let _ = write!(
            out,
            " data-justification=\"{}\"",
            justification_name(justification)
        );
    }
    out.push('>');
    write_nodes(&mut out, &paragraph.expression.nodes)?;
    out.push_str("</math>");
    Ok(out)
}

/// The MathML name of an OMML display justification.
fn justification_name(justification: MathJustification) -> &'static str {
    match justification {
        MathJustification::Left => "left",
        MathJustification::CenterGroup => "centerGroup",
        MathJustification::Center => "center",
        MathJustification::Right => "right",
        MathJustification::Default => "default",
    }
}

/// Escapes text for XML content.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Writes a list of sibling nodes inside the current container.
fn write_nodes(out: &mut String, nodes: &[MathNode]) -> Result<(), MathMlError> {
    for node in nodes {
        write_node(out, node)?;
    }
    Ok(())
}

/// Writes the children of an `m:e`-like argument.
fn write_argument(out: &mut String, argument: &MathArgument) -> Result<(), MathMlError> {
    let alignment = argument.properties.alignment;
    if let Some(alignment) = alignment {
        let _ = write!(
            out,
            "<mrow data-alignment=\"{}\">",
            alignment_name(alignment)
        );
        write_nodes(out, &argument.nodes)?;
        out.push_str("</mrow>");
    } else {
        write_nodes(out, &argument.nodes)?;
    }
    Ok(())
}

/// The MathML name of an OMML argument alignment.
fn alignment_name(alignment: MathAlignment) -> &'static str {
    match alignment {
        MathAlignment::Left => "left",
        MathAlignment::Center => "center",
        MathAlignment::Right => "right",
        MathAlignment::Inline => "inline",
        MathAlignment::Unset => "unset",
    }
}

/// Writes a single run as `mi`/`mn`/`mo`/`mtext`, as MathML requires.
fn write_run(out: &mut String, run: &MathRun) {
    let text = run.text.trim();
    if text.is_empty() {
        return;
    }
    let tag = run_tag(run, text);
    let mut attributes = String::new();
    if let Some(style) = run.properties.style {
        let _ = write!(attributes, " mathvariant=\"{}\"", style.as_str());
    }
    if let Some(script) = run.properties.script {
        let _ = write!(attributes, " mathvariant=\"{}\"", script_name(script));
    }
    if run.properties.normal {
        attributes.push_str(" mathvariant=\"normal\"");
    }
    if run.properties.literal {
        attributes.push_str(" mathvariant=\"normal\"");
    }
    let _ = write!(out, "<{tag}{attributes}>{}</{tag}>", escape(text));
}

/// Chooses the MathML token element of a run.
fn run_tag(run: &MathRun, text: &str) -> &'static str {
    if run.properties.literal {
        return "mtext";
    }
    if text.chars().all(|ch| ch.is_ascii_digit()) {
        return "mn";
    }
    if text.chars().count() == 1 {
        let ch = text.chars().next().unwrap_or(' ');
        if ch.is_ascii_alphabetic() {
            return "mi";
        }
        return "mo";
    }
    if text.chars().all(char::is_alphabetic) {
        return "mi";
    }
    "mtext"
}

/// The MathML name of an OMML script.
fn script_name(script: strict_ooxml_wml::model::math::MathScript) -> &'static str {
    use strict_ooxml_wml::model::math::MathScript;
    match script {
        MathScript::DoubleStruck => "double-struck",
        MathScript::Fraktur => "fraktur",
        MathScript::Roman => "normal",
        MathScript::SansSerif => "sans-serif",
        MathScript::Monospace => "monospace",
        MathScript::Script => "script",
    }
}

/// Writes one node.
#[allow(clippy::too_many_lines)]
fn write_node(out: &mut String, node: &MathNode) -> Result<(), MathMlError> {
    match node {
        MathNode::Run(run) => write_run(out, run),
        MathNode::Fraction(node) => {
            out.push_str("<mfrac>");
            write_argument(out, &node.numerator)?;
            write_argument(out, &node.denominator)?;
            out.push_str("</mfrac>");
        }
        MathNode::Radical(node) => {
            if node.hide_degree || node.degree.nodes.is_empty() {
                out.push_str("<msqrt>");
                write_argument(out, &node.radicand)?;
                out.push_str("</msqrt>");
            } else {
                out.push_str("<mroot>");
                write_argument(out, &node.radicand)?;
                write_argument(out, &node.degree)?;
                out.push_str("</mroot>");
            }
        }
        MathNode::Superscript(node) => {
            out.push_str("<msup>");
            write_argument(out, &node.base)?;
            write_argument(out, &node.superscript)?;
            out.push_str("</msup>");
        }
        MathNode::Subscript(node) => {
            out.push_str("<msub>");
            write_argument(out, &node.base)?;
            write_argument(out, &node.subscript)?;
            out.push_str("</msub>");
        }
        MathNode::SubSuperscript(node) => {
            out.push_str("<msubsup>");
            write_argument(out, &node.base)?;
            write_argument(out, &node.subscript)?;
            write_argument(out, &node.superscript)?;
            out.push_str("</msubsup>");
        }
        MathNode::PreScript(node) => {
            out.push_str("<mmultiscripts>");
            write_argument(out, &node.base)?;
            out.push_str("<mprescripts />");
            write_argument(out, &node.subscript)?;
            write_argument(out, &node.superscript)?;
            out.push_str("</mmultiscripts>");
        }
        MathNode::NaryOperator(node) => write_nary(out, node)?,
        MathNode::Delimiter(node) => {
            out.push_str("<mrow>");
            if let Some(begin) = node.begin {
                write_operator(out, &begin.to_string());
            }
            for (index, argument) in node.arguments.iter().enumerate() {
                if index > 0 {
                    if let Some(separator) = node.separator {
                        write_operator(out, &separator.to_string());
                    }
                }
                write_argument(out, argument)?;
            }
            if let Some(end) = node.end {
                write_operator(out, &end.to_string());
            }
            out.push_str("</mrow>");
        }
        MathNode::Function(node) => {
            out.push_str("<mrow>");
            write_argument(out, &node.name)?;
            // U+2061 FUNCTION APPLICATION keeps the name and the argument
            // adjacent, as in the OMML intent.
            write_operator(out, "\u{2061}");
            write_argument(out, &node.argument)?;
            out.push_str("</mrow>");
        }
        MathNode::Limit(node) => {
            let tag = if node.above { "mover" } else { "munder" };
            out.push('<');
            out.push_str(tag);
            out.push('>');
            write_argument(out, &node.base)?;
            write_argument(out, &node.limit)?;
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        MathNode::Matrix(node) => {
            out.push_str("<mtable>");
            for row in &node.rows {
                out.push_str("<mtr>");
                for cell in row {
                    out.push_str("<mtd>");
                    write_argument(out, cell)?;
                    out.push_str("</mtd>");
                }
                out.push_str("</mtr>");
            }
            out.push_str("</mtable>");
        }
        MathNode::EquationArray(node) => {
            out.push_str("<mtable columnalign=\"left\">");
            for row in &node.rows {
                out.push_str("<mtr><mtd>");
                write_argument(out, row)?;
                out.push_str("</mtd></mtr>");
            }
            out.push_str("</mtable>");
        }
        MathNode::Accent(node) => {
            out.push_str("<mover accent=\"true\">");
            write_argument(out, &node.base)?;
            let character = node.chr.unwrap_or('\u{0302}');
            write_run(out, &synthetic_run(character));
            out.push_str("</mover>");
        }
        MathNode::Bar(node) => {
            let tag = if node.position == Some(MathPosition::Bottom) {
                "munder"
            } else {
                "mover"
            };
            out.push('<');
            out.push_str(tag);
            out.push('>');
            write_argument(out, &node.base)?;
            write_run(out, &synthetic_run('\u{00AF}'));
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        MathNode::GroupCharacter(node) => {
            let character = node.chr.unwrap_or('\u{23DF}');
            let over = matches!(node.position, Some(MathPosition::Top) | None);
            let tag = if over { "mover" } else { "munder" };
            out.push('<');
            out.push_str(tag);
            out.push('>');
            write_argument(out, &node.base)?;
            write_run(out, &synthetic_run(character));
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        MathNode::Boxed(node) => {
            out.push_str("<mrow data-boxed=\"true\">");
            write_argument(out, &node.argument)?;
            out.push_str("</mrow>");
        }
        MathNode::BorderBox(node) => {
            out.push_str("<mrow data-border=\"true\">");
            for argument in &node.arguments {
                write_argument(out, argument)?;
            }
            out.push_str("</mrow>");
        }
        MathNode::Phantom(node) => {
            // A phantom reserves space without drawing, which is exactly
            // `mphantom`.
            out.push_str("<mphantom>");
            write_argument(out, &node.argument)?;
            out.push_str("</mphantom>");
        }
        MathNode::Unknown(node) => {
            return Err(MathMlError::Unmappable {
                element: node.local.to_string(),
            })
        }
    }
    Ok(())
}

/// Writes an n-ary operator as `munderover`/`munder`/`mover` plus its glyph.
fn write_nary(
    out: &mut String,
    node: &strict_ooxml_wml::model::math::NaryOperator,
) -> Result<(), MathMlError> {
    let has_lower = !node.hide_sub && !node.subscript.nodes.is_empty();
    let has_upper = !node.hide_sup && !node.superscript.nodes.is_empty();
    let tag = match (has_lower, has_upper) {
        (true, true) => "munderover",
        (true, false) => "munder",
        (false, true) => "mover",
        (false, false) => "mrow",
    };
    out.push('<');
    out.push_str(tag);
    out.push('>');
    if has_lower {
        write_argument(out, &node.subscript)?;
    }
    if has_upper {
        write_argument(out, &node.superscript)?;
    }
    let _ = write!(
        out,
        "<mo>{}</mo>",
        escape(&node.chr.unwrap_or('\u{222B}').to_string())
    );
    write_argument(out, &node.operand)?;
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Ok(())
}

/// Writes an operator token.
fn write_operator(out: &mut String, text: &str) {
    let _ = write!(out, "<mo>{}</mo>", escape(text));
}

/// A run holding one operator/decorative character.
fn synthetic_run(character: char) -> MathRun {
    MathRun {
        properties: MathRunProperties {
            literal: false,
            normal: true,
            ..MathRunProperties::default()
        },
        text: character.to_string(),
        location: strict_ooxml_core::error::SourceLocation::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{math_expression_to_mathml, math_paragraph_to_mathml, MathMlError};
    use strict_ooxml_wml::model::math::Fraction;
    use strict_ooxml_wml::model::math::{
        MathArgument, MathExpression, MathJustification, MathNode, MathParagraph,
        MathParagraphProperties, MathRun, MathRunProperties, UnknownMathNode,
    };

    fn location() -> strict_ooxml_core::error::SourceLocation {
        strict_ooxml_core::error::SourceLocation::default()
    }

    fn run(text: &str) -> MathNode {
        MathNode::Run(MathRun {
            properties: MathRunProperties::default(),
            text: text.to_owned(),
            location: location(),
        })
    }

    fn argument(nodes: Vec<MathNode>) -> MathArgument {
        MathArgument {
            properties: strict_ooxml_wml::model::math::ArgumentProperties::default(),
            nodes,
            location: location(),
        }
    }

    #[test]
    fn a_fraction_becomes_mfrac() {
        let expression = MathExpression {
            nodes: vec![MathNode::Fraction(Box::new(Fraction {
                bar_type: None,
                small_fraction: false,
                control: None,
                numerator: argument(vec![run("a")]),
                denominator: argument(vec![run("b")]),
                location: location(),
            }))],
            location: location(),
        };
        let mathml = math_expression_to_mathml(&expression).expect("mathml");
        assert!(mathml.contains("<mfrac>"));
        assert!(mathml.contains("<mi>a</mi>"));
        assert!(mathml.contains("<mi>b</mi>"));
        assert!(mathml.starts_with("<math xmlns=\"http://www.w3.org/1998/Math/MathML\""));
    }

    #[test]
    fn digits_become_mn_and_operators_mo() {
        let expression = MathExpression {
            nodes: vec![run("42"), run("+")],
            location: location(),
        };
        let mathml = math_expression_to_mathml(&expression).expect("mathml");
        assert!(mathml.contains("<mn>42</mn>"));
        assert!(mathml.contains("<mo>+</mo>"));
    }

    #[test]
    fn text_is_escaped() {
        let expression = MathExpression {
            nodes: vec![run("a<b&c")],
            location: location(),
        };
        let mathml = math_expression_to_mathml(&expression).expect("mathml");
        assert!(mathml.contains("a&lt;b&amp;c"));
    }

    #[test]
    fn a_display_formula_carries_its_justification() {
        let paragraph = MathParagraph {
            properties: Some(MathParagraphProperties {
                justification: Some(MathJustification::Center),
            }),
            expression: MathExpression {
                nodes: vec![run("x")],
                location: location(),
            },
            location: location(),
        };
        let mathml = math_paragraph_to_mathml(&paragraph).expect("mathml");
        assert!(mathml.contains("display=\"block\""));
        assert!(mathml.contains("data-justification=\"center\""));
    }

    #[test]
    fn an_unmodelled_construct_is_reported_not_dropped() {
        let expression = MathExpression {
            nodes: vec![MathNode::Unknown(Box::new(UnknownMathNode {
                local: "m:newThing".into(),
                location: location(),
            }))],
            location: location(),
        };
        assert_eq!(
            math_expression_to_mathml(&expression),
            Err(MathMlError::Unmappable {
                element: "m:newThing".to_owned()
            })
        );
    }
}
