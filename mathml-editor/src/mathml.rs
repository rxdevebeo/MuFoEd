//! Read and write the subset of Presentation `MathML` the editor edits.
//!
//! Unknown wrappers (`mstyle`, `mpadded`, `semantics`) are unwrapped, so a
//! formula produced by the document library still opens. The editor then
//! writes only the elements it draws.

use quick_xml::{events::Event, Reader, XmlVersion};

use crate::model::{Formula, Node, Row, TokenKind};

/// The formula could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MathmlError {
    /// The text is not well-formed XML.
    #[error("MathML is not valid XML: {0}")]
    Xml(String),
    /// There is no `math` element.
    #[error("MathML has no math element")]
    NoMath,
    /// The tree is nested deeper than the reader allows.
    #[error("MathML is nested too deeply")]
    TooDeep,
}

const MAX_DEPTH: usize = 64;

const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Parse a Presentation `MathML` document or fragment.
pub(crate) fn parse(source: &str) -> Result<Formula, MathmlError> {
    let root = read_tree(source)?;
    let math = find_math(&root).ok_or(MathmlError::NoMath)?;
    Ok(Formula {
        root: row_from(&math.children, 0)?,
    })
}

/// Serialize a formula as a Presentation `MathML` document.
pub(crate) fn write(formula: &Formula) -> String {
    let mut out = format!("<math xmlns=\"{MATHML_NS}\">");
    write_row(&mut out, &formula.root);
    out.push_str("</math>");
    out
}

/// `MathML` for a sequence of nodes, so a selection can be copied.
pub(crate) fn write_nodes(nodes: &[Node]) -> String {
    let mut out = format!("<math xmlns=\"{MATHML_NS}\">");
    if nodes.len() == 1 {
        write_node(&mut out, &nodes[0]);
    } else {
        out.push_str("<mrow>");
        for node in nodes {
            write_node(&mut out, node);
        }
        out.push_str("</mrow>");
    }
    out.push_str("</math>");
    out
}

/// Nodes from a `MathML` document or a fragment without the `math` wrapper.
pub(crate) fn nodes_from_mathml(source: &str) -> Result<Vec<Node>, MathmlError> {
    let formula = match parse(source) {
        Ok(formula) => formula,
        Err(MathmlError::NoMath) => parse(&format!("<math xmlns=\"{MATHML_NS}\">{source}</math>"))?,
        Err(error) => return Err(error),
    };
    Ok(formula.root.nodes)
}

struct Elem {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
}

enum Child {
    Elem(Elem),
    Text(String),
}

fn read_tree(source: &str) -> Result<Elem, MathmlError> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack = vec![Elem {
        name: "document".to_owned(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|error| MathmlError::Xml(error.to_string()))?;
        match event {
            Event::Start(start) => stack.push(own_elem(&start)?),
            Event::Empty(start) => {
                let elem = own_elem(&start)?;
                push_elem(&mut stack, elem)?;
            }
            Event::End(_) => {
                let elem = stack
                    .pop()
                    .ok_or_else(|| MathmlError::Xml("unbalanced end".into()))?;
                if stack.is_empty() {
                    return Err(MathmlError::Xml("unbalanced end".into()));
                }
                push_elem(&mut stack, elem)?;
            }
            Event::Text(text) => {
                let text = text
                    .xml10_content()
                    .map_err(|error| MathmlError::Xml(error.to_string()))?
                    .into_owned();
                push_text(&mut stack, text)?;
            }
            Event::CData(text) => {
                let text = text
                    .decode()
                    .map_err(|error| MathmlError::Xml(error.to_string()))?
                    .into_owned();
                push_text(&mut stack, text)?;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if stack.len() != 1 {
        return Err(MathmlError::Xml("unclosed element".into()));
    }
    stack.pop().ok_or(MathmlError::NoMath)
}

fn own_elem(start: &quick_xml::events::BytesStart<'_>) -> Result<Elem, MathmlError> {
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|error| MathmlError::Xml(error.to_string()))?;
        let key = local_name(attr.key.as_ref()).to_owned();
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|error| MathmlError::Xml(error.to_string()))?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Elem {
        name: local_name(start.name().as_ref()).to_owned(),
        attrs,
        children: Vec::new(),
    })
}

