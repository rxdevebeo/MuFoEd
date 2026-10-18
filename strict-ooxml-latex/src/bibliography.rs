//! BibTeX data reader for the importer's explicitly normalized bibliography.
//! No .bst code, TeX primitives or external processes are executed.
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Bibliography {
    pub(crate) items: Vec<(String, BTreeMap<String, String>)>,
    pub(crate) unsupported: Vec<String>,
}

pub(crate) fn read(text: &str) -> Bibliography {
    let mut chars = text.chars().peekable();
    let mut out = Bibliography::default();
    while let Some(c) = chars.next() {
        if c != '@' {
            continue;
        }
        let mut kind = String::new();
        while chars.peek().is_some_and(|c| c.is_alphabetic()) {
            kind.push(chars.next().unwrap());
        }
        whitespace(&mut chars);
        let Some(open @ ('{' | '(')) = chars.next() else {
            out.unsupported.push("entry-header".into());
            continue;
        };
        let close = if open == '{' { '}' } else { ')' };
        let mut depth = 1;
        let mut braces = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        let mut body = String::new();
        for c in chars.by_ref() {
            if !escaped {
                if c == '"' && ((open == '{' && depth == 1) || (open == '(' && braces == 0)) {
                    quoted = !quoted;
                }
                if open == '(' {
                    if c == '{' {
                        braces += 1;
                    }
                    if c == '}' {
                        braces = braces.saturating_sub(1);
                    }
                }
                if (!quoted || (open == '{' && (depth > 1 || c == '{'))) && braces == 0 {
                    if c == open {
                        depth += 1;
                    }
                    if c == close {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
            }
            body.push(c);
            escaped = c == '\\' && !escaped;
        }
        if depth != 0 {
            out.unsupported.push("unterminated-entry".into());
        }
        if kind.eq_ignore_ascii_case("comment") {
            continue;
        }
        if kind.eq_ignore_ascii_case("preamble") {
            out.unsupported.push("preamble".into());
            continue;
        }
        if kind.eq_ignore_ascii_case("string") {
            out.unsupported.push("string-definition".into());
            continue;
        }
        let Some((key, rest)) = body.split_once(',') else {
            out.unsupported.push("entry-key".into());
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.contains(['{', '}', '\\', '%']) {
            out.unsupported.push("invalid-key".into());
            continue;
        }
        let mut fields = BTreeMap::new();
        let mut chars = rest.chars().peekable();
        loop {
            while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
                chars.next();
            }
            if chars.peek().is_none() {
                break;
            }
            let mut name = String::new();
            while chars
                .peek()
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                name.push(chars.next().unwrap());
            }
            whitespace(&mut chars);
            if chars.next() != Some('=') {
                out.unsupported.push("field-syntax".into());
                break;
            }
            whitespace(&mut chars);
            let Some(first) = chars.next() else {
                break;
            };
            let mut value = String::new();
            if matches!(first, '{' | '"') {
                let mut braces = usize::from(first == '{');
                let mut escaped = false;
                let mut closed = false;
                for c in chars.by_ref() {
                    if !escaped {
                        if first == '"' && c == '"' && braces == 0 {
                            closed = true;
                            break;
                        }
                        if c == '{' {
                            braces += 1;
                        }
                        if c == '}' {
                            braces = braces.saturating_sub(1);
                            if first == '{' && braces == 0 {
                                closed = true;
                                break;
                            }
                        }
                    }
                    value.push(c);
                    escaped = c == '\\' && !escaped;
                }
                if !closed {
                    out.unsupported.push("unterminated-field".into());
                }
            } else {
                value.push(first);
                while chars.peek().is_some_and(|c| *c != ',') {
                    value.push(chars.next().unwrap());
                }
                if !value.trim().chars().all(|c| c.is_ascii_digit()) {
                    out.unsupported.push("string-expression".into());
                    value.clear();
                }
            }
            fields.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        }
        out.items.push((key.to_owned(), fields));
    }
    out
}

fn whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

pub(crate) fn normalized(items: &[(String, BTreeMap<String, String>)]) -> String {
    let mut out = String::from(r"\begin{thebibliography}{99}");
    for (key, fields) in items {
        out.push_str(r"\bibitem{");
        out.push_str(key);
        out.push('}');
        for name in [
            "author",
            "title",
            "booktitle",
            "journal",
            "publisher",
            "volume",
            "number",
            "pages",
            "year",
            "doi",
            "url",
            "note",
        ] {
            if let Some(value) = fields.get(name).filter(|s| !s.is_empty()) {
                if matches!(name, "doi" | "url") {
                    out.push_str(r"\url{");
                    out.push_str(value);
                    out.push('}');
                } else {
                    out.push_str(value);
                }
                out.push_str(". ");
            }
        }
    }
    out.push_str(r"\end{thebibliography}");
    out
}
