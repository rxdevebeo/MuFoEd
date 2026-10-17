//! Formulas typed as LaTeX: `\frac{a}{b}`, `x^{2}`, `\sum_{i=1}^{n}`, and
//! chemistry in mhchem's `\ce{2H2 + O2 -> 2H2O}`. The text goes to MathML
//! (`pulldown-latex`) and from there to the document's own formula model
//! (the library's MathML reader), so what is inserted is a real Word
//! equation; an equation already in the document comes back as LaTeX to be
//! edited the same way.

use std::fmt::Write as _;

use strict_ooxml::model::math::{MathArgument, MathExpression, MathNode, MathPosition};

/// The formula `source` describes, or what is wrong with it.
pub(crate) fn from_latex(source: &str) -> Result<MathExpression, String> {
    let latex = expand_chemistry(source.trim());
    if latex.is_empty() {
        return Err(String::new());
    }
    let storage = pulldown_latex::Storage::new();
    let events: Vec<_> = pulldown_latex::Parser::new(&latex, &storage).collect();
    if let Some(Err(error)) = events.iter().find(|event| event.is_err()) {
        return Err(error.to_string());
    }
    let mut mathml = String::new();
    let config = pulldown_latex::RenderConfig {
        display_mode: pulldown_latex::config::DisplayMode::Inline,
        ..pulldown_latex::RenderConfig::default()
    };
    pulldown_latex::push_mathml(&mut mathml, events.into_iter(), config)
        .map_err(|error| error.to_string())?;
    strict_ooxml::math_from_mathml(&mathml).map_err(|error| error.to_string())
}

/// The formula drawn as the document draws it, cropped to its ink:
/// width, height and premultiplied RGBA.
pub(crate) fn preview(expression: &MathExpression) -> Option<(usize, usize, Vec<u8>)> {
    preview_inline(strict_ooxml::model::Inline::Math(expression.clone()))
}

/// An inline (a formula, a chart) drawn alone as the document draws it,
/// cropped to its ink: width, height and premultiplied RGBA.
pub(crate) fn preview_inline(
    inline: strict_ooxml::model::Inline,
) -> Option<(usize, usize, Vec<u8>)> {
    use std::sync::OnceLock;
    use strict_ooxml::model::{Block, Paragraph, Rsids};
    static BLANK: OnceLock<Option<strict_ooxml::model::Document>> = OnceLock::new();
    let blank = BLANK.get_or_init(|| {
        strict_ooxml::StrictDocument::import_rtf(
            crate::formats::NEW_DOCUMENT_RTF.as_bytes(),
            &strict_ooxml::RtfOptions::default(),
        )
        .ok()
        .map(|opened| opened.document.document().clone())
    });
    let mut document = blank.clone()?;
    document.body.blocks = vec![Block::Paragraph(Paragraph {
        props: strict_ooxml::model::ParagraphProperties::default(),
        inlines: vec![inline],
        rsids: Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    })];
    let pages = strict_ooxml_render_svg::render(
        &document,
        &strict_ooxml_render_svg::RenderOptions::default(),
    )
    .ok()?;
    let bitmap = crate::raster::rasterize(&pages.first()?.svg, 3.0).ok()?;
    crop(bitmap.width, bitmap.height, &bitmap.rgba)
}

/// The smallest rectangle of a bitmap with anything but white in it, with
/// a little room around.
fn crop(width: usize, height: usize, rgba: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let inked = |x: usize, y: usize| {
        rgba.get((y * width + x) * 4..(y * width + x) * 4 + 3)
            .is_some_and(|p| p.iter().any(|v| *v < 235))
            && rgba
                .get((y * width + x) * 4 + 3)
                .is_some_and(|alpha| *alpha > 40)
    };
    let (mut x0, mut y0, mut x1, mut y1) = (width, height, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if inked(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < x0 || y1 < y0 {
        return None;
    }
    let pad = 6;
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad).min(width - 1), (y1 + pad).min(height - 1));
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut out = Vec::with_capacity(w * h * 4);
    for y in y0..=y1 {
        out.extend_from_slice(rgba.get((y * width + x0) * 4..(y * width + x1 + 1) * 4)?);
    }
    Some((w, h, out))
}

// ----- chemistry ----------------------------------------------------------

