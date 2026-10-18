//! LaTeX source to the Strict WordprocessingML model.
//!
//! The parser is a bounded recursive-descent walk over the token stream. It
//! never panics on hostile input: every failure is a typed [`LatexError`] and
//! every approximation is a named diagnostic in the [`LatexReport`].
//!
//! [`LatexReport`]: crate::LatexReport

use std::collections::BTreeMap;
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::drawing::MediaIndex;
use strict_ooxml_wml::model::math::MathJustification;
use strict_ooxml_wml::model::{
    AbstractNum, AbstractNumId, Block, Body, Bookmark, BreakKind, Color, Columns, Document,
    DocumentSource, Field, Fonts, HalfPoints, HeaderFooter, HeaderFooterKind, HeaderFooterRef,
    Hyperlink, Ilvl, Indentation, Inline, Justification, Level, MathExpression, MathParagraph,
    MathParagraphProperties, Note, NoteKind, NoteProperties, NoteTable, Num, NumId, NumPr,
    NumberingTable, PageMargins, PageOrientation, PageSize, Paragraph, ParagraphProperties, Run,
    RunContent, RunProperties, Section, SectionProperties, Settings, Shading, Space, Spacing,
    Style, StyleId, StyleTable, StyleType, TextNode, TriState, Twips, Underline, VertAlign,
};

use crate::lexer::{lex, Token, TokenKind};
use crate::math::{translate_environment, MathContext};
use crate::media;
use crate::report::{LatexSpan, ReportBuilder, Severity};
use crate::source::SourceProvider;
use crate::{LatexError, LatexImported, LatexOptions, UnsupportedPolicy};

/// Why an inline run stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InlineEnd {
    Group,
    Paragraph,
    Block,
    Cell,
    Row,
    Eof,
}

/// The context an inline run stops in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Group,
    BodyGroup,
    Block,
    Cell,
}

/// Character formatting in effect while inline content is collected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Format {
    bold: bool,
    italic: bool,
    underline: bool,
    mono: bool,
    sans: bool,
    small_caps: bool,
    vert: Option<VertAlign>,
    size: Option<i32>,
    color: Option<Arc<str>>,
    shading: Option<Arc<str>>,
}

impl Format {
    fn to_props(&self) -> RunProperties {
        let mut props = RunProperties::default();
        if self.bold {
            props.bold = TriState::On;
        }
        if self.italic {
            props.italic = TriState::On;
        }
        if self.underline {
            props.underline = Some(Underline::Single);
        }
        if self.small_caps {
            props.small_caps = TriState::On;
        }
        if let Some(vert) = self.vert {
            props.vert_align = Some(vert);
        }
        if let Some(size) = self.size {
            props.size = Some(HalfPoints(size));
            props.size_cs = Some(HalfPoints(size));
        }
        if let Some(color) = &self.color {
            props.color = Some(Color::new(color.clone()));
        }
        if let Some(fill) = &self.shading {
            props.shading = Some(Shading {
                pattern: Some(Arc::from("clear")),
                fill: Some(Color::new(fill.clone())),
                ..Shading::default()
            });
        }
        if self.mono || self.sans {
            let name: Arc<str> = Arc::from(if self.mono { "Consolas" } else { "Calibri" });
            props.fonts = Some(Fonts {
                ascii: Some(name.clone()),
                h_ansi: Some(name),
                ..Fonts::default()
            });
        }
        props
    }
}

struct Frame {
    file_id: usize,
    include_name: Option<String>,
    tokens: Vec<Token>,
    pos: usize,
}

/// Restorable local assignments. Definitions marked global are propagated by
/// `Macros` into its saved scopes and therefore survive restoration.
struct LocalScope {
    explicit: bool,
    format: Format,
    alignment: Option<Justification>,
    colors: BTreeMap<String, String>,
    graphics_paths: Vec<String>,
    column_types: BTreeMap<String, crate::macros::Definition>,
    environments: BTreeMap<String, (crate::macros::Definition, Vec<Token>)>,
}

impl Frame {
    fn done(&self) -> bool {
        self.pos >= self.tokens.len()
    }
}

struct Parser<'a> {
    options: &'a LatexOptions,
    builder: ReportBuilder,
    provider: &'a dyn SourceProvider,
    frames: Vec<Frame>,
    file_names: Vec<String>,
    blocks: Vec<Block>,
    inlines: Vec<Inline>,
    block_format: Format,
    local_scopes: Vec<LocalScope>,
    alignment: Option<Justification>,
    pending_indent: Option<Indentation>,
    pending_outline: Option<u8>,
    footnotes: NoteTable,
    footnote_counter: i32,
    document_footnote: NoteProperties,
    last_footnote_mark: Option<i32>,
    endnotes: NoteTable,
    endnote_counter: i32,
    document_endnote: NoteProperties,
    bookmark_counter: u32,
    media: Vec<(PartId, Vec<u8>)>,
    media_index: MediaIndex,
    media_bytes: usize,
    sections: Vec<Section>,
    title: Vec<Inline>,
    author: Vec<Inline>,
    date: Vec<Inline>,
    image_counter: usize,
    defined_colors: BTreeMap<String, String>,
    graphics_paths: Vec<String>,
    numbering: NumberingTable,
    num_counter: u32,
    bullet_abstract: AbstractNumId,
    decimal_abstract: AbstractNumId,
    numbering_ready: bool,
    styles: StyleTable,
    section_counters: [u32; 5],
    columns: Option<u16>,
    table_rows: usize,
    table_cells: usize,
    list_depth: usize,
    nesting: usize,
    in_document: bool,
    token_count: usize,
    text_scalars: usize,
    node_count: usize,
    included_files: usize,
    macros: crate::macros::Macros,
    expanding: bool,
    expanded_tokens: usize,
    labels: BTreeMap<String, crate::references::Value>,
    reference_context: crate::references::Value,
    reference_requests: Vec<(String, String, LatexSpan, String)>,
    equation_counter: usize,
    figure_counter: usize,
    table_counter: usize,
    float_kind: Option<&'static str>,
    bibliography_counter: usize,
    bibliography_loaded: bool,
    loaded_packages: std::collections::BTreeSet<String>,
    document_class: String,
    render_preamble: String,
    column_types: BTreeMap<String, crate::macros::Definition>,
    environments: BTreeMap<String, (crate::macros::Definition, Vec<Token>)>,
    affiliations: Vec<Vec<Inline>>,
    frontmatter_title_emitted: bool,
    affiliation_numbers: BTreeMap<String, usize>,
    addressed_affiliations: std::collections::BTreeSet<String>,
    frontmatter_contacts: Vec<Vec<Inline>>,
    frontmatter_note_ids: BTreeMap<String, i32>,
}

fn starts_with_document_class(tokens: &[Token]) -> bool {
    let mut depth = 0usize;
    for token in tokens {
        match &token.kind {
            TokenKind::LBrace => depth += 1,
            TokenKind::RBrace => depth = depth.saturating_sub(1),
            TokenKind::ControlWord(word)
                if depth == 0 && matches!(word.as_str(), "documentclass" | "documentstyle") =>
            {
                return true
            }
            _ => {}
        }
    }
    false
}

fn is_block_starter(word: &str) -> bool {
    matches!(
        word.strip_prefix("__native:").unwrap_or(word),
        "section"
            | "subsection"
            | "subsubsection"
            | "paragraph"
            | "subparagraph"
            | "chapter"
            | "part"
            | "item"
            | "begin"
            | "end"
            | "par"
            | "newpage"
            | "clearpage"
            | "pagebreak"
            | "input"
            | "include"
            | "maketitle"
            | "centering"
            | "raggedright"
            | "raggedleft"
            | "tableofcontents"
            | "bibliography"
            | "bibliographystyle"
            | "caption"
            | "title"
            | "author"
            | "date"
            | "documentclass"
            | "documentstyle"
            | "usepackage"
            | "RequirePackage"
            | "newcommand"
            | "renewcommand"
            | "providecommand"
            | "address"
            | "affiliation"
            | "endinput"
            | "newcolumntype"
    )
}

fn heading_level(word: &str) -> Option<u8> {
    match word {
        "part" | "chapter" | "section" => Some(0),
        "subsection" => Some(1),
        "subsubsection" => Some(2),
        "paragraph" => Some(3),
        "subparagraph" => Some(4),
        _ => None,
    }
}

fn is_definition(word: &str) -> bool {
    matches!(
        word.trim_end_matches('*'),
        "def"
            | "gdef"
            | "edef"
            | "xdef"
            | "newcommand"
            | "renewcommand"
            | "providecommand"
            | "DeclareRobustCommand"
            | "DeclareMathOperator"
            | "newenvironment"
            | "renewenvironment"
    )
}

fn heading_size(level: u8) -> i32 {
    match level {
        0 => 36,
        1 => 32,
        2 => 28,
        3 => 26,
        _ => 24,
    }
}

impl<'a> Parser<'a> {
    fn new(
        entry: &str,
        bytes: &[u8],
        provider: &'a dyn SourceProvider,
        options: &'a LatexOptions,
    ) -> Result<Self, LatexError> {
        let text = decode(bytes, entry)?;
        let tokens = lex(&text);
        let in_document = !starts_with_document_class(&tokens);
        let render_preamble = graphic_preamble(&tokens);
        Ok(Self {
            options,
            builder: ReportBuilder::new(options.limits.issues),
            provider,
            frames: vec![Frame {
                file_id: 0,
                include_name: None,
                tokens,
                pos: 0,
            }],
            file_names: vec![entry.to_owned()],
            blocks: Vec::new(),
            inlines: Vec::new(),
            block_format: Format::default(),
            local_scopes: Vec::new(),
            alignment: None,
            pending_indent: None,
            pending_outline: None,
            footnotes: NoteTable::new(),
            footnote_counter: 0,
            document_footnote: NoteProperties::default(),
            last_footnote_mark: None,
            endnotes: NoteTable::new(),
            endnote_counter: 0,
            document_endnote: NoteProperties::default(),
            bookmark_counter: 0,
            media: Vec::new(),
            media_index: MediaIndex::new(),
            media_bytes: 0,
            sections: Vec::new(),
            title: Vec::new(),
            author: Vec::new(),
            date: Vec::new(),
            image_counter: 0,
            defined_colors: BTreeMap::new(),
            graphics_paths: Vec::new(),
            numbering: NumberingTable::new(),
            num_counter: 0,
            bullet_abstract: AbstractNumId(0),
            decimal_abstract: AbstractNumId(1),
            numbering_ready: false,
            styles: build_styles(),
            section_counters: [0; 5],
            columns: None,
            table_rows: 0,
            table_cells: 0,
            list_depth: 0,
            nesting: 0,
            in_document,
            token_count: 0,
            text_scalars: 0,
            node_count: 0,
            included_files: 0,
            macros: crate::macros::Macros::default(),
            expanding: true,
            expanded_tokens: 0,
            labels: BTreeMap::new(),
            reference_context: Default::default(),
            reference_requests: Vec::new(),
            equation_counter: 0,
            figure_counter: 0,
            table_counter: 0,
            float_kind: None,
            bibliography_counter: 0,
            bibliography_loaded: false,
            loaded_packages: Default::default(),
            document_class: String::new(),
            render_preamble,
            column_types: BTreeMap::new(),
            environments: BTreeMap::new(),
            affiliations: Vec::new(),
            frontmatter_title_emitted: false,
            affiliation_numbers: BTreeMap::new(),
            addressed_affiliations: Default::default(),
            frontmatter_contacts: Vec::new(),
            frontmatter_note_ids: BTreeMap::new(),
        })
    }

    fn current_file(&self) -> &str {
        let id = self.frames.last().map_or(0, |frame| frame.file_id);
        self.file_names.get(id).map_or("<latex>", String::as_str)
    }

