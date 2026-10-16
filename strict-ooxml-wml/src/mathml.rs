//! Presentation MathML into the OMML model: what a recogniser or another
//! program hands over as MathML becomes an equation Word can edit.
//!
//! The mapping is the inverse of the renderer's OMML-to-MathML export, for
//! the presentation elements a formula is written with: tokens become runs,
//! `mfrac` a fraction, `msqrt`/`mroot` a radical, scripts on a large operator
//! an n-ary operator, `munder`/`mover` a limit, an accent or a bar, a row
//! between matching fences a delimiter, `mtable` a matrix. Wrappers that only
//! style or group (`mrow`, `mstyle`, `mpadded`, `semantics`) are flattened;
//! an element this does not know contributes its content.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

use crate::model::math::{
    Accent, ArgumentProperties, Bar, BorderBox, Delimiter, Fraction, LimitLocation, MathAlignment,
    MathArgument, MathExpression, MathLimit, MathNode, MathPosition, MathRun, MathRunProperties,
    MathStyle, Matrix, MatrixColumn, NaryOperator, Phantom, Radical, SubSuperscript, Subscript,
    Superscript,
};

/// Why MathML could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathMlReadError {
    /// Not well-formed XML.
    Xml(String),
    /// No `math` element.
    NoMath,
    /// Nested deeper than a formula needs.
    TooDeep,
}

impl std::fmt::Display for MathMlReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Xml(detail) => write!(f, "the MathML is not well-formed XML: {detail}"),
            Self::NoMath => f.write_str("there is no <math> element"),
            Self::TooDeep => f.write_str("the MathML is nested too deep"),
        }
    }
}

impl std::error::Error for MathMlReadError {}

/// How deep elements may nest.
const MAX_DEPTH: usize = 64;

/// Large operators: scripts on one make an n-ary operator.
const NARY: &[char] = &[
    '\u{2211}', '\u{220F}', '\u{2210}', '\u{222B}', '\u{222C}', '\u{222D}', '\u{222E}', '\u{222F}',
    '\u{2230}', '\u{22C0}', '\u{22C1}', '\u{22C2}', '\u{22C3}', '\u{2A00}', '\u{2A01}', '\u{2A02}',
    '\u{2A04}', '\u{2A06}',
];

/// Opening fences and the closing fence each pairs with.
const FENCES: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('|', '|'),
    ('\u{2016}', '\u{2016}'),
    ('\u{27E8}', '\u{27E9}'),
    ('\u{2329}', '\u{232A}'),
    ('\u{230A}', '\u{230B}'),
    ('\u{2308}', '\u{2309}'),
];

/// One element of the MathML tree.
#[derive(Debug, Default)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Item>,
}

#[derive(Debug)]
enum Item {
    Element(Element),
    Text(String),
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|item| match item {
            Item::Element(element) => Some(element),
            Item::Text(_) => None,
        })
    }

    fn child(&self, index: usize) -> Option<&Element> {
        self.elements().nth(index)
    }

    /// The text of a token element, whitespace collapsed as MathML does.
    fn text(&self) -> String {
        let mut out = String::new();
        for item in &self.children {
            match item {
                Item::Text(text) => out.push_str(text),
                Item::Element(element) => out.push_str(&element.text()),
            }
        }
        let collapsed: Vec<&str> = out.split_whitespace().collect();
        collapsed
            .join(" ")
            // Invisible operators carry no glyph in OMML.
            .chars()
            .filter(|c| !('\u{2061}'..='\u{2064}').contains(c))
            .collect()
    }
}

/// Reads presentation MathML (a `math` element, with or without the MathML
/// namespace and an XML declaration; text before it is ignored) into an
/// equation.
///
/// # Errors
///
/// [`MathMlReadError`] when the text is not XML, holds no `math` element or
/// nests too deep.
pub fn math_from_mathml(source: &str) -> Result<MathExpression, MathMlReadError> {
    let start = source.find("<math").ok_or(MathMlReadError::NoMath)?;
    let end = source
        .rfind("</math>")
        .map(|at| at + "</math>".len())
        .or_else(|| source.rfind("/>").map(|at| at + 2))
        .ok_or(MathMlReadError::NoMath)?;
    let fragment = source.get(start..end).ok_or(MathMlReadError::NoMath)?;
    let root = parse(fragment)?;
    if root.name != "math" {
        return Err(MathMlReadError::NoMath);
    }
    Ok(MathExpression {
        nodes: nodes_of(&root, 0)?,
        location: SourceLocation::unknown(),
    })
}

