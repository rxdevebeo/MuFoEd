//! LaTeX math to Office Math Markup Language (OMML).
//!
//! The translator is a bounded recursive-descent parser over the token slice
//! the document parser captured between the math delimiters. Unsupported
//! commands are reported as losses and produce no node; every recognised
//! construct maps onto a real `m:*` node so the model stays exact.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{
    Accent, ArgumentProperties, Bar, Delimiter, EquationArray, Fraction, LimitLocation,
    MathArgument, MathExpression, MathLimit, MathNode, MathPosition, MathRun, MathRunProperties,
    MathStyle, Matrix, NaryOperator, Radical, SubSuperscript, Subscript, Superscript,
};

use crate::lexer::{Token, TokenKind};
use crate::report::{LatexSpan, ReportBuilder, Severity};
use crate::LatexError;

/// Shared state the math translator needs to report into.
pub(crate) struct MathContext<'a> {
    /// Report accumulator.
    pub builder: &'a mut ReportBuilder,
    /// Name of the file the tokens came from.
    pub file: String,
    /// Whether unsupported constructs must abort the import.
    pub reject: bool,
}

impl MathContext<'_> {
    fn loss(&mut self, span: LatexSpan, feature: &str) -> Result<(), LatexError> {
        self.builder
            .record(&self.file, span, Severity::Loss, feature);
        if self.reject {
            return Err(LatexError::UnsupportedRejected {
                feature: feature.to_owned(),
            });
        }
        Ok(())
    }

    fn info(&mut self, span: LatexSpan, feature: &str) {
        self.builder
            .record(&self.file, span, Severity::Info, feature);
    }
}

fn span(token: &Token) -> LatexSpan {
    LatexSpan::new(token.start, token.end)
}

fn argument(nodes: Vec<MathNode>) -> MathArgument {
    MathArgument {
        properties: ArgumentProperties::default(),
        nodes,
        location: SourceLocation::default(),
    }
}

fn normal_run(text: &str) -> MathNode {
    MathNode::Run(MathRun {
        field_char: None,
        properties: MathRunProperties {
            normal: true,
            normal_value: Some(true),
            ..MathRunProperties::default()
        },
        run_properties: None,
        text: text.to_owned(),
        location: SourceLocation::default(),
    })
}

fn alignment_run() -> MathNode {
    MathNode::Run(MathRun {
        field_char: None,
        properties: MathRunProperties {
            alignment_point: Some(true),
            ..MathRunProperties::default()
        },
        run_properties: None,
        text: String::new(),
        location: SourceLocation::default(),
    })
}

/// Translates an inline or display formula.
pub(crate) fn translate(
    tokens: &[Token],
    ctx: &mut MathContext<'_>,
) -> Result<MathExpression, LatexError> {
    let nodes = translate_nodes(tokens, ctx)?;
    Ok(MathExpression {
        nodes,
        location: SourceLocation::default(),
    })
}

/// Translates a math environment (`matrix`, `align`, `cases`, ...).
pub(crate) fn translate_environment(
    name: &str,
    tokens: &[Token],
    ctx: &mut MathContext<'_>,
) -> Result<MathExpression, LatexError> {
    let bare = name.trim_end_matches('*');
    let rows = split_top_level(tokens, &TokenKind::ControlSymbol('\\'));
    match bare {
        "equation" | "displaymath" | "math" => translate(tokens, ctx),
        "align" | "alignat" | "aligned" | "alignedat" | "gather" | "gathered" | "multline"
        | "split" | "eqnarray" => {
            let mut arguments = Vec::with_capacity(rows.len());
            for row in &rows {
                let cells = split_top_level(row, &TokenKind::Ampersand);
                let mut nodes = Vec::new();
                for (index, cell) in cells.iter().enumerate() {
                    if index > 0 {
                        nodes.push(alignment_run());
                    }
                    nodes.extend(translate_nodes(cell, ctx)?);
                }
                if !nodes.is_empty() {
                    arguments.push(argument(nodes));
                }
            }
            Ok(MathExpression {
                nodes: vec![MathNode::EquationArray(Box::new(EquationArray {
                    base_justification: None,
                    max_distance: None,
                    object_distance: None,
                    row_spacing_rule: None,
                    row_spacing: None,
                    control: None,
                    rows: arguments,
                    location: SourceLocation::default(),
                }))],
                location: SourceLocation::default(),
            })
        }
        "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix" | "cases"
        | "array" => {
            let mut grid = Vec::with_capacity(rows.len());
            for row in &rows {
                let cells = split_top_level(row, &TokenKind::Ampersand);
                let mut row_nodes = Vec::with_capacity(cells.len());
                for cell in &cells {
                    row_nodes.push(argument(translate_nodes(cell, ctx)?));
                }
                grid.push(row_nodes);
            }
            let matrix = MathNode::Matrix(Box::new(Matrix {
                base_justification: None,
                hide_placeholders: false,
                column_group_rule: None,
                column_spacing: None,
                column_group_spacing: None,
                row_spacing_rule: None,
                row_spacing: None,
                columns: Vec::new(),
                control: None,
                rows: grid,
                location: SourceLocation::default(),
            }));
            let (begin, end) = match bare {
                "pmatrix" => (Some('('), Some(')')),
                "bmatrix" => (Some('['), Some(']')),
                "Bmatrix" => (Some('{'), Some('}')),
                "vmatrix" => (Some('|'), Some('|')),
                "Vmatrix" => (Some('\u{2016}'), Some('\u{2016}')),
                "cases" => (Some('{'), None),
                _ => (None, None),
            };
            if begin.is_some() || end.is_some() {
                Ok(MathExpression {
                    nodes: vec![MathNode::Delimiter(Box::new(Delimiter {
                        begin,
                        separator: None,
                        end,
                        grow: Some(true),
                        shape: None,
                        control: None,
                        arguments: vec![argument(vec![matrix])],
                        location: SourceLocation::default(),
                    }))],
                    location: SourceLocation::default(),
                })
            } else {
                Ok(MathExpression {
                    nodes: vec![matrix],
                    location: SourceLocation::default(),
                })
            }
        }
        _ => translate(tokens, ctx),
    }
}