    fn peek(&self) -> Option<&Token> {
        for frame in self.frames.iter().rev() {
            if !frame.done() {
                return frame.tokens.get(frame.pos);
            }
        }
        None
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|token| &token.kind)
    }

    fn span_here(&self) -> LatexSpan {
        self.peek().map_or(LatexSpan::new(0, 0), |token| {
            LatexSpan::new(token.start, token.end)
        })
    }

    fn raw_token(&mut self) -> Result<Option<Token>, LatexError> {
        loop {
            let popped = match self.frames.last() {
                Some(frame) => frame.done(),
                None => return Ok(None),
            };
            if popped {
                self.frames.pop();
                if self.frames.is_empty() {
                    return Ok(None);
                }
                continue;
            }
            let frame = self.frames.last_mut().expect("frame is present");
            let token = frame.tokens.get(frame.pos).cloned();
            frame.pos += 1;
            self.token_count += 1;
            if self.token_count > self.options.limits.tokens {
                return Err(LatexError::LimitExceeded {
                    limit: "tokens",
                    observed: self.token_count,
                });
            }
            return Ok(token);
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, LatexError> {
        if self.expanding {
            self.expand_head()?;
        }
        self.raw_token()
    }

    fn expand_head(&mut self) -> Result<(), LatexError> {
        loop {
            while self.frames.last().is_some_and(Frame::done) {
                self.frames.pop();
            }
            let name = match self.peek_kind() {
                Some(TokenKind::ControlWord(name)) => name.clone(),
                Some(TokenKind::ControlSymbol(symbol)) => symbol.to_string(),
                _ => return Ok(()),
            };
            if name == "iftrue"
                || name == "iffalse"
                || self.macros.booleans.contains_key(&name)
                || primitive_conditional(&name)
            {
                let choice = if name == "iftrue" {
                    Some(true)
                } else if name == "iffalse" {
                    Some(false)
                } else {
                    self.macros.booleans.get(&name).copied()
                };
                let origin = self.raw_token()?.expect("conditional");
                self.conditional_branch(choice, LatexSpan::new(origin.start, origin.end))?;
                continue;
            }
            if let Some((key, value)) = self.macros.booleans.keys().find_map(|key| {
                let base = key.strip_prefix("if")?;
                if name == format!("{base}true") {
                    Some((key.clone(), true))
                } else if name == format!("{base}false") {
                    Some((key.clone(), false))
                } else {
                    None
                }
            }) {
                self.raw_token()?;
                self.macros.booleans.insert(key, value);
                continue;
            }
            let Some(definition) = self.macros.definitions.get(&name).cloned() else {
                return Ok(());
            };
            if self.frames.len() > self.options.limits.nesting + self.options.limits.depth {
                return Err(LatexError::LimitExceeded {
                    limit: "macro_expansion_depth",
                    observed: self.frames.len(),
                });
            }
            let origin = self.raw_token()?.expect("peeked macro");
            let old = self.expanding;
            self.expanding = false;
            let result = (|| {
                let mut args = Vec::new();
                if let Some(default) = &definition.default {
                    let argument = self.optional_bracket_text()?;
                    args.push(
                        argument.map_or_else(|| default.clone(), |s| crate::macros::tokens(&s)),
                    );
                }
                while args.len() < definition.args {
                    args.push(self.macro_argument()?);
                }
                crate::macros::substitute(
                    &definition.body,
                    &args,
                    self.options
                        .limits
                        .tokens
                        .saturating_sub(self.expanded_tokens),
                )
            })();
            self.expanding = old;
            let tokens = result?;
            self.expanded_tokens = self
                .expanded_tokens
                .saturating_add(tokens.len())
                .saturating_add(1);
            if self.expanded_tokens > self.options.limits.tokens {
                return Err(LatexError::LimitExceeded {
                    limit: "macro_expansion_tokens",
                    observed: self.expanded_tokens,
                });
            }
            self.info(
                LatexSpan::new(origin.start, origin.end),
                &format!("macro:expanded:{name}"),
            );
            let file_id = self.frames.last().map_or(0, |f| f.file_id);
            self.frames.push(Frame {
                file_id,
                include_name: None,
                tokens,
                pos: 0,
            });
        }
    }

    fn conditional_branch(
        &mut self,
        choice: Option<bool>,
        span: LatexSpan,
    ) -> Result<(), LatexError> {
        if choice.is_none() {
            self.loss(span, "conditional:requires-tex")?;
        }
        let mut selected = Vec::new();
        let mut first = true;
        let mut depth = 0usize;
        let mut closed = false;
        let mut declaration = false;
        while let Some(token) = self.raw_token()? {
            if let TokenKind::ControlWord(word) = &token.kind {
                if declaration {
                    declaration = false;
                } else if word == "newif" {
                    declaration = true;
                } else if word == "iftrue"
                    || word == "iffalse"
                    || primitive_conditional(word)
                    || self.macros.booleans.contains_key(word)
                {
                    depth += 1;
                } else if word == "fi" {
                    if depth == 0 {
                        closed = true;
                        break;
                    }
                    depth -= 1;
                } else if word == "else" && depth == 0 {
                    first = false;
                    continue;
                }
            }
            if choice == Some(first) {
                selected.push(token);
            }
        }
        if !closed {
            self.loss(span, "conditional:unterminated")?;
        }
        let file_id = self.frames.last().map_or(0, |f| f.file_id);
        self.frames.push(Frame {
            file_id,
            include_name: None,
            tokens: selected,
            pos: 0,
        });
        Ok(())
    }

    fn new_if(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        if let Some(Token {
            kind: TokenKind::ControlWord(name),
            ..
        }) = self.raw_token()?
        {
            if name.starts_with("if") {
                self.macros.booleans.insert(name, false);
                return Ok(());
            }
        }
        self.loss(span, "conditional:invalid-definition")
    }

    fn let_binding(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        let old = self.expanding;
        self.expanding = false;
        let result = (|| {
            self.skip_spaces()?;
            let Some(Token {
                kind: TokenKind::ControlWord(name),
                ..
            }) = self.raw_token()?
            else {
                return self.loss(span, "macro:let-name");
            };
            self.skip_spaces()?;
            if matches!(self.peek_kind(), Some(TokenKind::Text(s)) if s.starts_with('=')) {
                self.macro_argument()?;
            }
            self.skip_spaces()?;
            let Some(token) = self.raw_token()? else {
                return self.loss(span, "macro:let-value");
            };
            let def = if let TokenKind::ControlWord(target) = &token.kind {
                self.macros.definitions.get(target).map_or_else(
                    || crate::macros::Definition {
                        args: 0,
                        default: None,
                        body: vec![Token {
                            kind: TokenKind::ControlWord(format!("__native:{target}")),
                            ..token.clone()
                        }],
                    },
                    |d| (**d).clone(),
                )
            } else {
                crate::macros::Definition {
                    args: 0,
                    default: None,
                    body: vec![token],
                }
            };
            self.macros.define(name, def, false);
            Ok(())
        })();
        self.expanding = old;
        result
    }

    fn macro_argument(&mut self) -> Result<Vec<Token>, LatexError> {
        self.skip_spaces()?;
        if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.raw_token()?;
            let mut depth = 1;
            let mut tokens = Vec::new();
            while let Some(token) = self.raw_token()? {
                match token.kind {
                    TokenKind::LBrace => depth += 1,
                    TokenKind::RBrace => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(tokens);
                        }
                    }
                    _ => {}
                }
                tokens.push(token);
            }
            self.loss(self.span_here(), "macro:unterminated-argument")?;
            return Ok(tokens);
        }
        let Some(mut token) = self.raw_token()? else {
            self.loss(self.span_here(), "macro:missing-argument")?;
            return Ok(Vec::new());
        };
        if let TokenKind::Text(text) = &mut token.kind {
            if let Some(first) = text.chars().next() {
                let remainder = text[first.len_utf8()..].to_owned();
                *text = first.to_string();
                if !remainder.is_empty() {
                    let file_id = self.frames.last().map_or(0, |f| f.file_id);
                    self.frames.push(Frame {
                        file_id,
                        include_name: None,
                        tokens: vec![Token {
                            kind: TokenKind::Text(remainder),
                            ..token.clone()
                        }],
                        pos: 0,
                    });
                }
            }
        }
        Ok(vec![token])
    }
    fn loss(&mut self, span: LatexSpan, feature: &str) -> Result<(), LatexError> {
        let file = self.current_file().to_owned();
        self.builder.record(&file, span, Severity::Loss, feature);
        if self.options.unsupported_policy == UnsupportedPolicy::Reject {
            return Err(LatexError::UnsupportedRejected {
                feature: feature.to_owned(),
            });
        }
        Ok(())
    }

    fn info(&mut self, span: LatexSpan, feature: &str) {
        let file = self.current_file().to_owned();
        self.builder.record(&file, span, Severity::Info, feature);
    }

    fn math_context(&mut self) -> MathContext<'_> {
        let file = self.current_file().to_owned();
        MathContext {
            builder: &mut self.builder,
            file,
            reject: self.options.unsupported_policy == UnsupportedPolicy::Reject,
            depth: 0,
            max_depth: self.options.limits.nesting,
            max_columns: self.options.limits.table_columns,
            labels: Vec::new(),
        }
    }

    fn run(&mut self) -> Result<(), LatexError> {
        self.read_aux()?;
        self.parse_blocks(None)?;
        self.flush_paragraph()?;
        let mut fmt = self.block_format.clone();
        self.close_local_scopes(0, &mut fmt)?;
        for (name, kind, span, file) in self.reference_requests.clone() {
            let resolved = self.labels.get(&name).is_some_and(|v| match kind.as_str() {
                "pageref" => !v.page.is_empty(),
                "nameref" => !v.title.is_empty(),
                _ => !v.number.is_empty(),
            });
            if !resolved {
                self.builder.record(
                    &file,
                    span,
                    Severity::Loss,
                    &format!("reference:unresolved:{kind}:{name}"),
                );
                if self.options.unsupported_policy == UnsupportedPolicy::Reject {
                    return Err(LatexError::UnsupportedRejected {
                        feature: "reference:unresolved".to_owned(),
                    });
                }
            }
        }
        crate::references::resolve(&mut self.blocks, &self.labels);
        for table in [&mut self.footnotes, &mut self.endnotes] {
            let ids: Vec<_> = table.iter().map(|n| n.id).collect();
            for id in ids {
                if let Some(mut note) = table.remove(id) {
                    crate::references::resolve(&mut note.blocks, &self.labels);
                    table.insert(note);
                }
            }
        }
        Ok(())
    }

    // ----- paragraph and node accounting -------------------------------

    fn push_text(&mut self, fmt: &Format, text: &str) -> Result<(), LatexError> {
        if text.is_empty() {
            return Ok(());
        }
        self.text_scalars += text.chars().count();
        if self.text_scalars > self.options.limits.text_scalars {
            return Err(LatexError::LimitExceeded {
                limit: "text_scalars",
                observed: self.text_scalars,
            });
        }
        let props = fmt.to_props();
        if let Some(Inline::Run(run)) = self.inlines.last_mut() {
            if run.props == props {
                if let Some(RunContent::Text(node)) = run.content.last_mut() {
                    node.text.push_str(text);
                    return Ok(());
                }
            }
        }
        self.inlines.push(Inline::Run(Run {
            props,
            content: vec![RunContent::Text(TextNode {
                text: text.to_owned(),
                space: Space::Preserve,
            })],
            revision: None,
            location: SourceLocation::default(),
        }));
        Ok(())
    }

    fn push_space(&mut self, fmt: &Format) -> Result<(), LatexError> {
        let trailing = matches!(
            self.inlines.last(),
            Some(Inline::Run(run)) if matches!(run.content.last(), Some(RunContent::Text(node)) if node.text.ends_with(' ') || node.text.ends_with('\u{00A0}'))
        );
        if self.inlines.is_empty() || trailing {
            return Ok(());
        }
        self.push_text(fmt, " ")
    }

    fn check_nodes(&mut self, added: usize) -> Result<(), LatexError> {
        self.node_count += added;
        if self.node_count > self.options.limits.nodes {
            return Err(LatexError::LimitExceeded {
                limit: "nodes",
                observed: self.node_count,
            });
        }
        Ok(())
    }

    /// Raises the structural nesting depth, refusing a descent that would
    /// overflow the stack. Groups, environments, lists and tables each recurse,
    /// so an adversarial `{{{{…}}}}` or `\begin{center}…` a hundred thousand deep
    /// is a stack overflow and an aborted process without this bound.
    fn enter_nesting(&mut self) -> Result<(), LatexError> {
        if self.nesting >= self.options.limits.nesting {
            return Err(LatexError::LimitExceeded {
                limit: "nesting",
                observed: self.nesting + 1,
            });
        }
        self.nesting += 1;
        Ok(())
    }

    fn leave_nesting(&mut self) {
        self.nesting = self.nesting.saturating_sub(1);
    }

    fn take_paragraph_props(&mut self) -> ParagraphProperties {
        ParagraphProperties {
            alignment: self.alignment,
            indentation: self.pending_indent.take(),
            outline_level: self.pending_outline.take(),
            ..ParagraphProperties::default()
        }
    }

    fn push_paragraph(
        &mut self,
        props: ParagraphProperties,
        inlines: Vec<Inline>,
    ) -> Result<(), LatexError> {
        self.check_nodes(1 + inlines.len())?;
        self.blocks.push(Block::Paragraph(Paragraph {
            props,
            inlines,
            rsids: Default::default(),
            revision: None,
            para_id: None,
            text_id: None,
            location: SourceLocation::default(),
        }));
        Ok(())
    }

    fn push_block(&mut self, block: Block) -> Result<(), LatexError> {
        self.check_nodes(1)?;
        self.blocks.push(block);
        Ok(())
    }

    fn flush_paragraph(&mut self) -> Result<(), LatexError> {
        self.trim_trailing_space();
        let inlines = std::mem::take(&mut self.inlines);
        if inlines.is_empty() {
            self.pending_indent = None;
            self.pending_outline = None;
            return Ok(());
        }
        let props = self.take_paragraph_props();
        self.push_paragraph(props, inlines)
    }

    fn trim_trailing_space(&mut self) {
        loop {
            let Some(Inline::Run(run)) = self.inlines.last_mut() else {
                break;
            };
            if let Some(RunContent::Text(node)) = run.content.last_mut() {
                let trimmed = node.text.trim_end_matches([' ', '\u{00A0}']);
                let keep = trimmed.len();
                node.text.truncate(keep);
            }
            let all_empty = run
                .content
                .iter()
                .all(|content| matches!(content, RunContent::Text(node) if node.text.is_empty()));
            if all_empty {
                self.inlines.pop();
                continue;
            }
            break;
        }
    }

    // ----- token helpers ------------------------------------------------

    fn skip_spaces(&mut self) -> Result<(), LatexError> {
        while matches!(self.peek_kind(), Some(TokenKind::Space)) {
            self.next_token()?;
        }
        Ok(())
    }

    fn skip_balanced(&mut self) -> Result<(), LatexError> {
        let bracket = match self.peek_kind() {
            Some(TokenKind::LBracket) => true,
            Some(TokenKind::LBrace) => false,
            _ => return Ok(()),
        };
        let span = self.span_here();
        self.next_token()?;
        let mut braces = usize::from(!bracket);
        let mut brackets = usize::from(bracket);
        while braces > 0 || brackets > 0 {
            match self.next_token()? {
                None => {
                    self.loss(span, "group:unterminated")?;
                    break;
                }
                Some(Token {
                    kind: TokenKind::LBrace,
                    ..
                }) => braces += 1,
                Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => braces = braces.saturating_sub(1),
                Some(Token {
                    kind: TokenKind::LBracket,
                    ..
                }) if bracket && braces == 0 => brackets += 1,
                Some(Token {
                    kind: TokenKind::RBracket,
                    ..
                }) if bracket && braces == 0 => brackets -= 1,
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// Skips optional `[...]` and required `{...}` argument groups.
    fn skip_args(&mut self) -> Result<(), LatexError> {
        loop {
            self.skip_spaces()?;
            match self.peek_kind() {
                Some(TokenKind::LBracket | TokenKind::LBrace) => self.skip_balanced()?,
                _ => return Ok(()),
            }
        }
    }

    /// Interpret a bounded substitution definition; unsupported TeX primitives
    /// remain explicit losses rather than executing code or printing its body.
    fn skip_definition(&mut self, word: &str, span: LatexSpan) -> Result<(), LatexError> {
        let old = self.expanding;
        self.expanding = false;
        let result = self.read_definition(word.trim_end_matches('*'), span);
        self.expanding = old;
        result
    }

    fn read_definition(&mut self, word: &str, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        let name = if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.consume_braced_raw()?
        } else {
            self.raw_token()?.map_or_else(String::new, |t| t.kind.raw())
        };
        let name = name.trim_start_matches('\\').to_owned();
        if name.is_empty() {
            self.loss(span, "macro:missing-name")?;
            return Ok(());
        }
        let mut args = 0;
        let mut default = None;
        let mut supported = true;
        if matches!(word, "def" | "gdef" | "edef" | "xdef") {
            let mut signature = String::new();
            while !matches!(self.peek_kind(), Some(TokenKind::LBrace) | None) {
                let token = self.raw_token()?.expect("peeked signature");
                if matches!(&token.kind, TokenKind::ControlWord(w) if w == "begin" || w == "end") {
                    self.loss(span, "macro:missing-replacement")?;
                    return Ok(());
                }
                signature.push_str(&token.kind.raw());
            }
            let signature = signature.trim();
            for n in 1..=9 {
                if signature
                    == (1..=n)
                        .flat_map(|i| ['#', char::from(b'0' + i as u8)])
                        .collect::<String>()
                {
                    args = n;
                }
            }
            supported = signature.is_empty() || args > 0;
            // edef/xdef require expansion at definition time, which is not a
            // synonym for ordinary substitution.
            supported &= !matches!(word, "edef" | "xdef");
        } else if let Some(count) = self.optional_bracket_text()? {
            args = count.trim().parse().unwrap_or(10);
            default = self
                .optional_bracket_text()?
                .map(|s| crate::macros::tokens(&s));
            supported &= args <= 9 && (args > 0 || default.is_none());
        }
        self.skip_spaces()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.loss(span, "macro:missing-replacement")?;
            return Ok(());
        }
        let body = self.macro_argument()?;
        if matches!(word, "newenvironment" | "renewenvironment") {
            let ending = self.macro_argument()?;
            if !supported {
                return self.loss(span, "macro:unsupported-environment-definition");
            }
            self.environments.insert(
                name,
                (
                    crate::macros::Definition {
                        args,
                        default,
                        body,
                    },
                    ending,
                ),
            );
            self.info(span, "macro:environment-defined");
            return Ok(());
        }
        if body
            .iter()
            .any(|t| matches!(&t.kind, TokenKind::ControlWord(w) if w == &name))
        {
            self.loss(span, "macro:recursive-definition")?;
            return Ok(());
        }
        if body.iter().any(|t| matches!(&t.kind, TokenKind::ControlWord(w) if primitive_conditional(w) || matches!(w.as_str(), "expandafter" | "csname" | "endcsname" | "futurelet" | "afterassignment" | "aftergroup" | "the" | "numexpr" | "dimexpr" | "romannumeral" | "catcode" | "global" | "advance" | "multiply" | "divide" | "@ifnextchar" | "@ifstar" | "@ifundefined"))) {
            self.loss(span, "macro:requires-tex-definition")?;
            return Ok(());
        }
        if !supported {
            self.loss(span, "macro:unsupported-definition")?;
            return Ok(());
        }
        if word == "providecommand" && self.macros.definitions.contains_key(&name) {
            return Ok(());
        }
        let body = if word == "DeclareMathOperator" {
            let mut tokens = crate::macros::tokens(r"\operatorname{");
            tokens.extend(body);
            tokens.extend(crate::macros::tokens("}"));
            tokens
        } else {
            body
        };
        self.macros.define(
            name,
            crate::macros::Definition {
                args,
                default,
                body,
            },
            word == "gdef",
        );
        self.info(span, "macro:defined");
        Ok(())
    }

    /// Returns the raw source of a `{...}` group, or an empty string.
    fn consume_braced_raw(&mut self) -> Result<String, LatexError> {
        self.skip_spaces()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(String::new());
        }
        self.next_token()?;
        let mut text = String::new();
        let mut depth = 1usize;
        while let Some(token) = self.next_token()? {
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
        Ok(text)
    }

    fn consume_end(&mut self) -> Result<String, LatexError> {
        self.next_token()?;
        self.consume_braced_raw()
    }

    fn optional_bracket_text(&mut self) -> Result<Option<String>, LatexError> {
        self.skip_spaces()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBracket)) {
            return Ok(None);
        }
        self.next_token()?;
        let mut text = String::new();
        let mut depth = 1usize;
        while let Some(token) = self.next_token()? {
            match token.kind {
                TokenKind::LBracket => {
                    depth += 1;
                    text.push('[');
                }
                TokenKind::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    text.push(']');
                }
                kind => text.push_str(&kind.raw()),
            }
        }
        Ok(Some(text))
    }

    fn capture_group_inlines(&mut self) -> Result<Vec<Inline>, LatexError> {
        self.skip_spaces()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(Vec::new());
        }
        self.next_token()?;
        let mut fmt = Format::default();
        let (inlines, _) = self.parse_inlines_isolated(&mut fmt, Stop::Group)?;
        Ok(inlines)
    }

    // ----- environment bodies ------------------------------------------

    fn collect_environment_body(&mut self, env: &str) -> Result<Vec<Token>, LatexError> {
        let mut out = Vec::new();
        let mut depth = 0usize;
        while let Some(token) = self.next_token()? {
            match &token.kind {
                TokenKind::ControlWord(word) if word == "begin" => {
                    let name = self.consume_braced_raw()?;
                    depth += 1;
                    out.push(token);
                    push_synthetic_name(&mut out, &name);
                }
                TokenKind::ControlWord(word) if word == "end" => {
                    let name = self.consume_braced_raw()?;
                    if depth == 0 && name.trim_end_matches('*') == env.trim_end_matches('*') {
                        break;
                    }
                    depth = depth.saturating_sub(1);
                    out.push(token);
                    push_synthetic_name(&mut out, &name);
                }
                _ => out.push(token),
            }
        }
        Ok(out)
    }

    fn with_tokens<R>(
        &mut self,
        tokens: Vec<Token>,
        f: impl FnOnce(&mut Self) -> Result<R, LatexError>,
    ) -> Result<R, LatexError> {
        let file_id = self.frames.last().map_or(0, |frame| frame.file_id);
        let saved = std::mem::replace(
            &mut self.frames,
            vec![Frame {
                file_id,
                include_name: None,
                tokens,
                pos: 0,
            }],
        );
        let scope_depth = self.local_scopes.len();
        let mut fmt = self.block_format.clone();
        self.enter_local_scope(&fmt, false)?;
        let result = f(self);
        let cleanup = self.close_local_scopes(scope_depth, &mut fmt);
        self.block_format = fmt;
        self.frames = saved;
        cleanup?;
        result
    }

    fn parse_tokens_as_blocks(&mut self, tokens: Vec<Token>) -> Result<Vec<Block>, LatexError> {
        let saved = std::mem::take(&mut self.blocks);
        let result = self.with_tokens(tokens, |parser| {
            parser.parse_blocks(None)?;
            parser.flush_paragraph()
        });
        let out = std::mem::replace(&mut self.blocks, saved);
        result?;
        Ok(out)
    }

    fn parse_tokens_as_inlines(
        &mut self,
        tokens: Vec<Token>,
        fmt: &Format,
    ) -> Result<Vec<Inline>, LatexError> {
        let mut fmt = fmt.clone();
        self.with_tokens(tokens, |parser| {
            parser.skip_spaces()?;
            let (inlines, end) = parser.parse_inlines_isolated(&mut fmt, Stop::Block)?;
            if end == InlineEnd::Block {
                parser.info(LatexSpan::new(0, 0), "table:block-content-dropped");
            }
            Ok(inlines)
        })
    }

    // ----- block parsing -------------------------------------------------

    fn parse_blocks(&mut self, stop_env: Option<&str>) -> Result<(), LatexError> {
        self.enter_nesting()?;
        let result = self.parse_blocks_inner(stop_env);
        self.leave_nesting();
        result
    }

    fn parse_blocks_inner(&mut self, stop_env: Option<&str>) -> Result<(), LatexError> {
        loop {
            self.expand_head()?;
            if !self.in_document {
                self.preamble_step()?;
                continue;
            }
            while matches!(self.peek_kind(), Some(TokenKind::ParBreak)) {
                self.next_token()?;
            }
            let Some(kind) = self.peek_kind().cloned() else {
                return Ok(());
            };
            let TokenKind::ControlWord(word) = kind else {
                self.parse_paragraph(Stop::Block)?;
                continue;
            };
            let word = word.strip_prefix("__native:").unwrap_or(&word).to_owned();
            let span = self.span_here();
            match word.as_str() {
                "end" => {
                    let name = self.consume_end()?;
                    if stop_env == Some(name.as_str()) || name == "document" {
                        return Ok(());
                    }
                    self.loss(span, &format!("environment:unmatched-end:{name}"))?;
                }
                "begin" => {
                    self.next_token()?;
                    self.skip_optional_position()?;
                    let env = self.consume_braced_raw()?;
                    self.handle_environment(&env, span)?;
                }
                "item" => {
                    self.next_token()?;
                    self.loss(span, "list:item-outside-list")?;
                }
                "input" | "include" => {
                    self.next_token()?;
                    self.include(word == "include", span)?;
                }
                "par" => {
                    self.next_token()?;
                    self.flush_paragraph()?;
                }
                "newpage" | "clearpage" | "pagebreak" => {
                    self.next_token()?;
                    self.page_break()?;
                }
                "centering" => {
                    self.next_token()?;
                    self.alignment = Some(Justification::Center);
                }
                "raggedright" => {
                    self.next_token()?;
                    self.alignment = Some(Justification::Start);
                }
                "raggedleft" => {
                    self.next_token()?;
                    self.alignment = Some(Justification::End);
                }
                "bibliography" => {
                    self.next_token()?;
                    self.bibliography(span)?;
                }
                "bibliographystyle" => {
                    self.next_token()?;
                    self.consume_braced_raw()?;
                    self.info(span, "bibliography:style-declaration");
                }
                "maketitle" => {
                    self.next_token()?;
                    self.maketitle()?;
                }

                "newcolumntype" => {
                    self.next_token()?;
                    self.define_column_type(span)?;
                }
                "documentclass" | "documentstyle" => {
                    self.next_token()?;
                    self.optional_bracket_text()?;
                    self.document_class = self.consume_braced_raw()?;
                    self.info(span, &format!("document-class:{}", self.document_class));
                }
                "usepackage" | "RequirePackage" => {
                    self.next_token()?;
                    self.load_packages(span)?;
                }
                "endinput" => {
                    self.raw_token()?;
                    if let Some(frame) = self.frames.last_mut() {
                        frame.pos = frame.tokens.len();
                    }
                }
                "address" | "affiliation" => {
                    self.next_token()?;
                    self.read_address()?;
                }
                "title" => {
                    self.next_token()?;
                    self.title = self.capture_group_inlines()?;
                }
                "author" => {
                    self.next_token()?;
                    self.read_author(span)?;
                }
                "date" => {
                    self.next_token()?;
                    self.date = self.capture_group_inlines()?;
                }
                "caption" => {
                    self.next_token()?;
                    self.caption(span)?;
                }
                _ if heading_level(&word).is_some() => {
                    self.next_token()?;
                    let level = heading_level(&word).unwrap_or(0);
                    self.heading(level, !word.ends_with('*'), span)?;
                }
                "tableofcontents" | "listoffigures" | "listoftables" => {
                    self.next_token()?;
                    self.table_of_contents()?;
                }
                "twocolumn" => {
                    self.next_token()?;
                    self.columns = Some(2);
                    self.info(span, "layout:twocolumn");
                }
                "onecolumn" => {
                    self.next_token()?;
                    self.columns = Some(1);
                    self.info(span, "layout:onecolumn");
                }
                "pagestyle" | "thispagestyle" | "pagenumbering" | "pagecolor" => {
                    self.next_token()?;
                    self.skip_args()?;
                    self.info(span, &format!("layout:{word}"));
                }
                _ if is_definition(&word) => {
                    self.next_token()?;
                    self.skip_definition(&word, span)?;
                }
                _ if is_block_starter(&word) => {
                    self.next_token()?;
                    self.skip_args()?;
                    self.info(span, &format!("command:{word}"));
                }
                _ => self.parse_paragraph(Stop::Block)?,
            }
        }
    }

    fn preamble_step(&mut self) -> Result<(), LatexError> {
        let Some(kind) = self.peek_kind().cloned() else {
            self.in_document = true;
            return Ok(());
        };
        match kind {
            TokenKind::ControlWord(word) => {
                let span = self.span_here();
                match word.as_str() {
                    word if is_graphic_configuration(word) => {
                        self.next_token()?;
                        self.skip_graphic_configuration(word, span)?;
                    }
                    "begingroup" => {
                        self.next_token()?;
                        self.enter_local_scope(&self.block_format.clone(), true)?;
                    }
                    "endgroup" => {
                        self.next_token()?;
                        let mut fmt = self.block_format.clone();
                        if self.local_scopes.last().is_some_and(|s| s.explicit) {
                            self.leave_local_scope(&mut fmt);
                            self.block_format = fmt;
                        } else {
                            self.loss(span, "group:unmatched-endgroup")?;
                        }
                    }
                    "input" | "include" => {
                        self.next_token()?;
                        self.include(false, span)?;
                    }
                    "let" => {
                        self.raw_token()?;
                        self.let_binding(span)?;
                    }
                    "newif" => {
                        self.raw_token()?;
                        self.new_if(span)?;
                    }
                    "makeatletter" | "makeatother" | "relax" => {
                        self.raw_token()?;
                    }
                    "begin" => {
                        self.next_token()?;
                        self.skip_optional_position()?;
                        let env = self.consume_braced_raw()?;
                        if env.trim_end_matches('*') == "document" {
                            self.in_document = true;
                            self.parse_blocks(Some(&env))?;
                        } else {
                            self.skip_args()?;
                        }
                    }

                    "newcolumntype" => {
                        self.next_token()?;
                        self.define_column_type(span)?;
                    }
                    "documentclass" | "documentstyle" => {
                        self.next_token()?;
                        self.optional_bracket_text()?;
                        self.document_class = self.consume_braced_raw()?;
                        self.info(span, &format!("document-class:{}", self.document_class));
                    }
                    "usepackage" | "RequirePackage" => {
                        self.next_token()?;
                        self.load_packages(span)?;
                    }
                    "endinput" => {
                        self.raw_token()?;
                        if let Some(frame) = self.frames.last_mut() {
                            frame.pos = frame.tokens.len();
                        }
                    }
                    "address" | "affiliation" => {
                        self.next_token()?;
                        self.read_address()?;
                    }
                    "title" => {
                        self.next_token()?;
                        self.title = self.capture_group_inlines()?;
                    }
                    "author" => {
                        self.next_token()?;
                        self.read_author(span)?;
                    }
                    "date" => {
                        self.next_token()?;
                        self.date = self.capture_group_inlines()?;
                    }
                    _ if is_definition(&word) => {
                        self.next_token()?;
                        self.skip_definition(&word, span)?;
                    }
                    _ => {
                        self.next_token()?;
                        self.skip_args()?;
                        if matches!(
                            word.as_str(),
                            "ProvidesPackage"
                                | "ProvidesClass"
                                | "NeedsTeXFormat"
                                | "PassOptionsToPackage"
                                | "PassOptionsToClass"
                        ) {
                            self.info(span, &format!("preamble:{word}"));
                        } else {
                            self.loss(span, &format!("preamble:unsupported:{word}"))?;
                        }
                    }
                }
            }
            TokenKind::LBrace => {
                self.next_token()?;
                self.enter_local_scope(&self.block_format.clone(), false)?;
            }
            TokenKind::RBrace => {
                let span = self.span_here();
                self.next_token()?;
                if self.local_scopes.last().is_some_and(|s| !s.explicit) {
                    let mut fmt = self.block_format.clone();
                    self.leave_local_scope(&mut fmt);
                    self.block_format = fmt;
                } else {
                    self.loss(span, "group:unmatched-brace")?;
                }
            }
            _ => {
                self.next_token()?;
            }
        }
        Ok(())
    }

    fn skip_optional_position(&mut self) -> Result<(), LatexError> {
        self.optional_bracket_text()?;
        Ok(())
    }

    fn parse_paragraph(&mut self, stop: Stop) -> Result<(), LatexError> {
        let mut fmt = self.block_format.clone();
        self.parse_inlines(&mut fmt, stop)?;
        self.block_format = fmt;
        self.flush_paragraph()
    }

    fn parse_inlines_isolated(
        &mut self,
        fmt: &mut Format,
        stop: Stop,
    ) -> Result<(Vec<Inline>, InlineEnd), LatexError> {
        let saved = std::mem::take(&mut self.inlines);
        let end = self.parse_inlines(fmt, stop)?;
        let out = std::mem::replace(&mut self.inlines, saved);
        Ok((out, end))
    }

    fn parse_inlines(&mut self, fmt: &mut Format, stop: Stop) -> Result<InlineEnd, LatexError> {
        self.enter_nesting()?;
        let scope_depth = self.local_scopes.len();
        if matches!(stop, Stop::Group | Stop::BodyGroup) {
            self.enter_local_scope(fmt, false)?;
        }
        let result = self.parse_inlines_inner(fmt, stop);
        if matches!(stop, Stop::Group | Stop::BodyGroup) {
            self.close_local_scopes(scope_depth, fmt)?;
        }
        self.leave_nesting();
        result
    }

    fn parse_inlines_inner(
        &mut self,
        fmt: &mut Format,
        stop: Stop,
    ) -> Result<InlineEnd, LatexError> {
        loop {
            self.expand_head()?;
            let Some(kind) = self.peek_kind().cloned() else {
                return Ok(InlineEnd::Eof);
            };
            match &kind {
                TokenKind::RBrace if matches!(stop, Stop::Group | Stop::BodyGroup) => {
                    self.next_token()?;
                    return Ok(InlineEnd::Group);
                }
                TokenKind::ParBreak => {
                    self.next_token()?;
                    if stop == Stop::BodyGroup {
                        self.flush_paragraph()?;
                        continue;
                    }
                    return Ok(if stop == Stop::Cell {
                        InlineEnd::Cell
                    } else {
                        InlineEnd::Paragraph
                    });
                }
                TokenKind::Ampersand if stop == Stop::Cell => return Ok(InlineEnd::Cell),
                TokenKind::ControlSymbol('\\') if stop == Stop::Cell => return Ok(InlineEnd::Row),
                TokenKind::ControlWord(word)
                    if stop == Stop::BodyGroup
                        && word.strip_prefix("__native:").unwrap_or(word) == "begin" =>
                {
                    self.flush_paragraph()?;
                    self.next_token()?;
                    self.skip_optional_position()?;
                    let span = self.span_here();
                    let env = self.consume_braced_raw()?;
                    let saved = std::mem::replace(&mut self.block_format, fmt.clone());
                    let result = self.handle_environment(&env, span);
                    self.block_format = saved;
                    result?;
                    continue;
                }
                TokenKind::ControlWord(word)
                    if matches!(stop, Stop::Block | Stop::Cell) && is_block_starter(word) =>
                {
                    return Ok(InlineEnd::Block);
                }
                _ => {}
            }
            let Some(token) = self.next_token()? else {
                return Ok(InlineEnd::Eof);
            };
            let span = LatexSpan::new(token.start, token.end);
            match token.kind {
                TokenKind::Text(text) => self.push_text(fmt, &text)?,
                TokenKind::Raw(text) => {
                    let mut mono = fmt.clone();
                    mono.mono = true;
                    self.push_text(&mono, &text)?;
                }
                TokenKind::Space => self.push_space(fmt)?,
                TokenKind::Tilde => self.push_text(fmt, "\u{00A0}")?,
                TokenKind::Sub => {
                    self.loss(span, "text:underscore")?;
                    self.push_text(fmt, "_")?;
                }
                TokenKind::Sup => {
                    self.loss(span, "text:caret")?;
                    self.push_text(fmt, "^")?;
                }
                TokenKind::Ampersand => {
                    self.loss(span, "alignment:outside-table")?;
                    self.push_text(fmt, "&")?;
                }
                TokenKind::LBrace => {
                    let mut sub = fmt.clone();
                    let nested_stop = if stop == Stop::Group {
                        Stop::Group
                    } else {
                        Stop::BodyGroup
                    };
                    self.parse_inlines(&mut sub, nested_stop)?;
                }
                TokenKind::RBrace => self.push_text(fmt, "}")?,
                TokenKind::LBracket => self.push_text(fmt, "[")?,
                TokenKind::RBracket => self.push_text(fmt, "]")?,
                TokenKind::MathShift => self.math(fmt)?,
                TokenKind::ParBreak => {}
                TokenKind::ControlSymbol(c) => self.control_symbol(c, fmt, span)?,
                TokenKind::ControlWord(word) => self.control_word(&word, fmt, span)?,
            }
        }
    }

    // ----- inline commands ----------------------------------------------

    /// A control word's lexical separator is not a visible space. Keep a real
    /// paragraph break; unlike argument scanning it must not be swallowed.
    fn skip_control_space(&mut self) -> Result<(), LatexError> {
        while matches!(self.peek_kind(), Some(TokenKind::Space)) {
            self.next_token()?;
        }
        Ok(())
    }

    fn enter_local_scope(&mut self, fmt: &Format, explicit: bool) -> Result<(), LatexError> {
        if self.local_scopes.len() >= self.options.limits.nesting {
            return Err(LatexError::LimitExceeded {
                limit: "nesting",
                observed: self.local_scopes.len() + 1,
            });
        }
        self.local_scopes.push(LocalScope {
            explicit,
            format: fmt.clone(),
            alignment: self.alignment,
            colors: self.defined_colors.clone(),
            graphics_paths: self.graphics_paths.clone(),
            column_types: self.column_types.clone(),
            environments: self.environments.clone(),
        });
        self.macros.enter();
        Ok(())
    }

    fn leave_local_scope(&mut self, fmt: &mut Format) {
        if let Some(saved) = self.local_scopes.pop() {
            *fmt = saved.format;
            self.alignment = saved.alignment;
            self.defined_colors = saved.colors;
            self.graphics_paths = saved.graphics_paths;
            self.column_types = saved.column_types;
            self.environments = saved.environments;
            self.macros.leave();
        }
    }

    fn close_local_scopes(&mut self, depth: usize, fmt: &mut Format) -> Result<(), LatexError> {
        while self.local_scopes.len() > depth {
            if self.local_scopes.last().is_some_and(|s| s.explicit) {
                self.loss(self.span_here(), "group:unterminated-begingroup")?;
            }
            self.leave_local_scope(fmt);
        }
        Ok(())
    }

    fn control_symbol(
        &mut self,
        symbol: char,
        fmt: &Format,
        span: LatexSpan,
    ) -> Result<(), LatexError> {
        match symbol {
            '\\' => self.inlines.push(Inline::Break(BreakKind::TextWrapping)),
            '%' => self.push_text(fmt, "%")?,
            '&' => self.push_text(fmt, "&")?,
            '_' => self.push_text(fmt, "_")?,
            '#' => self.push_text(fmt, "#")?,
            '$' => self.push_text(fmt, "$")?,
            '{' => self.push_text(fmt, "{")?,
            '}' => self.push_text(fmt, "}")?,
            ' ' | ',' | ';' | ':' | '!' | '/' => {
                self.info(span, "spacing:control-symbol");
                self.push_text(fmt, " ")?;
            }
            '-' => self.push_text(fmt, "-")?,
            '\'' | '`' | '^' | '"' | '~' | '=' | '.' | 'u' | 'v' | 'H' | 'c' | 'd' | 'b' | 'r'
            | 'k' | 't' => {
                self.accent(fmt, accent_mark(symbol))?;
            }
            '[' => self.delimited_math(true, ']')?,
            '(' => self.delimited_math(false, ')')?,
            ']' | ')' => self.info(span, "math:stray-close"),
            other => {
                self.info(span, &format!("control-symbol:{other}"));
            }
        }
        Ok(())
    }

    /// Composes an accent (base letter + combining mark) into precomposed text.
    fn accent(&mut self, fmt: &Format, mark: char) -> Result<(), LatexError> {
        let base = self.read_accent_base()?;
        if base.is_empty() {
            self.push_text(fmt, &mark.to_string())?;
            return Ok(());
        }
        let mut composed = String::new();
        for (index, ch) in base.chars().enumerate() {
            if index == 0 {
                let base_char = match ch {
                    '\u{0131}' => 'i',
                    '\u{0237}' => 'j',
                    other => other,
                };
                let mut pair = String::new();
                pair.push(base_char);
                pair.push(mark);
                composed.extend(pair.nfc());
            } else {
                composed.push(ch);
            }
        }
        self.push_text(fmt, &composed)
    }

    /// Reads the argument of an accent command as text. A group keeps its
    /// supported formatting; a bare token contributes its literal text.
    fn read_accent_base(&mut self) -> Result<String, LatexError> {
        self.skip_spaces()?;
        match self.peek_kind() {
            Some(TokenKind::LBrace) => {
                self.next_token()?;
                let mut fmt = Format::default();
                let (inlines, _) = self.parse_inlines_isolated(&mut fmt, Stop::Group)?;
                let mut text = String::new();
                for inline in &inlines {
                    if let Inline::Run(run) = inline {
                        for content in &run.content {
                            if let RunContent::Text(node) = content {
                                text.push_str(&node.text);
                            }
                        }
                    }
                }
                Ok(text)
            }
            Some(TokenKind::Text(_)) => {
                if let Some(token) = self.next_token()? {
                    if let TokenKind::Text(text) = token.kind {
                        return Ok(text);
                    }
                }
                Ok(String::new())
            }
            Some(TokenKind::ControlSymbol(c)) => {
                let symbol = *c;
                self.next_token()?;
                Ok(symbol.to_string())
            }
            _ => Ok(String::new()),
        }
    }

    /// Parses `\[...\]` or `\(...\)` math into an OMML expression.
    fn delimited_math(&mut self, display: bool, close: char) -> Result<(), LatexError> {
        let mut tokens = Vec::new();
        let mut closed = false;
        while let Some(token) = self.next_token()? {
            if matches!(&token.kind, TokenKind::ControlSymbol(c) if *c == close) {
                closed = true;
                break;
            }
            tokens.push(token);
        }
        if !closed {
            self.loss(self.span_here(), "math:unterminated")?;
        }
        if self.render_math_graphic(&tokens, display, None)? {
            return Ok(());
        }
        let (expression, labels) = {
            let mut ctx = self.math_context();
            let expression = crate::math::translate(&tokens, &mut ctx)?;
            (expression, ctx.labels)
        };
        let marks = self.math_label_marks(labels);
        if display {
            self.flush_paragraph()?;
            let paragraph = Paragraph {
                props: display_paragraph_properties(),
                inlines: marks
                    .into_iter()
                    .chain([Inline::MathParagraph(display_paragraph(expression))])
                    .collect(),
                rsids: Default::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location: SourceLocation::default(),
            };
            self.push_block(Block::Paragraph(paragraph))
        } else {
            self.inlines.extend(marks);
            self.inlines.push(Inline::Math(expression));
            Ok(())
        }
    }

    fn apply_argument(
        &mut self,
        fmt: &Format,
        span: LatexSpan,
        modify: impl FnOnce(&mut Format),
    ) -> Result<(), LatexError> {
        self.skip_spaces()?;
        self.optional_bracket_text()?;
        let mut sub = fmt.clone();
        modify(&mut sub);
        if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            self.parse_inlines(&mut sub, Stop::Group)?;
        } else {
            self.info(span, "formatting:missing-argument");
        }
        Ok(())
    }

    fn select_argument(
        &mut self,
        count: usize,
        selected: Option<usize>,
        fmt: &Format,
    ) -> Result<(), LatexError> {
        let old = self.expanding;
        self.expanding = false;
        let result = (|| {
            let mut chosen = Vec::new();
            for index in 0..count {
                let argument = self.macro_argument()?;
                if selected == Some(index) {
                    chosen = argument;
                }
            }
            Ok::<_, LatexError>(chosen)
        })();
        self.expanding = old;
        let chosen = self.parse_tokens_as_inlines(result?, fmt)?;
        self.inlines.extend(chosen);
        Ok(())
    }

    fn control_word(
        &mut self,
        word: &str,
        fmt: &mut Format,
        span: LatexSpan,
    ) -> Result<(), LatexError> {
        let word = word.strip_prefix("__native:").unwrap_or(word);
        match word {
            word if is_graphic_configuration(word) => {
                self.skip_graphic_configuration(word, span)?;
            }
            "ifthenelse" => {
                // Conditions from package ifthen require its evaluator. Never
                // execute both branches as prose: they may contain unmatched
                // begin/end templates or local/global assignments.
                self.loss(span, "conditional:requires-tex")?;
                self.select_argument(3, None, fmt)?;
            }
            "begingroup" => {
                self.enter_local_scope(fmt, true)?;
                self.skip_control_space()?;
            }
            "endgroup" => {
                if self.local_scopes.last().is_some_and(|s| s.explicit) {
                    self.leave_local_scope(fmt);
                } else {
                    self.loss(span, "group:unmatched-endgroup")?;
                }
                self.skip_control_space()?;
            }
            "@firstofone" => self.select_argument(1, Some(0), fmt)?,
            "@firstoftwo" => self.select_argument(2, Some(0), fmt)?,
            "@secondoftwo" | "bibinfo" | "bibfield" | "href@noop" => {
                self.select_argument(2, Some(1), fmt)?;
            }
            "@gobble" => self.select_argument(1, None, fmt)?,
            "@gobbletwo" | "bibAnnoteFile" => self.select_argument(2, None, fmt)?,
            "bibnamefont" | "bibfnamefont" | "bibfont" => self.apply_argument(fmt, span, |_| {})?,
            "makeatletter" | "makeatother" => {}
            "let" => self.let_binding(span)?,
            "newif" => self.new_if(span)?,
            word if is_definition(word) => self.skip_definition(word, span)?,
            "setlength" | "addtolength" | "setcounter" | "addtocounter" => {
                self.loss(span, &format!("layout:{word}"))?;
                for _ in 0..2 {
                    self.skip_spaces()?;
                    if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
                        self.skip_balanced()?;
                    } else {
                        self.loss(span, "layout:missing-argument")?;
                        break;
                    }
                }
            }
            "textbf" | "bf" => self.apply_argument(fmt, span, |f| f.bold = true)?,
            "textit" | "it" | "textsl" | "sl" => {
                self.apply_argument(fmt, span, |f| f.italic = true)?;
            }
            "emph" => self.apply_argument(fmt, span, |f| f.italic = !f.italic)?,
            "em" => {
                fmt.italic = !fmt.italic;
                self.skip_control_space()?;
            }
            "textunderscore" => {
                self.push_text(fmt, "_")?;
                self.skip_control_space()?;
            }
            "texttt" | "tt" => self.apply_argument(fmt, span, |f| f.mono = true)?,
            "textsf" | "sf" => self.apply_argument(fmt, span, |f| f.sans = true)?,
            "textrm" | "rm" | "textup" => {
                self.apply_argument(fmt, span, |f| {
                    f.italic = false;
                    f.bold = false;
                })?;
            }
            "textnormal" | "normalfont" => {
                self.apply_argument(fmt, span, |f| *f = Format::default())?;
            }
            "underline" | "uline" => self.apply_argument(fmt, span, |f| f.underline = true)?,
            "textsc" | "sc" => self.apply_argument(fmt, span, |f| f.small_caps = true)?,
            "textsuperscript" => {
                self.apply_argument(fmt, span, |f| f.vert = Some(VertAlign::Superscript))?;
            }
            "textsubscript" => {
                self.apply_argument(fmt, span, |f| f.vert = Some(VertAlign::Subscript))?;
            }
            "mathbf" => self.apply_argument(fmt, span, |f| f.bold = true)?,
            "mathit" | "mathcal" | "mathbb" | "mathfrak" => {
                self.apply_argument(fmt, span, |f| f.italic = true)?;
            }
            "mathrm" => self.apply_argument(fmt, span, |_| {})?,
            "mathsf" => self.apply_argument(fmt, span, |f| f.sans = true)?,
            "mathtt" => self.apply_argument(fmt, span, |f| f.mono = true)?,
            "text" | "mbox" | "hbox" => self.apply_argument(fmt, span, |_| {})?,
            "bfseries" => fmt.bold = true,
            "itshape" | "slshape" => fmt.italic = true,
            "ttfamily" => fmt.mono = true,
            "sffamily" => fmt.sans = true,
            "rmfamily" | "upshape" | "mdseries" => {
                fmt.bold = false;
                fmt.italic = false;
            }
            "scshape" => fmt.small_caps = true,
            "tiny" => fmt.size = Some(10),
            "scriptsize" => fmt.size = Some(14),
            "footnotesize" => fmt.size = Some(16),
            "small" => fmt.size = Some(18),
            "normalsize" => fmt.size = Some(20),
            "large" => fmt.size = Some(24),
            "Large" => fmt.size = Some(28),
            "LARGE" => fmt.size = Some(34),
            "huge" => fmt.size = Some(40),
            "Huge" => fmt.size = Some(50),
            "selectfont" => {}
            "fontsize" => {
                let size = self.consume_braced_raw()?;
                let _skip = self.consume_braced_raw()?;
                if let Some(points) = parse_points(&size) {
                    fmt.size = Some(points);
                }
            }
            "textcolor" => self.textcolor(fmt, span)?,
            "color" => {
                self.skip_spaces()?;
                self.optional_bracket_text()?;
                let name = self.consume_braced_raw()?;
                match self.resolve_color(&name) {
                    Some(hex) => fmt.color = Some(Arc::from(hex)),
                    None => self.info(span, &format!("color:unknown:{name}")),
                }
            }
            "colorbox" => self.colorbox(fmt, span)?,
            "definecolor" => self.definecolor(span)?,
            "newline" | "linebreak" | "nolinebreak" => {
                self.inlines.push(Inline::Break(BreakKind::TextWrapping));
            }
            "i" => self.push_text(fmt, "\u{0131}")?,
            "j" => self.push_text(fmt, "\u{0237}")?,
            "quad" => self.push_text(fmt, "\u{2003}")?,
            "qquad" => self.push_text(fmt, "\u{2003}\u{2003}")?,
            "thinspace" | "enspace" => self.push_text(fmt, "\u{2009}")?,
            " " => self.push_text(fmt, " ")?,
            "url" => self.url(fmt)?,
            "href" => self.href(fmt)?,
            "label" => self.label(span)?,
            "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref" | "nameref" => {
                self.reference(word, span)?;
            }
            "hypertarget" => self.hypertarget(fmt)?,
            "cite" | "citep" | "citet" | "citealp" | "citeauthor" | "Citep" | "Citet" => {
                self.citation(word, fmt, span)?;
            }
            "bibitem" => self.bibitem(fmt, span)?,
            "newblock" => self.push_space(fmt)?,
            "corref" | "tnoteref" | "fnref" => {
                let keys = self.consume_braced_raw()?;
                for key in keys.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let id = self.frontmatter_note_id(key);
                    self.inlines.push(note_reference(fmt, false, id));
                }
            }
            "cortext" | "tnotetext" | "fntext" => {
                let key = self
                    .optional_bracket_text()?
                    .unwrap_or_else(|| format!("note-{}", self.footnote_counter + 1));
                let content = self.capture_group_inlines()?;
                let id = self.frontmatter_note_id(&key);
                self.store_note_with_id(false, id, content);
                self.info(span, "frontmatter:note-number-normalized");
            }
            "ead" => self.frontmatter_contact(fmt)?,
            "sep" => self.push_text(fmt, "; ")?,
            "protect" => {}
            "unskip" => {
                if let Some(Inline::Run(run)) = self.inlines.last_mut() {
                    if let Some(RunContent::Text(t)) = run.content.last_mut() {
                        t.text.truncate(t.text.trim_end().len());
                    }
                }
            }
            "footnote" => self.footnote(fmt)?,
            "footnotemark" => self.footnotemark(fmt)?,
            "footnotetext" => self.footnotetext()?,
            "endnote" => self.endnote(fmt)?,
            "includegraphics" => self.includegraphics(fmt, span)?,
            "tikz" | "chemfig" => {
                let options = if word == "tikz" {
                    self.optional_bracket_text()?
                        .map_or_else(String::new, |s| format!("[{s}]"))
                } else {
                    String::new()
                };
                let old = self.expanding;
                self.expanding = false;
                let body = self.macro_argument();
                self.expanding = old;
                let source = raw_tokens(&body?);
                let tex = format!("\\{word}{options}{{{source}}}");
                self.render_opaque(
                    if word == "tikz" {
                        crate::GraphicKind::Tikz
                    } else {
                        crate::GraphicKind::Chemfig
                    },
                    &tex,
                    span,
                )?;
            }
            "graphicspath" => self.graphicspath(span)?,
            "ensuremath" => self.ensuremath(fmt)?,
            "texorpdfstring" => {
                self.apply_argument(fmt, span, |_| {})?;
                let old = self.expanding;
                self.expanding = false;
                let skipped = self.macro_argument();
                self.expanding = old;
                skipped?;
                self.info(span, "hyperref:pdf-string-alternate");
            }
            "xspace" => {
                if matches!(self.peek_kind(), Some(TokenKind::Text(t)) if t.starts_with(char::is_alphanumeric))
                {
                    self.push_space(fmt)?;
                }
            }
            "hspace" | "vspace" | "hskip" | "vskip" | "kern" | "mkern" | "mskip" | "vphantom"
            | "hphantom" | "phantom" => {
                self.info(span, &format!("spacing:{word}"));
                self.skip_args()?;
            }
            "hfill" | "vfill" | "vfil" | "hfil" | "smallskip" | "medskip" | "bigskip"
            | "noindent" | "indent" | "relax" => {
                self.info(span, &format!("spacing:{word}"));
            }
            "rule" => {
                self.info(span, "spacing:rule");
                self.skip_args()?;
            }
            "par" => {
                self.info(span, "text:paragraph-command");
                self.inlines.push(Inline::Break(BreakKind::TextWrapping));
            }
            "today" => {
                self.info(span, "text:today-dropped");
                self.push_text(fmt, "\u{2014}")?;
            }
            _ => {
                if let Some(text) = text_symbol(word) {
                    self.push_text(fmt, text)?;
                } else {
                    self.loss(span, &format!("command:{word}"))?;
                    self.optional_bracket_text()?;
                    if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
                        self.next_token()?;
                        let mut sub = fmt.clone();
                        self.parse_inlines(&mut sub, Stop::Group)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn url(&mut self, fmt: &Format) -> Result<(), LatexError> {
        let target = self.consume_braced_raw()?;
        if target.is_empty() {
            return Ok(());
        }
        let run = Run {
            props: fmt.to_props(),
            content: vec![RunContent::Text(TextNode {
                text: target.clone(),
                space: Space::Preserve,
            })],
            revision: None,
            location: SourceLocation::default(),
        };
        self.inlines.push(Inline::Hyperlink(Hyperlink {
            rel_id: None,
            target: Some(Arc::from(target)),
            anchor: None,
            tooltip: None,
            history: false,
            tgt_frame: None,
            inlines: vec![Inline::Run(run)],
            location: SourceLocation::default(),
        }));
        Ok(())
    }

    fn href(&mut self, fmt: &Format) -> Result<(), LatexError> {
        let target = self.consume_braced_raw()?;
        let mut sub = fmt.clone();
        let (inlines, _) = if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            self.parse_inlines_isolated(&mut sub, Stop::Group)?
        } else {
            (Vec::new(), InlineEnd::Eof)
        };
        self.inlines.push(Inline::Hyperlink(Hyperlink {
            rel_id: None,
            target: Some(Arc::from(target)),
            anchor: None,
            tooltip: None,
            history: false,
            tgt_frame: None,
            inlines,
            location: SourceLocation::default(),
        }));
        Ok(())
    }

    /// Resolves a colour name to a six-digit hex triplet.
    fn resolve_color(&self, name: &str) -> Option<String> {
        if let Some(hex) = self.defined_colors.get(name) {
            return Some(hex.clone());
        }
        named_color(name).map(str::to_owned)
    }

    fn textcolor(&mut self, fmt: &Format, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        self.optional_bracket_text()?;
        let name = self.consume_braced_raw()?;
        let hex = self.resolve_color(&name);
        let mut sub = fmt.clone();
        if let Some(hex) = hex {
            sub.color = Some(Arc::from(hex));
        } else {
            self.info(span, &format!("color:unknown:{name}"));
        }
        if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            self.parse_inlines(&mut sub, Stop::Group)?;
        }
        Ok(())
    }

    fn colorbox(&mut self, fmt: &Format, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        self.optional_bracket_text()?;
        let name = self.consume_braced_raw()?;
        let hex = self.resolve_color(&name);
        let mut sub = fmt.clone();
        if let Some(hex) = hex {
            sub.shading = Some(Arc::from(hex));
        } else {
            self.info(span, &format!("color:unknown:{name}"));
        }
        if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            self.parse_inlines(&mut sub, Stop::Group)?;
        }
        Ok(())
    }

    fn definecolor(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        let name = self.consume_braced_raw()?;
        let model = self.consume_braced_raw()?;
        let spec = self.consume_braced_raw()?;
        match color_from_spec(&model, &spec) {
            Some(hex) => {
                self.defined_colors.insert(name, hex);
            }
            None => self.info(span, &format!("color:unsupported-model:{model}")),
        }
        Ok(())
    }

    fn ensuremath(&mut self, _fmt: &Format) -> Result<(), LatexError> {
        self.skip_spaces()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(());
        }
        self.next_token()?;
        let mut depth = 1usize;
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token()? {
            match token.kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            tokens.push(token);
        }
        if self.render_math_graphic(&tokens, false, None)? {
            return Ok(());
        }
        let (expression, labels) = {
            let mut ctx = self.math_context();
            let expression = crate::math::translate(&tokens, &mut ctx)?;
            (expression, ctx.labels)
        };
        let marks = self.math_label_marks(labels);
        self.inlines.extend(marks);
        self.inlines.push(Inline::Math(expression));
        Ok(())
    }

    fn includegraphics(&mut self, fmt: &Format, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        let options = self.parse_image_options()?;
        let path = self.consume_braced_raw()?;
        if path.is_empty() {
            self.loss(span, "image:missing-path")?;
            return Ok(());
        }
        self.place_image(fmt, &path, span, options)
    }

    /// Parses the `[key=value,...]` options of `\includegraphics`.
    fn parse_image_options(&mut self) -> Result<crate::media::ImageOptions, LatexError> {
        let Some(text) = self.optional_bracket_text()? else {
            return Ok(crate::media::ImageOptions::default());
        };
        for option in text.split(',') {
            let key = option.split('=').next().unwrap_or("").trim();
            if !key.is_empty()
                && !matches!(
                    key,
                    "width" | "height" | "scale" | "angle" | "trim" | "clip" | "keepaspectratio"
                )
            {
                self.loss(self.span_here(), &format!("image:unsupported-option:{key}"))?;
            }
        }
        Ok(parse_image_options(&text))
    }

    /// Records the search directories declared by `\graphicspath`.
    fn graphicspath(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(());
        }
        self.next_token()?;
        let mut depth = 1usize;
        let mut inner = String::new();
        while let Some(token) = self.next_token()? {
            match token.kind {
                TokenKind::LBrace => {
                    depth += 1;
                    if depth > 1 {
                        inner.push('{');
                    }
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    inner.push('}');
                }
                kind => inner.push_str(&kind.raw()),
            }
        }
        let (paths, truncated) = parse_graphics_paths(&inner);
        self.graphics_paths = paths;
        // Each directory multiplies the names tried per figure, so a declaration
        // with millions of them is a work and allocation bomb; the extra
        // directories are dropped and named.
        if truncated {
            self.loss(span, "graphicspath:too-many-directories")?;
        }
        Ok(())
    }

    fn place_image(
        &mut self,
        _fmt: &Format,
        path: &str,
        span: LatexSpan,
        options: crate::media::ImageOptions,
    ) -> Result<(), LatexError> {
        let mut candidate_names = Vec::new();
        let prefixes: Vec<String> = if self.graphics_paths.is_empty() {
            vec![String::new()]
        } else {
            self.graphics_paths.clone()
        };
        for prefix in &prefixes {
            let base = format!("{prefix}{path}");
            candidate_names.push(base.clone());
            if std::path::Path::new(path).extension().is_none() {
                for extension in [
                    "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "emf", "wmf", "pdf",
                ] {
                    candidate_names.push(format!("{base}.{extension}"));
                }
            }
        }
        let mut found = None;
        for name in &candidate_names {
            if let Ok(bytes) = self.provider.read(name) {
                found = Some((name.clone(), bytes));
                break;
            }
        }
        let Some((name, bytes)) = found else {
            self.loss(span, &format!("image:not-found:{path}"))?;
            return Ok(());
        };
        if bytes.len() > self.options.limits.input_bytes {
            return Err(LatexError::LimitExceeded {
                limit: "input_bytes",
                observed: bytes.len(),
            });
        }
        if media::sniff(&name, &bytes).is_none() {
            let request = crate::RenderRequest {
                kind: crate::GraphicKind::Image,
                name: &name,
                tex: "",
                preamble: &self.render_preamble,
                bytes: &bytes,
                max_output_bytes: self.options.limits.media_part_bytes,
            };
            let rendered = self.provider.render_graphic(&request);
            if let Some(image) = self.conversion_result(rendered, span)? {
                self.loss(span, "image:rasterized")?;
                let options = rendered_image_options(&image.bytes, options);
                return self.embed_image(&image.name, image.bytes, span, options);
            }
            self.loss(span, &format!("image:unsupported-container:{name}"))?;
            return Ok(());
        }
        self.embed_image(&name, bytes, span, options)
    }

    fn conversion_result(
        &mut self,
        result: Result<Option<crate::RenderedGraphic>, LatexError>,
        span: LatexSpan,
    ) -> Result<Option<crate::RenderedGraphic>, LatexError> {
        match result {
            Ok(image) => Ok(image),
            Err(e @ LatexError::LimitExceeded { .. }) => Err(e),
            Err(_) => {
                self.loss(span, "graphic:conversion-failed")?;
                Ok(None)
            }
        }
    }

    fn render_math_graphic(
        &mut self,
        tokens: &[Token],
        display: bool,
        env: Option<&str>,
    ) -> Result<bool, LatexError> {
        let kind = if tokens
            .iter()
            .any(|t| matches!(&t.kind, TokenKind::ControlWord(w) if w == "chemfig"))
        {
            crate::GraphicKind::Chemfig
        } else if tokens
            .iter()
            .any(|t| matches!(&t.kind, TokenKind::ControlWord(w) if w == "tikz"))
        {
            crate::GraphicKind::Tikz
        } else {
            return Ok(false);
        };
        let source = raw_tokens(tokens);
        let tex = env.map_or_else(
            || format!("${source}$"),
            |env| format!("\\begin{{{env}}}{source}\\end{{{env}}}"),
        );
        if display {
            self.flush_paragraph()?;
        }
        self.render_opaque(kind, &tex, self.span_here())?;
        if display {
            self.flush_paragraph()?;
        }
        Ok(true)
    }

    fn render_opaque(
        &mut self,
        kind: crate::GraphicKind,
        tex: &str,
        span: LatexSpan,
    ) -> Result<(), LatexError> {
        let request = crate::RenderRequest {
            kind,
            name: self.current_file(),
            tex,
            preamble: &self.render_preamble,
            bytes: &[],
            max_output_bytes: self.options.limits.media_part_bytes,
        };
        let rendered = self.provider.render_graphic(&request);
        let Some(image) = self.conversion_result(rendered, span)? else {
            self.loss(span, &format!("graphic:renderer-unavailable:{kind:?}"))?;
            return Ok(());
        };
        self.loss(span, "graphic:rasterized")?;
        let options = rendered_image_options(&image.bytes, crate::media::ImageOptions::default());
        self.embed_image(&image.name, image.bytes, span, options)
    }

    fn embed_image(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        span: LatexSpan,
        options: crate::media::ImageOptions,
    ) -> Result<(), LatexError> {
        let index = self.image_counter + 1;
        let built = match media::build(
            index,
            name,
            bytes,
            self.media_bytes,
            self.options.limits.media_part_bytes,
            self.options.limits.media_total_bytes,
            options,
        ) {
            Ok(built) => built,
            // A container the importer cannot measure or embed (a PDF figure, or
            // any other unrecognized payload) is a loss the report names, not a
            // failure: the documented contract makes only invalid text, exceeded
            // budgets and an explicit rejection fatal.
            Err(LatexError::UnsupportedImage { name }) => {
                self.loss(span, &format!("image:unsupported-container:{name}"))?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        self.media_bytes += built.bytes.len();
        self.image_counter = index;
        self.media.push((built.item.part.clone(), built.bytes));
        self.media_index.insert(built.item);
        self.inlines.push(Inline::Drawing(built.drawing));
        Ok(())
    }

    fn footnote(&mut self, fmt: &Format) -> Result<(), LatexError> {
        self.skip_spaces()?;
        self.optional_bracket_text()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(());
        }
        self.next_token()?;
        let mut sub = Format::default();
        let (inlines, _) = self.parse_inlines_isolated(&mut sub, Stop::Group)?;
        let id = self.store_note(false, inlines);
        self.inlines.push(note_reference(fmt, false, id));
        Ok(())
    }

    /// Inserts a note body, adding the separator definitions on first use.
    fn store_note(&mut self, endnote: bool, inlines: Vec<Inline>) -> i32 {
        let (table, counter, props) = if endnote {
            (
                &mut self.endnotes,
                &mut self.endnote_counter,
                &mut self.document_endnote,
            )
        } else {
            (
                &mut self.footnotes,
                &mut self.footnote_counter,
                &mut self.document_footnote,
            )
        };
        if table.is_empty() {
            table.insert(Note {
                id: -1,
                kind: NoteKind::Separator,
                blocks: vec![separator_paragraph(false)],
                location: SourceLocation::default(),
            });
            table.insert(Note {
                id: 0,
                kind: NoteKind::ContinuationSeparator,
                blocks: vec![separator_paragraph(true)],
                location: SourceLocation::default(),
            });
            props.separator_ids = vec![-1, 0];
        }
        *counter += 1;
        let id = *counter;
        let mut marker = Run {
            props: RunProperties::default(),
            content: vec![RunContent::NoteRef],
            revision: None,
            location: SourceLocation::default(),
        };
        marker.content.push(RunContent::Text(TextNode {
            text: " ".to_owned(),
            space: Space::Preserve,
        }));
        let mut items = vec![Inline::Run(marker)];
        items.extend(inlines);
        table.insert(Note {
            id,
            kind: NoteKind::Normal,
            blocks: vec![Block::Paragraph(Paragraph {
                props: ParagraphProperties::default(),
                inlines: items,
                rsids: Default::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location: SourceLocation::default(),
            })],
            location: SourceLocation::default(),
        });
        id
    }

    fn footnotemark(&mut self, fmt: &Format) -> Result<(), LatexError> {
        self.skip_spaces()?;
        let id = self
            .optional_bracket_text()?
            .and_then(|text| text.trim().parse::<i32>().ok())
            .unwrap_or_else(|| {
                self.footnote_counter += 1;
                self.footnote_counter
            });
        self.last_footnote_mark = Some(id);
        self.inlines.push(note_reference(fmt, false, id));
        Ok(())
    }

    fn footnotetext(&mut self) -> Result<(), LatexError> {
        self.skip_spaces()?;
        self.optional_bracket_text()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(());
        }
        self.next_token()?;
        let mut sub = Format::default();
        let (inlines, _) = self.parse_inlines_isolated(&mut sub, Stop::Group)?;
        let id = if let Some(id) = self.last_footnote_mark {
            self.last_footnote_mark = None;
            // Reuse the reserved number if no separator was needed yet.
            if self.footnote_counter < id {
                self.footnote_counter = id;
            }
            self.store_note_with_id(false, id, inlines);
            id
        } else {
            self.store_note(false, inlines)
        };
        let _ = id;
        Ok(())
    }

    /// Inserts a note under a caller-chosen id, adding separators if needed.
    fn store_note_with_id(&mut self, endnote: bool, id: i32, inlines: Vec<Inline>) {
        let (table, props) = if endnote {
            (&mut self.endnotes, &mut self.document_endnote)
        } else {
            (&mut self.footnotes, &mut self.document_footnote)
        };
        if table.is_empty() {
            table.insert(Note {
                id: -1,
                kind: NoteKind::Separator,
                blocks: vec![separator_paragraph(false)],
                location: SourceLocation::default(),
            });
            table.insert(Note {
                id: 0,
                kind: NoteKind::ContinuationSeparator,
                blocks: vec![separator_paragraph(true)],
                location: SourceLocation::default(),
            });
            props.separator_ids = vec![-1, 0];
        }
        let mut marker = Run {
            props: RunProperties::default(),
            content: vec![RunContent::NoteRef],
            revision: None,
            location: SourceLocation::default(),
        };
        marker.content.push(RunContent::Text(TextNode {
            text: " ".to_owned(),
            space: Space::Preserve,
        }));
        let mut items = vec![Inline::Run(marker)];
        items.extend(inlines);
        table.insert(Note {
            id,
            kind: NoteKind::Normal,
            blocks: vec![Block::Paragraph(Paragraph {
                props: ParagraphProperties::default(),
                inlines: items,
                rsids: Default::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location: SourceLocation::default(),
            })],
            location: SourceLocation::default(),
        });
    }

    fn endnote(&mut self, fmt: &Format) -> Result<(), LatexError> {
        self.skip_spaces()?;
        self.optional_bracket_text()?;
        if !matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            return Ok(());
        }
        self.next_token()?;
        let mut sub = Format::default();
        let (inlines, _) = self.parse_inlines_isolated(&mut sub, Stop::Group)?;
        let id = self.store_note(true, inlines);
        self.inlines.push(note_reference(fmt, true, id));
        Ok(())
    }

    /// `\label{key}`: an empty bookmark a `\ref` can point at.
    fn label(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        let key = self.consume_braced_raw()?;
        if key.is_empty() {
            return Ok(());
        }
        let marks = self.math_label_marks(vec![key.clone()]);
        self.inlines.extend(marks);
        self.info(span, &format!("label:{key}"));
        Ok(())
    }

    /// Zero-width bookmark pairs in the paragraph containing a formula.
    fn math_label_marks(&mut self, labels: Vec<String>) -> Vec<Inline> {
        let mut marks = Vec::new();
        for key in labels {
            self.labels
                .entry(bookmark_name(&key))
                .or_insert_with(|| self.reference_context.clone());
            self.bookmark_counter += 1;
            let id = self.bookmark_counter.to_string();
            marks.push(Inline::BookmarkStart(Bookmark::new(
                id.clone(),
                bookmark_name(&key),
            )));
            marks.push(Inline::BookmarkEnd(
                strict_ooxml_wml::model::BookmarkId::new(id),
            ));
        }
        marks
    }

    fn read_aux(&mut self) -> Result<(), LatexError> {
        let name = std::path::Path::new(&self.file_names[0])
            .with_extension("aux")
            .to_string_lossy()
            .into_owned();
        if let Ok(bytes) = self.provider.read(&name) {
            if bytes.len() > self.options.limits.input_bytes {
                return Err(LatexError::LimitExceeded {
                    limit: "input_bytes",
                    observed: bytes.len(),
                });
            }
            let text = decode(&bytes, &name)?;
            for (key, value) in crate::references::aux(&text) {
                self.labels.insert(bookmark_name(&key), value);
            }
        }
        Ok(())
    }

    /// Graphics declarations are configuration, never document prose. Consume
    /// without expansion so style bodies cannot execute nested commands.
    fn skip_graphic_configuration(&mut self, word: &str, span: LatexSpan) -> Result<(), LatexError> {
        self.loss(span, &format!("graphic:configuration:{word}"))?;
        let expanding = std::mem::replace(&mut self.expanding, false);
        let result = (|| {
            self.skip_spaces()?;
            self.skip_balanced()?;
            if word == "tikzstyle" {
                self.skip_spaces()?;
                if matches!(self.peek_kind(), Some(TokenKind::Text(s)) if s.starts_with('=')) {
                    self.macro_argument()?;
                }
                self.skip_spaces()?;
                if matches!(self.peek_kind(), Some(TokenKind::LBracket)) {
                    self.skip_balanced()?;
                }
            }
            Ok(())
        })();
        self.expanding = expanding;
        result
    }

    fn citation(&mut self, word: &str, fmt: &Format, span: LatexSpan) -> Result<(), LatexError> {
        let first = self.optional_bracket_text()?;
        let second = self.optional_bracket_text()?;
        let keys = self.consume_braced_raw()?;
        if !matches!(word, "cite" | "citep" | "Citep" | "citealp") {
            self.loss(span, "citation:author-style")?;
        }
        self.push_text(fmt, "[")?;
        if second.is_some() {
            if let Some(pre) = &first {
                self.push_text(fmt, pre)?;
                self.push_text(fmt, " ")?;
            }
        }
        for (i, key) in keys
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .enumerate()
        {
            if i > 0 {
                self.push_text(fmt, ", ")?;
            }
            let tokens = crate::macros::tokens(&format!("{{__bib_{key}}}"));
            let file_id = self.frames.last().map_or(0, |f| f.file_id);
            self.frames.push(Frame {
                file_id,
                include_name: None,
                tokens,
                pos: 0,
            });
            self.reference("ref", span)?;
        }
        if let Some(post) = second.or(first) {
            self.push_text(fmt, ", ")?;
            self.push_text(fmt, &post)?;
        }
        self.push_text(fmt, "]")
    }

    fn bibitem(&mut self, fmt: &Format, span: LatexSpan) -> Result<(), LatexError> {
        self.flush_paragraph()?;
        let label = self.optional_bracket_text()?;
        let key = self.consume_braced_raw()?;
        self.bibliography_counter += 1;
        let label = if let Some(label) = label {
            let rendered = self.parse_tokens_as_inlines(crate::macros::tokens(&label), fmt)?;
            let plain = plain_inlines(&rendered);
            if let Some((author, rest)) = plain.split_once('(') {
                if let Some((year, _long_authors)) = rest.split_once(')') {
                    self.info(span, "bibliography:natbib-label");
                    format!("{}, {}", author.trim(), year.trim())
                } else {
                    plain
                }
            } else {
                plain
            }
        } else {
            self.bibliography_counter.to_string()
        };
        let old = std::mem::replace(
            &mut self.reference_context,
            crate::references::Value {
                number: label.clone(),
                ..Default::default()
            },
        );
        let marks = self.math_label_marks(vec![format!("__bib_{key}")]);
        self.reference_context = old;
        self.inlines.extend(marks);
        self.push_text(fmt, &format!("[{label}] "))?;
        self.info(span, "bibliography:item");
        Ok(())
    }

    fn bibliography(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        let names = self.consume_braced_raw()?;
        if self.bibliography_loaded {
            return Ok(());
        }
        let main = std::path::Path::new(&self.file_names[0])
            .with_extension("bbl")
            .to_string_lossy()
            .into_owned();
        let candidates =
            std::iter::once(main).chain(names.split(',').map(|s| format!("{}.bbl", s.trim())));
        for name in candidates {
            if let Ok(bytes) = self.provider.read(&name) {
                if bytes.len() > self.options.limits.input_bytes {
                    return Err(LatexError::LimitExceeded {
                        limit: "input_bytes",
                        observed: bytes.len(),
                    });
                }
                let text = decode(&bytes, &name)?;
                let file_id = self.file_names.len();
                self.file_names.push(name.clone());
                self.frames.push(Frame {
                    file_id,
                    include_name: Some(name),
                    tokens: lex(&text),
                    pos: 0,
                });
                self.bibliography_loaded = true;
                return Ok(());
            }
        }
        let mut items = Vec::new();
        for name in names.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let path = if std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bib"))
            {
                name.to_owned()
            } else {
                format!("{name}.bib")
            };
            if let Ok(bytes) = self.provider.read(&path) {
                if bytes.len() > self.options.limits.input_bytes {
                    return Err(LatexError::LimitExceeded {
                        limit: "input_bytes",
                        observed: bytes.len(),
                    });
                }
                let parsed = crate::bibliography::read(&decode(&bytes, &path)?);
                for feature in parsed.unsupported {
                    self.loss(span, &format!("bibliography:{feature}"))?;
                }
                items.extend(parsed.items);
            } else {
                self.loss(span, &format!("bibliography:missing-source:{path}"))?;
            }
        }
        if items.is_empty() {
            return self.loss(span, "bibliography:missing-bbl");
        }
        self.loss(span, "bibliography:normalized-style")?;
        let tokens = lex(&crate::bibliography::normalized(&items));
        let file_id = self.frames.last().map_or(0, |f| f.file_id);
        self.frames.push(Frame {
            file_id,
            include_name: None,
            tokens,
            pos: 0,
        });
        self.bibliography_loaded = true;
        Ok(())
    }

    fn reference(&mut self, word: &str, span: LatexSpan) -> Result<(), LatexError> {
        let key = self.consume_braced_raw()?;
        let name = bookmark_name(&key);
        self.reference_requests.push((
            name.clone(),
            word.to_owned(),
            span,
            self.current_file().to_owned(),
        ));
        let (instruction, cached) = match word {
            "pageref" => (format!(" PAGEREF {name} \\h "), "??".to_owned()),
            "nameref" => (format!(" REF {name} \\n \\h "), "??".to_owned()),
            "eqref" => (format!(" REF {name} \\h "), "(??)".to_owned()),
            _ => (format!(" REF {name} \\h "), "??".to_owned()),
        };
        self.info(span, &format!("cross-reference:{key}"));
        self.inlines
            .push(Inline::Field(strict_ooxml_wml::model::Field {
                instruction: Some(Arc::from(instruction)),
                inlines: vec![Inline::Run(Run {
                    props: RunProperties::default(),
                    content: vec![RunContent::Text(TextNode {
                        text: cached,
                        space: Space::Preserve,
                    })],
                    revision: None,
                    location: SourceLocation::default(),
                })],
                location: SourceLocation::default(),
            }));
        Ok(())
    }

    /// `\hypertarget{name}{text}`: a bookmark around the target text.
    fn hypertarget(&mut self, fmt: &Format) -> Result<(), LatexError> {
        let key = self.consume_braced_raw()?;
        self.bookmark_counter += 1;
        let id = self.bookmark_counter.to_string();
        self.inlines.push(Inline::BookmarkStart(Bookmark::new(
            id.clone(),
            bookmark_name(&key),
        )));
        if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            let mut sub = fmt.clone();
            self.parse_inlines(&mut sub, Stop::Group)?;
        }
        self.inlines.push(Inline::BookmarkEnd(
            strict_ooxml_wml::model::BookmarkId::new(id),
        ));
        Ok(())
    }

    // ----- block constructs ---------------------------------------------

    fn heading(&mut self, level: u8, numbered: bool, span: LatexSpan) -> Result<(), LatexError> {
        self.flush_paragraph()?;
        self.optional_bracket_text()?;
        let fmt = Format {
            bold: true,
            size: Some(heading_size(level)),
            ..Format::default()
        };
        let mut inlines = Vec::new();
        if numbered {
            let number = self.bump_section(usize::from(level));
            self.reference_context = crate::references::Value {
                number: number.clone(),
                ..Default::default()
            };
            inlines.push(Inline::Run(Run {
                props: RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: format!("{number}\u{00A0}"),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::default(),
            }));
        }
        if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            let mut fmt = fmt;
            let (title, _) = self.parse_inlines_isolated(&mut fmt, Stop::Group)?;
            self.reference_context.title = plain_inlines(&title);
            inlines.extend(title);
        }
        let props = ParagraphProperties {
            style: Some(StyleId::new(format!("Heading{}", usize::from(level) + 1))),
            outline_level: Some(level),
            keep_next: TriState::On,
            spacing: Some(Spacing {
                before: Some(Twips(240)),
                after: Some(Twips(120)),
                ..Spacing::default()
            }),
            ..ParagraphProperties::default()
        };
        let _ = span;
        self.push_paragraph(props, inlines)
    }

    /// Increments the section counter at `level` and returns the number label.
    fn bump_section(&mut self, level: usize) -> String {
        if let Some(slot) = self.section_counters.get_mut(level) {
            *slot += 1;
        }
        for deeper in (level + 1)..self.section_counters.len() {
            self.section_counters[deeper] = 0;
        }
        self.section_counters
            .get(..=level)
            .map(|counters| {
                counters
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default()
    }

    /// `\tableofcontents`: a `TOC` field with a placeholder result.
    fn table_of_contents(&mut self) -> Result<(), LatexError> {
        self.flush_paragraph()?;
        let inlines = vec![Inline::Field(Field {
            instruction: Some(Arc::from(" TOC \\o \"1-3\" \\h \\z \\u ")),
            inlines: vec![Inline::Run(Run {
                props: RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: "Table of Contents".to_owned(),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::default(),
            })],
            location: SourceLocation::default(),
        })];
        self.push_paragraph(ParagraphProperties::default(), inlines)
    }

    fn caption(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        self.flush_paragraph()?;
        self.optional_bracket_text()?;
        let number = match self.float_kind {
            Some("figure") => {
                self.figure_counter += 1;
                self.figure_counter.to_string()
            }
            Some("table") => {
                self.table_counter += 1;
                self.table_counter.to_string()
            }
            _ => {
                self.loss(span, "reference:unsupported-caption-counter")?;
                String::new()
            }
        };
        self.reference_context = crate::references::Value {
            number,
            ..Default::default()
        };
        let mut fmt = Format {
            italic: true,
            ..Format::default()
        };
        let (inlines, _) = if matches!(self.peek_kind(), Some(TokenKind::LBrace)) {
            self.next_token()?;
            self.parse_inlines_isolated(&mut fmt, Stop::Group)?
        } else {
            (Vec::new(), InlineEnd::Eof)
        };
        let props = ParagraphProperties {
            alignment: Some(Justification::Center),
            ..ParagraphProperties::default()
        };
        self.reference_context.title = plain_inlines(&inlines);
        self.info(span, "float:caption");
        self.push_paragraph(props, inlines)
    }

    fn maketitle(&mut self) -> Result<(), LatexError> {
        let title = std::mem::take(&mut self.title);
        if !title.is_empty() {
            self.push_paragraph(
                ParagraphProperties {
                    alignment: Some(Justification::Center),
                    ..ParagraphProperties::default()
                },
                title,
            )?;
        }
        let author = std::mem::take(&mut self.author);
        if !author.is_empty() {
            self.push_paragraph(
                ParagraphProperties {
                    alignment: Some(Justification::Center),
                    ..ParagraphProperties::default()
                },
                author,
            )?;
        }
        let date = std::mem::take(&mut self.date);
        if !date.is_empty() {
            self.push_paragraph(
                ParagraphProperties {
                    alignment: Some(Justification::Center),
                    ..ParagraphProperties::default()
                },
                date,
            )?;
        }
        self.info(LatexSpan::new(0, 0), "maketitle:rendered");
        Ok(())
    }

    fn page_break(&mut self) -> Result<(), LatexError> {
        self.flush_paragraph()?;
        self.push_paragraph(
            ParagraphProperties::default(),
            vec![Inline::Break(BreakKind::Page)],
        )
    }

    fn include(&mut self, force_page: bool, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_spaces()?;
        let raw = self.consume_braced_raw()?;
        if raw.is_empty() {
            self.loss(span, "include:missing-name")?;
            return Ok(());
        }
        if force_page {
            self.page_break()?;
        }
        let depth = self
            .frames
            .iter()
            .filter(|frame| frame.include_name.is_some())
            .count();
        if depth + 1 > self.options.limits.depth {
            self.loss(span, "include:depth-exceeded")?;
            return Ok(());
        }
        if self
            .frames
            .iter()
            .any(|frame| frame.include_name.as_deref() == Some(raw.as_str()))
        {
            self.loss(span, &format!("include:cycle:{raw}"))?;
            return Ok(());
        }
        if self.included_files + 1 > self.options.limits.include_files {
            self.loss(span, "include:count-exceeded")?;
            return Ok(());
        }
        let mut candidates = vec![raw.clone()];
        let has_tex_extension = std::path::Path::new(&raw)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tex"));
        if !has_tex_extension {
            candidates.push(format!("{raw}.tex"));
        }
        let mut found = None;
        for name in &candidates {
            if let Ok(bytes) = self.provider.read(name) {
                found = Some((name.clone(), bytes));
                break;
            }
        }
        let Some((name, bytes)) = found else {
            self.loss(span, &format!("include:not-found:{raw}"))?;
            return Ok(());
        };
        if bytes.len() > self.options.limits.input_bytes {
            self.loss(span, "include:file-too-large")?;
            return Ok(());
        }
        let text = match decode(&bytes, &name) {
            Ok(text) => text,
            Err(_) => {
                self.loss(span, "include:invalid-utf8")?;
                return Ok(());
            }
        };
        let tokens = lex(&text);
        let file_id = self.file_names.len();
        self.file_names.push(name);
        self.frames.push(Frame {
            file_id,
            include_name: Some(raw),
            tokens,
            pos: 0,
        });
        self.included_files += 1;
        Ok(())
    }

    fn read_author(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        let affiliations = self.optional_bracket_text()?;
        let content = self.capture_group_inlines()?;
        if !self.author.is_empty() {
            let separator =
                self.parse_tokens_as_inlines(crate::macros::tokens(", "), &Format::default())?;
            self.author.extend(separator);
        }
        self.author.extend(content);
        if let Some(keys) = affiliations {
            let numbers = self.affiliation_indices(&keys);
            let marker = self.parse_tokens_as_inlines(
                crate::macros::tokens(&numbers),
                &Format {
                    vert: Some(VertAlign::Superscript),
                    ..Format::default()
                },
            )?;
            self.author.extend(marker);
        }
        let _ = span;
        Ok(())
    }

    fn frontmatter_note_id(&mut self, key: &str) -> i32 {
        if let Some(id) = self.frontmatter_note_ids.get(key) {
            return *id;
        }
        self.footnote_counter += 1;
        let id = self.footnote_counter;
        self.frontmatter_note_ids.insert(key.to_owned(), id);
        id
    }

    fn frontmatter_contact(&mut self, fmt: &Format) -> Result<(), LatexError> {
        let kind = self.optional_bracket_text()?;
        let address = self.consume_braced_raw()?;
        let saved = std::mem::take(&mut self.inlines);
        self.push_text(fmt, &address)?;
        let content = std::mem::replace(&mut self.inlines, saved);
        self.frontmatter_contacts
            .push(vec![Inline::Hyperlink(Hyperlink {
                rel_id: None,
                target: Some(Arc::from(if kind.as_deref() == Some("url") {
                    address
                } else {
                    format!("mailto:{address}")
                })),
                anchor: None,
                tooltip: None,
                history: false,
                tgt_frame: None,
                inlines: content,
                location: SourceLocation::default(),
            })]);
        Ok(())
    }

    fn affiliation_indices(&mut self, keys: &str) -> String {
        keys.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|key| {
                let next = self.affiliation_numbers.len() + 1;
                self.affiliation_numbers
                    .entry(key.to_owned())
                    .or_insert(next)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn read_address(&mut self) -> Result<(), LatexError> {
        let keys = self.optional_bracket_text()?;
        let mut content = Vec::new();
        if let Some(keys) = keys {
            let numbers = self.affiliation_indices(&keys);
            for key in keys.split(',').map(str::trim) {
                self.addressed_affiliations.insert(key.to_owned());
            }
            content = self.parse_tokens_as_inlines(
                crate::macros::tokens(&format!("{numbers} ")),
                &Format::default(),
            )?;
        }
        content.extend(self.capture_group_inlines()?);
        self.affiliations.push(content);
        Ok(())
    }

    fn emit_frontmatter(&mut self) -> Result<(), LatexError> {
        if !self.frontmatter_title_emitted {
            if self
                .affiliation_numbers
                .keys()
                .any(|k| !self.addressed_affiliations.contains(k))
            {
                self.loss(self.span_here(), "frontmatter:missing-affiliation")?;
            }
            self.maketitle()?;
            self.frontmatter_title_emitted = true;
            let affiliations = std::mem::take(&mut self.affiliations);
            for content in affiliations {
                self.push_paragraph(ParagraphProperties::default(), content)?;
            }
            for content in std::mem::take(&mut self.frontmatter_contacts) {
                self.push_paragraph(ParagraphProperties::default(), content)?;
            }
        }
        Ok(())
    }

    fn load_packages(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        let options = self.optional_bracket_text()?;
        let names = self.consume_braced_raw()?;
        if options.as_deref().is_some_and(|s| !s.is_empty()) {
            self.loss(span, "package:options")?;
        }
        let mut frames = Vec::new();
        let (first, remaining) = names.split_once(',').unwrap_or((&names, ""));
        if !remaining.trim().is_empty() {
            let file_id = self.frames.last().map_or(0, |f| f.file_id);
            let tokens = vec![
                Token {
                    kind: TokenKind::ControlWord("RequirePackage".into()),
                    start: span.start,
                    end: span.end,
                },
                Token {
                    kind: TokenKind::LBrace,
                    start: span.start,
                    end: span.end,
                },
                Token {
                    kind: TokenKind::Text(remaining.to_owned()),
                    start: span.start,
                    end: span.end,
                },
                Token {
                    kind: TokenKind::RBrace,
                    start: span.start,
                    end: span.end,
                },
            ];
            self.frames.push(Frame {
                file_id,
                include_name: None,
                tokens,
                pos: 0,
            });
        }
        for name in std::iter::once(first.trim()).filter(|s| !s.is_empty()) {
            if !self.loaded_packages.insert(name.to_owned()) {
                continue;
            }
            if self.loaded_packages.len() > self.options.limits.include_files {
                return Err(LatexError::LimitExceeded {
                    limit: "package_files",
                    observed: self.loaded_packages.len(),
                });
            }
            let path = format!("{name}.sty");
            if let Ok(bytes) = self.provider.read(&path) {
                if bytes.len() > self.options.limits.input_bytes {
                    return Err(LatexError::LimitExceeded {
                        limit: "input_bytes",
                        observed: bytes.len(),
                    });
                }
                let text = decode(&bytes, &path)?;
                let tokens = crate::lexer::lex_with_at_letter(&text, true);
                if tokens.iter().any(|t| matches!(&t.kind, TokenKind::ControlWord(w) if primitive_conditional(w) || matches!(w.as_str(), "expandafter" | "csname" | "endcsname" | "futurelet" | "afterassignment" | "aftergroup" | "catcode" | "numexpr" | "dimexpr"))) {
                    self.loss(span, &format!("package:requires-tex:{name}"))?;
                    continue;
                }
                let file_id = self.file_names.len();
                self.file_names.push(path.clone());
                frames.push(Frame {
                    file_id,
                    include_name: Some(path),
                    tokens,
                    pos: 0,
                });
                self.included_files += 1;
                self.info(span, &format!("package:local:{name}"));
            } else if matches!(
                name,
                "amsmath"
                    | "amssymb"
                    | "amsfonts"
                    | "mathtools"
                    | "graphicx"
                    | "graphics"
                    | "color"
                    | "xcolor"
                    | "hyperref"
                    | "url"
                    | "array"
                    | "booktabs"
                    | "multirow"
                    | "longtable"
                    | "tabularx"
                    | "enumerate"
                    | "enumitem"
                    | "textcomp"
                    | "fontenc"
                    | "inputenc"
            ) {
                self.info(span, &format!("package:builtin-subset:{name}"));
            } else {
                self.loss(span, &format!("package:unavailable:{name}"))?;
            }
        }
        self.frames.extend(frames.into_iter().rev());
        Ok(())
    }

    fn handle_environment(&mut self, env: &str, span: LatexSpan) -> Result<(), LatexError> {
        let depth = self.local_scopes.len();
        let mut fmt = self.block_format.clone();
        self.enter_local_scope(&fmt, false)?;
        let result = self.handle_environment_inner(env, span);
        let cleanup = self.close_local_scopes(depth, &mut fmt);
        self.block_format = fmt;
        cleanup?;
        result
    }

    fn handle_environment_inner(&mut self, env: &str, span: LatexSpan) -> Result<(), LatexError> {
        if let Some((definition, ending)) = self.environments.get(env).cloned() {
            let old = self.expanding;
            self.expanding = false;
            let result = (|| {
                let mut args = Vec::new();
                if let Some(default) = &definition.default {
                    args.push(
                        self.optional_bracket_text()?
                            .map_or_else(|| default.clone(), |s| crate::macros::tokens(&s)),
                    );
                }
                while args.len() < definition.args {
                    args.push(self.macro_argument()?);
                }
                let mut tokens =
                    crate::macros::substitute(&definition.body, &args, self.options.limits.tokens)?;
                tokens.extend(self.collect_environment_body(env)?);
                tokens.extend(crate::macros::substitute(
                    &ending,
                    &args,
                    self.options.limits.tokens,
                )?);
                Ok::<_, LatexError>(tokens)
            })();
            self.expanding = old;
            self.macros.enter();
            let result = self.parse_tokens_as_blocks(result?);
            self.macros.leave();
            for block in result? {
                self.push_block(block)?;
            }
            return Ok(());
        }
        let bare = env.trim_end_matches('*');
        match bare {
            "tikzpicture" | "tikzcd" => {
                let old = self.expanding;
                self.expanding = false;
                let body = self.collect_environment_body(env);
                self.expanding = old;
                let source = raw_tokens(&body?);
                let tex = format!("\\begin{{{env}}}{source}\\end{{{env}}}");
                self.render_opaque(crate::GraphicKind::Tikz, &tex, span)?;
            }
            "document" => {
                self.in_document = true;
                self.parse_blocks(Some(env))?;
            }
            "itemize" | "enumerate" | "description" => self.parse_list(env)?,
            "tabular" | "tabularx" | "longtable" | "array" => self.parse_table(env, span)?,
            "figure" | "table" | "wrapfigure" | "subfigure" | "figurebox" => {
                self.skip_optional_position()?;
                self.info(span, &format!("float:{env}"));
                let saved = self.float_kind;
                self.float_kind = Some(if bare == "table" {
                    "table"
                } else if bare == "subfigure" {
                    "subfigure"
                } else {
                    "figure"
                });
                let result = self.parse_blocks(Some(env));
                self.float_kind = saved;
                result?;
            }
            "center" => {
                let saved = self.alignment;
                self.alignment = Some(Justification::Center);
                self.parse_blocks(Some(env))?;
                self.alignment = saved;
            }
            "flushleft" => {
                let saved = self.alignment;
                self.alignment = Some(Justification::Start);
                self.parse_blocks(Some(env))?;
                self.alignment = saved;
            }
            "flushright" => {
                let saved = self.alignment;
                self.alignment = Some(Justification::End);
                self.parse_blocks(Some(env))?;
                self.alignment = saved;
            }
            "verbatim" | "lstlisting" | "minted" | "Verbatim" | "filecontents" => {
                self.parse_verbatim(env)?;
            }
            "equation" | "displaymath" | "math" | "align" | "alignat" | "gather" | "multline"
            | "eqnarray" | "split" => self.display_math(env)?,
            "frontmatter" if self.document_class == "elsarticle" => {
                self.parse_blocks(Some(env))?;
                self.emit_frontmatter()?;
            }
            "abstract" if self.document_class == "elsarticle" => {
                self.emit_frontmatter()?;
                self.parse_blocks(Some(env))?;
            }
            "keyword" | "highlights" if self.document_class == "elsarticle" => {
                self.parse_blocks(Some(env))?;
            }
            "thebibliography" => {
                self.consume_braced_raw()?;
                self.parse_blocks(Some(env))?;
            }
            "quote" | "quotation" | "verse" | "abstract" => {
                self.parse_blocks(Some(env))?;
            }
            "titlepage" => {
                let saved = self.alignment;
                self.alignment = Some(Justification::Center);
                self.parse_blocks(Some(env))?;
                self.alignment = saved;
            }
            _ => {
                self.loss(span, &format!("environment:{env}"))?;
                self.parse_blocks(Some(env))?;
            }
        }
        Ok(())
    }

    fn display_math(&mut self, env: &str) -> Result<(), LatexError> {
        let body = self.collect_environment_body(env)?;
        if !env.ends_with('*') && !matches!(env, "math" | "displaymath" | "split") {
            let rows = if matches!(env, "align" | "alignat" | "gather" | "eqnarray") {
                crate::math::split_top_level(&body, &TokenKind::ControlSymbol('\\'))
            } else {
                vec![body.clone()]
            };
            for row in rows.iter().filter(|r| {
                r.iter()
                    .any(|t| !matches!(t.kind, TokenKind::Space | TokenKind::ParBreak))
            }) {
                if row.iter().any(|t| matches!(&t.kind, TokenKind::ControlWord(w) if w == "notag" || w == "nonumber")) { continue; }
                self.equation_counter += 1;
                self.reference_context = crate::references::Value {
                    number: self.equation_counter.to_string(),
                    ..Default::default()
                };
                // Label groups are metadata; scan only for this known command.
                for (i, t) in row.iter().enumerate() {
                    if matches!(&t.kind, TokenKind::ControlWord(w) if w == "label") {
                        let tail = &row[i + 1..];
                        let start = tail
                            .iter()
                            .position(|t| matches!(t.kind, TokenKind::LBrace));
                        if let Some(start) = start {
                            let key = tail[start + 1..]
                                .iter()
                                .take_while(|t| !matches!(t.kind, TokenKind::RBrace))
                                .map(|t| t.kind.raw())
                                .collect::<String>();
                            self.labels
                                .entry(bookmark_name(&key))
                                .or_insert_with(|| self.reference_context.clone());
                        }
                    }
                }
            }
        }
        self.flush_paragraph()?;
        if self.render_math_graphic(&body, true, Some(env))? {
            return Ok(());
        }
        let (expression, labels) = {
            let mut ctx = self.math_context();
            let expression = translate_environment(env, &body, &mut ctx)?;
            (expression, ctx.labels)
        };
        let marks = self.math_label_marks(labels);
        let paragraph = Paragraph {
            props: display_paragraph_properties(),
            inlines: marks
                .into_iter()
                .chain([Inline::MathParagraph(MathParagraph {
                    properties: Some(MathParagraphProperties {
                        justification: Some(MathJustification::Center),
                    }),
                    equations: vec![expression],
                    location: SourceLocation::default(),
                })])
                .collect(),
            rsids: Default::default(),
            revision: None,
            para_id: None,
            text_id: None,
            location: SourceLocation::default(),
        };
        self.push_block(Block::Paragraph(paragraph))
    }

    fn parse_verbatim(&mut self, env: &str) -> Result<(), LatexError> {
        let body = self.collect_environment_body(env)?;
        let mut text = String::new();
        for token in &body {
            text.push_str(&token.kind.raw());
        }
        let props = Format {
            mono: true,
            ..Format::default()
        }
        .to_props();
        for paragraph in text.split("\n\n") {
            let inlines = if paragraph.is_empty() {
                Vec::new()
            } else {
                vec![Inline::Run(Run {
                    props: props.clone(),
                    content: vec![RunContent::Text(TextNode {
                        text: paragraph.to_owned(),
                        space: Space::Preserve,
                    })],
                    revision: None,
                    location: SourceLocation::default(),
                })]
            };
            self.push_paragraph(ParagraphProperties::default(), inlines)?;
        }
        self.info(LatexSpan::new(0, 0), &format!("verbatim:{env}"));
        Ok(())
    }

    fn parse_list(&mut self, env: &str) -> Result<(), LatexError> {
        self.ensure_numbering();
        let numbered = env.trim_end_matches('*') == "enumerate";
        let abstract_id = if numbered {
            self.decimal_abstract
        } else {
            self.bullet_abstract
        };
        self.num_counter += 1;
        let num_id = NumId(self.num_counter);
        self.numbering.insert_num(Num {
            num_id,
            abstract_num_id: abstract_id,
            overrides: Vec::new(),
            location: SourceLocation::default(),
        });

        let body = self.collect_environment_body(env)?;
        let segments = split_top_level_tokens(&body, &TokenKind::ControlWord("item".to_owned()));
        let depth = self.list_depth;
        self.list_depth += 1;
        for (index, segment) in segments.into_iter().enumerate() {
            if index == 0 {
                if !segment.is_empty() {
                    let blocks = self.parse_tokens_as_blocks(segment)?;
                    for block in blocks {
                        self.push_block(block)?;
                    }
                }
                continue;
            }
            let mut segment = segment;
            let label = extract_leading_optional(&mut segment);
            let blocks = self.parse_tokens_as_blocks(segment)?;
            self.emit_list_item(num_id, depth, label, blocks)?;
        }
        self.list_depth -= 1;
        Ok(())
    }

    /// Creates the two abstract numbering definitions on first use.
    fn ensure_numbering(&mut self) {
        if self.numbering_ready {
            return;
        }
        self.numbering_ready = true;
        self.numbering.insert_abstract(AbstractNum {
            id: self.bullet_abstract,
            name: Some(Arc::from("LaTeX itemize")),
            multi_level_type: Some(Arc::from("hybridMultilevel")),
            num_style_link: None,
            style_link: None,
            levels: (0..9).map(bullet_level).collect(),
            location: SourceLocation::default(),
        });
        self.numbering.insert_abstract(AbstractNum {
            id: self.decimal_abstract,
            name: Some(Arc::from("LaTeX enumerate")),
            multi_level_type: Some(Arc::from("hybridMultilevel")),
            num_style_link: None,
            style_link: None,
            levels: (0..9).map(decimal_level).collect(),
            location: SourceLocation::default(),
        });
    }

    fn emit_list_item(
        &mut self,
        num_id: NumId,
        depth: usize,
        label: Option<String>,
        mut blocks: Vec<Block>,
    ) -> Result<(), LatexError> {
        let ilvl = Ilvl(u8::try_from(depth).unwrap_or(0).min(Ilvl::MAX));
        let numbering = NumPr {
            num_id: Some(num_id),
            ilvl: Some(ilvl),
        };
        match blocks.first_mut() {
            Some(Block::Paragraph(paragraph)) => {
                paragraph.props.numbering = Some(numbering);
                if let Some(label) = label {
                    // A `description` label replaces the level's own marker.
                    paragraph.inlines.insert(
                        0,
                        Inline::Run(Run {
                            props: RunProperties {
                                bold: TriState::On,
                                ..RunProperties::default()
                            },
                            content: vec![
                                RunContent::Text(TextNode {
                                    text: label,
                                    space: Space::Preserve,
                                }),
                                RunContent::Tab,
                            ],
                            revision: None,
                            location: SourceLocation::default(),
                        }),
                    );
                }
                for block in blocks {
                    self.push_block(block)?;
                }
            }
            _ => {
                self.push_paragraph(
                    ParagraphProperties {
                        numbering: Some(numbering),
                        ..ParagraphProperties::default()
                    },
                    Vec::new(),
                )?;
                for block in blocks {
                    self.push_block(block)?;
                }
            }
        }
        Ok(())
    }

    fn define_column_type(&mut self, span: LatexSpan) -> Result<(), LatexError> {
        let name = self.consume_braced_raw()?;
        let args = self
            .optional_bracket_text()?
            .map_or(0, |s| s.trim().parse::<usize>().unwrap_or(10));
        let old = self.expanding;
        self.expanding = false;
        let body = self.macro_argument();
        self.expanding = old;
        let body = body?;
        if name.chars().count() != 1 || args > 9 {
            self.loss(span, "table:invalid-column-type")?;
        } else {
            self.column_types.insert(
                name,
                crate::macros::Definition {
                    args,
                    default: None,
                    body,
                },
            );
        }
        Ok(())
    }

    fn parse_table(&mut self, env: &str, span: LatexSpan) -> Result<(), LatexError> {
        self.skip_optional_position()?;
        if env.trim_end_matches('*') == "tabularx" {
            self.consume_braced_raw()?;
            self.loss(span, "table:tabularx-target-width")?;
        }
        let raw_spec = self.consume_braced_raw()?;
        let (spec, unsupported) = crate::columns::expand(
            &raw_spec,
            &self.column_types,
            self.options.limits.nesting,
            self.options.limits.input_bytes,
        )?;
        for feature in unsupported {
            self.loss(span, &format!("table:column-spec:{feature}"))?;
        }
        let body = self.collect_environment_body(env)?;
        let max_columns = self.options.limits.table_columns;
        let max_cells = self
            .options
            .limits
            .table_cells
            .saturating_sub(self.table_cells);
        let table = {
            let mut render = |tokens: &[Token]| {
                self.parse_tokens_as_inlines(tokens.to_vec(), &Format::default())
            };
            crate::table::build(&spec, &body, max_columns, max_cells, &mut render)?.table
        };

        let cell_count: usize = table.rows.iter().map(|row| row.cells.len()).sum();
        self.table_cells += cell_count;
        if self.table_cells > self.options.limits.table_cells {
            return Err(LatexError::LimitExceeded {
                limit: "table_cells",
                observed: self.table_cells,
            });
        }
        self.table_rows += table.rows.len();
        if self.table_rows > self.options.limits.table_rows {
            return Err(LatexError::LimitExceeded {
                limit: "table_rows",
                observed: self.table_rows,
            });
        }
        self.info(span, "table:imported");
        self.check_nodes(1 + table.rows.len())?;
        self.blocks.push(Block::Table(table));
        Ok(())
    }

    // ----- math ----------------------------------------------------------

    fn math(&mut self, _fmt: &Format) -> Result<(), LatexError> {
        let display = matches!(self.peek_kind(), Some(TokenKind::MathShift));
        if display {
            self.next_token()?;
        }
        let mut tokens = Vec::new();
        let mut closed = false;
        let mut braces = 0usize;
        while let Some(token) = self.next_token()? {
            if braces == 0 && matches!(token.kind, TokenKind::MathShift) {
                closed = true;
                break;
            }
            match token.kind {
                TokenKind::LBrace => braces += 1,
                TokenKind::RBrace => braces = braces.saturating_sub(1),
                _ => {}
            }
            tokens.push(token);
        }
        if closed && display {
            if matches!(self.peek_kind(), Some(TokenKind::MathShift)) {
                self.next_token()?;
            } else {
                self.loss(self.span_here(), "math:missing-display-close")?;
            }
        }
        if !closed {
            self.loss(self.span_here(), "math:unterminated")?;
        }
        if self.render_math_graphic(&tokens, display, None)? {
            return Ok(());
        }
        let (expression, labels) = {
            let mut ctx = self.math_context();
            let expression = crate::math::translate(&tokens, &mut ctx)?;
            (expression, ctx.labels)
        };
        let marks = self.math_label_marks(labels);
        if display {
            self.flush_paragraph()?;
            let paragraph = Paragraph {
                props: display_paragraph_properties(),
                inlines: marks
                    .into_iter()
                    .chain([Inline::MathParagraph(display_paragraph(expression))])
                    .collect(),
                rsids: Default::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location: SourceLocation::default(),
            };
            self.push_block(Block::Paragraph(paragraph))
        } else {
            self.inlines.extend(marks);
            self.inlines.push(Inline::Math(expression));
            Ok(())
        }
    }
}

fn is_graphic_configuration(word: &str) -> bool {
    matches!(
        word,
        "tikzset" | "tikzstyle" | "pgfkeys" | "pgfplotsset"
            | "usetikzlibrary" | "usepgflibrary" | "usepgfplotslibrary"
    )
}

/// Explicit display skips survive DOCX serialization and separate adjacent
/// formula paragraphs as well as a formula from surrounding prose.
fn display_paragraph_properties() -> ParagraphProperties {
    ParagraphProperties {
        spacing: Some(Spacing {
            before: Some(Twips(240)),
            after: Some(Twips(240)),
            ..Spacing::default()
        }),
        ..ParagraphProperties::default()
    }
}

fn display_paragraph(expression: MathExpression) -> MathParagraph {
    MathParagraph {
        properties: Some(MathParagraphProperties {
            justification: Some(MathJustification::Center),
        }),
        equations: vec![expression],
        location: SourceLocation::default(),
    }
}

fn separator_paragraph(continuation: bool) -> Block {
    Block::Paragraph(Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Run(Run {
            props: RunProperties::default(),
            content: vec![RunContent::NoteSeparator(continuation)],
            revision: None,
            location: SourceLocation::default(),
        })],
        rsids: Default::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::default(),
    })
}

fn push_synthetic_name(out: &mut Vec<Token>, name: &str) {
    out.push(Token {
        kind: TokenKind::LBrace,
        start: 0,
        end: 0,
    });
    out.push(Token {
        kind: TokenKind::Text(name.to_owned()),
        start: 0,
        end: 0,
    });
    out.push(Token {
        kind: TokenKind::RBrace,
        start: 0,
        end: 0,
    });
}

fn split_top_level_tokens(tokens: &[Token], separator: &TokenKind) -> Vec<Vec<Token>> {
    let mut parts: Vec<Vec<Token>> = vec![Vec::new()];
    let mut braces = 0i32;
    let mut envs = 0i32;
    for token in tokens {
        match &token.kind {
            TokenKind::LBrace => braces += 1,
            TokenKind::RBrace => braces -= 1,
            TokenKind::ControlWord(word) if word == "begin" => envs += 1,
            TokenKind::ControlWord(word) if word == "end" => envs -= 1,
            _ => {}
        }
        if braces == 0 && envs == 0 && &token.kind == separator {
            parts.push(Vec::new());
        } else if let Some(current) = parts.last_mut() {
            current.push(token.clone());
        }
    }
    parts
}

fn extract_leading_optional(tokens: &mut Vec<Token>) -> Option<String> {
    let start = tokens
        .iter()
        .position(|token| !matches!(token.kind, TokenKind::Space))?;
    if !matches!(
        tokens.get(start).map(|t| &t.kind),
        Some(TokenKind::LBracket)
    ) {
        return None;
    }
    let mut depth = 0usize;
    let mut text = String::new();
    let mut end = start;
    for (offset, token) in tokens.iter().enumerate().skip(start) {
        match &token.kind {
            TokenKind::LBracket => {
                depth += 1;
                if depth > 1 {
                    text.push('[');
                }
            }
            TokenKind::RBracket => {
                depth -= 1;
                if depth == 0 {
                    end = offset + 1;
                    break;
                }
                text.push(']');
            }
            kind if depth >= 1 => text.push_str(&kind.raw()),
            _ => {}
        }
    }
    tokens.drain(start..end);
    Some(text)
}

pub(crate) fn text_symbol(word: &str) -> Option<&'static str> {
    Some(match word {
        "textunderscore" => "_",
        "ldots" | "dots" | "textellipsis" => "\u{2026}",
        "textbackslash" => "\\",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "amp" => "&",
        "textbar" => "|",
        "textless" => "<",
        "textgreater" => ">",
        "textemdash" => "\u{2014}",
        "textendash" => "\u{2013}",
        "textquotedblleft" => "\u{201C}",
        "textquotedblright" => "\u{201D}",
        "textquoteleft" => "\u{2018}",
        "textquoteright" => "\u{2019}",
        "copyright" => "\u{00A9}",
        "textregistered" => "\u{00AE}",
        "texttrademark" => "\u{2122}",
        "textdegree" => "\u{00B0}",
        "textbullet" => "\u{2022}",
        "dag" | "dagger" => "\u{2020}",
        "ddag" | "ddagger" => "\u{2021}",
        "pounds" => "\u{00A3}",
        "texteuro" => "\u{20AC}",
        "textdollar" => "$",
        "textcent" => "\u{00A2}",
        "textyen" => "\u{00A5}",
        "textsection" => "\u{00A7}",
        "textparagraph" => "\u{00B6}",
        "S" => "\u{00A7}",
        "P" => "\u{00B6}",
        "aa" => "\u{00E5}",
        "AA" => "\u{00C5}",
        "oe" => "\u{0153}",
        "OE" => "\u{0152}",
        "ae" => "\u{00E6}",
        "AE" => "\u{00C6}",
        "o" => "\u{00F8}",
        "O" => "\u{00D8}",
        "l" => "\u{0142}",
        "L" => "\u{0141}",
        "ss" => "\u{00DF}",
        "LaTeX" => "LaTeX",
        "TeX" => "TeX",
        "XeTeX" => "XeTeX",
        "BibTeX" => "BibTeX",
        _ => return None,
    })
}

