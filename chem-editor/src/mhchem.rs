//! `mhchem`-style LaTeX for reaction equations: `\ce{2H2 + O2 -> 2H2O}`.
//!
//! This is a bounded reader/writer for the subset the editor draws: element
//! symbols with subscript counts, parenthesised groups, hydrate dots, charges,
//! phase annotations, gas/precipitate markers, reaction arrows and the
//! conditions above and below them. It is deliberately not a full `mhchem`
//! implementation and never touches OOXML.

use std::fmt::Write as _;

use crate::model::{Arrow, ArrowKind, Marker, Part, Reaction, Species, State};

/// Parses a reaction written as `mhchem` LaTeX, with or without the `\ce{…}`
/// wrapper.
///
/// # Errors
///
/// Returns a diagnostic when the input has no arrow, an empty side or an
/// unreadable fragment.
pub fn parse(input: &str) -> Result<Reaction, String> {
    let body = strip_ce(input);
    let (lhs, arrow, rhs) = split_arrow(&body)?;
    let reactants = parse_side(&lhs)?;
    let products = parse_side(&rhs)?;
    Ok(Reaction {
        id: 0,
        origin: crate::model::Point::default(),
        reactants,
        arrow,
        products,
    })
}

/// Writes a reaction as a single `\ce{…}` expression that [`parse`] reads back.
#[must_use]
pub fn write(reaction: &Reaction) -> String {
    let mut out = String::from("\\ce{ ");
    out.push_str(&write_side(&reaction.reactants));
    out.push(' ');
    out.push_str(&write_arrow(&reaction.arrow));
    out.push(' ');
    out.push_str(&write_side(&reaction.products));
    out.push_str(" }");
    out
}

fn strip_ce(input: &str) -> String {
    let trimmed = input.trim();
    let Some(inner) = trimmed.strip_prefix("\\ce{") else {
        return trimmed.to_owned();
    };
    let mut depth = 1_usize;
    let mut end = inner.len();
    for (offset, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = offset;
                    break;
                }
            }
            _ => {}
        }
    }
    inner[..end].trim().to_owned()
}

fn split_arrow(body: &str) -> Result<(String, Arrow, String), String> {
    const TOKENS: [(&str, ArrowKind); 6] = [
        ("<=>", ArrowKind::Equilibrium),
        ("<->", ArrowKind::LeftRight),
        ("->", ArrowKind::Right),
        ("<-", ArrowKind::Left),
        ("=>", ArrowKind::Double),
        ("<=", ArrowKind::ReverseEquilibrium),
    ];
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        for (token, kind) in TOKENS {
            if body[index..].starts_with(token) {
                let lhs = body[..index].trim().to_owned();
                if lhs.is_empty() {
                    return Err("A reaction needs a left-hand side".into());
                }
                let after = &body[index + token.len()..];
                let (arrow, rest) = read_conditions(kind, after)?;
                let rhs = rest.trim().to_owned();
                if rhs.is_empty() {
                    return Err("A reaction needs a right-hand side".into());
                }
                return Ok((lhs, arrow, rhs));
            }
        }
        index += char_len(&body[index..]);
    }
    Err("A reaction needs an arrow such as -> or <=>".into())
}

fn read_conditions(kind: ArrowKind, after: &str) -> Result<(Arrow, String), String> {
    let mut above = vec![];
    let mut below = vec![];
    let mut rest = after.trim_start();
    for slot in 0..2 {
        let Some(inner) = rest.strip_prefix('[') else {
            break;
        };
        let Some(close) = inner.find(']') else {
            return Err("Unclosed arrow condition".into());
        };
        let text = inner[..close].trim();
        let target = if slot == 0 { &mut above } else { &mut below };
        for piece in text.split(',') {
            let piece = piece.trim();
            if !piece.is_empty() {
                target.push(piece.to_owned());
            }
        }
        rest = inner[close + 1..].trim_start();
    }
    Ok((Arrow { kind, above, below }, rest.to_owned()))
}

fn char_len(rest: &str) -> usize {
    rest.chars().next().map_or(1, char::len_utf8)
}

fn parse_side(side: &str) -> Result<Vec<Species>, String> {
    let mut species = vec![];
    for piece in split_plus(side) {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        species.push(parse_species(piece)?);
    }
    if species.is_empty() {
        return Err("A reaction needs species on both sides".into());
    }
    Ok(species)
}