fn push_text(stack: &mut [Elem], text: String) -> Result<(), MathmlError> {
    if text.is_empty() {
        return Ok(());
    }
    stack
        .last_mut()
        .ok_or_else(|| MathmlError::Xml("text outside the document".into()))?
        .children
        .push(Child::Text(text));
    Ok(())
}

fn push_elem(stack: &mut [Elem], elem: Elem) -> Result<(), MathmlError> {
    stack
        .last_mut()
        .ok_or_else(|| MathmlError::Xml("element outside the document".into()))?
        .children
        .push(Child::Elem(elem));
    Ok(())
}

fn local_name(name: &[u8]) -> &str {
    let name = std::str::from_utf8(name).unwrap_or("");
    name.rsplit(':').next().unwrap_or(name)
}

fn find_math(elem: &Elem) -> Option<&Elem> {
    if elem.name == "math" {
        return Some(elem);
    }
    elem.children.iter().find_map(|child| match child {
        Child::Elem(elem) => find_math(elem),
        Child::Text(_) => None,
    })
}

fn row_from(children: &[Child], depth: usize) -> Result<Row, MathmlError> {
    if depth > MAX_DEPTH {
        return Err(MathmlError::TooDeep);
    }
    let mut nodes = Vec::new();
    for child in children {
        push_child(&mut nodes, child, depth)?;
    }
    Ok(Row { nodes })
}

fn push_child(out: &mut Vec<Node>, child: &Child, depth: usize) -> Result<(), MathmlError> {
    let Child::Elem(elem) = child else {
        return Ok(());
    };
    match elem.name.as_str() {
        "mrow" | "mstyle" | "mpadded" | "mphantom" | "menclose" | "math" | "semantics" => {
            if let Some(node) = operator_with_body(elem, depth)? {
                out.push(node);
            } else {
                for child in &elem.children {
                    push_child(out, child, depth + 1)?;
                }
            }
        }
        "annotation" | "annotation-xml" | "mprescripts" | "none" | "mspace" => {
            if elem.name == "mspace" {
                out.push(Node::Space);
            }
        }
        "mmultiscripts" => push_multiscripts(out, elem, depth)?,
        _ => {
            if let Some(node) = node_from(elem, depth)? {
                out.push(node);
            }
        }
    }
    Ok(())
}

fn push_multiscripts(out: &mut Vec<Node>, elem: &Elem, depth: usize) -> Result<(), MathmlError> {
    let elements: Vec<&Elem> = elem
        .children
        .iter()
        .filter_map(|child| match child {
            Child::Elem(elem) => Some(elem),
            Child::Text(_) => None,
        })
        .collect();
    let Some((base, rest)) = elements.split_first() else {
        return Ok(());
    };
    let base = *base;
    let prescripts = rest.iter().position(|elem| elem.name == "mprescripts");
    let (post, pre) = match prescripts {
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (rest, &[][..]),
    };
    let parts = pre
        .iter()
        .copied()
        .chain(std::iter::once(base))
        .chain(post.iter().copied());
    for elem in parts {
        push_child(out, &Child::Elem(elem.clone_shallow()), depth + 1)?;
    }
    Ok(())
}

impl Elem {
    /// A copy without children, so a multiscript part can be read again.
    fn clone_shallow(&self) -> Self {
        Self {
            name: self.name.clone(),
            attrs: self.attrs.clone(),
            children: self.children.iter().map(Child::shallow).collect(),
        }
    }
}

impl Child {
    fn shallow(&self) -> Self {
        match self {
            Self::Text(text) => Self::Text(text.clone()),
            Self::Elem(elem) => Self::Elem(elem.clone_shallow()),
        }
    }
}