/// The indentation of a list level.
fn level_indent(ilvl: usize) -> Indentation {
    let index = i32::try_from(ilvl).unwrap_or(0);
    Indentation {
        start: Some(Twips(720 * (index + 1))),
        hanging: Some(Twips(360)),
        ..Indentation::default()
    }
}

/// A bullet level of the itemize abstract definition.
fn bullet_level(ilvl: usize) -> Level {
    const GLYPHS: [&str; 3] = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
    let mut level = Level::new(Ilvl(u8::try_from(ilvl).unwrap_or(0)));
    level.start = Some(1);
    level.format = Some(Arc::from("bullet"));
    level.text = Some(Arc::from(GLYPHS[ilvl % 3]));
    level.justification = Some(Justification::Start);
    level.suffix = Some(Arc::from("tab"));
    level.paragraph = ParagraphProperties {
        indentation: Some(level_indent(ilvl)),
        ..ParagraphProperties::default()
    };
    level
}

/// A numbered level of the enumerate abstract definition.
fn decimal_level(ilvl: usize) -> Level {
    const FORMATS: [&str; 3] = ["decimal", "lowerLetter", "lowerRoman"];
    let mut level = Level::new(Ilvl(u8::try_from(ilvl).unwrap_or(0)));
    level.start = Some(1);
    level.format = Some(Arc::from(FORMATS[ilvl % 3]));
    level.text = Some(Arc::from(format!("%{}.", ilvl + 1)));
    level.justification = Some(Justification::Start);
    level.suffix = Some(Arc::from("tab"));
    level.paragraph = ParagraphProperties {
        indentation: Some(level_indent(ilvl)),
        ..ParagraphProperties::default()
    };
    level
}