/// Splits a side on the `+` separator, keeping charge signs attached.
///
/// A `+` is a separator when it is surrounded by spaces (`A + B`) or directly
/// followed by the start of a new species (`A+B`). A trailing `+` with no
/// following letter, as in `Fe3+`, is a charge.
fn split_plus(side: &str) -> Vec<String> {
    let chars: Vec<char> = side.chars().collect();
    let mut pieces = vec![];
    let mut current = String::new();
    for (index, &c) in chars.iter().enumerate() {
        if c == '+' {
            let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(index + 1).copied();
            let after_space = previous.is_none_or(|p| p == ' ');
            let starts_species = previous != Some('^')
                && next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '(' | '['));
            if after_space || starts_species {
                pieces.push(std::mem::take(&mut current));
                continue;
            }
        }
        current.push(c);
    }
    pieces.push(current);
    pieces
}

/// Parses one species fragment sequence.
///
/// # Errors
///
/// Returns a diagnostic for an unknown character, a zero count or a charge
/// without a sign.
pub fn parse_species(text: &str) -> Result<Species, String> {
    let (body, marker) = split_marker(text);
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0;
    let mut coefficient = 1_u32;
    let mut started = false;
    while let Some(&c) = chars.get(index) {
        if c.is_ascii_digit() && !started {
            let mut value = 0_u32;
            while let Some(&d) = chars.get(index) {
                let Some(digit) = d.to_digit(10) else {
                    break;
                };
                value = value.saturating_mul(10).saturating_add(digit);
                index += 1;
            }
            coefficient = value;
            started = true;
            continue;
        }
        break;
    }
    if coefficient == 0 {
        return Err("A coefficient must be at least 1".into());
    }
    let mut parts = vec![];
    let mut charge = 0_i8;
    let mut state = State::None;
    while let Some(&c) = chars.get(index) {
        match c {
            '.' | '*' => {
                parts.push(Part::Dot);
                index += 1;
            }
            '(' => {
                if let Some(consumed) = match_state(&chars, index) {
                    state = consumed.0;
                    index = consumed.1;
                    continue;
                }
                let (group, next) = read_group(&chars, index)?;
                parts.push(group);
                index = next;
            }
            '^' => {
                if let Some((mass, next)) = read_isotope(&chars, index)? {
                    let (mut part, after) = read_element(&chars, next);
                    if let Part::Element { isotope, .. } = &mut part {
                        *isotope = Some(mass);
                    }
                    parts.push(part);
                    index = after;
                } else {
                    let (value, next) = read_caret_charge(&chars, index)?;
                    charge = value;
                    index = next;
                }
            }
            '+' | '-' => {
                charge = if c == '+' { 1 } else { -1 };
                index += 1;
            }
            ' ' => index += 1,
            'e' if chars.get(index + 1) == Some(&'^') => {
                if let Some((mass, next)) = read_isotope(&chars, index + 1)? {
                    parts.push(Part::Element {
                        symbol: "e".into(),
                        count: 1,
                        isotope: Some(mass),
                    });
                    index = next;
                } else {
                    let (value, next) = read_caret_charge(&chars, index + 1)?;
                    parts.push(Part::Element {
                        symbol: "e".into(),
                        count: 1,
                        isotope: None,
                    });
                    charge = value;
                    index = next;
                }
            }
            'A'..='Z' => {
                let (part, next) = read_element(&chars, index);
                parts.push(part);
                index = next;
            }
            _ => return Err(format!("Cannot read '{c}' in a chemical formula")),
        }
    }
    if parts.is_empty() {
        return Err("A species needs at least one element".into());
    }
    Ok(Species {
        coefficient,
        parts,
        charge,
        state,
        marker,
    })
}

fn split_marker(text: &str) -> (String, Marker) {
    let trimmed = text.trim_end();
    if let Some(body) = trimmed.strip_suffix(" ^") {
        (body.trim_end().to_owned(), Marker::Gas)
    } else if let Some(body) = trimmed.strip_suffix(" v") {
        (body.trim_end().to_owned(), Marker::Precipitate)
    } else {
        (trimmed.to_owned(), Marker::None)
    }
}

fn match_state(chars: &[char], start: usize) -> Option<(State, usize)> {
    for (token, state) in [
        ("(aq)", State::Aqueous),
        ("(s)", State::Solid),
        ("(l)", State::Liquid),
        ("(g)", State::Gas),
    ] {
        let token: Vec<char> = token.chars().collect();
        if chars[start..].starts_with(&token) {
            return Some((state, start + token.len()));
        }
    }
    None
}