fn parse(fragment: &str) -> Result<Element, MathMlReadError> {
    let limits = ResourceLimits::default();
    let mut reader = XmlReader::new(fragment.as_bytes(), PartId::new("/mathml.xml"), &limits)
        .map_err(|error| MathMlReadError::Xml(error.to_string()))?;
    let mut stack: Vec<Element> = vec![Element::default()];
    loop {
        match reader
            .next_event()
            .map_err(|error| MathMlReadError::Xml(error.to_string()))?
        {
            XmlEvent::StartElement { name, attrs } => {
                if stack.len() > MAX_DEPTH {
                    return Err(MathMlReadError::TooDeep);
                }
                stack.push(Element {
                    name: name.local().to_owned(),
                    attrs: attrs
                        .into_iter()
                        .map(|attr| (attr.name.local().to_owned(), attr.value))
                        .collect(),
                    children: Vec::new(),
                });
            }
            XmlEvent::EndElement { .. } => {
                let done = stack.pop().ok_or(MathMlReadError::NoMath)?;
                let parent = stack.last_mut().ok_or(MathMlReadError::NoMath)?;
                parent.children.push(Item::Element(done));
            }
            XmlEvent::Text(text) | XmlEvent::CData(text) => {
                if let Some(top) = stack.last_mut() {
                    top.children.push(Item::Text(text));
                }
            }
            XmlEvent::Eof => break,
        }
    }
    let document = stack.pop().ok_or(MathMlReadError::NoMath)?;
    document
        .children
        .into_iter()
        .find_map(|item| match item {
            Item::Element(element) => Some(element),
            Item::Text(_) => None,
        })
        .ok_or(MathMlReadError::NoMath)
}

fn location() -> SourceLocation {
    SourceLocation::unknown()
}

fn argument(nodes: Vec<MathNode>) -> MathArgument {
    MathArgument {
        properties: ArgumentProperties::default(),
        nodes,
        location: location(),
    }
}

fn run(text: String, properties: MathRunProperties) -> MathNode {
    MathNode::Run(MathRun {
        field_char: None,
        properties,
        run_properties: None,
        text,
        location: location(),
    })
}

/// The nodes a sequence of elements stands for.
fn nodes_of(parent: &Element, depth: usize) -> Result<Vec<MathNode>, MathMlReadError> {
    let children: Vec<&Element> = parent.elements().collect();
    row(&children, depth)
}

/// A row: fenced stretches become delimiters, the rest node by node.
fn row(children: &[&Element], depth: usize) -> Result<Vec<MathNode>, MathMlReadError> {
    if depth > MAX_DEPTH {
        return Err(MathMlReadError::TooDeep);
    }
    let mut out = Vec::new();
    let mut index = 0;
    while let Some(element) = children.get(index) {
        // An opening fence: up to its matching close is one delimiter.
        if let Some(close) = opening_fence(element) {
            if let Some(span) = closing_index(children, index, close) {
                let inner = children.get(index + 1..span).unwrap_or_default();
                let begin = fence_char(element);
                let end = children.get(span).and_then(|e| fence_char(e));
                out.push(delimiter(begin, end, inner, depth)?);
                index = span + 1;
                continue;
            }
        }
        out.extend(node(element, depth + 1)?);
        index += 1;
    }
    Ok(out)
}

fn fence_char(element: &Element) -> Option<char> {
    (element.name == "mo")
        .then(|| element.text())
        .and_then(|text| {
            let mut chars = text.chars();
            let first = chars.next()?;
            chars.next().is_none().then_some(first)
        })
}

/// The closing fence an opening `mo` pairs with.
fn opening_fence(element: &Element) -> Option<char> {
    let open = fence_char(element)?;
    FENCES
        .iter()
        .find(|(begin, _)| *begin == open)
        .map(|(_, end)| *end)
}

/// Where the fence opened at `from` closes, nesting counted.
fn closing_index(children: &[&Element], from: usize, close: char) -> Option<usize> {
    let open = children.get(from).and_then(|e| fence_char(e))?;
    let mut depth = 0usize;
    for (index, element) in children.iter().enumerate().skip(from + 1) {
        let Some(c) = fence_char(element) else {
            continue;
        };
        if c == close && (depth == 0 || open == close) {
            if depth == 0 {
                return Some(index);
            }
            depth -= 1;
        } else if c == open {
            depth += 1;
        }
    }
    None
}