fn node_from(elem: &Elem, depth: usize) -> Result<Option<Node>, MathmlError> {
    if depth > MAX_DEPTH {
        return Err(MathmlError::TooDeep);
    }
    let next = depth + 1;
    let node = match elem.name.as_str() {
        "mi" | "mn" | "mo" | "mtext" | "ms" => token_node(elem),
        "mfrac" => Some(Node::Fraction {
            numerator: argument(elem, 0, next)?,
            denominator: argument(elem, 1, next)?,
        }),
        "msqrt" => Some(Node::Radical {
            degree: None,
            radicand: row_from(&elem.children, next)?,
        }),
        "mroot" => Some(Node::Radical {
            degree: Some(argument(elem, 1, next)?),
            radicand: argument(elem, 0, next)?,
        }),
        "msub" => Some(script_node(
            argument(elem, 0, next)?,
            Some(argument(elem, 1, next)?),
            None,
        )),
        "msup" => Some(script_node(
            argument(elem, 0, next)?,
            None,
            Some(argument(elem, 1, next)?),
        )),
        "msubsup" => Some(script_node(
            argument(elem, 0, next)?,
            Some(argument(elem, 1, next)?),
            Some(argument(elem, 2, next)?),
        )),
        "munder" => Some(limits_node(
            argument(elem, 0, next)?,
            Some(argument(elem, 1, next)?),
            None,
        )),
        "mover" => Some(limits_node(
            argument(elem, 0, next)?,
            None,
            Some(argument(elem, 1, next)?),
        )),
        "munderover" => Some(limits_node(
            argument(elem, 0, next)?,
            Some(argument(elem, 1, next)?),
            Some(argument(elem, 2, next)?),
        )),
        "mfenced" => Some(fenced_node(elem, next)?),
        "mtable" => Some(matrix_node(elem, "", "", next)?),
        _ => None,
    };
    Ok(node)
}

fn script_node(base: Row, sub: Option<Row>, sup: Option<Row>) -> Node {
    Node::Script { base, sub, sup }
}

fn limits_node(base: Row, under: Option<Row>, over: Option<Row>) -> Node {
    Node::Limits {
        base,
        under,
        over,
        body: None,
    }
}

/// `<mrow><munderover>…</munderover>…</mrow>` keeps the tail as the operand.
fn operator_with_body(elem: &Elem, depth: usize) -> Result<Option<Node>, MathmlError> {
    if elem.name != "mrow" {
        return Ok(None);
    }
    let elements: Vec<&Elem> = elem.children.iter().filter_map(as_elem).collect();
    let Some((head, rest)) = elements.split_first() else {
        return Ok(None);
    };
    if rest.is_empty() || !matches!(head.name.as_str(), "munder" | "mover" | "munderover") {
        return Ok(None);
    }
    let Some(Node::Limits {
        base, under, over, ..
    }) = node_from(head, depth + 1)?
    else {
        return Ok(None);
    };
    let mut nodes = Vec::new();
    for child in rest {
        push_child(&mut nodes, &Child::Elem(child.clone_shallow()), depth + 1)?;
    }
    Ok(Some(Node::Limits {
        base,
        under,
        over,
        body: Some(Row { nodes }),
    }))
}

fn token_node(elem: &Elem) -> Option<Node> {
    let text = text_of(elem);
    if text.is_empty() {
        return None;
    }
    let kind = match elem.name.as_str() {
        "mn" => TokenKind::Number,
        "mo" => TokenKind::Operator,
        "mtext" | "ms" => TokenKind::Text,
        _ => TokenKind::Identifier,
    };
    let variant = attr(elem, "mathvariant");
    let upright = matches!(variant, Some("normal" | "bold"))
        || matches!(elem.name.as_str(), "mtext" | "ms")
        || (kind == TokenKind::Identifier && text.chars().count() > 1);
    Some(Node::token(kind, &text, upright))
}

fn text_of(elem: &Elem) -> String {
    let mut text = String::new();
    for child in &elem.children {
        if let Child::Text(part) = child {
            text.push_str(part);
        }
    }
    text.trim().to_owned()
}