/// Every `\ce{…}` turned into plain LaTeX.
fn expand_chemistry(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(at) = rest.find("\\ce{") {
        out.push_str(rest.get(..at).unwrap_or_default());
        let inner_start = at + "\\ce{".len();
        let mut depth = 1;
        let mut end = None;
        for (offset, c) in rest.get(inner_start..).unwrap_or_default().char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(inner_start + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push_str(rest.get(at..).unwrap_or_default());
            return out;
        };
        out.push('{');
        out.push_str(&chemistry(rest.get(inner_start..end).unwrap_or_default()));
        out.push('}');
        rest = rest.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// One `\ce` argument: elements upright, counts lowered, charges raised,
/// arrows and states as mhchem draws them.
fn chemistry(text: &str) -> String {
    const ARROWS: [(&str, &str); 6] = [
        ("<=>", "\\rightleftharpoons"),
        ("<->", "\\leftrightarrow"),
        ("->", "\\rightarrow"),
        ("<-", "\\leftarrow"),
        ("=", "="),
        ("+", "+"),
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // At the start of a species: digits are a coefficient, not a count.
    let mut fresh = true;
    let take = |from: usize, test: &dyn Fn(char) -> bool| -> usize {
        let mut end = from;
        while chars.get(end).is_some_and(|c| test(*c)) {
            end += 1;
        }
        end
    };
    let slice =
        |a: usize, b: usize| -> String { chars.get(a..b).unwrap_or_default().iter().collect() };
    while let Some(&c) = chars.get(i) {
        let rest: String = chars.get(i..).unwrap_or_default().iter().collect();
        // Arrows and operators between species, with spaces around.
        if fresh || c == ' ' || rest.starts_with('<') || rest.starts_with("->") {
            if let Some((from, to)) = ARROWS
                .iter()
                .find(|(from, _)| rest.starts_with(from) && (fresh || from.len() > 1))
            {
                out.push(' ');
                out.push_str(to);
                out.push(' ');
                i += from.chars().count();
                fresh = true;
                continue;
            }
        }
        match c {
            ' ' => {
                // A lone `^` or `v` after a space: a gas or a precipitate.
                let next = chars.get(i + 1).copied();
                let after = chars.get(i + 2).copied();
                if matches!(next, Some('^' | 'v')) && after.is_none_or(|a| a == ' ') {
                    out.push_str(if next == Some('^') {
                        " \\uparrow"
                    } else {
                        " \\downarrow"
                    });
                    i += 2;
                } else {
                    out.push(' ');
                    i += 1;
                }
                fresh = true;
            }
            'A'..='Z' => {
                let end = take(i + 1, &|c: char| c.is_ascii_lowercase());
                let _ = write!(out, "\\mathrm{{{}}}", slice(i, end));
                i = end;
                i = count(&chars, i, &mut out);
                fresh = false;
            }
            '0'..='9' if fresh => {
                let end = take(i, &|c: char| c.is_ascii_digit() || c == '/');
                out.push_str(&slice(i, end));
                i = end;
            }
            ')' | ']' => {
                out.push(c);
                i = count(&chars, i + 1, &mut out);
                fresh = false;
            }
            '(' => {
                let close = take(i + 1, &|c: char| c.is_ascii_lowercase());
                let state = slice(i + 1, close);
                if chars.get(close) == Some(&')') && ["aq", "s", "l", "g"].contains(&state.as_str())
                {
                    let _ = write!(out, "\\,\\mathrm{{({state})}}");
                    i = close + 1;
                } else {
                    out.push('(');
                    i += 1;
                }
                fresh = false;
            }
            '^' => {
                if chars.get(i + 1) == Some(&'{') {
                    let end = take(i + 1, &|c: char| c != '}');
                    out.push_str(&slice(i, end + 1));
                    i = end + 1;
                } else {
                    let end = take(i + 1, &|c: char| c.is_ascii_digit() || c == '+' || c == '-');
                    let _ = write!(out, "^{{{}}}", slice(i + 1, end));
                    i = end;
                }
            }
            '+' | '-' => {
                // After a species: its charge.
                let _ = write!(out, "^{{{c}}}");
                i += 1;
            }
            '.' | '*' => {
                out.push_str("\\cdot ");
                i += 1;
                fresh = true;
            }
            'a'..='z' => {
                let end = take(i, &|c: char| c.is_ascii_lowercase());
                let _ = write!(out, "\\mathrm{{{}}}", slice(i, end));
                i = end;
                fresh = false;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A count after an element or a bracket, lowered.
fn count(chars: &[char], from: usize, out: &mut String) -> usize {
    let mut end = from;
    while chars.get(end).is_some_and(char::is_ascii_digit) {
        end += 1;
    }
    if end > from {
        let digits: String = chars.get(from..end).unwrap_or_default().iter().collect();
        let _ = write!(out, "_{{{digits}}}");
    }
    end
}

// ----- back to LaTeX ------------------------------------------------------

/// A formula of the document as LaTeX, to be edited.
pub(crate) fn to_latex(expression: &MathExpression) -> String {
    let mut out = String::new();
    nodes(&expression.nodes, &mut out);
    out.trim().to_owned()
}

fn nodes(list: &[MathNode], out: &mut String) {
    for node in list {
        node_latex(node, out);
    }
}

fn group(argument: &MathArgument) -> String {
    let mut out = String::new();
    nodes(&argument.nodes, &mut out);
    format!("{{{}}}", out.trim())
}

fn plain(argument: &MathArgument) -> String {
    let mut out = String::new();
    nodes(&argument.nodes, &mut out);
    out.trim().to_owned()
}

/// Known function names, written with their command.
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "log", "ln", "lg", "exp", "lim", "max", "min", "sup", "inf", "det", "gcd", "deg", "dim", "ker",
    "arg",
];

#[allow(clippy::too_many_lines)]
fn node_latex(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Run(run) => {
            let upright = run.properties.normal
                || matches!(
                    run.properties.style,
                    Some(strict_ooxml::model::math::MathStyle::Plain)
                );
            let text = &run.text;
            let letters = text.chars().filter(|c| c.is_alphabetic()).count();
            if upright && letters > 1 && FUNCTIONS.contains(&text.trim()) {
                let _ = write!(out, "\\{} ", text.trim());
            } else if upright && letters > 0 {
                let _ = write!(out, "\\mathrm{{{}}}", escape_text(text));
            } else {
                for c in text.chars() {
                    out.push_str(&symbol(c));
                }
            }
        }
        MathNode::Fraction(f) => {
            let _ = write!(
                out,
                "\\frac{}{}",
                group(&f.numerator),
                group(&f.denominator)
            );
        }
        MathNode::Radical(r) => {
            let degree = plain(&r.degree);
            if r.hide_degree || degree.is_empty() {
                let _ = write!(out, "\\sqrt{}", group(&r.radicand));
            } else {
                let _ = write!(out, "\\sqrt[{degree}]{}", group(&r.radicand));
            }
        }
        MathNode::Superscript(s) => {
            let _ = write!(out, "{}^{}", group(&s.base), group(&s.superscript));
        }
        MathNode::Subscript(s) => {
            let _ = write!(out, "{}_{}", group(&s.base), group(&s.subscript));
        }
        MathNode::SubSuperscript(s) => {
            let _ = write!(
                out,
                "{}_{}^{}",
                group(&s.base),
                group(&s.subscript),
                group(&s.superscript)
            );
        }
        MathNode::PreScript(s) => {
            let _ = write!(
                out,
                "{{}}_{}^{}{}",
                group(&s.subscript),
                group(&s.superscript),
                group(&s.base)
            );
        }
        MathNode::NaryOperator(n) => {
            let command = match n.chr.unwrap_or('∫') {
                '∑' => "\\sum",
                '∏' => "\\prod",
                '∐' => "\\coprod",
                '∬' => "\\iint",
                '∭' => "\\iiint",
                '∮' => "\\oint",
                '⋃' => "\\bigcup",
                '⋂' => "\\bigcap",
                _ => "\\int",
            };
            out.push_str(command);
            if !n.hide_sub && !n.subscript.nodes.is_empty() {
                let _ = write!(out, "_{}", group(&n.subscript));
            }
            if !n.hide_sup && !n.superscript.nodes.is_empty() {
                let _ = write!(out, "^{}", group(&n.superscript));
            }
            let _ = write!(out, " {}", plain(&n.operand));
        }
        MathNode::Delimiter(d) => {
            let open = d.begin.unwrap_or('(');
            let close = d.end.unwrap_or(')');
            let separator = d.separator.unwrap_or('|');
            let inner: Vec<String> = d.arguments.iter().map(plain).collect();
            let _ = write!(
                out,
                "\\left{} {} \\right{}",
                fence(open),
                inner.join(&format!(" {} ", symbol(separator))),
                fence(close)
            );
        }
        MathNode::Function(f) => {
            let name = plain(&f.name);
            let bare = name.trim_start_matches("\\mathrm{").trim_end_matches('}');
            if name.starts_with('\\') && !name.starts_with("\\mathrm") {
                out.push_str(&name);
            } else if FUNCTIONS.contains(&bare) {
                let _ = write!(out, "\\{bare}");
            } else {
                let _ = write!(out, "\\operatorname{{{bare}}}");
            }
            let _ = write!(out, " {}", plain(&f.argument));
        }
        MathNode::Limit(l) => {
            let base = plain(&l.base);
            if base.trim_start_matches('\\').trim() == "lim"
                || FUNCTIONS.contains(&base.trim_start_matches('\\').trim())
            {
                let joiner = if l.above { "^" } else { "_" };
                let _ = write!(out, "{base}{joiner}{}", group(&l.limit));
            } else if l.above {
                let _ = write!(out, "\\overset{}{{{base}}}", group(&l.limit));
            } else {
                let _ = write!(out, "\\underset{}{{{base}}}", group(&l.limit));
            }
        }
        MathNode::Matrix(m) => {
            let rows: Vec<String> = m
                .rows
                .iter()
                .map(|row| row.iter().map(plain).collect::<Vec<_>>().join(" & "))
                .collect();
            let _ = write!(
                out,
                "\\begin{{matrix}} {} \\end{{matrix}}",
                rows.join(" \\\\ ")
            );
        }
        MathNode::EquationArray(e) => {
            let rows: Vec<String> = e.rows.iter().map(plain).collect();
            let _ = write!(
                out,
                "\\begin{{aligned}} {} \\end{{aligned}}",
                rows.join(" \\\\ ")
            );
        }
        MathNode::Accent(a) => {
            let command = match a.chr.unwrap_or('\u{302}') {
                '\u{303}' => "\\tilde",
                '\u{307}' => "\\dot",
                '\u{308}' => "\\ddot",
                '\u{20d7}' | '\u{2192}' => "\\vec",
                '\u{304}' | '\u{305}' => "\\bar",
                _ => "\\hat",
            };
            let _ = write!(out, "{command}{}", group(&a.base));
        }
        MathNode::Bar(b) => {
            let command = if b.position == Some(MathPosition::Top) {
                "\\overline"
            } else {
                "\\underline"
            };
            let _ = write!(out, "{command}{}", group(&b.base));
        }
        MathNode::GroupCharacter(g) => {
            let command = if g.chr == Some('\u{23de}') || g.position == Some(MathPosition::Top) {
                "\\overbrace"
            } else {
                "\\underbrace"
            };
            let _ = write!(out, "{command}{}", group(&g.base));
        }
        MathNode::Boxed(b) => out.push_str(&plain(&b.argument)),
        MathNode::BorderBox(b) => {
            let inner: Vec<String> = b.arguments.iter().map(plain).collect();
            let _ = write!(out, "\\boxed{{{}}}", inner.join(" "));
        }
        MathNode::Phantom(p) => {
            let _ = write!(out, "\\phantom{}", group(&p.argument));
        }
        MathNode::Unknown(_) => {}
    }
}

fn fence(c: char) -> String {
    match c {
        '{' => "\\{".to_owned(),
        '}' => "\\}".to_owned(),
        '⟨' => "\\langle".to_owned(),
        '⟩' => "\\rangle".to_owned(),
        '‖' => "\\|".to_owned(),
        '⌊' => "\\lfloor".to_owned(),
        '⌋' => "\\rfloor".to_owned(),
        '⌈' => "\\lceil".to_owned(),
        '⌉' => "\\rceil".to_owned(),
        '\0' | ' ' => ".".to_owned(),
        c => c.to_string(),
    }
}

fn escape_text(text: &str) -> String {
    text.replace('\\', "\\backslash ")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

/// A character of a formula as LaTeX: commands for Greek letters and the
/// common operators, the rest as it is.
fn symbol(c: char) -> String {
    const TABLE: &[(char, &str)] = &[
        ('α', "\\alpha"),
        ('β', "\\beta"),
        ('γ', "\\gamma"),
        ('δ', "\\delta"),
        ('ε', "\\varepsilon"),
        ('ϵ', "\\epsilon"),
        ('ζ', "\\zeta"),
        ('η', "\\eta"),
        ('θ', "\\theta"),
        ('ϑ', "\\vartheta"),
        ('ι', "\\iota"),
        ('κ', "\\kappa"),
        ('λ', "\\lambda"),
        ('μ', "\\mu"),
        ('ν', "\\nu"),
        ('ξ', "\\xi"),
        ('π', "\\pi"),
        ('ρ', "\\rho"),
        ('σ', "\\sigma"),
        ('ς', "\\varsigma"),
        ('τ', "\\tau"),
        ('υ', "\\upsilon"),
        ('φ', "\\varphi"),
        ('ϕ', "\\phi"),
        ('χ', "\\chi"),
        ('ψ', "\\psi"),
        ('ω', "\\omega"),
        ('Γ', "\\Gamma"),
        ('Δ', "\\Delta"),
        ('Θ', "\\Theta"),
        ('Λ', "\\Lambda"),
        ('Ξ', "\\Xi"),
        ('Π', "\\Pi"),
        ('Σ', "\\Sigma"),
        ('Φ', "\\Phi"),
        ('Ψ', "\\Psi"),
        ('Ω', "\\Omega"),
        ('∞', "\\infty"),
        ('±', "\\pm"),
        ('∓', "\\mp"),
        ('×', "\\times"),
        ('·', "\\cdot"),
        ('⋅', "\\cdot"),
        ('÷', "\\div"),
        ('≤', "\\le"),
        ('≥', "\\ge"),
        ('≠', "\\ne"),
        ('≈', "\\approx"),
        ('≡', "\\equiv"),
        ('∼', "\\sim"),
        ('∝', "\\propto"),
        ('→', "\\to"),
        ('←', "\\leftarrow"),
        ('⇒', "\\Rightarrow"),
        ('⇔', "\\Leftrightarrow"),
        ('↔', "\\leftrightarrow"),
        ('⇌', "\\rightleftharpoons"),
        ('∈', "\\in"),
        ('∉', "\\notin"),
        ('⊂', "\\subset"),
        ('⊆', "\\subseteq"),
        ('∪', "\\cup"),
        ('∩', "\\cap"),
        ('∅', "\\emptyset"),
        ('∀', "\\forall"),
        ('∃', "\\exists"),
        ('∂', "\\partial"),
        ('∇', "\\nabla"),
        ('…', "\\ldots"),
        ('⋯', "\\cdots"),
        ('′', "'"),
        ('∘', "\\circ"),
        ('°', "^{\\circ}"),
        ('ℏ', "\\hbar"),
        ('↑', "\\uparrow"),
        ('↓', "\\downarrow"),
        ('∧', "\\wedge"),
        ('∨', "\\vee"),
        ('¬', "\\neg"),
        ('{', "\\{"),
        ('}', "\\}"),
        ('%', "\\%"),
        ('#', "\\#"),
        ('&', "\\&"),
        ('_', "\\_"),
    ];
    match TABLE.iter().find(|(from, _)| *from == c) {
        Some((_, to))
            if to.starts_with('\\') && to.chars().last().is_some_and(char::is_alphabetic) =>
        {
            format!("{to} ")
        }
        Some((_, to)) => (*to).to_owned(),
        None => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemistry_becomes_latex() {
        assert_eq!(
            chemistry("2H2 + O2 -> 2H2O"),
            "2\\mathrm{H}_{2} + \\mathrm{O}_{2} \\rightarrow 2\\mathrm{H}_{2}\\mathrm{O}"
        );
        assert_eq!(chemistry("SO4^2-"), r"\mathrm{S}\mathrm{O}_{4}^{2-}");
        assert_eq!(chemistry("Na+ + Cl-"), r"\mathrm{Na}^{+} + \mathrm{Cl}^{-}");
        assert_eq!(
            chemistry("CaCO3(s) <=> CaO + CO2 ^"),
            r"\mathrm{Ca}\mathrm{C}\mathrm{O}_{3}\,\mathrm{(s)} \rightleftharpoons \mathrm{Ca}\mathrm{O} + \mathrm{C}\mathrm{O}_{2} \uparrow"
        );
        assert_eq!(
            expand_chemistry("x = \\ce{H2O}"),
            "x = {\\mathrm{H}_{2}\\mathrm{O}}"
        );
    }

    #[test]
    fn latex_goes_in_and_comes_back() {
        for source in [
            "\\frac{a+b}{2}",
            "x^{2}+y_{i}",
            "\\sqrt{x}",
            "\\sum_{i=1}^{n} i",
            "\\alpha \\le \\beta",
            "\\ce{2H2 + O2 -> 2H2O}",
        ] {
            let math = from_latex(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            assert!(!math.nodes.is_empty(), "{source}");
            let again = to_latex(&math);
            let reread =
                from_latex(&again).unwrap_or_else(|error| panic!("{source} -> {again}: {error}"));
            assert_eq!(reread.text(), math.text(), "{source} -> {again}");
        }
        assert!(from_latex("\\frac{a").is_err());
    }
}

#[cfg(test)]
mod preview_tests {
    #[test]
    fn the_preview_is_cropped_to_the_formula() {
        let math = super::from_latex("x^{2}").unwrap();
        let (w, h, _) = super::preview(&math).unwrap();
        assert!(w < 300 && h < 200, "{w}x{h}");
    }
}