/// A delimiter around `inner`, split into arguments at its separators.
fn delimiter(
    begin: Option<char>,
    end: Option<char>,
    inner: &[&Element],
    depth: usize,
) -> Result<MathNode, MathMlReadError> {
    let mut arguments = Vec::new();
    let mut current: Vec<&Element> = Vec::new();
    let mut separator = None;
    for element in inner {
        let c = fence_char(element);
        if matches!(c, Some(',' | ';')) && depth < MAX_DEPTH {
            separator = c;
            arguments.push(argument(row(&current, depth + 1)?));
            current.clear();
        } else {
            current.push(element);
        }
    }
    arguments.push(argument(row(&current, depth + 1)?));
    Ok(MathNode::Delimiter(Box::new(Delimiter {
        begin,
        separator,
        end,
        grow: None,
        shape: None,
        control: None,
        arguments,
        location: location(),
    })))
}

/// The argument one child element makes.
fn arg(element: Option<&Element>, depth: usize) -> Result<MathArgument, MathMlReadError> {
    Ok(argument(match element {
        Some(element) => node(element, depth + 1)?,
        None => Vec::new(),
    }))
}

/// The single large operator a base is, if it is one.
fn nary_char(element: Option<&Element>) -> Option<char> {
    let element = element?;
    let element = if element.name == "mrow" && element.elements().count() == 1 {
        element.child(0)?
    } else {
        element
    };
    let c = fence_char(element)?;
    NARY.contains(&c).then_some(c)
}

fn nary(chr: char, under_over: bool, lower: MathArgument, upper: MathArgument) -> MathNode {
    MathNode::NaryOperator(Box::new(NaryOperator {
        chr: Some(chr),
        limit_location: Some(if under_over {
            LimitLocation::UnderOver
        } else {
            LimitLocation::SubSup
        }),
        grow: None,
        hide_sub: lower.nodes.is_empty(),
        hide_sup: upper.nodes.is_empty(),
        control: None,
        subscript: lower,
        superscript: upper,
        operand: argument(Vec::new()),
        location: location(),
    }))
}

