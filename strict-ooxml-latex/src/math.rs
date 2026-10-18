//! LaTeX math to Office Math Markup Language (OMML).
//!
//! The translator is a bounded recursive-descent parser over the token slice
//! the document parser captured between the math delimiters. Unsupported
//! commands are reported as losses and produce no node; every recognised
//! construct maps onto a real `m:*` node so the model stays exact.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::math::MathVerticalAlign;
use strict_ooxml_wml::model::{
    Accent, ArgumentProperties, Bar, Delimiter, EquationArray, Fraction, LimitLocation,
    MathAlignment, MathArgument, MathExpression, MathLimit, MathNode, MathPosition, MathRun,
    MathRunProperties, MathStyle, Matrix, MatrixColumn, NaryOperator, Radical, RunProperties,
    SubSuperscript, Subscript, Superscript, TriState,
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
    /// Current recursive descent depth, shared across nested `translate` calls
    /// (`\text{$\frac{…}$}`, `\substack`), so a deeply nested formula cannot
    /// overflow the stack.
    pub depth: usize,
    /// Maximum [`depth`](Self::depth).
    pub max_depth: usize,
    pub max_columns: usize,
    /// Equation metadata transferred to the containing WML paragraph.
    pub labels: Vec<String>,
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

/// Rejoin compatible scalar atoms after script binding, keeping text compact
/// without treating a whole lexer text run as one TeX argument.
fn append_nodes(out: &mut Vec<MathNode>, incoming: Vec<MathNode>) {
    for node in incoming {
        if let (Some(MathNode::Run(left)), MathNode::Run(right)) = (out.last_mut(), &node) {
            if !left.text.is_empty()
                && !right.text.is_empty()
                && left.properties == right.properties
                && left.run_properties == right.run_properties
                && left.field_char == right.field_char
            {
                left.text.push_str(&right.text);
                continue;
            }
        }
        out.push(node);
    }
}

/// In math mode ensuremath is TeX's identity on its argument, not a
/// nested mathematical group. Flatten it iteratively before descent so chains
/// of valid automatic-math macros do not consume the structural nesting budget.
fn math_mode_tokens(tokens: &[Token]) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut groups = Vec::new();
    let mut identity = false;
    let mut text_pending = false;
    let mut text_depth = 0usize;
    for token in tokens {
        if text_depth > 0 {
            match token.kind {
                TokenKind::LBrace => text_depth += 1,
                TokenKind::RBrace => text_depth -= 1,
                _ => {}
            }
            out.push(token.clone());
            continue;
        }
        if text_pending {
            if matches!(token.kind, TokenKind::Space) {
                out.push(token.clone());
                continue;
            }
            text_pending = false;
            if matches!(token.kind, TokenKind::LBrace) {
                text_depth = 1;
                out.push(token.clone());
                continue;
            }
        }
        if matches!(&token.kind, TokenKind::ControlWord(w) if matches!(w.strip_prefix("__native:").unwrap_or(w), "text" | "textrm" | "textnormal" | "mbox" | "operatorname" | "textbf" | "textit" | "textup" | "emph"))
        {
            text_pending = true;
        }
        if matches!(&token.kind, TokenKind::ControlWord(w) if w.strip_prefix("__native:").unwrap_or(w) == "ensuremath")
        {
            identity = true;
            continue;
        }
        match token.kind {
            TokenKind::Space if identity => continue,
            TokenKind::LBrace => {
                groups.push(identity);
                if identity {
                    identity = false;
                    continue;
                }
            }
            TokenKind::RBrace => {
                if groups.pop() == Some(true) {
                    continue;
                }
            }
            _ => {}
        }
        identity = false;
        out.push(token.clone());
    }
    out
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
    let tokens = math_mode_tokens(tokens);
    let nodes = translate_nodes(&tokens, ctx)?;
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
    let normalized = math_mode_tokens(tokens);
    let tokens = normalized.as_slice();
    let bare = name.trim_end_matches('*');
    let mut body = tokens;
    let mut columns = Vec::new();
    let mut base_justification = None;
    if bare == "array" {
        while matches!(
            body.first().map(|t| &t.kind),
            Some(TokenKind::Space | TokenKind::ParBreak)
        ) {
            body = &body[1..];
        }
        if let Some(end) = bracket_end(body, 0) {
            let position: String = body[1..end - 1].iter().map(|t| t.kind.raw()).collect();
            base_justification = match position.trim() {
                "t" => Some(MathVerticalAlign::Top),
                "b" => Some(MathVerticalAlign::Bottom),
                "c" => Some(MathVerticalAlign::Center),
                _ => {
                    ctx.loss(span(&body[0]), "math.array:position")?;
                    None
                }
            };
            body = &body[end..];
            while matches!(
                body.first().map(|t| &t.kind),
                Some(TokenKind::Space | TokenKind::ParBreak)
            ) {
                body = &body[1..];
            }
        }
        // The column declaration is syntax, never a mathematical first cell.
        if let Some(end) = braced_end(body, 0) {
            let declaration: String = body[1..end - 1].iter().map(|t| t.kind.raw()).collect();
            let mut unsupported = false;
            for ch in declaration.chars().filter(|c| !c.is_whitespace()) {
                let alignment = match ch {
                    'l' => Some(MathAlignment::Left),
                    'c' => Some(MathAlignment::Center),
                    'r' => Some(MathAlignment::Right),
                    _ => None,
                };
                if let Some(alignment) = alignment {
                    columns.push(MatrixColumn {
                        justification: Some(alignment),
                        count: Some(1),
                    });
                    if columns.len() > ctx.max_columns {
                        return Err(LatexError::LimitExceeded {
                            limit: "table_columns",
                            observed: columns.len(),
                        });
                    }
                } else {
                    unsupported = true;
                }
            }
            if unsupported {
                columns.clear();
                ctx.loss(span(&body[0]), "math.array:column-preamble")?;
            }
            body = &body[end..];
        } else {
            ctx.loss(
                body.first().map_or(LatexSpan::new(0, 0), span),
                "math.array:missing-preamble",
            )?;
        }
    }
    let mut rows = split_top_level(body, &TokenKind::ControlSymbol('\\'));
    for row in rows.iter_mut().skip(1) {
        let start = row
            .iter()
            .position(|t| !matches!(t.kind, TokenKind::Space | TokenKind::ParBreak))
            .unwrap_or(row.len());
        if let Some(end) = bracket_end(row, start) {
            ctx.loss(span(&row[start]), "math.spacing:row-gap")?;
            row.drain(..end);
        }
    }
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
                base_justification,
                hide_placeholders: false,
                column_group_rule: None,
                column_spacing: None,
                column_group_spacing: None,
                row_spacing_rule: None,
                row_spacing: None,
                columns,
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
        text_offset: 0,
        ctx,
        style: None,
        normal: false,
    };
    parser.parse_sequence()
}

