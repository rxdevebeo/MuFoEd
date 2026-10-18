//! LaTeX tokenizer.
//!
//! The lexer turns one decoded UTF-8 source into a flat token stream. It knows
//! only lexical structure (control words, groups, math shifts, comments and
//! blank-line paragraph breaks); grammar lives in the parser. Verbatim content
//! (`\verb` and the verbatim environments) is captured raw so a `%` or `\` in it
//! is never mistaken for a comment or a control sequence.

/// One lexical token and the source byte range it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// Token kind.
    pub kind: TokenKind,
    /// Half-open byte range in the originating file.
    pub start: usize,
    /// Half-open byte range end.
    pub end: usize,
}

/// The lexical categories the parser distinguishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// A control word such as `\section`; a trailing `*` is kept.
    ControlWord(String),
    /// A control symbol such as `\%` or `\\`, holding the escaped character.
    ControlSymbol(char),
    /// `{`.
    LBrace,
    /// `}`.
    RBrace,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `$`.
    MathShift,
    /// `&`.
    Ampersand,
    /// `~`.
    Tilde,
    /// `_`.
    Sub,
    /// `^`.
    Sup,
    /// Literal text.
    Text(String),
    /// A single whitespace run that is not a paragraph break.
    Space,
    /// A blank line (two or more line endings): a paragraph boundary.
    ParBreak,
    /// Verbatim content between `\verb` delimiters or inside a verbatim
    /// environment, with every character preserved.
    Raw(String),
}

impl TokenKind {
    /// Returns the source spelling of this token, for raw reconstruction.
    pub(crate) fn raw(&self) -> String {
        match self {
            Self::ControlWord(word) => format!("\\{word}"),
            Self::ControlSymbol(c) => format!("\\{c}"),
            Self::LBrace => "{".to_owned(),
            Self::RBrace => "}".to_owned(),
            Self::LBracket => "[".to_owned(),
            Self::RBracket => "]".to_owned(),
            Self::MathShift => "$".to_owned(),
            Self::Ampersand => "&".to_owned(),
            Self::Tilde => "~".to_owned(),
            Self::Sub => "_".to_owned(),
            Self::Sup => "^".to_owned(),
            Self::Text(text) => text.clone(),
            Self::Space => " ".to_owned(),
            Self::ParBreak => "\n\n".to_owned(),
            Self::Raw(text) => text.clone(),
        }
    }
}

/// Environments whose body is captured verbatim.
const VERBATIM_ENVS: &[&str] = &[
    "verbatim",
    "lstlisting",
    "Verbatim",
    "filecontents",
    "minted",
];

fn is_special(c: char) -> bool {
    matches!(
        c,
        '\\' | '{' | '}' | '[' | ']' | '$' | '&' | '~' | '_' | '^' | '%'
    )
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

/// Tokenizes a decoded LaTeX source.
#[must_use]
pub(crate) fn lex(input: &str) -> Vec<Token> {
    lex_with_at_letter(input, false)
}

pub(crate) fn lex_with_at_letter(input: &str, mut at_letter: bool) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut iter = input.char_indices().peekable();
    while let Some((start, c)) = iter.next() {
        match c {
            '%' => {
                // A comment runs to the end of the line; the line ending itself
                // is part of the comment (it does not create a paragraph break).
                while let Some(&(_, ch)) = iter.peek() {
                    iter.next();
                    if ch == '\n' {
                        break;
                    }
                }
            }
            '\\' => {
                if let Some((consumed, body)) = peek_verb(&iter) {
                    for _ in 0..consumed {
                        iter.next();
                    }
                    push(&mut tokens, TokenKind::Raw(body), start, start + 1);
                } else if let Some((consumed, name)) = peek_verbatim_env(&iter) {
                    for _ in 0..consumed {
                        iter.next();
                    }
                    lex_verbatim_body(&mut tokens, start, &name, &mut iter);
                } else {
                    lex_control(&mut tokens, start, &mut iter, at_letter);
                    if let Some(Token {
                        kind: TokenKind::ControlWord(word),
                        ..
                    }) = tokens.last()
                    {
                        if word == "makeatletter" {
                            at_letter = true;
                        }
                        if word == "makeatother" {
                            at_letter = false;
                        }
                    }
                }
            }
            '{' => push(&mut tokens, TokenKind::LBrace, start, start + 1),
            '}' => push(&mut tokens, TokenKind::RBrace, start, start + 1),
            '[' => push(&mut tokens, TokenKind::LBracket, start, start + 1),
            ']' => push(&mut tokens, TokenKind::RBracket, start, start + 1),
            '$' => push(&mut tokens, TokenKind::MathShift, start, start + 1),
            '&' => push(&mut tokens, TokenKind::Ampersand, start, start + 1),
            '~' => push(&mut tokens, TokenKind::Tilde, start, start + 1),
            '_' => push(&mut tokens, TokenKind::Sub, start, start + 1),
            '^' => push(&mut tokens, TokenKind::Sup, start, start + 1),
            c if c.is_whitespace() => lex_whitespace(&mut tokens, start, c, &mut iter),
            _ => lex_text(&mut tokens, start, c, &mut iter),
        }
    }
    tokens
}

fn push(tokens: &mut Vec<Token>, kind: TokenKind, start: usize, end: usize) {
    tokens.push(Token { kind, start, end });
}

