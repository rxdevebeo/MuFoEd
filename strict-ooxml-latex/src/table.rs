//! Tabular construction: column specifications, merged cells, colours and rules.
//!
//! The builder turns the token slice between `\begin{tabular}` and
//! `\end{tabular}` into a Strict [`Table`]. Cell inline content is supplied by a
//! callback so the document parser keeps ownership of inline translation.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{
    Block, Border, BorderStyle, Borders, CellMargins, CellProperties, Color, EighthsPoint, GridCol,
    Inline, Justification, Paragraph, ParagraphProperties, RowProperties, Shading, Table,
    TableCell, TableLayout, TableProperties, TableRow, Twips, VerticalMerge, Width, WidthKind,
};

use crate::lexer::{Token, TokenKind};
use crate::LatexError;

/// The default text width in twips (A4, one-inch margins).
const TEXT_WIDTH: i32 = 9026;

/// One declared table column.
#[derive(Clone, Copy, Debug, Default)]
struct ColumnDef {
    align: Option<Justification>,
    /// Fixed width in twips; `None` means automatic.
    width: Option<i32>,
}

/// A cell parsed from its raw tokens.
#[derive(Default)]
struct ParsedCell {
    span: usize,
    align: Option<Justification>,
    shading: Option<String>,
    vmerge_restart: bool,
    vmerge_rows: usize,
    tokens: Vec<Token>,
}

/// A row with everything needed to draw it.
struct BuiltRow {
    cells: Vec<ParsedCell>,
    top: Vec<bool>,
    bottom: Vec<bool>,
    fill: Option<String>,
}

/// The result of building a table.
pub(crate) struct BuiltTable {
    /// The table.
    pub table: Table,
}

/// A callback that renders one cell's tokens to inline content.
type CellRenderer<'a> = dyn FnMut(&[Token]) -> Result<Vec<Inline>, LatexError> + 'a;

/// Builds a table from a column specification and the body tokens.
pub(crate) fn build(
    spec: &str,
    body: &[Token],
    render_cell: &mut CellRenderer<'_>,
) -> Result<BuiltTable, LatexError> {
    let (columns, bars) = parse_columns(spec);
    let mut column_defs = columns;
    if column_defs.is_empty() {
        column_defs.push(ColumnDef::default());
    }
    let column_count = column_defs.len();

    let segments = split_top_level(body, &TokenKind::ControlSymbol('\\'));
    let mut rows: Vec<BuiltRow> = Vec::new();
    let mut pending_top: Option<Vec<bool>> = None;
    let mut extra_columns = 0usize;

    for segment in segments {
        let extracted = extract_row(segment, column_count);
        if !extracted.has_content {
            if extracted.top.iter().any(|v| *v) || extracted.bottom.iter().any(|v| *v) {
                // A rule-only segment closes the previous row and opens the next.
                let flags = union_flags(&extracted.top, &extracted.bottom);
                if let Some(last) = rows.last_mut() {
                    last.bottom = union_flags(&last.bottom, &flags);
                }
                pending_top = Some(flags);
            }
            continue;
        }
        let top = match pending_top.take() {
            Some(previous) => union_flags(&previous, &extracted.top),
            None => extracted.top,
        };
        let cells = split_top_level(&extracted.cells, &TokenKind::Ampersand);
        let parsed: Vec<ParsedCell> = cells.iter().map(|cell| parse_cell(cell)).collect();
        let width = parsed.iter().map(|cell| cell.span.max(1)).sum::<usize>();
        extra_columns = extra_columns.max(width.saturating_sub(column_count));
        rows.push(BuiltRow {
            cells: parsed,
            top,
            bottom: extracted.bottom,
            fill: extracted.fill,
        });
    }

    let columns = (column_count + extra_columns).max(1);
    let mut bars = bars;
    bars.resize(columns + 1, false);
    let grid = build_grid(&column_defs, columns);

    let mut pending_vmerge: Vec<(usize, usize)> = Vec::new();
    let mut table_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let mut placed: Vec<TableCell> = Vec::new();
        let mut iter = row.cells.into_iter();
        let mut column = 0usize;
        while column < columns {
            if let Some(entry) = pending_vmerge.iter_mut().find(|(c, _)| *c == column) {
                placed.push(continuation_cell(
                    bars.get(column + 1).copied().unwrap_or(false),
                ));
                entry.1 -= 1;
                column += 1;
                continue;
            }
            let Some(cell) = iter.next() else {
                break;
            };
            let span = cell.span.max(1);
            let align = cell.align.or(column_defs.get(column).and_then(|c| c.align));
            let borders = cell_borders(
                &bars,
                column,
                span,
                flag_range(&row.top, column, span),
                flag_range(&row.bottom, column, span),
            );
            let fill = cell.shading.clone().or_else(|| row.fill.clone());
            placed.push(content_cell(
                render_cell(&cell.tokens)?,
                align,
                borders,
                fill,
                if cell.vmerge_restart {
                    Some(VerticalMerge::Restart)
                } else {
                    None
                },
                span,
            ));
            if cell.vmerge_restart && cell.vmerge_rows > 1 {
                pending_vmerge.push((column, cell.vmerge_rows - 1));
            }
            column += span;
        }
        // Rightmost continuation cells that were not consumed by a real cell.
        for entry in &mut pending_vmerge {
            if entry.1 > 0 && entry.0 >= column {
                while placed.len() < columns && entry.1 > 0 {
                    placed.push(continuation_cell(false));
                    entry.1 -= 1;
                }
            }
        }
        pending_vmerge.retain(|(_, remaining)| *remaining > 0);
        while placed.len() < columns {
            placed.push(empty_cell());
        }
        table_rows.push(TableRow {
            props: RowProperties::default(),
            cells: placed,
            sdt: None,
            location: SourceLocation::default(),
        });
    }

    let table = Table {
        props: TableProperties {
            width: Some(Width {
                kind: WidthKind::Dxa,
                value: Some(TEXT_WIDTH),
            }),
            layout: Some(TableLayout::Fixed),
            ..TableProperties::default()
        },
        grid,
        grid_change: None,
        rows: table_rows,
        location: SourceLocation::default(),
    };
    Ok(BuiltTable { table })
}