fn read_group(chars: &[char], start: usize) -> Result<(Part, usize), String> {
    let mut depth = 0_usize;
    let mut end = None;
    for (offset, &c) in chars[start..].iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.ok_or("Unclosed group in a chemical formula")?;
    let inner: String = chars[start + 1..end].iter().collect();
    let inner_species = parse_species(&inner)?;
    if inner_species.charge != 0
        || inner_species.state != State::None
        || inner_species.marker != Marker::None
    {
        return Err("A group cannot carry a charge, state or marker".into());
    }
    let (count, next) = read_count(chars, end + 1);
    Ok((
        Part::Group {
            parts: inner_species.parts,
            count,
        },
        next,
    ))
}

fn read_count(chars: &[char], start: usize) -> (u32, usize) {
    let mut index = start;
    let mut value = 0_u32;
    let mut any = false;
    while let Some(&c) = chars.get(index) {
        let Some(digit) = c.to_digit(10) else {
            break;
        };
        value = value.saturating_mul(10).saturating_add(digit);
        index += 1;
        any = true;
    }
    if any && value > 0 {
        (value, index)
    } else {
        (1, index)
    }
}

fn read_element(chars: &[char], start: usize) -> (Part, usize) {
    let mut end = start + 1;
    if matches!(chars.get(end), Some(c) if c.is_ascii_lowercase()) {
        end += 1;
    }
    let symbol: String = chars[start..end].iter().collect();
    let (count, next) = read_count(chars, end);
    (
        Part::Element {
            symbol,
            count,
            isotope: None,
        },
        next,
    )
}

/// Reads an isotope prescript (`^{14}` or `^14`) if the caret starts one.
///
/// Returns `None` when the `^` begins a charge instead.
fn read_isotope(chars: &[char], caret: usize) -> Result<Option<(u16, usize)>, String> {
    let mut index = caret + 1;
    let digits: String;
    if chars.get(index) == Some(&'{') {
        let mut end = None;
        for (offset, &c) in chars[index..].iter().enumerate() {
            if c == '}' {
                end = Some(index + offset);
                break;
            }
        }
        let end = end.ok_or("Unclosed isotope braces")?;
        digits = chars[index + 1..end].iter().collect();
        index = end + 1;
    } else {
        let start = index;
        while chars.get(index).is_some_and(char::is_ascii_digit) {
            index += 1;
        }
        digits = chars[start..index].iter().collect();
    }
    let follows_element = chars
        .get(index)
        .is_some_and(|c| c.is_ascii_uppercase() || *c == 'e');
    if digits.is_empty() || !follows_element || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let mass = digits
        .parse::<u16>()
        .map_err(|_| "Isotope mass is too large")?;
    Ok(Some((mass, index)))
}

fn read_caret_charge(chars: &[char], caret: usize) -> Result<(i8, usize), String> {
    let mut index = caret + 1;
    let body: String;
    if chars.get(index) == Some(&'{') {
        let mut depth = 0_usize;
        let mut end = None;
        for (offset, &c) in chars[index..].iter().enumerate() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or("Unclosed charge braces")?;
        body = chars[index + 1..end].iter().collect();
        index = end + 1;
    } else {
        let start = index;
        while chars
            .get(index)
            .is_some_and(|c| c.is_ascii_digit() || *c == '+' || *c == '-')
        {
            index += 1;
        }
        body = chars[start..index].iter().collect();
    }
    let mut digits = String::new();
    let mut sign = 0_i8;
    for c in body.chars() {
        match c {
            '+' => sign = 1,
            '-' => sign = -1,
            d if d.is_ascii_digit() => digits.push(d),
            _ => return Err("A charge may only contain digits and a sign".into()),
        }
    }
    if sign == 0 {
        return Err("A charge needs a + or - sign".into());
    }
    let magnitude: i8 = digits.parse().unwrap_or(1);
    let magnitude = if digits.is_empty() { 1 } else { magnitude };
    Ok((sign * magnitude, index))
}