/// Parses the `key=value` options of `\includegraphics`.
fn parse_image_options(text: &str) -> crate::media::ImageOptions {
    let mut options = crate::media::ImageOptions::default();
    for part in text.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "width" => options.width_emu = parse_length_emu(value),
            "height" => options.height_emu = parse_length_emu(value),
            "scale" => options.scale = value.parse::<f64>().ok(),
            "angle" => options.angle = value.parse::<f64>().ok(),
            "trim" => {
                let values: Vec<f64> = value
                    .split_whitespace()
                    .filter_map(|item| item.parse().ok())
                    .collect();
                if values.len() == 4 {
                    options.trim = Some((values[0], values[1], values[2], values[3]));
                }
            }
            _ => {}
        }
    }
    options
}

/// Parses a TeX length (for example `3cm`, `0.5\textwidth`, `40pt`) into EMU.
fn parse_length_emu(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    const TEXT_WIDTH_TWIPS: f64 = 9026.0;
    const EMU_PER_TWIP: f64 = 635.0;
    if trimmed.contains("\\textwidth")
        || trimmed.contains("\\linewidth")
        || trimmed.contains("\\columnwidth")
    {
        let factor = trimmed
            .split('\\')
            .next()
            .and_then(|value| value.trim().parse::<f64>().ok())
            .unwrap_or(1.0);
        return Some((TEXT_WIDTH_TWIPS * factor * EMU_PER_TWIP) as i64);
    }
    for (unit, emu) in [
        ("cm", 360_000.0),
        ("mm", 36_000.0),
        ("in", 914_400.0),
        ("pt", 12_700.0),
        ("bp", 12_700.0),
        ("pc", 152_400.0),
        ("em", 152_400.0),
        ("ex", 76_200.0),
    ] {
        if let Some(number) = trimmed.strip_suffix(unit) {
            if let Ok(value) = number.trim().parse::<f64>() {
                return Some((value * emu) as i64);
            }
        }
    }
    None
}