fn argument(elem: &Elem, index: usize, depth: usize) -> Result<Row, MathmlError> {
    match elem.children.iter().filter_map(as_elem).nth(index) {
        Some(child) => {
            let mut nodes = Vec::new();
            push_child(&mut nodes, &Child::Elem(child.clone_shallow()), depth)?;
            Ok(Row { nodes })
        }
        None => Ok(Row::default()),
    }
}

fn as_elem(child: &Child) -> Option<&Elem> {
    match child {
        Child::Elem(elem) => Some(elem),
        Child::Text(_) => None,
    }
}

fn fenced_node(elem: &Elem, depth: usize) -> Result<Node, MathmlError> {
    let open = attr(elem, "open").unwrap_or("(").to_owned();
    let close = attr(elem, "close").unwrap_or(")").to_owned();
    let elements: Vec<&Elem> = elem.children.iter().filter_map(as_elem).collect();
    if elements.len() == 1 && elements[0].name == "mtable" {
        return matrix_node(elements[0], &open, &close, depth);
    }
    Ok(Node::Fenced {
        open,
        close,
        body: row_from(&elem.children, depth)?,
    })
}

fn matrix_node(elem: &Elem, open: &str, close: &str, depth: usize) -> Result<Node, MathmlError> {
    let mut cells = Vec::new();
    for row in elem
        .children
        .iter()
        .filter_map(as_elem)
        .filter(|elem| elem.name == "mtr")
    {
        let mut cells_in_row = Vec::new();
        for cell in row
            .children
            .iter()
            .filter_map(as_elem)
            .filter(|elem| elem.name == "mtd")
        {
            cells_in_row.push(row_from(&cell.children, depth + 1)?);
        }
        if !cells_in_row.is_empty() {
            cells.push(cells_in_row);
        }
    }
    if cells.is_empty() {
        cells.push(vec![Row::default()]);
    }
    let width = cells.iter().map(Vec::len).max().unwrap_or(1);
    for row in &mut cells {
        row.resize_with(width, Row::default);
    }
    Ok(Node::Matrix {
        open: open.to_owned(),
        close: close.to_owned(),
        cells,
    })
}

fn attr<'a>(elem: &'a Elem, name: &str) -> Option<&'a str> {
    elem.attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn write_row(out: &mut String, row: &Row) {
    if row.nodes.len() == 1 {
        write_node(out, &row.nodes[0]);
        return;
    }
    out.push_str("<mrow>");
    for node in &row.nodes {
        write_node(out, node);
    }
    out.push_str("</mrow>");
}

fn write_node(out: &mut String, node: &Node) {
    match node {
        Node::Token(token) => write_token(out, token),
        Node::Space => out.push_str("<mspace width=\"0.3em\"/>"),
        Node::Fraction {
            numerator,
            denominator,
        } => {
            out.push_str("<mfrac>");
            write_row(out, numerator);
            write_row(out, denominator);
            out.push_str("</mfrac>");
        }
        Node::Radical {
            degree: None,
            radicand,
        } => {
            out.push_str("<msqrt>");
            write_row(out, radicand);
            out.push_str("</msqrt>");
        }
        Node::Radical {
            degree: Some(degree),
            radicand,
        } => {
            out.push_str("<mroot>");
            write_row(out, radicand);
            write_row(out, degree);
            out.push_str("</mroot>");
        }
        Node::Script { base, sub, sup } => write_script(out, base, sub.as_ref(), sup.as_ref()),
        Node::Limits {
            base,
            under,
            over,
            body,
        } => {
            if let Some(body) = body {
                out.push_str("<mrow>");
                write_limits(out, base, under.as_ref(), over.as_ref());
                write_row(out, body);
                out.push_str("</mrow>");
            } else {
                write_limits(out, base, under.as_ref(), over.as_ref());
            }
        }
        Node::Fenced { open, close, body } => {
            write_fenced(out, open, close, |out| write_row(out, body));
        }
        Node::Matrix { open, close, cells } => {
            let table = |out: &mut String| write_table(out, cells);
            if open.is_empty() && close.is_empty() {
                table(out);
            } else {
                write_fenced(out, open, close, table);
            }
        }
    }
}