fn write_side(species: &[Species]) -> String {
    species
        .iter()
        .map(write_species)
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Writes one species back to `mhchem` text.
#[must_use]
pub fn write_species(species: &Species) -> String {
    let mut out = String::new();
    if species.coefficient != 1 {
        let _ = write!(out, "{}", species.coefficient);
    }
    write_parts(&species.parts, &mut out);
    if species.charge != 0 {
        let magnitude = species.charge.unsigned_abs();
        let sign = if species.charge > 0 { '+' } else { '-' };
        if magnitude == 1 {
            let _ = write!(out, "^{sign}");
        } else {
            let _ = write!(out, "^{magnitude}{sign}");
        }
    }
    if species.state != State::None {
        let _ = write!(out, "({})", species.state.code());
    }
    match species.marker {
        Marker::None => {}
        Marker::Gas => out.push_str(" ^"),
        Marker::Precipitate => out.push_str(" v"),
    }
    out
}

fn write_parts(parts: &[Part], out: &mut String) {
    for part in parts {
        match part {
            Part::Element {
                symbol,
                count,
                isotope,
            } => {
                if let Some(mass) = isotope {
                    let _ = write!(out, "^{{{mass}}}");
                }
                out.push_str(symbol);
                if *count > 1 {
                    let _ = write!(out, "{count}");
                }
            }
            Part::Group { parts, count } => {
                out.push('(');
                write_parts(parts, out);
                out.push(')');
                if *count > 1 {
                    let _ = write!(out, "{count}");
                }
            }
            Part::Dot => out.push('.'),
        }
    }
}

fn write_arrow(arrow: &Arrow) -> String {
    let token = match arrow.kind {
        ArrowKind::Right => "->",
        ArrowKind::Left => "<-",
        ArrowKind::LeftRight => "<->",
        ArrowKind::Equilibrium => "<=>",
        ArrowKind::Double => "=>",
        ArrowKind::ReverseEquilibrium => "<=",
    };
    let mut out = token.to_owned();
    if !arrow.above.is_empty() {
        let _ = write!(out, "[{}]", arrow.above.join(", "));
    }
    if !arrow.below.is_empty() {
        let _ = write!(out, "[{}]", arrow.below.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(source: &str) {
        let reaction = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        let written = write(&reaction);
        let again = parse(&written).unwrap_or_else(|error| panic!("{written}: {error}"));
        assert_eq!(
            write(&again),
            written,
            "{source} -> {written} -> {}",
            write(&again)
        );
    }

    #[test]
    fn simple_reactions_round_trip() {
        for source in [
            "2H2 + O2 -> 2H2O",
            "N2 + 3H2 <=> 2NH3",
            "Na^+ + Cl^- -> NaCl(s)",
            "CaCO3(s) <=> CaO + CO2 ^",
            "Ag^+ + Cl^- -> AgCl(s) v",
            "CH3COOH <=> CH3COO^- + H^+",
            "CuSO4.H2O -> CuSO4 + H2O",
            "(NH4)2SO4 -> 2NH4^+ + SO4^2-",
            "2 H2 + O2 -> 2 H2O",
            "^{14}C -> ^{14}N + e^-",
        ] {
            round_trip(source);
        }
    }

    #[test]
    fn isotopes_are_read_and_written() {
        let species = parse_species("^{14}C").unwrap();
        assert_eq!(
            species.parts[0],
            Part::Element {
                symbol: "C".into(),
                count: 1,
                isotope: Some(14),
            }
        );
        assert!(write_species(&species).contains("^{14}C"));
        let unbraced = parse_species("^14C").unwrap();
        assert_eq!(unbraced.parts[0], species.parts[0]);
    }

    #[test]
    fn conditions_are_read_and_written() {
        let reaction = parse("N2 + 3H2 ->[Fe, 500 C][200 atm] 2NH3").unwrap();
        assert_eq!(reaction.arrow.above, vec!["Fe", "500 C"]);
        assert_eq!(reaction.arrow.below, vec!["200 atm"]);
        assert!(write(&reaction).contains("[Fe, 500 C][200 atm]"));
    }

    #[test]
    fn structures_are_read() {
        let reaction = parse("2H2 + O2 -> 2H2O").unwrap();
        assert_eq!(reaction.reactants.len(), 2);
        assert_eq!(reaction.reactants[0].coefficient, 2);
        assert_eq!(
            reaction.reactants[0].parts[0],
            Part::Element {
                symbol: "H".into(),
                count: 2,
                isotope: None,
            }
        );
        assert_eq!(reaction.products[0].coefficient, 2);
        let sulfate = parse_species("SO4^2-").unwrap();
        assert_eq!(sulfate.charge, -2);
        assert_eq!(sulfate.parts.len(), 2);
    }

    #[test]
    fn malformed_input_is_rejected() {
        assert!(parse("no arrow here").is_err());
        assert!(parse("H2O -> ").is_err());
        assert!(parse_species("^").is_err());
        assert!(parse_species("(s)").is_err());
    }
}