/// Maximum search directories honoured from one `\graphicspath`. Each directory
/// multiplies the names tried per figure, so this bounds both the candidate list
/// and the number of provider reads a single `\includegraphics` can cause.
const MAX_GRAPHICS_PATHS: usize = 64;

/// Parses `\graphicspath{{a/}{b/}}` into directory prefixes, truncated to
/// [`MAX_GRAPHICS_PATHS`]. The flag is `true` when more were declared.
fn parse_graphics_paths(text: &str) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut truncated = false;
    let mut chars = text.trim().chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '{' {
            chars.next();
            let mut group = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                group.push(inner);
            }
            let group = group.trim().to_owned();
            if !group.is_empty() {
                if paths.len() >= MAX_GRAPHICS_PATHS {
                    truncated = true;
                } else {
                    paths.push(group);
                }
            }
        } else {
            chars.next();
        }
    }
    if paths.is_empty() {
        for part in text.split_whitespace() {
            if paths.len() >= MAX_GRAPHICS_PATHS {
                truncated = true;
                break;
            }
            paths.push(part.to_owned());
        }
    }
    (paths, truncated)
}

/// Builds a footnote or endnote reference run.
fn note_reference(fmt: &Format, endnote: bool, id: i32) -> Inline {
    let content = if endnote {
        RunContent::EndnoteRef(id.cast_unsigned())
    } else {
        RunContent::FootnoteRef(id.cast_unsigned())
    };
    Inline::Run(Run {
        props: fmt.to_props(),
        content: vec![content],
        revision: None,
        location: SourceLocation::default(),
    })
}