fn content_cell(
    inlines: Vec<Inline>,
    align: Option<Justification>,
    borders: Borders,
    fill: Option<String>,
    vertical_merge: Option<VerticalMerge>,
    span: usize,
) -> TableCell {
    let shading = fill.map(|hex| Shading {
        pattern: Some(Arc::from("clear")),
        fill: Some(Color::new(hex)),
        ..Shading::default()
    });
    TableCell {
        props: CellProperties {
            borders,
            shading,
            vertical_merge,
            grid_span: (span > 1).then(|| u16::try_from(span).unwrap_or(1)),
            margins: default_margins(),
            ..CellProperties::default()
        },
        blocks: vec![Block::Paragraph(paragraph(inlines, align))],
        sdt: None,
        location: SourceLocation::default(),
    }
}

fn continuation_cell(right_bar: bool) -> TableCell {
    let borders = Borders {
        end: right_bar.then(solid_border),
        ..Borders::default()
    };
    TableCell {
        props: CellProperties {
            borders,
            vertical_merge: Some(VerticalMerge::Continue),
            margins: default_margins(),
            ..CellProperties::default()
        },
        blocks: vec![Block::Paragraph(paragraph(Vec::new(), None))],
        sdt: None,
        location: SourceLocation::default(),
    }
}

fn empty_cell() -> TableCell {
    TableCell {
        props: CellProperties {
            margins: default_margins(),
            ..CellProperties::default()
        },
        blocks: vec![Block::Paragraph(paragraph(Vec::new(), None))],
        sdt: None,
        location: SourceLocation::default(),
    }
}

fn paragraph(inlines: Vec<Inline>, align: Option<Justification>) -> Paragraph {
    Paragraph {
        props: ParagraphProperties {
            alignment: align,
            ..ParagraphProperties::default()
        },
        inlines,
        rsids: Default::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::default(),
    }
}

fn default_margins() -> CellMargins {
    CellMargins {
        top: Some(Twips(40)),
        start: Some(Twips(80)),
        bottom: Some(Twips(40)),
        end: Some(Twips(80)),
    }
}

fn solid_border() -> Border {
    Border {
        style: Some(BorderStyle::Single),
        size: Some(EighthsPoint(4)),
        color: Some(Color::new("auto")),
        ..Border::default()
    }
}

/// Builds cell borders from vertical bars and horizontal rule flags.
fn cell_borders(bars: &[bool], column: usize, span: usize, top: bool, bottom: bool) -> Borders {
    Borders {
        top: top.then(solid_border),
        bottom: bottom.then(solid_border),
        start: bars
            .get(column)
            .copied()
            .unwrap_or(false)
            .then(solid_border),
        end: bars
            .get(column + span)
            .copied()
            .unwrap_or(false)
            .then(solid_border),
        ..Borders::default()
    }
}