fn write_token(out: &mut String, token: &crate::model::Token) {
    let tag = match token.kind {
        TokenKind::Identifier => "mi",
        TokenKind::Number => "mn",
        TokenKind::Operator => "mo",
        TokenKind::Text => "mtext",
    };
    let variant = if token.kind == TokenKind::Identifier && token.upright {
        " mathvariant=\"normal\""
    } else {
        ""
    };
    out.push('<');
    out.push_str(tag);
    out.push_str(variant);
    out.push('>');
    push_escaped(out, &token.text);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn write_script(out: &mut String, base: &Row, sub: Option<&Row>, sup: Option<&Row>) {
    let tag = match (sub, sup) {
        (Some(_), Some(_)) => "msubsup",
        (Some(_), None) => "msub",
        _ => "msup",
    };
    out.push('<');
    out.push_str(tag);
    out.push('>');
    write_row(out, base);
    if let Some(sub) = sub {
        write_row(out, sub);
    }
    if let Some(sup) = sup {
        write_row(out, sup);
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn write_limits(out: &mut String, base: &Row, under: Option<&Row>, over: Option<&Row>) {
    let tag = match (under, over) {
        (Some(_), Some(_)) => "munderover",
        (Some(_), None) => "munder",
        _ => "mover",
    };
    out.push('<');
    out.push_str(tag);
    out.push('>');
    write_row(out, base);
    if let Some(under) = under {
        write_row(out, under);
    }
    if let Some(over) = over {
        write_row(out, over);
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn write_fenced(out: &mut String, open: &str, close: &str, body: impl FnOnce(&mut String)) {
    out.push_str("<mfenced open=\"");
    push_escaped(out, open);
    out.push_str("\" close=\"");
    push_escaped(out, close);
    out.push_str("\">");
    body(out);
    out.push_str("</mfenced>");
}

fn write_table(out: &mut String, cells: &[Vec<Row>]) {
    out.push_str("<mtable>");
    for row in cells {
        out.push_str("<mtr>");
        for cell in row {
            out.push_str("<mtd>");
            write_row(out, cell);
            out.push_str("</mtd>");
        }
        out.push_str("</mtr>");
    }
    out.push_str("</mtable>");
}

fn push_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, write};

    const QUADRATIC: &str = concat!(
        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\">",
        "<mrow><mi>x</mi><mo>=</mo><mfrac><mrow><mo>-</mo><mi>b</mi><mo>±</mo>",
        "<msqrt><mrow><msup><mi>b</mi><mn>2</mn></msup><mo>-</mo><mn>4</mn>",
        "<mi>a</mi><mi>c</mi></mrow></msqrt></mrow><mrow><mn>2</mn><mi>a</mi></mrow>",
        "</mfrac></mrow></math>"
    );

    #[test]
    fn a_quadratic_formula_roundtrips() {
        let formula = parse(QUADRATIC).unwrap();
        let written = write(&formula);
        let again = parse(&written).unwrap();
        assert_eq!(written, write(&again));
        assert!(written.contains("<mfrac>"));
        assert!(written.contains("<msqrt>"));
        assert!(written.contains("<msup>"));
    }

    #[test]
    fn a_matrix_keeps_its_fences() {
        let source = "<math><mfenced open=\"[\" close=\"]\"><mtable>\
            <mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr>\
            <mtr><mtd><mi>c</mi></mtd><mtd><mi>d</mi></mtd></mtr>\
            </mtable></mfenced></math>";
        let written = write(&parse(source).unwrap());
        assert!(written.contains("open=\"[\""));
        assert!(written.contains("<mi>d</mi>"));
    }

    #[test]
    fn missing_math_is_an_error() {
        assert!(parse("<mi>x</mi>").is_err());
    }
}