/// Sanitizes a LaTeX key into a Word bookmark name (letters, digits, `_`).
fn bookmark_name(key: &str) -> String {
    let mut name = String::with_capacity(key.len());
    for ch in key.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            name.push(ch);
        } else {
            name.push('_');
        }
    }
    if name.is_empty() {
        name.push('_');
    }
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        name.insert(0, '_');
    }
    // Cap the name, but only on a character boundary: a key of multi-byte
    // alphanumerics can have its 40th byte inside a character, and `truncate`
    // panics there (the module promises never to panic on hostile input).
    while name.len() > 40 {
        name.pop();
    }
    name
}

/// The combining mark a LaTeX accent control symbol stands for.
fn accent_mark(symbol: char) -> char {
    match symbol {
        '\'' => '\u{0301}',
        '`' => '\u{0300}',
        '^' => '\u{0302}',
        '"' => '\u{0308}',
        '~' => '\u{0303}',
        '=' => '\u{0304}',
        '.' => '\u{0307}',
        'u' => '\u{0306}',
        'v' => '\u{030C}',
        'H' => '\u{030B}',
        'c' => '\u{0327}',
        'd' => '\u{0323}',
        'b' => '\u{0331}',
        'r' => '\u{030A}',
        'k' => '\u{0328}',
        _ => '\u{0361}',
    }
}