fn flag_range(flags: &[bool], column: usize, span: usize) -> bool {
    (column..column + span).any(|index| flags.get(index).copied().unwrap_or(false))
}

fn union_flags(a: &[bool], b: &[bool]) -> Vec<bool> {
    let length = a.len().max(b.len());
    (0..length)
        .map(|index| {
            a.get(index).copied().unwrap_or(false) || b.get(index).copied().unwrap_or(false)
        })
        .collect()
}

/// Builds the column widths, distributing automatic columns over the text width.
fn build_grid(defs: &[ColumnDef], columns: usize) -> Vec<GridCol> {
    // Accumulate in `i64`: a declared width can saturate near `i32::MAX`, and a
    // sum of those overflows `i32` (a panic in debug builds).
    let fixed: i64 = (0..columns)
        .map(|index| i64::from(defs.get(index).and_then(|d| d.width).unwrap_or(0)))
        .sum();
    let auto = (0..columns)
        .filter(|index| defs.get(*index).and_then(|d| d.width).is_none())
        .count();
    let remaining = (i64::from(TEXT_WIDTH) - fixed).max(1);
    let auto_width = if auto > 0 {
        i32::try_from(remaining / auto as i64)
            .unwrap_or(i32::MAX)
            .max(1)
    } else {
        0
    };
    (0..columns)
        .map(|index| {
            let width = defs.get(index).and_then(|d| d.width).unwrap_or(auto_width);
            GridCol {
                width: Some(Twips(width.max(1))),
            }
        })
        .collect()
}

struct ExtractedRow {
    cells: Vec<Token>,
    has_content: bool,
    top: Vec<bool>,
    bottom: Vec<bool>,
    fill: Option<String>,
}

/// Splits a raw row segment into content tokens and rule/colour markers.
fn extract_row(segment: Vec<Token>, columns: usize) -> ExtractedRow {
    let mut cells = Vec::new();
    let mut top: Vec<bool> = vec![false; columns];
    let mut bottom: Vec<bool> = vec![false; columns];
    let mut fill = None;
    let mut has_content = false;
    let mut seen_content = false;
    let mut index = 0;
    while index < segment.len() {
        let token = &segment[index];
        if let Some((range, next)) = read_rule(&segment, index) {
            let flags = range_flags(range, columns);
            if seen_content {
                bottom = union_flags(&bottom, &flags);
            } else {
                top = union_flags(&top, &flags);
            }
            index = next;
            continue;
        }
        if let TokenKind::ControlWord(word) = &token.kind {
            if word == "rowcolor" {
                fill = read_color_arg(&segment, &mut index);
                continue;
            }
        }
        if !matches!(token.kind, TokenKind::Space) {
            seen_content = true;
            has_content = true;
        }
        cells.push(token.clone());
        index += 1;
    }
    ExtractedRow {
        cells,
        has_content,
        top,
        bottom,
        fill,
    }
}

/// Reads `\hline`, `\cline{m-n}`, `\toprule`, `\midrule`, `\bottomrule`.
fn read_rule(tokens: &[Token], index: usize) -> Option<(Option<(usize, usize)>, usize)> {
    let TokenKind::ControlWord(word) = &tokens[index].kind else {
        return None;
    };
    match word.as_str() {
        "hline" | "toprule" | "midrule" | "bottomrule" | "hdashline" => Some((None, index + 1)),
        "cline" | "cmidrule" => {
            let mut cursor = index + 1;
            while matches!(tokens.get(cursor).map(|t| &t.kind), Some(TokenKind::Space)) {
                cursor += 1;
            }
            if !matches!(tokens.get(cursor).map(|t| &t.kind), Some(TokenKind::LBrace)) {
                return Some((None, index + 1));
            }
            cursor += 1;
            let mut text = String::new();
            while let Some(token) = tokens.get(cursor) {
                match &token.kind {
                    TokenKind::RBrace => {
                        cursor += 1;
                        break;
                    }
                    kind => text.push_str(&kind.raw()),
                }
                cursor += 1;
            }
            Some((parse_cline_range(&text), cursor))
        }
        _ => None,
    }
}