fn translate_nodes(
    tokens: &[Token],
    ctx: &mut MathContext<'_>,
) -> Result<Vec<MathNode>, LatexError> {
    let mut parser = MathParser {
        tokens,
        pos: 0,
        ctx,
        style: None,
        normal: false,
    };
    parser.parse_sequence()
}

/// Splits a token slice at top-level occurrences of `separator` (brace depth 0).
fn split_top_level(tokens: &[Token], separator: &TokenKind) -> Vec<Vec<Token>> {
    let mut rows: Vec<Vec<Token>> = vec![Vec::new()];
    let mut depth = 0usize;
    for token in tokens {
        match &token.kind {
            TokenKind::LBrace => {
                depth += 1;
                if let Some(current) = rows.last_mut() {
                    current.push(token.clone());
                }
            }
            TokenKind::RBrace => {
                depth = depth.saturating_sub(1);
                if let Some(current) = rows.last_mut() {
                    current.push(token.clone());
                }
            }
            kind if depth == 0 && kind == separator => rows.push(Vec::new()),
            _ => {
                if let Some(current) = rows.last_mut() {
                    current.push(token.clone());
                }
            }
        }
    }
    rows
}

struct MathParser<'t, 'a, 'm> {
    tokens: &'t [Token],
    pos: usize,
    ctx: &'m mut MathContext<'a>,
    style: Option<MathStyle>,
    normal: bool,
}