/// End (exclusive) of a curly group. Square brackets inside it are literals.
fn braced_end(tokens: &[Token], start: usize) -> Option<usize> {
    if !matches!(tokens.get(start)?.kind, TokenKind::LBrace) {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.kind {
            TokenKind::LBrace => depth += 1,
            TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn bracket_end(tokens: &[Token], start: usize) -> Option<usize> {
    if !matches!(tokens.get(start)?.kind, TokenKind::LBracket) {
        return None;
    }
    let mut braces = 0usize;
    let mut brackets = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.kind {
            TokenKind::LBrace => braces += 1,
            TokenKind::RBrace => braces = braces.saturating_sub(1),
            TokenKind::LBracket if braces == 0 => brackets += 1,
            TokenKind::RBracket if braces == 0 => {
                brackets -= 1;
                if brackets == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Separators in nested environments belong to those environments, even when
/// their cells are not enclosed in curly groups.
pub(crate) fn split_top_level(tokens: &[Token], separator: &TokenKind) -> Vec<Vec<Token>> {
    let mut rows: Vec<Vec<Token>> = vec![Vec::new()];
    let mut depth = 0usize;
    let mut environments = 0usize;
    for token in tokens {
        if depth == 0 {
            if let TokenKind::ControlWord(word) = &token.kind {
                if word == "begin" {
                    environments += 1;
                }
                if word == "end" {
                    environments = environments.saturating_sub(1);
                }
            }
        }
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
            kind if depth == 0 && environments == 0 && kind == separator => rows.push(Vec::new()),
            _ => {
                if let Some(current) = rows.last_mut() {
                    current.push(token.clone());
                }
            }
        }
    }
    rows
}

/// Compose text font switches without discarding an inherited bold/italic face.
fn text_style(current: MathStyle, command: &str) -> MathStyle {
    let mut bold = matches!(current, MathStyle::Bold | MathStyle::BoldItalic);
    let mut italic = matches!(current, MathStyle::Italic | MathStyle::BoldItalic);
    match command {
        "textbf" | "bfseries" => bold = true,
        "textit" | "itshape" => italic = true,
        "textup" | "upshape" => italic = false,
        "mdseries" => bold = false,
        "emph" | "em" => italic = !italic,
        "textnormal" | "normalfont" => {
            bold = false;
            italic = false;
        }
        _ => {}
    }
    match (bold, italic) {
        (true, true) => MathStyle::BoldItalic,
        (true, false) => MathStyle::Bold,
        (false, true) => MathStyle::Italic,
        (false, false) => MathStyle::Plain,
    }
}

struct MathParser<'t, 'a, 'm> {
    tokens: &'t [Token],
    pos: usize,
    /// Byte offset into a lexical text run. TeX math consumes scalar atoms,
    /// while the document lexer intentionally batches ordinary prose.
    text_offset: usize,
    ctx: &'m mut MathContext<'a>,
    style: Option<MathStyle>,
    normal: bool,
}

impl MathParser<'_, '_, '_> {
    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|token| &token.kind)
    }

    fn advance(&mut self) -> Option<Token> {
        let mut token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            if self.text_offset > 0 {
                if let Some(Token {
                    kind: TokenKind::Text(text),
                    start,
                    ..
                }) = &mut token
                {
                    *text = text[self.text_offset..].to_owned();
                    *start += self.text_offset;
                }
            }
            self.pos += 1;
            self.text_offset = 0;
        }
        token
    }

    fn advance_atom(&mut self) -> Option<Token> {
        if let Some(Token {
            kind: TokenKind::Text(text),
            start,
            ..
        }) = self.tokens.get(self.pos)
        {
            let ch = text[self.text_offset..].chars().next()?;
            let start = *start + self.text_offset;
            self.text_offset += ch.len_utf8();
            if self.text_offset == text.len() {
                self.pos += 1;
                self.text_offset = 0;
            }
            return Some(Token {
                kind: TokenKind::Text(ch.to_string()),
                start,
                end: start + ch.len_utf8(),
            });
        }
        self.advance()
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(TokenKind::Space | TokenKind::ParBreak)) {
            self.advance();
        }
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
            append_nodes(&mut nodes, self.apply_scripts(atom)?);
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
            self.skip_spaces();
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
            self.skip_spaces();
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
        self.skip_spaces();
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
            // m:nor selects ordinary text: its face comes from w:rPr,
            // whereas m:sty applies to mathematical characters.
            run_properties: self.normal.then(|| {
                let style = self.style.unwrap_or(MathStyle::Plain);
                Box::new(RunProperties {
                    bold: if matches!(style, MathStyle::Bold | MathStyle::BoldItalic) {
                        TriState::On
                    } else {
                        TriState::Off
                    },
                    italic: if matches!(style, MathStyle::Italic | MathStyle::BoldItalic) {
                        TriState::On
                    } else {
                        TriState::Off
                    },
                    ..RunProperties::default()
                })
            }),
            text: text.to_owned(),
            location: SourceLocation::default(),
        })
    }

    fn parse_atom(&mut self) -> Result<Vec<MathNode>, LatexError> {
        if self.ctx.depth >= self.ctx.max_depth {
            return Err(LatexError::LimitExceeded {
                limit: "nesting",
                observed: self.ctx.depth + 1,
            });
        }
        self.ctx.depth += 1;
        let result = self.parse_atom_inner();
        self.ctx.depth -= 1;
        result
    }

    fn parse_atom_inner(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let Some(token) = self.advance_atom() else {
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
        let word = word.strip_prefix("__native:").unwrap_or(word);
        match word {
            "texorpdfstring" => {
                let first = self.parse_argument()?.nodes;
                self.skip_spaces();
                self.raw_group();
                self.ctx.info(span, "math:pdf-string-alternate");
                Ok(first)
            }
            "@firstofone" | "@firstoftwo" | "@secondoftwo" | "@gobble" | "@gobbletwo"
            | "bibinfo" | "bibfield" => {
                let count = if matches!(word, "@firstofone" | "@gobble") {
                    1
                } else {
                    2
                };
                let selected = match word {
                    "@gobble" | "@gobbletwo" => None,
                    "@secondoftwo" | "bibinfo" | "bibfield" => Some(1),
                    _ => Some(0),
                };
                let mut chosen = Vec::new();
                for index in 0..count {
                    self.skip_spaces();
                    if selected == Some(index) {
                        chosen = self.parse_argument()?.nodes;
                    } else if matches!(self.peek(), Some(TokenKind::LBrace)) {
                        self.raw_group();
                    } else {
                        self.advance_atom();
                    }
                }
                Ok(chosen)
            }
            "xspace" | "protect" => Ok(Vec::new()),
            "textunderscore" => Ok(vec![normal_run("_")]),
            "frac" | "dfrac" | "tfrac" | "cfrac" => self.fraction().map(|node| vec![node]),
            "sqrt" => self.radical().map(|node| vec![node]),
            "left" => self.delimiters().map(|node| vec![node]),
            "begin" => self.math_environment(),
            "label" => {
                self.skip_spaces();
                let key = self.raw_group();
                if !key.is_empty() {
                    self.ctx.info(span, &format!("label:{key}"));
                    self.ctx.labels.push(key);
                }
                Ok(Vec::new())
            }
            "nonumber" | "notag" => {
                self.ctx.info(span, &format!("math.numbering:{word}"));
                Ok(Vec::new())
            }
            "hspace" | "hspace*" | "vspace" | "vspace*" | "kern" | "mkern" | "mskip" => {
                // Unsupported spacing is still a loss, but its dimension is
                // metadata, not mathematical content.
                self.ctx.loss(span, &format!("math.spacing:{word}"))?;
                self.skip_spaces();
                if matches!(self.peek(), Some(TokenKind::LBrace)) {
                    let _ = self.raw_group();
                } else {
                    self.skip_dimension();
                }
                Ok(Vec::new())
            }
            "text" | "textrm" | "textnormal" | "mbox" | "operatorname" => self.text_argument(),
            "textbf" | "textit" | "textup" | "emph" => {
                self.text_argument_styled(text_style(MathStyle::Plain, word))
            }
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

    fn skip_dimension(&mut self) {
        // TeX dimensions without braces: optional sign, decimal magnitude,
        // then a unit. Leave adjacent following mathematical atoms intact.
        while matches!(self.peek(), Some(TokenKind::Text(_))) {
            let Some(Token {
                kind: TokenKind::Text(text),
                ..
            }) = self.tokens.get(self.pos)
            else {
                break;
            };
            let rest = &text[self.text_offset..];
            let length = rest
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
                .map(|(i, c)| i + c.len_utf8())
                .last()
                .unwrap_or(0);
            if length == 0 {
                break;
            }
            for _ in 0..rest[..length].chars().count() {
                self.advance_atom();
            }
        }
        self.skip_spaces();
        if let Some(Token {
            kind: TokenKind::Text(text),
            ..
        }) = self.tokens.get(self.pos)
        {
            let rest = &text[self.text_offset..];
            for unit in [
                "pt", "pc", "in", "bp", "cm", "mm", "dd", "cc", "sp", "em", "ex", "mu",
            ] {
                if rest.starts_with(unit) {
                    self.advance_atom();
                    self.advance_atom();
                    break;
                }
            }
        }
    }

    fn radical(&mut self) -> Result<MathNode, LatexError> {
        self.skip_spaces();
        let degree = if matches!(self.peek(), Some(TokenKind::LBracket)) {
            let open = self.advance().expect("opening bracket");
            let mut brackets = 1usize;
            let mut braces = 0usize;
            let mut tokens = Vec::new();
            while let Some(token) = self.advance() {
                match token.kind {
                    TokenKind::LBrace => braces += 1,
                    TokenKind::RBrace => braces = braces.saturating_sub(1),
                    TokenKind::LBracket if braces == 0 => brackets += 1,
                    TokenKind::RBracket if braces == 0 => {
                        brackets -= 1;
                        if brackets == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                tokens.push(token);
            }
            if brackets != 0 {
                self.ctx
                    .loss(span(&open), "math.radical:unterminated-degree")?;
            }
            let mut degree = MathParser {
                tokens: &tokens,
                pos: 0,
                text_offset: 0,
                ctx: self.ctx,
                style: self.style,
                normal: self.normal,
            };
            argument(degree.parse_sequence()?)
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
            append_nodes(&mut nodes, self.apply_scripts(atom)?);
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
        let token = self.advance_atom()?;
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
        self.text_argument_styled(MathStyle::Plain)
    }

    fn text_argument_styled(&mut self, style: MathStyle) -> Result<Vec<MathNode>, LatexError> {
        self.skip_spaces();
        let tokens = if matches!(self.peek(), Some(TokenKind::LBrace)) {
            self.read_group_tokens()
        } else {
            self.advance_atom().into_iter().collect()
        };
        if self.ctx.depth >= self.ctx.max_depth {
            return Err(LatexError::LimitExceeded {
                limit: "nesting",
                observed: self.ctx.depth + 1,
            });
        }
        self.ctx.depth += 1;
        let result = MathParser {
            tokens: &tokens,
            pos: 0,
            text_offset: 0,
            ctx: self.ctx,
            normal: true,
            style: Some(style),
        }
        .text_sequence();
        self.ctx.depth -= 1;
        result
    }

    /// Text mode has its own grouping, whitespace and escapes. Never append a
    /// control word's raw spelling to an OMML run.
    fn text_sequence(&mut self) -> Result<Vec<MathNode>, LatexError> {
        let mut nodes = Vec::new();
        let mut styles = Vec::new();
        while let Some(token) = self.advance() {
            let token_span = span(&token);
            let incoming = match token.kind {
                TokenKind::LBrace => {
                    if styles.len() + self.ctx.depth >= self.ctx.max_depth {
                        return Err(LatexError::LimitExceeded {
                            limit: "nesting",
                            observed: styles.len() + self.ctx.depth + 1,
                        });
                    }
                    styles.push(self.style);
                    Vec::new()
                }
                TokenKind::RBrace => {
                    if let Some(style) = styles.pop() {
                        self.style = style;
                    } else {
                        self.ctx.loss(token_span, "math.text:unmatched-group")?;
                    }
                    Vec::new()
                }
                TokenKind::MathShift => {
                    let start = self.pos;
                    while !matches!(self.peek(), None | Some(TokenKind::MathShift)) {
                        self.advance();
                    }
                    let end = self.pos;
                    if self.advance().is_none() {
                        self.ctx.loss(token_span, "math.text:unterminated-math")?;
                    }
                    translate(&self.tokens[start..end], self.ctx)?.nodes
                }
                TokenKind::ControlWord(word) => {
                    let word = word.strip_prefix("__native:").unwrap_or(&word);
                    while matches!(self.peek(), Some(TokenKind::Space)) {
                        self.advance();
                    }
                    match word {
                        "textbf" | "textit" | "textup" | "textnormal" | "textrm" | "text"
                        | "mbox" | "emph" => {
                            let style = text_style(self.style.unwrap_or(MathStyle::Plain), word);
                            self.text_argument_styled(style)?
                        }
                        "bfseries" | "itshape" | "upshape" | "mdseries" | "normalfont" | "em" => {
                            self.style =
                                Some(text_style(self.style.unwrap_or(MathStyle::Plain), word));
                            Vec::new()
                        }
                        "ensuremath" => {
                            self.skip_spaces();
                            let tokens = if matches!(self.peek(), Some(TokenKind::LBrace)) {
                                self.read_group_tokens()
                            } else {
                                self.advance_atom().into_iter().collect()
                            };
                            translate(&tokens, self.ctx)?.nodes
                        }
                        "protect" | "relax" => Vec::new(),
                        _ => {
                            if let Some(text) = crate::parser::text_symbol(word) {
                                vec![self.styled_run(text)]
                            } else {
                                self.ctx
                                    .loss(token_span, &format!("math.text.command:{word}"))?;
                                Vec::new()
                            }
                        }
                    }
                }
                TokenKind::ControlSymbol(c) => {
                    let text = match c {
                        '{' | '}' | '%' | '$' | '#' | '&' | '_' | ' ' => Some(c.to_string()),
                        _ => None,
                    };
                    if let Some(text) = text {
                        vec![self.styled_run(&text)]
                    } else {
                        self.ctx
                            .loss(token_span, &format!("math.text.symbol:{c}"))?;
                        Vec::new()
                    }
                }
                TokenKind::Space | TokenKind::ParBreak => vec![self.styled_run(" ")],
                TokenKind::Tilde => vec![self.styled_run("\u{00A0}")],
                kind => vec![self.styled_run(&kind.raw())],
            };
            append_nodes(&mut nodes, incoming);
        }
        if !styles.is_empty() {
            self.ctx
                .loss(LatexSpan::new(0, 0), "math.text:unterminated-group")?;
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
        let mut environments = vec![name.clone()];
        let mut captured = Vec::new();
        while let Some(token) = self.advance() {
            if let TokenKind::ControlWord(word) = &token.kind {
                if word == "begin" || word == "end" {
                    self.skip_spaces();
                    let start = self.pos;
                    let nested = self.raw_group();
                    if word == "begin" {
                        if environments.len() + self.ctx.depth >= self.ctx.max_depth {
                            return Err(LatexError::LimitExceeded {
                                limit: "nesting",
                                observed: environments.len() + self.ctx.depth + 1,
                            });
                        }
                        environments.push(nested);
                    } else if environments.last() == Some(&nested) {
                        environments.pop();
                        if environments.is_empty() {
                            break;
                        }
                    } else {
                        self.ctx
                            .loss(span(&token), "math.environment:mismatched-end")?;
                    }
                    captured.push(token);
                    captured.extend_from_slice(&self.tokens[start..self.pos]);
                    continue;
                }
            }
            captured.push(token);
        }
        if !environments.is_empty() {
            self.ctx.loss(
                captured.first().map_or(LatexSpan::new(0, 0), span),
                "math.environment:unterminated",
            )?;
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
            "longrightarrow" => "\u{27F6}",
            "longleftarrow" => "\u{27F5}",
            "longleftrightarrow" => "\u{27F7}",
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
            "mid" => "\u{2223}",
            "top" => "\u{22A4}",
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