/// The nodes one element stands for.
#[allow(clippy::too_many_lines)]
fn node(element: &Element, depth: usize) -> Result<Vec<MathNode>, MathMlReadError> {
    if depth > MAX_DEPTH {
        return Err(MathMlReadError::TooDeep);
    }
    let one = |n: MathNode| Ok(vec![n]);
    match element.name.as_str() {
        "mi" | "mn" | "mo" | "mtext" | "ms" => {
            let text = element.text();
            if text.is_empty() {
                return Ok(Vec::new());
            }
            let mut properties = MathRunProperties::default();
            let variant = element.attr("mathvariant");
            let upright = match element.name.as_str() {
                // A one-letter identifier is italic; a name (sin) is not.
                "mi" => text.chars().count() > 1 || variant == Some("normal"),
                "mtext" | "ms" => {
                    properties.normal = true;
                    false
                }
                _ => false,
            };
            properties.style = match variant {
                Some("bold") => Some(MathStyle::Bold),
                Some("bold-italic") => Some(MathStyle::BoldItalic),
                Some("italic") => Some(MathStyle::Italic),
                _ if upright => Some(MathStyle::Plain),
                _ => None,
            };
            one(run(text, properties))
        }
        "mspace" | "none" | "mprescripts" | "annotation" | "annotation-xml" => Ok(Vec::new()),
        "mfrac" => {
            let no_bar = element
                .attr("linethickness")
                .is_some_and(|t| t.trim() == "0" || t.trim().starts_with("0p"));
            one(MathNode::Fraction(Box::new(Fraction {
                bar_type: no_bar.then(|| Arc::from("noBar")),
                small_fraction: false,
                control: None,
                numerator: arg(element.child(0), depth)?,
                denominator: arg(element.child(1), depth)?,
                location: location(),
            })))
        }
        "msqrt" => one(MathNode::Radical(Box::new(Radical {
            hide_degree: true,
            control: None,
            degree: argument(Vec::new()),
            radicand: argument(nodes_of(element, depth + 1)?),
            location: location(),
        }))),
        "mroot" => one(MathNode::Radical(Box::new(Radical {
            hide_degree: false,
            control: None,
            degree: arg(element.child(1), depth)?,
            radicand: arg(element.child(0), depth)?,
            location: location(),
        }))),
        "msup" => {
            let base = element.child(0);
            if let Some(chr) = nary_char(base) {
                return one(nary(
                    chr,
                    false,
                    argument(Vec::new()),
                    arg(element.child(1), depth)?,
                ));
            }
            one(MathNode::Superscript(Box::new(Superscript {
                control: None,
                base: arg(base, depth)?,
                superscript: arg(element.child(1), depth)?,
                location: location(),
            })))
        }
        "msub" => {
            let base = element.child(0);
            if let Some(chr) = nary_char(base) {
                return one(nary(
                    chr,
                    false,
                    arg(element.child(1), depth)?,
                    argument(Vec::new()),
                ));
            }
            one(MathNode::Subscript(Box::new(Subscript {
                control: None,
                base: arg(base, depth)?,
                subscript: arg(element.child(1), depth)?,
                location: location(),
            })))
        }
        "msubsup" => {
            let base = element.child(0);
            if let Some(chr) = nary_char(base) {
                return one(nary(
                    chr,
                    false,
                    arg(element.child(1), depth)?,
                    arg(element.child(2), depth)?,
                ));
            }
            one(MathNode::SubSuperscript(Box::new(SubSuperscript {
                control: None,
                base: arg(base, depth)?,
                subscript: arg(element.child(1), depth)?,
                superscript: arg(element.child(2), depth)?,
                location: location(),
            })))
        }
        "munderover" => {
            let base = element.child(0);
            let lower = arg(element.child(1), depth)?;
            let upper = arg(element.child(2), depth)?;
            if let Some(chr) = nary_char(base) {
                return one(nary(chr, true, lower, upper));
            }
            let inner = MathNode::Limit(Box::new(MathLimit {
                above: false,
                control: None,
                base: arg(base, depth)?,
                limit: lower,
                location: location(),
            }));
            one(MathNode::Limit(Box::new(MathLimit {
                above: true,
                control: None,
                base: argument(vec![inner]),
                limit: upper,
                location: location(),
            })))
        }
        "munder" | "mover" => {
            let above = element.name == "mover";
            let base = element.child(0);
            let script = element.child(1);
            if let Some(chr) = nary_char(base) {
                let script = arg(script, depth)?;
                let (lower, upper) = if above {
                    (argument(Vec::new()), script)
                } else {
                    (script, argument(Vec::new()))
                };
                return one(nary(chr, true, lower, upper));
            }
            // A single mark over or under the base: a bar or an accent.
            if let Some(mark) = script.and_then(fence_char) {
                if matches!(
                    mark,
                    '\u{00AF}' | '\u{0305}' | '_' | '\u{2015}' | '\u{2014}'
                ) {
                    return one(MathNode::Bar(Box::new(Bar {
                        position: Some(if above {
                            MathPosition::Top
                        } else {
                            MathPosition::Bottom
                        }),
                        control: None,
                        base: arg(base, depth)?,
                        location: location(),
                    })));
                }
                if above && element.attr("accent") != Some("false") {
                    return one(MathNode::Accent(Box::new(Accent {
                        chr: Some(combining(mark)),
                        control: None,
                        base: arg(base, depth)?,
                        location: location(),
                    })));
                }
            }
            one(MathNode::Limit(Box::new(MathLimit {
                above,
                control: None,
                base: arg(base, depth)?,
                limit: arg(script, depth)?,
                location: location(),
            })))
        }
        "mfenced" => {
            let begin = element.attr("open").unwrap_or("(").chars().next();
            let end = element.attr("close").unwrap_or(")").chars().next();
            let separator = element
                .attr("separators")
                .unwrap_or(",")
                .chars()
                .find(|c| !c.is_whitespace());
            let mut arguments = Vec::new();
            for child in element.elements() {
                arguments.push(argument(node(child, depth + 1)?));
            }
            if arguments.is_empty() {
                arguments.push(argument(Vec::new()));
            }
            one(MathNode::Delimiter(Box::new(Delimiter {
                begin,
                separator,
                end,
                grow: None,
                shape: None,
                control: None,
                arguments,
                location: location(),
            })))
        }
        "mtable" => {
            let mut rows = Vec::new();
            for row_element in element.elements() {
                if !matches!(row_element.name.as_str(), "mtr" | "mlabeledtr") {
                    continue;
                }
                // A labelled row's first cell is its label.
                let skip = usize::from(row_element.name == "mlabeledtr");
                let mut cells = Vec::new();
                for cell in row_element.elements().skip(skip) {
                    cells.push(argument(nodes_of(cell, depth + 1)?));
                }
                rows.push(cells);
            }
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            for cells in &mut rows {
                cells.resize_with(width, || argument(Vec::new()));
            }
            one(MathNode::Matrix(Box::new(Matrix {
                base_justification: None,
                hide_placeholders: true,
                column_group_rule: None,
                column_spacing: None,
                column_group_spacing: None,
                row_spacing_rule: None,
                row_spacing: None,
                columns: vec![MatrixColumn {
                    justification: Some(MathAlignment::Center),
                    count: i32::try_from(width).ok(),
                }],
                control: None,
                rows,
                location: location(),
            })))
        }
        "menclose" => one(MathNode::BorderBox(Box::new(BorderBox {
            hide_edges: [None; 4],
            strikes: [None; 4],
            alignment: None,
            spacing: None,
            shadow: false,
            lines: None,
            control: None,
            arguments: vec![argument(nodes_of(element, depth + 1)?)],
            location: location(),
        }))),
        "mphantom" => one(MathNode::Phantom(Box::new(Phantom {
            show: false,
            transparent: false,
            zero_width: false,
            zero_ascent: false,
            zero_descent: false,
            show_all: false,
            rtl: false,
            control: None,
            argument: argument(nodes_of(element, depth + 1)?),
            location: location(),
        }))),
        "semantics" => element
            .child(0)
            .map_or_else(|| Ok(Vec::new()), |first| node(first, depth + 1)),
        // mrow, mstyle, mpadded, math and anything unknown: the content.
        _ => nodes_of(element, depth + 1),
    }
}

