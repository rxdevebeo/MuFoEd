//! Column-type substitution, keeping declaration groups opaque to the scanner.
use crate::{
    lexer::lex,
    macros::{substitute, Definition},
    LatexError,
};
use std::collections::BTreeMap;

pub(crate) fn expand(
    spec: &str,
    types: &BTreeMap<String, Definition>,
    depth: usize,
    budget: usize,
) -> Result<(String, Vec<String>), LatexError> {
    if depth == 0 {
        return Err(LatexError::LimitExceeded {
            limit: "column_type_depth",
            observed: 1,
        });
    }
    let mut chars = spec.chars().peekable();
    let mut out = String::new();
    let mut losses = Vec::new();
    while let Some(c) = chars.next() {
        if let Some(def) = types.get(&c.to_string()) {
            let mut args = Vec::new();
            for _ in 0..def.args {
                args.push(lex(&group(&mut chars).unwrap_or_default()));
            }
            let tokens = substitute(&def.body, &args, budget)?;
            let replacement = tokens.iter().map(|t| t.kind.raw()).collect::<String>();
            let (expanded, unsupported) = expand(
                &replacement,
                types,
                depth - 1,
                budget.saturating_sub(out.len()),
            )?;
            out.push_str(&expanded);
            losses.extend(unsupported);
        } else {
            out.push(c);
            match c {
                '>' | '<' | '@' | '!' | 'p' | 'm' | 'b' => {
                    if let Some(value) = group(&mut chars) {
                        out.push('{');
                        out.push_str(&value);
                        out.push('}');
                    } else {
                        losses.push("missing-group".to_owned());
                    }
                    if matches!(c, '@' | '!') {
                        losses.push("intercolumn-material".to_owned());
                    }
                }
                '*' => {
                    let count = group(&mut chars).unwrap_or_default();
                    let inner = group(&mut chars).unwrap_or_default();
                    let (inner, unsupported) = expand(&inner, types, depth - 1, budget)?;
                    out.push('{');
                    out.push_str(&count);
                    out.push_str("}{");
                    out.push_str(&inner);
                    out.push('}');
                    losses.extend(unsupported);
                }
                'l' | 'c' | 'r' | 'X' | '|' => {}
                c if c.is_whitespace() => {}
                _ => {
                    losses.push(format!("unknown-{c}"));
                }
            }
        }
        if out.len() > budget {
            return Err(LatexError::LimitExceeded {
                limit: "column_spec_bytes",
                observed: out.len(),
            });
        }
    }
    Ok((out, losses))
}

fn group(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
    if chars.next()? != '{' {
        return None;
    }
    let mut depth = 1;
    let mut out = String::new();
    for c in chars.by_ref() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
            }
            _ => {}
        }
        out.push(c);
    }
    None
}