/// Parses a point size like `12` or `10.5` into half-points.
fn parse_points(text: &str) -> Option<i32> {
    let value: f64 = text.trim().parse().ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some((value * 2.0).round() as i32)
}

/// A small table of the common `xcolor`-style named colours, as hex triplets.
fn named_color(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => "000000",
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "00FF00",
        "blue" => "0000FF",
        "cyan" => "00FFFF",
        "magenta" => "FF00FF",
        "yellow" => "FFFF00",
        "gray" | "grey" => "808080",
        "darkgray" | "darkgrey" => "404040",
        "lightgray" | "lightgrey" => "C0C0C0",
        "brown" => "A52A2A",
        "lime" => "00FF00",
        "olive" => "808000",
        "orange" => "FFA500",
        "pink" => "FFC0CB",
        "purple" => "800080",
        "teal" => "008080",
        "violet" => "EE82EE",
        "navy" => "000080",
        "maroon" => "800000",
        _ => return None,
    })
}

fn channel_unit(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rgb_hex(r: f64, g: f64, b: f64) -> String {
    format!(
        "{:02X}{:02X}{:02X}",
        channel_unit(r),
        channel_unit(g),
        channel_unit(b)
    )
}

/// Resolves a `\definecolor` model and specification to a hex triplet.
fn color_from_spec(model: &str, spec: &str) -> Option<String> {
    match model.to_ascii_lowercase().as_str() {
        "named" => named_color(spec.trim()).map(str::to_owned),
        "html" => {
            let hex = spec.trim().trim_start_matches('#');
            (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| hex.to_ascii_uppercase())
        }
        "rgb" => {
            let parts: Vec<f64> = spec
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            (parts.len() == 3).then(|| rgb_hex(parts[0], parts[1], parts[2]))
        }
        "rgb255" | "rgb0-255" => {
            let parts: Vec<u8> = spec
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            (parts.len() == 3).then(|| format!("{:02X}{:02X}{:02X}", parts[0], parts[1], parts[2]))
        }
        "gray" | "grey" => {
            let g: f64 = spec.trim().parse().ok()?;
            Some(rgb_hex(g, g, g))
        }
        "cmyk" => {
            let parts: Vec<f64> = spec
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            if parts.len() != 4 {
                return None;
            }
            let (c, m, y, k) = (parts[0], parts[1], parts[2], parts[3]);
            Some(rgb_hex(
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
            ))
        }
        _ => None,
    }
}

fn rendered_image_options(
    bytes: &[u8],
    mut options: crate::media::ImageOptions,
) -> crate::media::ImageOptions {
    if options.width_emu.is_none() && options.height_emu.is_none() {
        if let Some((w, h)) = media::rendered_png_extent(bytes) {
            options.width_emu = Some(w);
            options.height_emu = Some(h);
        }
    }
    options
}

fn primitive_conditional(word: &str) -> bool {
    matches!(
        word,
        "if" | "ifx"
            | "ifnum"
            | "ifdim"
            | "ifcase"
            | "ifdefined"
            | "ifcsname"
            | "ifmmode"
            | "ifhmode"
            | "ifvmode"
            | "ifcat"
            | "ifodd"
            | "ifhbox"
            | "ifvbox"
            | "ifeof"
            | "ifinner"
    )
}

fn raw_tokens(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        out.push_str(&token.kind.raw());
        if matches!(token.kind, TokenKind::ControlWord(_)) {
            out.push(' ');
        }
    }
    out
}