fn parse_cline_range(text: &str) -> Option<(usize, usize)> {
    let text = text.trim().trim_start_matches('(').trim_end_matches(')');
    let (first, last) = text.split_once('-')?;
    let first: usize = first.trim().parse().ok()?;
    let last: usize = last.trim().parse().ok()?;
    (first >= 1 && last >= first).then_some((first, last))
}

fn range_flags(range: Option<(usize, usize)>, columns: usize) -> Vec<bool> {
    match range {
        None => vec![true; columns],
        Some((first, last)) => (0..columns)
            .map(|index| {
                let column = index + 1;
                column >= first && column <= last
            })
            .collect(),
    }
}

/// Reads a `{name}` colour argument, leaving `index` after the group.
fn read_color_arg(tokens: &[Token], index: &mut usize) -> Option<String> {
    *index += 1;
    while matches!(tokens.get(*index).map(|t| &t.kind), Some(TokenKind::Space)) {
        *index += 1;
    }
    if matches!(
        tokens.get(*index).map(|t| &t.kind),
        Some(TokenKind::LBracket)
    ) {
        skip_group(tokens, index);
    }
    if !matches!(tokens.get(*index).map(|t| &t.kind), Some(TokenKind::LBrace)) {
        return None;
    }
    *index += 1;
    let mut text = String::new();
    while let Some(token) = tokens.get(*index) {
        match &token.kind {
            TokenKind::RBrace => {
                *index += 1;
                break;
            }
            kind => text.push_str(&kind.raw()),
        }
        *index += 1;
    }
    Some(text)
}

fn skip_group(tokens: &[Token], index: &mut usize) {
    let mut depth = 0usize;
    while let Some(token) = tokens.get(*index) {
        match &token.kind {
            TokenKind::LBracket | TokenKind::LBrace => depth += 1,
            TokenKind::RBracket | TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    *index += 1;
                    break;
                }
            }
            _ => {}
        }
        *index += 1;
    }
}

/// Parses a cell's leading commands (`\multicolumn`, `\multirow`, `\cellcolor`).
fn parse_cell(tokens: &[Token]) -> ParsedCell {
    let mut cell = ParsedCell {
        span: 1,
        ..ParsedCell::default()
    };
    let mut index = 0;
    let mut explicit = false;
    while index < tokens.len() {
        while matches!(tokens.get(index).map(|t| &t.kind), Some(TokenKind::Space)) {
            index += 1;
        }
        let Some(TokenKind::ControlWord(word)) = tokens.get(index).map(|t| &t.kind) else {
            break;
        };
        match word.as_str() {
            "multicolumn" => {
                let mut cursor = index + 1;
                let count = read_group(tokens, &mut cursor);
                let spec = read_group(tokens, &mut cursor);
                let text = read_group_tokens(tokens, &mut cursor);
                cell.span = count
                    .as_deref()
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(1);
                cell.align = spec.as_deref().and_then(align_of);
                cell.tokens = text.unwrap_or_default();
                index = cursor;
                explicit = true;
            }
            "multirow" => {
                let mut cursor = index + 1;
                let count = read_group(tokens, &mut cursor);
                let _width = read_group(tokens, &mut cursor);
                let text = read_group_tokens(tokens, &mut cursor);
                cell.vmerge_rows = count
                    .as_deref()
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(1);
                cell.vmerge_restart = cell.vmerge_rows > 1;
                cell.tokens = text.unwrap_or_default();
                index = cursor;
                explicit = true;
            }
            "cellcolor" => {
                let mut cursor = index + 1;
                cell.shading = read_color_arg(tokens, &mut cursor);
                index = cursor;
            }
            _ => break,
        }
    }
    if !explicit {
        cell.tokens = tokens
            .get(index..)
            .map(<[Token]>::to_vec)
            .unwrap_or_default();
    }
    cell
}

fn read_group(tokens: &[Token], index: &mut usize) -> Option<String> {
    while matches!(tokens.get(*index).map(|t| &t.kind), Some(TokenKind::Space)) {
        *index += 1;
    }
    if !matches!(tokens.get(*index).map(|t| &t.kind), Some(TokenKind::LBrace)) {
        return None;
    }
    *index += 1;
    let mut text = String::new();
    let mut depth = 1usize;
    while let Some(token) = tokens.get(*index) {
        match &token.kind {
            TokenKind::LBrace => {
                depth += 1;
                text.push('{');
            }
            TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    *index += 1;
                    break;
                }
                text.push('}');
            }
            kind => text.push_str(&kind.raw()),
        }
        *index += 1;
    }
    Some(text)
}