/// The combining mark OMML's accent wants for a spacing one.
fn combining(mark: char) -> char {
    match mark {
        '^' | '\u{02C6}' => '\u{0302}',
        '~' | '\u{02DC}' => '\u{0303}',
        '\u{02D9}' | '.' => '\u{0307}',
        '\u{00A8}' => '\u{0308}',
        '\u{2192}' | '\u{20D7}' => '\u{20D7}',
        '\u{02C7}' => '\u{030C}',
        '\u{00B4}' => '\u{0301}',
        '`' => '\u{0300}',
        '\u{02D8}' => '\u{0306}',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(nodes: &[MathNode]) -> Vec<&'static str> {
        nodes.iter().map(MathNode::element_name).collect()
    }

    #[test]
    fn a_quadratic_formula_reads_into_its_omml_shape() {
        let source = r#"Here it is: <math xmlns="http://www.w3.org/1998/Math/MathML">
          <mi>x</mi><mo>=</mo>
          <mfrac>
            <mrow><mo>&#x2212;</mo><mi>b</mi><mo>&#xB1;</mo>
              <msqrt><msup><mi>b</mi><mn>2</mn></msup><mo>&#x2212;</mo><mn>4</mn><mi>a</mi><mi>c</mi></msqrt>
            </mrow>
            <mrow><mn>2</mn><mi>a</mi></mrow>
          </mfrac></math> done"#;
        let expression = math_from_mathml(source).expect("reads");
        assert_eq!(kinds(&expression.nodes), ["m:r", "m:r", "m:f"]);
        let Some(MathNode::Fraction(fraction)) = expression.nodes.get(2) else {
            panic!("fraction")
        };
        assert!(kinds(&fraction.numerator.nodes).contains(&"m:rad"));
    }

    #[test]
    fn scripts_on_a_large_operator_make_an_nary_and_fences_a_delimiter() {
        let source =
            "<math><munderover><mo>\u{2211}</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow>\
            <mi>n</mi></munderover><mo>(</mo><mi>a</mi><mo>,</mo><mi>b</mi><mo>)</mo>\
            <mover><mi>v</mi><mo>\u{2192}</mo></mover><mi>sin</mi></math>";
        let expression = math_from_mathml(source).expect("reads");
        assert_eq!(kinds(&expression.nodes), ["m:nary", "m:d", "m:acc", "m:r"]);
        let Some(MathNode::Delimiter(delimiter)) = expression.nodes.get(1) else {
            panic!("delimiter")
        };
        assert_eq!(delimiter.arguments.len(), 2);
        let Some(MathNode::Run(name)) = expression.nodes.get(3) else {
            panic!("run")
        };
        assert_eq!(name.properties.style, Some(MathStyle::Plain));
    }

    #[test]
    fn a_table_is_a_matrix_and_bad_input_is_refused() {
        let source = "<math><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>0</mn></mtd></mtr>\
            <mtr><mtd><mn>0</mn></mtd></mtr></mtable></math>";
        let expression = math_from_mathml(source).expect("reads");
        let Some(MathNode::Matrix(matrix)) = expression.nodes.first() else {
            panic!("matrix")
        };
        assert_eq!(matrix.rows.len(), 2);
        assert!(matrix.rows.iter().all(|row| row.len() == 2));
        assert_eq!(math_from_mathml("no formula"), Err(MathMlReadError::NoMath));
        assert!(math_from_mathml("<math><mi>x</mo></math>").is_err());
    }
}