fn graphic_preamble(tokens: &[Token]) -> String {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < tokens.len() {
        if matches!(&tokens[pos].kind, TokenKind::ControlWord(w) if w == "begin") {
            let tail: Vec<_> = tokens[pos + 1..]
                .iter()
                .filter(|t| !matches!(t.kind, TokenKind::Space))
                .take(3)
                .collect();
            if matches!(tail.as_slice(), [Token { kind: TokenKind::LBrace, .. }, Token { kind: TokenKind::Text(w), .. }, Token { kind: TokenKind::RBrace, .. }] if w == "document")
            {
                break;
            }
        }
        if matches!(&tokens[pos].kind, TokenKind::ControlWord(w) if w == "documentclass" || w == "documentstyle")
        {
            pos += 1;
            while pos < tokens.len() {
                if matches!(tokens[pos].kind, TokenKind::LBracket | TokenKind::LBrace) {
                    let bracket = matches!(tokens[pos].kind, TokenKind::LBracket);
                    let mut depth = 1;
                    pos += 1;
                    while pos < tokens.len() && depth > 0 {
                        match tokens[pos].kind {
                            TokenKind::LBracket if bracket => depth += 1,
                            TokenKind::RBracket if bracket => depth -= 1,
                            TokenKind::LBrace if !bracket => depth += 1,
                            TokenKind::RBrace if !bracket => depth -= 1,
                            _ => {}
                        }
                        pos += 1;
                    }
                    if !bracket {
                        break;
                    }
                } else if matches!(tokens[pos].kind, TokenKind::Space) {
                    pos += 1;
                } else {
                    break;
                }
            }
        } else {
            out.push(tokens[pos].clone());
            pos += 1;
        }
    }
    // A fragment without a document class has no preamble; copying its body
    // here would execute/print it twice inside the external renderer.
    if starts_with_document_class(tokens) {
        raw_tokens(&out)
    } else {
        String::new()
    }
}

fn plain_inlines(items: &[Inline]) -> String {
    items
        .iter()
        .filter_map(|i| {
            if let Inline::Run(run) = i {
                Some(run)
            } else {
                None
            }
        })
        .flat_map(|r| r.content.iter())
        .filter_map(|c| {
            if let RunContent::Text(t) = c {
                Some(t.text.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn decode(bytes: &[u8], name: &str) -> Result<String, LatexError> {
    String::from_utf8(bytes.to_vec()).map_err(|error| LatexError::InvalidEncoding {
        offset: error.utf8_error().valid_up_to(),
        file: name.to_owned(),
    })
}

fn default_section() -> Section {
    Section {
        properties: SectionProperties {
            page_size: Some(PageSize {
                width: Some(Twips(11906)),
                height: Some(Twips(16838)),
                orientation: Some(PageOrientation::Portrait),
                code: None,
            }),
            page_margins: Some(PageMargins {
                top: Some(Twips(1440)),
                bottom: Some(Twips(1440)),
                left: Some(Twips(1440)),
                right: Some(Twips(1440)),
                // The generated PAGE footer needs an explicit edge distance:
                // absent margins are serialized as zero in Strict w:pgMar.
                footer: Some(Twips(720)),
                ..PageMargins::default()
            }),
            ..SectionProperties::default()
        },
        location: SourceLocation::default(),
    }
}

impl Parser<'_> {
    fn finish(self) -> Result<LatexImported, LatexError> {
        let Parser {
            mut sections,
            blocks,
            footnotes,
            document_footnote,
            endnotes,
            document_endnote,
            media,
            media_index,
            numbering,
            styles,
            columns,
            options,
            builder,
            token_count,
            text_scalars,
            node_count,
            included_files,
            ..
        } = self;
        if sections.is_empty() {
            sections.push(default_section());
        }
        let mut headers_footers = Vec::new();
        if let Some(section) = sections.last_mut() {
            if let Some(count) = columns {
                section.properties.columns = Some(Columns {
                    count: Some(count),
                    equal_width: true,
                    ..Columns::default()
                });
            }
            if options.page_numbers {
                let footer = page_number_footer();
                let part = footer.part.clone();
                headers_footers.push(footer);
                section.properties.footers.push(HeaderFooterRef {
                    kind: HeaderFooterKind::Default,
                    rel_id: RelId::new("rIdFooter"),
                    part: Some(part),
                });
            }
        }
        let report = builder.finish(token_count, text_scalars, node_count, included_files);
        let footnotes_part = (!footnotes.is_empty()).then(|| PartId::new("/word/footnotes.xml"));
        let endnotes_part = (!endnotes.is_empty()).then(|| PartId::new("/word/endnotes.xml"));
        let styles_part = (!styles.is_empty()).then(|| PartId::new("/word/styles.xml"));
        let document = Document {
            body: Body { blocks },
            styles,
            numbering,
            footnotes,
            endnotes,
            comments: Vec::new(),
            settings: Settings {
                footnote_properties: document_footnote,
                endnote_properties: document_endnote,
                default_style: Some(StyleId::new("Normal")),
                ..Settings::default()
            },
            theme: None,
            font_table: None,
            sections,
            headers_footers,
            media: media_index,
            support: Default::default(),
            source: DocumentSource {
                main_document: PartId::new("/word/document.xml"),
                styles: styles_part,
                numbering: None,
                settings: None,
                font_table: None,
                footnotes: footnotes_part,
                endnotes: endnotes_part,
                comments: None,
                theme: None,
            },
        };
        Ok(LatexImported {
            document,
            media,
            report,
        })
    }
}

/// The default paragraph and heading styles every imported document carries.
fn build_styles() -> StyleTable {
    let mut table = StyleTable::new();
    table.insert(style(
        "Normal",
        "Normal",
        None,
        true,
        ParagraphProperties::default(),
        RunProperties::default(),
    ));
    for level in 0..4u8 {
        let name = format!("Heading{}", usize::from(level) + 1);
        let paragraph = ParagraphProperties {
            outline_level: Some(level),
            keep_next: TriState::On,
            spacing: Some(Spacing {
                before: Some(Twips(240)),
                after: Some(Twips(120)),
                ..Spacing::default()
            }),
            ..ParagraphProperties::default()
        };
        let run = RunProperties {
            bold: TriState::On,
            size: Some(HalfPoints(heading_size(level))),
            size_cs: Some(HalfPoints(heading_size(level))),
            ..RunProperties::default()
        };
        table.insert(style(
            &name,
            &format!("heading {}", level + 1),
            Some("Normal"),
            false,
            paragraph,
            run,
        ));
    }
    table
}

/// Builds one paragraph style definition.
fn style(
    id: &str,
    name: &str,
    based_on: Option<&str>,
    is_default: bool,
    paragraph: ParagraphProperties,
    run: RunProperties,
) -> Style {
    Style {
        id: StyleId::new(id),
        style_type: StyleType::Paragraph,
        name: Some(Arc::from(name)),
        based_on: based_on.map(StyleId::new),
        next: Some(StyleId::new("Normal")),
        aliases: None,
        link: None,
        is_default,
        custom_style: false,
        auto_redefine: false,
        semi_hidden: false,
        hidden: false,
        q_format: false,
        locked: false,
        unhide_when_used: false,
        ui_priority: None,
        table: Default::default(),
        row: Default::default(),
        cell: Default::default(),
        paragraph,
        run,
        conditions: Vec::new(),
        based_on_chain: Vec::new(),
        location: SourceLocation::default(),
    }
}

/// A centered footer holding a `PAGE` field.
fn page_number_footer() -> HeaderFooter {
    let paragraph = Paragraph {
        props: ParagraphProperties {
            alignment: Some(Justification::Center),
            ..ParagraphProperties::default()
        },
        inlines: vec![Inline::Field(Field {
            instruction: Some(Arc::from(" PAGE \\* MERGEFORMAT ")),
            inlines: vec![Inline::Run(Run {
                props: RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: "1".to_owned(),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::default(),
            })],
            location: SourceLocation::default(),
        })],
        rsids: Default::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::default(),
    };
    HeaderFooter {
        part: PartId::new("/word/footer1.xml"),
        is_header: false,
        blocks: vec![Block::Paragraph(paragraph)],
        location: SourceLocation::default(),
    }
}

/// Imports LaTeX bytes with no external source; `\input`/`\include` are losses.
pub(crate) fn import_bytes(
    bytes: &[u8],
    options: &LatexOptions,
) -> Result<LatexImported, LatexError> {
    let source = crate::source::MemorySource::new();
    import_entry("<input>", bytes, &source, options)
}

pub(crate) fn import_entry(
    name: &str,
    bytes: &[u8],
    provider: &dyn SourceProvider,
    options: &LatexOptions,
) -> Result<LatexImported, LatexError> {
    if bytes.len() > options.limits.input_bytes {
        return Err(LatexError::LimitExceeded {
            limit: "input_bytes",
            observed: bytes.len(),
        });
    }
    let mut parser = Parser::new(name, bytes, provider, options)?;
    parser.run()?;
    parser.finish()
}
