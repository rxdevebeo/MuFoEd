//! Resolve field caches only from known counters or auxiliary data. Page numbers
//! are never guessed from the importer's unrelated pagination.
use std::collections::BTreeMap;
use strict_ooxml_wml::model::{Block, Inline, RunContent};

#[derive(Clone, Default)]
pub(crate) struct Value {
    pub(crate) number: String,
    pub(crate) title: String,
    pub(crate) page: String,
}

pub(crate) fn resolve(blocks: &mut [Block], labels: &BTreeMap<String, Value>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => inlines(&mut p.inlines, labels),
            Block::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        resolve(&mut cell.blocks, labels);
                    }
                }
            }
            _ => {}
        }
    }
}

fn inlines(items: &mut [Inline], labels: &BTreeMap<String, Value>) {
    for item in items {
        match item {
            Inline::Field(field) => {
                let instruction = field.instruction.as_deref().unwrap_or("");
                let mut words = instruction.split_whitespace();
                let kind = words.next().unwrap_or("");
                let name = words.next().unwrap_or("");
                if matches!(kind, "REF" | "PAGEREF") {
                    let value = labels.get(name);
                    let text = value
                        .map(|v| {
                            if kind == "PAGEREF" {
                                &v.page
                            } else if instruction.contains(r"\n") {
                                &v.title
                            } else {
                                &v.number
                            }
                        })
                        .filter(|s| !s.is_empty())
                        .map_or("??", String::as_str);
                    for item in &mut field.inlines {
                        if let Inline::Run(run) = item {
                            for content in &mut run.content {
                                if let RunContent::Text(node) = content {
                                    node.text = if node.text.starts_with('(') {
                                        format!("({text})")
                                    } else {
                                        text.to_owned()
                                    };
                                }
                            }
                        }
                    }
                } else {
                    inlines(&mut field.inlines, labels);
                }
            }
            Inline::Hyperlink(link) => inlines(&mut link.inlines, labels),
            _ => {}
        }
    }
}

/// Balanced groups in .aux are data, never executable input.
pub(crate) fn aux(text: &str) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    let tokens = crate::lexer::lex(text);
    let mut pos = 0;
    while pos < tokens.len() {
        let command = match &tokens[pos].kind {
            crate::lexer::TokenKind::ControlWord(s) => s.as_str(),
            _ => "",
        };
        pos += 1;
        if command == "newlabel" {
            if let (Some(key), Some(data)) = (group(&tokens, &mut pos), group(&tokens, &mut pos)) {
                let parts = crate::lexer::lex(&data);
                let mut cursor = 0;
                let number = group(&parts, &mut cursor).unwrap_or_default();
                let page = group(&parts, &mut cursor).unwrap_or_default();
                let title = group(&parts, &mut cursor).unwrap_or_default();
                // A computed TeX value needs an engine; don't print its code.
                let plain = |s: String| {
                    if s.contains(['\\', '{', '}', '#']) {
                        String::new()
                    } else {
                        s
                    }
                };
                result.insert(
                    key,
                    Value {
                        number: plain(number),
                        page: plain(page),
                        title: plain(title),
                    },
                );
            }
        } else if command == "bibcite" {
            if let (Some(key), Some(number)) = (group(&tokens, &mut pos), group(&tokens, &mut pos))
            {
                if !number.contains(['\\', '{', '}']) {
                    result.insert(
                        format!("__bib_{key}"),
                        Value {
                            number,
                            ..Value::default()
                        },
                    );
                }
            }
        }
    }
    result
}

fn group(tokens: &[crate::lexer::Token], pos: &mut usize) -> Option<String> {
    use crate::lexer::TokenKind;
    while tokens
        .get(*pos)
        .is_some_and(|t| matches!(t.kind, TokenKind::Space | TokenKind::ParBreak))
    {
        *pos += 1;
    }
    if !matches!(tokens.get(*pos)?.kind, TokenKind::LBrace) {
        return None;
    }
    *pos += 1;
    let mut depth = 1;
    let mut out = String::new();
    while let Some(t) = tokens.get(*pos) {
        *pos += 1;
        match t.kind {
            TokenKind::LBrace => depth += 1,
            TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
            }
            _ => {}
        }
        out.push_str(&t.kind.raw());
    }
    None
}