impl MathParser<'_, '_, '_> {
    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|token| &token.kind)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn is_stop(&self) -> bool {
        match self.peek() {
            None | Some(TokenKind::RBrace | TokenKind::Ampersand) => true,
            Some(TokenKind::ControlSymbol('\\')) => true,
            Some(TokenKind::ControlWord(word)) => word == "right",
            _ => false,
        }
    }

    fn parse_sequence(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let mut nodes = Vec::new();
        while !self.is_stop() {
            if matches!(self.peek(), Some(TokenKind::Space)) {
                self.advance();
                continue;
            }
            if let Some(TokenKind::ControlWord(word)) = self.peek() {
                let infix = matches!(word.as_str(), "over" | "atop" | "choose" | "above");
                if infix {
                    let word = word.clone();
                    return self.infix(nodes, &word);
                }
            }
            let atom = self.parse_atom()?;
            nodes.extend(self.apply_scripts(atom)?);
        }
        Ok(nodes)
    }

    /// Builds a fraction from the TeX infix primitives `\over`, `\atop`,
    /// `\choose` and `\above`; `nodes` is the numerator parsed so far.
    fn infix(&mut self, nodes: Vec<MathNode>, word: &str) -> Result<Vec<MathNode>, LatexError> {
        self.advance();
        if word == "above" {
            // `\above` takes a dimension; the rule thickness is not modelled.
            let _ = self.parse_atom();
        }
        let denominator = self.parse_sequence()?;
        let bar_type = match word {
            "over" => None,
            _ => Some(Arc::from("noBar")),
        };
        let fraction = MathNode::Fraction(Box::new(Fraction {
            bar_type,
            small_fraction: false,
            control: None,
            numerator: argument(nodes),
            denominator: argument(denominator),
            location: SourceLocation::default(),
        }));
        if word == "choose" {
            Ok(vec![MathNode::Delimiter(Box::new(Delimiter {
                begin: Some('('),
                separator: None,
                end: Some(')'),
                grow: Some(true),
                shape: None,
                control: None,
                arguments: vec![argument(vec![fraction])],
                location: SourceLocation::default(),
            }))])
        } else {
            Ok(vec![fraction])
        }
    }

    fn apply_scripts(&mut self, base: Vec<MathNode>) -> Result<Vec<MathNode>, LatexError> {
        let mut limit_location = None;
        loop {
            match self.peek() {
                Some(TokenKind::ControlWord(word)) if word == "limits" => {
                    self.advance();
                    limit_location = Some(LimitLocation::UnderOver);
                }
                Some(TokenKind::ControlWord(word)) if word == "nolimits" => {
                    self.advance();
                    limit_location = Some(LimitLocation::SubSup);
                }
                _ => break,
            }
        }
        let mut sub = None;
        let mut sup = None;
        loop {
            match self.peek() {
                Some(TokenKind::Sub) => {
                    self.advance();
                    sub = Some(self.parse_argument()?);
                }
                Some(TokenKind::Sup) => {
                    self.advance();
                    sup = Some(self.parse_argument()?);
                }
                _ => break,
            }
        }
        if sub.is_none() && sup.is_none() {
            return Ok(base);
        }
        let location = base
            .first()
            .map_or_else(SourceLocation::default, |node| node.location().clone());
        let mut base = base;
        if let [MathNode::NaryOperator(_)] = base.as_slice() {
            if let MathNode::NaryOperator(mut operator) = base.swap_remove(0) {
                operator.subscript = sub.unwrap_or_else(|| argument(Vec::new()));
                operator.superscript = sup.unwrap_or_else(|| argument(Vec::new()));
                if limit_location.is_some() {
                    operator.limit_location = limit_location;
                }
                return Ok(vec![MathNode::NaryOperator(operator)]);
            }
        }
        let base_argument = argument(base);
        match (sub, sup) {
            (Some(subscript), Some(superscript)) => {
                Ok(vec![MathNode::SubSuperscript(Box::new(SubSuperscript {
                    control: None,
                    base: base_argument,
                    subscript,
                    superscript,
                    location,
                }))])
            }
            (Some(subscript), None) => Ok(vec![MathNode::Subscript(Box::new(Subscript {
                control: None,
                base: base_argument,
                subscript,
                location,
            }))]),
            (None, Some(superscript)) => Ok(vec![MathNode::Superscript(Box::new(Superscript {
                control: None,
                base: base_argument,
                superscript,
                location,
            }))]),
            (None, None) => Ok(Vec::new()),
        }
    }

    /// Parses one script argument: a braced group or a single atom.
    fn parse_argument(&mut self) -> Result<MathArgument, LatexError> {
        if matches!(self.peek(), Some(TokenKind::LBrace)) {
            self.advance();
            let nodes = self.parse_sequence()?;
            if matches!(self.peek(), Some(TokenKind::RBrace)) {
                self.advance();
            }
            Ok(argument(nodes))
        } else {
            let atom = self.parse_atom()?;
            Ok(argument(atom))
        }
    }

    fn styled_run(&self, text: &str) -> MathNode {
        let mut properties = MathRunProperties::default();
        if self.normal {
            properties.normal = true;
            properties.normal_value = Some(true);
        }
        if let Some(style) = self.style {
            properties.style = Some(style);
        }
        MathNode::Run(MathRun {
            field_char: None,
            properties,
            run_properties: None,
            text: text.to_owned(),
            location: SourceLocation::default(),
        })
    }

    fn parse_atom(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let Some(token) = self.advance() else {
            return Ok(Vec::new());
        };
        let token_span = span(&token);
        match token.kind {
            TokenKind::LBrace => {
                let nodes = self.parse_sequence()?;
                if matches!(self.peek(), Some(TokenKind::RBrace)) {
                    self.advance();
                }
                Ok(nodes)
            }
            TokenKind::Text(text) => Ok(vec![self.styled_run(&text)]),
            TokenKind::Raw(text) => Ok(vec![normal_run(&text)]),
            TokenKind::Tilde => Ok(vec![normal_run("\u{00A0}")]),
            TokenKind::LBracket => Ok(vec![normal_run("[")]),
            TokenKind::RBracket => Ok(vec![normal_run("]")]),
            TokenKind::Sub => Ok(vec![normal_run("_")]),
            TokenKind::Sup => Ok(vec![normal_run("^")]),
            TokenKind::ControlSymbol(c) => self.control_symbol(c, token_span),
            TokenKind::ControlWord(word) => self.control_word(&word, token_span),
            TokenKind::Space
            | TokenKind::MathShift
            | TokenKind::ParBreak
            | TokenKind::Ampersand
            | TokenKind::RBrace => Ok(Vec::new()),
        }
    }

    fn control_symbol(
        &mut self,
        symbol: char,
        span: LatexSpan,
    ) -> Result<Vec<MathNode>, LatexError> {
        let text = match symbol {
            '{' => "{",
            '}' => "}",
            '%' => "%",
            '&' => "&",
            '#' => "#",
            '$' => "$",
            '_' => "_",
            ' ' | ',' | ';' | ':' | '!' => "\u{2009}",
            '\\' => return Ok(Vec::new()),
            other => {
                self.ctx.info(span, &format!("math.control-symbol:{other}"));
                return Ok(vec![normal_run(&other.to_string())]);
            }
        };
        Ok(vec![normal_run(text)])
    }

    fn control_word(&mut self, word: &str, span: LatexSpan) -> Result<Vec<MathNode>, LatexError> {
        match word {
            "frac" | "dfrac" | "tfrac" | "cfrac" => self.fraction().map(|node| vec![node]),
            "sqrt" => self.radical().map(|node| vec![node]),
            "left" => self.delimiters().map(|node| vec![node]),
            "begin" => self.math_environment(),
            "text" | "textrm" | "textnormal" | "mbox" | "operatorname" => self.text_argument(),
            "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" => {
                let style = match word {
                    "mathbf" => MathStyle::Bold,
                    "mathit" => MathStyle::Italic,
                    _ => MathStyle::Plain,
                };
                self.styled_group(style)
            }
            "mathbb" => self.blackboard_group(),
            "mathcal" | "mathfrak" | "boldsymbol" | "bm" => {
                self.ctx.info(span, &format!("math.style:{word}"));
                self.styled_group(MathStyle::BoldItalic)
            }
            "binom" | "dbinom" | "tbinom" => self.binomial(),
            "genfrac" => self.genfrac(),
            "substack" => self.substack(),
            "stackrel" => self.stackrel(),
            "not" => self.negate(),
            "mathop" | "mathrel" | "mathbin" | "mathord" | "mathpunct" | "mathinner" => {
                self.parse_argument().map(|argument| argument.nodes)
            }
            "limits" | "nolimits" => Ok(Vec::new()),
            "pmod" => {
                let mut nodes = vec![normal_run("\u{2009}(mod\u{2009}")];
                nodes.extend(self.parse_argument()?.nodes);
                nodes.push(normal_run(")"));
                Ok(nodes)
            }
            "bmod" => Ok(vec![normal_run("\u{2009}mod\u{2009}")]),
            "tag" => {
                let text = self.raw_group();
                Ok(vec![
                    normal_run("\u{2003}\u{2003}("),
                    normal_run(&text),
                    normal_run(")"),
                ])
            }
            "overline" => self.bar(MathPosition::Top),
            "underline" => self.bar(MathPosition::Bottom),
            "bar" => self.accent('\u{0304}'),
            "hat" | "widehat" => self.accent('\u{0302}'),
            "tilde" | "widetilde" => self.accent('\u{0303}'),
            "vec" => self.accent('\u{20D7}'),
            "dot" => self.accent('\u{0307}'),
            "ddot" => self.accent('\u{0308}'),
            "acute" => self.accent('\u{0301}'),
            "grave" => self.accent('\u{0300}'),
            "breve" => self.accent('\u{0306}'),
            "check" => self.accent('\u{030C}'),
            "quad" | "qquad" | "thinspace" | "enspace" => Ok(vec![normal_run("\u{2009}")]),
            _ => {
                if let Some(node) = operator(word) {
                    return Ok(vec![node]);
                }
                if let Some(text) = math_symbols::greek(word) {
                    return Ok(vec![self.styled_run(text)]);
                }
                if let Some(text) = math_symbols::symbol(word) {
                    return Ok(vec![normal_run(text)]);
                }
                self.ctx.loss(span, &format!("math.command:{word}"))?;
                Ok(Vec::new())
            }
        }
    }

    fn fraction(&mut self) -> Result<MathNode, LatexError> {
        let numerator = self.parse_argument()?;
        let denominator = self.parse_argument()?;
        Ok(MathNode::Fraction(Box::new(Fraction {
            bar_type: None,
            small_fraction: false,
            control: None,
            numerator,
            denominator,
            location: SourceLocation::default(),
        })))
    }

    fn radical(&mut self) -> Result<MathNode, LatexError> {
        let degree = if matches!(self.peek(), Some(TokenKind::LBracket)) {
            self.advance();
            let nodes = self.parse_sequence()?;
            if matches!(self.peek(), Some(TokenKind::RBracket)) {
                self.advance();
            }
            argument(nodes)
        } else {
            argument(Vec::new())
        };
        let radicand = self.parse_argument()?;
        Ok(MathNode::Radical(Box::new(Radical {
            hide_degree: degree.nodes.is_empty(),
            control: None,
            degree,
            radicand,
            location: SourceLocation::default(),
        })))
    }

    fn delimiters(&mut self) -> Result<MathNode, LatexError> {
        let begin = self.delimiter();
        let mut nodes = Vec::new();
        while !self.is_stop() {
            if matches!(self.peek(), Some(TokenKind::ControlWord(word)) if word == "middle") {
                self.advance();
                if let Some(chr) = self.delimiter() {
                    nodes.push(normal_run(&chr.to_string()));
                }
                continue;
            }
            let atom = self.parse_atom()?;
            nodes.extend(self.apply_scripts(atom)?);
        }
        let end = if matches!(self.peek(), Some(TokenKind::ControlWord(word)) if word == "right") {
            self.advance();
            self.delimiter()
        } else {
            None
        };
        Ok(MathNode::Delimiter(Box::new(Delimiter {
            begin,
            separator: Some('|'),
            end,
            grow: Some(true),
            shape: None,
            control: None,
            arguments: vec![argument(nodes)],
            location: SourceLocation::default(),
        })))
    }

    fn delimiter(&mut self) -> Option<char> {
        let token = self.advance()?;
        match token.kind {
            TokenKind::Text(text) => text.chars().next().filter(|c| *c != '.'),
            TokenKind::LBrace => Some('{'),
            TokenKind::RBrace => Some('}'),
            TokenKind::LBracket => Some('['),
            TokenKind::RBracket => Some(']'),
            TokenKind::ControlSymbol(c) => Some(c),
            TokenKind::ControlWord(word) => match word.as_str() {
                "langle" => Some('\u{27E8}'),
                "rangle" => Some('\u{27E9}'),
                "lvert" | "rvert" | "vert" => Some('|'),
                "Vert" => Some('\u{2016}'),
                "lbrace" => Some('{'),
                "rbrace" => Some('}'),
                _ => Some(word.chars().next().unwrap_or('?')),
            },
            _ => None,
        }
    }

    fn bar(&mut self, position: MathPosition) -> Result<Vec<MathNode>, LatexError> {
        let base = self.parse_argument()?;
        Ok(vec![MathNode::Bar(Box::new(Bar {
            position: Some(position),
            control: None,
            base,
            location: SourceLocation::default(),
        }))])
    }

    fn accent(&mut self, chr: char) -> Result<Vec<MathNode>, LatexError> {
        let base = self.parse_argument()?;
        Ok(vec![MathNode::Accent(Box::new(Accent {
            chr: Some(chr),
            control: None,
            base,
            location: SourceLocation::default(),
        }))])
    }

    /// Renders `\text{...}`, translating any nested `$...$` back to math.
    fn text_argument(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let tokens = self.read_group_tokens();
        let mut nodes = Vec::new();
        let mut current = String::new();
        let mut index = 0;
        let mut in_math = false;
        while index < tokens.len() {
            if matches!(tokens[index].kind, TokenKind::MathShift) {
                if !current.is_empty() {
                    nodes.push(normal_run(&current));
                    current.clear();
                }
                in_math = !in_math;
                index += 1;
                continue;
            }
            if in_math {
                let start = index;
                while index < tokens.len() && !matches!(tokens[index].kind, TokenKind::MathShift) {
                    index += 1;
                }
                let expression = translate(&tokens[start..index], self.ctx)?;
                nodes.extend(expression.nodes);
                continue;
            }
            current.push_str(&tokens[index].kind.raw());
            index += 1;
        }
        if !current.is_empty() {
            nodes.push(normal_run(&current));
        }
        Ok(nodes)
    }

    /// `\mathbb{...}`: maps letters and digits to their double-struck forms.
    fn blackboard_group(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let text = self.raw_group();
        let mapped: String = text
            .chars()
            .map(|c| double_struck(c).unwrap_or(c))
            .collect();
        Ok(vec![normal_run(&mapped)])
    }

    fn binomial(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let numerator = self.parse_argument()?;
        let denominator = self.parse_argument()?;
        let fraction = MathNode::Fraction(Box::new(Fraction {
            bar_type: Some(Arc::from("noBar")),
            small_fraction: false,
            control: None,
            numerator,
            denominator,
            location: SourceLocation::default(),
        }));
        Ok(vec![MathNode::Delimiter(Box::new(Delimiter {
            begin: Some('('),
            separator: None,
            end: Some(')'),
            grow: Some(true),
            shape: None,
            control: None,
            arguments: vec![argument(vec![fraction])],
            location: SourceLocation::default(),
        }))])
    }

    fn genfrac(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let begin = self.raw_group();
        let end = self.raw_group();
        let thickness = self.raw_group();
        let _style = self.raw_group();
        let numerator = self.parse_argument()?;
        let denominator = self.parse_argument()?;
        let trimmed = thickness.trim();
        let bar_type = if trimmed == "0" || trimmed == "0pt" {
            Some(Arc::from("noBar"))
        } else {
            None
        };
        let fraction = MathNode::Fraction(Box::new(Fraction {
            bar_type,
            small_fraction: false,
            control: None,
            numerator,
            denominator,
            location: SourceLocation::default(),
        }));
        let begin_char = begin.trim().chars().next();
        let end_char = end.trim().chars().next();
        if begin_char.is_some() || end_char.is_some() {
            Ok(vec![MathNode::Delimiter(Box::new(Delimiter {
                begin: begin_char,
                separator: None,
                end: end_char,
                grow: Some(true),
                shape: None,
                control: None,
                arguments: vec![argument(vec![fraction])],
                location: SourceLocation::default(),
            }))])
        } else {
            Ok(vec![fraction])
        }
    }

    fn substack(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let tokens = self.read_group_tokens();
        let rows = split_top_level(&tokens, &TokenKind::ControlSymbol('\\'));
        let mut arguments = Vec::new();
        for row in rows {
            let nodes = translate_nodes(&row, self.ctx)?;
            if !nodes.is_empty() {
                arguments.push(argument(nodes));
            }
        }
        Ok(vec![MathNode::EquationArray(Box::new(EquationArray {
            base_justification: None,
            max_distance: None,
            object_distance: None,
            row_spacing_rule: None,
            row_spacing: None,
            control: None,
            rows: arguments,
            location: SourceLocation::default(),
        }))])
    }

    fn stackrel(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let over = self.parse_argument()?;
        let base = self.parse_argument()?;
        Ok(vec![MathNode::Limit(Box::new(MathLimit {
            above: true,
            control: None,
            base,
            limit: over,
            location: SourceLocation::default(),
        }))])
    }

    fn negate(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let nodes = self.parse_atom()?;
        let mut nodes = nodes;
        if let Some(MathNode::Run(run)) = nodes.first_mut() {
            if let Some(ch) = run.text.chars().next() {
                if let Some(negated) = negated(ch) {
                    run.text = format!("{negated}{}", &run.text[ch.len_utf8()..]);
                } else {
                    run.text.insert(0, '\u{0338}');
                }
            }
        }
        Ok(nodes)
    }

    /// Reads a `{...}` group as its inner tokens (braces removed).
    fn read_group_tokens(&mut self) -> Vec<Token> {
        if !matches!(self.peek(), Some(TokenKind::LBrace)) {
            return Vec::new();
        }
        self.advance();
        let mut out = Vec::new();
        let mut depth = 1usize;
        while let Some(token) = self.advance() {
            match token.kind {
                TokenKind::LBrace => {
                    depth += 1;
                    out.push(token);
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(token);
                }
                _ => out.push(token),
            }
        }
        out
    }

    fn styled_group(&mut self, style: MathStyle) -> Result<Vec<MathNode>, LatexError> {
        if matches!(self.peek(), Some(TokenKind::LBrace)) {
            self.advance();
            let previous = self.style;
            self.style = Some(style);
            let nodes = self.parse_sequence()?;
            self.style = previous;
            if matches!(self.peek(), Some(TokenKind::RBrace)) {
                self.advance();
            }
            Ok(nodes)
        } else {
            self.parse_atom()
        }
    }

    /// Consumes a `{...}` group and returns its raw source spelling.
    fn raw_group(&mut self) -> String {
        if !matches!(self.peek(), Some(TokenKind::LBrace)) {
            return String::new();
        }
        self.advance();
        let mut text = String::new();
        let mut depth = 1usize;
        while let Some(token) = self.advance() {
            match token.kind {
                TokenKind::LBrace => {
                    depth += 1;
                    text.push('{');
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    text.push('}');
                }
                kind => text.push_str(&kind.raw()),
            }
        }
        text
    }

    fn math_environment(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let name = self.raw_group();
        let stop = TokenKind::ControlWord("end".to_owned());
        let mut captured = Vec::new();
        while let Some(kind) = self.peek() {
            if *kind == stop {
                break;
            }
            if let Some(token) = self.advance() {
                captured.push(token);
            }
        }
        if matches!(self.peek(), Some(kind) if *kind == stop) {
            self.advance();
            let _ = self.raw_group();
        }
        let expression = translate_environment(&name, &captured, self.ctx)?;
        Ok(expression.nodes)
    }
}

