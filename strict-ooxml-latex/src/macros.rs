//! Bounded, token-based substitution. This is deliberately not a TeX interpreter.
use std::collections::BTreeMap;

use crate::lexer::{lex, Token, TokenKind};
use crate::LatexError;

#[derive(Clone)]
pub(crate) struct Definition {
    pub args: usize,
    pub default: Option<Vec<Token>>,
    pub body: Vec<Token>,
}

#[derive(Default)]
pub(crate) struct Macros {
    pub definitions: BTreeMap<String, std::sync::Arc<Definition>>,
    scopes: Vec<BTreeMap<String, std::sync::Arc<Definition>>>,
    pub(crate) booleans: BTreeMap<String, bool>,
    boolean_scopes: Vec<BTreeMap<String, bool>>,
}

impl Macros {
    pub(crate) fn enter(&mut self) {
        self.scopes.push(self.definitions.clone());
        self.boolean_scopes.push(self.booleans.clone());
    }

    pub(crate) fn leave(&mut self) {
        if let Some(saved) = self.boolean_scopes.pop() {
            self.booleans = saved;
        }
        if let Some(saved) = self.scopes.pop() {
            self.definitions = saved;
        }
    }

    pub(crate) fn define(&mut self, name: String, def: Definition, global: bool) {
        let def = std::sync::Arc::new(def);
        if global {
            for saved in &mut self.scopes {
                saved.insert(name.clone(), def.clone());
            }
        }
        self.definitions.insert(name, def);
    }
}

/// Substitute only parameter tokens: escaped hashes and argument boundaries
/// survive, and inserted arguments are never re-lexed as source code.
pub(crate) fn substitute(
    body: &[Token],
    args: &[Vec<Token>],
    budget: usize,
) -> Result<Vec<Token>, LatexError> {
    let mut out = Vec::new();
    for token in body {
        if let TokenKind::Text(text) = &token.kind {
            let mut chars = text.chars().peekable();
            let mut literal = String::new();
            while let Some(c) = chars.next() {
                if c == '#' {
                    if let Some(d) = chars.peek().copied().and_then(|c| c.to_digit(10)) {
                        if d > 0 && d as usize <= args.len() {
                            chars.next();
                            append_literal(&mut out, &mut literal, token);
                            let arg = &args[d as usize - 1];
                            if out.len().saturating_add(arg.len()) > budget {
                                return Err(limit());
                            }
                            out.extend(arg.iter().cloned());
                            continue;
                        }
                    } else if chars.peek() == Some(&'#') {
                        chars.next();
                    }
                }
                literal.push(c);
            }
            append_literal(&mut out, &mut literal, token);
        } else {
            out.push(token.clone());
        }
        if out.len() > budget {
            return Err(limit());
        }
    }
    Ok(out)
}

fn append_literal(out: &mut Vec<Token>, text: &mut String, origin: &Token) {
    if !text.is_empty() {
        // A literal piece of a Text token cannot contain a control sequence.
        out.push(Token {
            kind: TokenKind::Text(std::mem::take(text)),
            ..origin.clone()
        });
    }
}

fn limit() -> LatexError {
    LatexError::LimitExceeded {
        limit: "macro_expansion_tokens",
        observed: usize::MAX,
    }
}

pub(crate) fn tokens(text: &str) -> Vec<Token> {
    lex(text)
}