/// Recognizes `\verb`, `\lstinline` or `\mintinline`, returning the character
/// count to consume and the verbatim body.
fn peek_verb(iter: &Chars<'_>) -> Option<(usize, String)> {
    let mut probe = iter.clone();
    let mut consumed = 0usize;
    let mut word = String::new();
    while let Some(&(_, ch)) = probe.peek() {
        if ch.is_ascii_alphabetic() {
            word.push(ch);
            probe.next();
            consumed += 1;
        } else {
            break;
        }
    }
    if !matches!(word.as_str(), "verb" | "lstinline" | "mintinline") {
        return None;
    }
    if let Some(&(_, '*')) = probe.peek() {
        probe.next();
        consumed += 1;
    }
    // `\mintinline` takes a language group before the delimiter.
    if word == "mintinline" {
        let (_, brace) = probe.next()?;
        if brace != '{' {
            return None;
        }
        consumed += 1;
        loop {
            let (_, ch) = probe.next()?;
            consumed += 1;
            if ch == '}' {
                break;
            }
        }
    }
    let (_, delimiter) = probe.next()?;
    consumed += 1;
    let mut body = String::new();
    loop {
        let (_, ch) = probe.next()?;
        consumed += 1;
        if ch == delimiter || ch == '\n' {
            break;
        }
        body.push(ch);
    }
    Some((consumed, body))
}

/// Recognizes `\begin{verbatim}` and returns the character count to consume and
/// the environment name.
fn peek_verbatim_env(iter: &Chars<'_>) -> Option<(usize, String)> {
    let mut probe = iter.clone();
    let mut consumed = 0usize;
    for expected in ['b', 'e', 'g', 'i', 'n'] {
        let (_, ch) = probe.next()?;
        if ch != expected {
            return None;
        }
        consumed += 1;
    }
    let (_, brace) = probe.next()?;
    if brace != '{' {
        return None;
    }
    consumed += 1;
    let mut name = String::new();
    loop {
        let (_, ch) = probe.next()?;
        consumed += 1;
        if ch == '}' {
            break;
        }
        name.push(ch);
    }
    let bare = name.trim_end_matches('*');
    if VERBATIM_ENVS.contains(&bare) || VERBATIM_ENVS.contains(&name.as_str()) {
        Some((consumed, name))
    } else {
        None
    }
}

fn lex_verbatim_body(tokens: &mut Vec<Token>, start: usize, name: &str, iter: &mut Chars<'_>) {
    let marker = format!("\\end{{{name}}}");
    let mut body = String::new();
    loop {
        let Some((_, ch)) = iter.next() else {
            break;
        };
        body.push(ch);
        if body.len() >= marker.len() && body.ends_with(&marker) {
            body.truncate(body.len() - marker.len());
            break;
        }
    }
    push(
        tokens,
        TokenKind::ControlWord("begin".to_owned()),
        start,
        start + 1,
    );
    push(tokens, TokenKind::LBrace, 0, 0);
    push(tokens, TokenKind::Text(name.to_owned()), 0, 0);
    push(tokens, TokenKind::RBrace, 0, 0);
    push(tokens, TokenKind::Raw(body), 0, 0);
    push(tokens, TokenKind::ControlWord("end".to_owned()), 0, 0);
    push(tokens, TokenKind::LBrace, 0, 0);
    push(tokens, TokenKind::Text(name.to_owned()), 0, 0);
    push(tokens, TokenKind::RBrace, 0, 0);
}

fn lex_control(tokens: &mut Vec<Token>, start: usize, iter: &mut Chars<'_>, at_letter: bool) {
    let Some(&(_, first)) = iter.peek() else {
        push(tokens, TokenKind::ControlSymbol('\\'), start, start + 1);
        return;
    };
    if first.is_ascii_alphabetic() || (at_letter && first == '@') {
        let mut word = String::new();
        while let Some(&(_, ch)) = iter.peek() {
            if ch.is_ascii_alphabetic() || (at_letter && ch == '@') {
                word.push(ch);
                iter.next();
            } else {
                break;
            }
        }
        let mut end = start + 1 + word.len();
        if let Some(&(_, '*')) = iter.peek() {
            word.push('*');
            iter.next();
            end += 1;
        }
        push(tokens, TokenKind::ControlWord(word), start, end);
    } else {
        iter.next();
        push(
            tokens,
            TokenKind::ControlSymbol(first),
            start,
            start + 1 + first.len_utf8(),
        );
    }
}

fn lex_whitespace(tokens: &mut Vec<Token>, start: usize, first: char, iter: &mut Chars<'_>) {
    let mut newlines = usize::from(first == '\n');
    let mut end = start + first.len_utf8();
    while let Some(&(index, ch)) = iter.peek() {
        if !ch.is_whitespace() {
            break;
        }
        newlines += usize::from(ch == '\n');
        iter.next();
        end = index + ch.len_utf8();
    }
    if newlines >= 2 {
        push(tokens, TokenKind::ParBreak, start, end);
    } else {
        push(tokens, TokenKind::Space, start, end);
    }
}

fn lex_text(tokens: &mut Vec<Token>, start: usize, first: char, iter: &mut Chars<'_>) {
    let mut text = String::new();
    text.push(first);
    let mut end = start + first.len_utf8();
    while let Some(&(index, ch)) = iter.peek() {
        if is_special(ch) || ch.is_whitespace() {
            break;
        }
        text.push(ch);
        iter.next();
        end = index + ch.len_utf8();
    }
    push(tokens, TokenKind::Text(text), start, end);
}