/// Maps a recognized n-ary operator or function word to a math node.
fn operator(word: &str) -> Option<MathNode> {
    if let Some((chr, sub_sup)) = math_symbols::nary(word) {
        return Some(MathNode::NaryOperator(Box::new(NaryOperator {
            chr: Some(chr),
            limit_location: Some(if sub_sup {
                LimitLocation::SubSup
            } else {
                LimitLocation::UnderOver
            }),
            grow: Some(true),
            hide_sub: false,
            hide_sup: false,
            control: None,
            subscript: argument(Vec::new()),
            superscript: argument(Vec::new()),
            operand: argument(Vec::new()),
            location: SourceLocation::default(),
        })));
    }
    math_symbols::function(word).map(normal_run)
}

/// Maps a letter or digit to its Unicode double-struck form (`\mathbb`).
fn double_struck(c: char) -> Option<char> {
    match c {
        'C' => Some('\u{2102}'),
        'H' => Some('\u{210D}'),
        'N' => Some('\u{2115}'),
        'P' => Some('\u{2119}'),
        'Q' => Some('\u{211A}'),
        'R' => Some('\u{211D}'),
        'Z' => Some('\u{2124}'),
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32)),
        'a'..='z' => char::from_u32(0x1D552 + (c as u32 - 'a' as u32)),
        '0'..='9' => char::from_u32(0x1D7D8 + (c as u32 - '0' as u32)),
        _ => None,
    }
}