fn read_group_tokens(tokens: &[Token], index: &mut usize) -> Option<Vec<Token>> {
    while matches!(tokens.get(*index).map(|t| &t.kind), Some(TokenKind::Space)) {
        *index += 1;
    }
    if !matches!(tokens.get(*index).map(|t| &t.kind), Some(TokenKind::LBrace)) {
        return None;
    }
    *index += 1;
    let mut out = Vec::new();
    let mut depth = 1usize;
    while let Some(token) = tokens.get(*index) {
        match &token.kind {
            TokenKind::LBrace => {
                depth += 1;
                out.push(token.clone());
            }
            TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    *index += 1;
                    break;
                }
                out.push(token.clone());
            }
            _ => out.push(token.clone()),
        }
        *index += 1;
    }
    Some(out)
}

fn align_of(spec: &str) -> Option<Justification> {
    if spec.contains('c') {
        Some(Justification::Center)
    } else if spec.contains('r') {
        Some(Justification::End)
    } else {
        Some(Justification::Start)
    }
}

/// Splits tokens at a top-level separator, tracking braces and nested envs.
fn split_top_level(tokens: &[Token], separator: &TokenKind) -> Vec<Vec<Token>> {
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

/// Parses a column specification into column definitions and vertical bars.
fn parse_columns(spec: &str) -> (Vec<ColumnDef>, Vec<bool>) {
    let mut columns: Vec<ColumnDef> = Vec::new();
    let mut bars: Vec<bool> = vec![false];
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '|' => {
                if let Some(last) = bars.last_mut() {
                    *last = true;
                }
            }
            ' ' | '\n' | '\t' => {}
            'l' => push_column(&mut columns, &mut bars, Justification::Start, None),
            'c' => push_column(&mut columns, &mut bars, Justification::Center, None),
            'r' => push_column(&mut columns, &mut bars, Justification::End, None),
            'X' => push_column(&mut columns, &mut bars, Justification::Both, None),
            'p' | 'm' | 'b' => {
                let width = read_dimension(&mut chars);
                push_column(&mut columns, &mut bars, Justification::Start, width);
            }
            '@' => {
                skip_braces(&mut chars);
            }
            '*' => {
                let count = read_group_chars(&mut chars)
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let inner = read_group_chars(&mut chars).unwrap_or_default();
                let (inner_columns, inner_bars) = parse_columns(&inner);
                for _ in 0..count {
                    columns.extend(inner_columns.iter().copied());
                    bars.extend(inner_bars.iter().copied());
                }
            }
            _ => {}
        }
    }
    (columns, bars)
}

fn push_column(
    columns: &mut Vec<ColumnDef>,
    bars: &mut Vec<bool>,
    align: Justification,
    width: Option<i32>,
) {
    columns.push(ColumnDef {
        align: Some(align),
        width,
    });
    bars.push(false);
}

fn read_dimension(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<i32> {
    let text = read_group_chars(chars)?;
    parse_dimension(&text)
}

fn parse_dimension(text: &str) -> Option<i32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("\\textwidth")
        || trimmed.contains("\\linewidth")
        || trimmed.contains("\\columnwidth")
    {
        let factor = trimmed
            .split('\\')
            .next()
            .and_then(|value| value.trim().parse::<f64>().ok())
            .unwrap_or(1.0);
        return Some((f64::from(TEXT_WIDTH) * factor) as i32);
    }
    for (unit, twips) in [
        ("cm", 566.929),
        ("mm", 56.6929),
        ("in", 1440.0),
        ("pt", 20.0),
        ("bp", 20.0),
        ("pc", 240.0),
    ] {
        if let Some(number) = trimmed.strip_suffix(unit) {
            if let Ok(value) = number.trim().parse::<f64>() {
                return Some((value * twips) as i32);
            }
        }
    }
    None
}

fn read_group_chars(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    if chars.peek() != Some(&'{') {
        return None;
    }
    chars.next();
    let mut text = String::new();
    let mut depth = 1usize;
    for c in chars.by_ref() {
        match c {
            '{' => {
                depth += 1;
                text.push('{');
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                text.push('}');
            }
            other => text.push(other),
        }
    }
    Some(text)
}

fn skip_braces(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let _ = read_group_chars(chars);
}