/// Maps a relation to its negated (`\not`) form.
fn negated(c: char) -> Option<char> {
    Some(match c {
        '=' => '\u{2260}',
        '<' => '\u{226E}',
        '>' => '\u{226F}',
        '\u{2264}' => '\u{2270}',
        '\u{2265}' => '\u{2271}',
        '\u{2261}' => '\u{2262}',
        '\u{2208}' => '\u{2209}',
        '\u{220B}' => '\u{220C}',
        '\u{2282}' => '\u{2284}',
        '\u{2283}' => '\u{2285}',
        '\u{2286}' => '\u{2288}',
        '\u{2287}' => '\u{2289}',
        '\u{2223}' | '|' => '\u{2224}',
        '\u{2225}' => '\u{2226}',
        '\u{223C}' => '\u{2241}',
        '\u{2243}' => '\u{2244}',
        '\u{2248}' => '\u{2249}',
        '\u{2245}' => '\u{2247}',
        '\u{2203}' => '\u{2204}',
        _ => return None,
    })
}

/// Symbol tables used by [`operator`] and the parser.
pub(crate) mod math_symbols {
    /// N-ary operator words to (character, use sub/superscript limits).
    pub(crate) fn nary(word: &str) -> Option<(char, bool)> {
        Some(match word {
            "sum" => ('\u{2211}', false),
            "prod" => ('\u{220F}', false),
            "coprod" => ('\u{2210}', false),
            "bigcup" => ('\u{22C3}', false),
            "bigcap" => ('\u{22C2}', false),
            "bigoplus" => ('\u{2A01}', false),
            "bigotimes" => ('\u{2A02}', false),
            "bigodot" => ('\u{2A00}', false),
            "biguplus" => ('\u{2A04}', false),
            "bigsqcup" => ('\u{2A06}', false),
            "bigvee" => ('\u{22C1}', false),
            "bigwedge" => ('\u{22C0}', false),
            "int" => ('\u{222B}', true),
            "iint" => ('\u{222C}', true),
            "iiint" => ('\u{222D}', true),
            "oint" => ('\u{222E}', true),
            _ => return None,
        })
    }

    /// Function names that typeset upright.
    pub(crate) fn function(word: &str) -> Option<&'static str> {
        Some(match word {
            "sin" => "sin",
            "cos" => "cos",
            "tan" => "tan",
            "cot" => "cot",
            "sec" => "sec",
            "csc" => "csc",
            "arcsin" => "arcsin",
            "arccos" => "arccos",
            "arctan" => "arctan",
            "sinh" => "sinh",
            "cosh" => "cosh",
            "tanh" => "tanh",
            "log" => "log",
            "ln" => "ln",
            "lg" => "lg",
            "exp" => "exp",
            "max" => "max",
            "min" => "min",
            "lim" => "lim",
            "limsup" => "lim sup",
            "liminf" => "lim inf",
            "sup" => "sup",
            "inf" => "inf",
            "det" => "det",
            "dim" => "dim",
            "ker" => "ker",
            "deg" => "deg",
            "gcd" => "gcd",
            "hom" => "hom",
            "arg" => "arg",
            "mod" => "mod",
            "bmod" => "mod",
            _ => return None,
        })
    }

    /// Curated mapping of common LaTeX math symbols to Unicode.
    pub(crate) fn symbol(word: &str) -> Option<&'static str> {
        Some(match word {
            "cdot" => "\u{22C5}",
            "times" => "\u{00D7}",
            "div" => "\u{00F7}",
            "pm" => "\u{00B1}",
            "mp" => "\u{2213}",
            "ast" => "\u{2217}",
            "star" => "\u{22C6}",
            "circ" => "\u{2218}",
            "bullet" => "\u{2219}",
            "oplus" => "\u{2295}",
            "ominus" => "\u{2296}",
            "otimes" => "\u{2297}",
            "odot" => "\u{2299}",
            "le" | "leq" => "\u{2264}",
            "ge" | "geq" => "\u{2265}",
            "ne" | "neq" => "\u{2260}",
            "approx" => "\u{2248}",
            "equiv" => "\u{2261}",
            "cong" => "\u{2245}",
            "sim" => "\u{223C}",
            "simeq" => "\u{2243}",
            "propto" => "\u{221D}",
            "infty" => "\u{221E}",
            "partial" => "\u{2202}",
            "nabla" => "\u{2207}",
            "forall" => "\u{2200}",
            "exists" => "\u{2203}",
            "nexists" => "\u{2204}",
            "in" => "\u{2208}",
            "notin" => "\u{2209}",
            "ni" => "\u{220B}",
            "subset" => "\u{2282}",
            "subseteq" => "\u{2286}",
            "supset" => "\u{2283}",
            "supseteq" => "\u{2287}",
            "cup" => "\u{222A}",
            "cap" => "\u{2229}",
            "setminus" => "\u{2216}",
            "wedge" | "land" => "\u{2227}",
            "vee" | "lor" => "\u{2228}",
            "neg" | "lnot" => "\u{00AC}",
            "to" | "rightarrow" => "\u{2192}",
            "gets" | "leftarrow" => "\u{2190}",
            "leftrightarrow" => "\u{2194}",
            "Rightarrow" | "implies" => "\u{21D2}",
            "Leftarrow" => "\u{21D0}",
            "Leftrightarrow" | "iff" => "\u{21D4}",
            "mapsto" => "\u{21A6}",
            "uparrow" => "\u{2191}",
            "downarrow" => "\u{2193}",
            "ldots" | "dots" => "\u{2026}",
            "cdots" => "\u{22EF}",
            "vdots" => "\u{22EE}",
            "ddots" => "\u{22F1}",
            "prime" => "\u{2032}",
            "angle" => "\u{2220}",
            "perp" => "\u{22A5}",
            "parallel" => "\u{2225}",
            "vdash" => "\u{22A2}",
            "dashv" => "\u{22A3}",
            "models" => "\u{22A8}",
            "ll" => "\u{226A}",
            "gg" => "\u{226B}",
            "prec" => "\u{227A}",
            "succ" => "\u{227B}",
            "langle" => "\u{27E8}",
            "rangle" => "\u{27E9}",
            "Vert" => "\u{2016}",
            "vert" => "|",
            "dagger" => "\u{2020}",
            "ddagger" => "\u{2021}",
            "ell" => "\u{2113}",
            "hbar" => "\u{210F}",
            "aleph" => "\u{2135}",
            "wp" => "\u{2118}",
            "emptyset" => "\u{2205}",
            "triangle" => "\u{25B3}",
            "square" => "\u{25A1}",
            _ => return None,
        })
    }

    /// Greek-letter names to Unicode.
    pub(crate) fn greek(word: &str) -> Option<&'static str> {
        Some(match word {
            "alpha" => "\u{03B1}",
            "beta" => "\u{03B2}",
            "gamma" => "\u{03B3}",
            "delta" => "\u{03B4}",
            "epsilon" => "\u{03F5}",
            "varepsilon" => "\u{03B5}",
            "zeta" => "\u{03B6}",
            "eta" => "\u{03B7}",
            "theta" => "\u{03B8}",
            "vartheta" => "\u{03D1}",
            "iota" => "\u{03B9}",
            "kappa" => "\u{03BA}",
            "lambda" => "\u{03BB}",
            "mu" => "\u{03BC}",
            "nu" => "\u{03BD}",
            "xi" => "\u{03BE}",
            "omicron" => "\u{03BF}",
            "pi" => "\u{03C0}",
            "varpi" => "\u{03D6}",
            "rho" => "\u{03C1}",
            "varrho" => "\u{03F1}",
            "sigma" => "\u{03C3}",
            "varsigma" => "\u{03C2}",
            "tau" => "\u{03C4}",
            "upsilon" => "\u{03C5}",
            "phi" => "\u{03D5}",
            "varphi" => "\u{03C6}",
            "chi" => "\u{03C7}",
            "psi" => "\u{03C8}",
            "omega" => "\u{03C9}",
            "Gamma" => "\u{0393}",
            "Delta" => "\u{0394}",
            "Theta" => "\u{0398}",
            "Lambda" => "\u{039B}",
            "Xi" => "\u{039E}",
            "Pi" => "\u{03A0}",
            "Sigma" => "\u{03A3}",
            "Upsilon" => "\u{03A5}",
            "Phi" => "\u{03A6}",
            "Psi" => "\u{03A8}",
            "Omega" => "\u{03A9}",
            _ => return None,
        })
    }
}
