//! Caret movement and structural edits.

use crate::model::{clamp_caret, Caret, Formula, Node, Row, Slot, Step, TokenKind};

/// Letters and digits join a run of the same kind. `^` and `_` open a script.
pub(crate) fn insert_str(formula: &mut Formula, caret: &mut Caret, text: &str) -> bool {
    let mut changed = false;
    for ch in text.chars() {
        if ch.is_control() {
            continue;
        }
        changed = true;
        match ch {
            '^' => insert_script(formula, caret, false, true),
            '_' => insert_script(formula, caret, true, false),
            ' ' => insert_space(formula, caret),
            _ => insert_char(formula, caret, ch),
        }
    }
    changed
}

fn insert_char(formula: &mut Formula, caret: &mut Caret, ch: char) {
    let kind = kind_of(ch);
    let Some(row) = formula.row_mut(&caret.trail) else {
        return;
    };
    if caret.index > 0 {
        if let Some(Node::Token(token)) = row.nodes.get_mut(caret.index - 1) {
            let same = token.kind == kind && kind != TokenKind::Operator;
            if same {
                token.text.push(ch);
                if token.kind == TokenKind::Identifier && token.text.chars().count() > 1 {
                    token.upright = true;
                }
                return;
            }
        }
    }
    row.nodes
        .insert(caret.index, Node::token(kind, &ch.to_string(), false));
    caret.index += 1;
}

fn kind_of(ch: char) -> TokenKind {
    if ch.is_ascii_digit() {
        TokenKind::Number
    } else if ch.is_alphabetic() {
        TokenKind::Identifier
    } else {
        TokenKind::Operator
    }
}

fn insert_space(formula: &mut Formula, caret: &mut Caret) {
    let Some(row) = formula.row_mut(&caret.trail) else {
        return;
    };
    row.nodes.insert(caret.index, Node::Space);
    caret.index += 1;
}

pub(crate) fn backspace(formula: &mut Formula, caret: &mut Caret) {
    if caret.index > 0 {
        let Some(row) = formula.row_mut(&caret.trail) else {
            return;
        };
        let shrink = matches!(
            row.nodes.get(caret.index - 1),
            Some(Node::Token(token)) if token.text.chars().count() > 1
        );
        if shrink {
            if let Some(Node::Token(token)) = row.nodes.get_mut(caret.index - 1) {
                token.text.pop();
                if token.kind == TokenKind::Identifier && token.text.chars().count() == 1 {
                    token.upright = false;
                }
            }
            return;
        }
        row.nodes.remove(caret.index - 1);
        caret.index -= 1;
        return;
    }
    exit_or_unwrap(formula, caret);
}

pub(crate) fn delete_forward(formula: &mut Formula, caret: &mut Caret) {
    let Some(row) = formula.row(&caret.trail) else {
        return;
    };
    if caret.index >= row.nodes.len() {
        return;
    }
    let shrink = matches!(
        row.nodes.get(caret.index),
        Some(Node::Token(token)) if token.text.chars().count() > 1
    );
    let Some(row) = formula.row_mut(&caret.trail) else {
        return;
    };
    if shrink {
        if let Some(Node::Token(token)) = row.nodes.get_mut(caret.index) {
            token.text = token.text.chars().skip(1).collect();
        }
        return;
    }
    row.nodes.remove(caret.index);
}

/// Backspace at the start of a slot leaves it, and removes the slot when it is empty.
fn exit_or_unwrap(formula: &mut Formula, caret: &mut Caret) {
    let Some(step) = caret.trail.pop() else {
        return;
    };
    let empty = formula
        .row(&caret.trail)
        .and_then(|row| row.nodes.get(step.index))
        .and_then(|node| node.slot(step.slot))
        .is_some_and(|row| row.nodes.is_empty());
    if empty {
        if let Some(row) = formula.row_mut(&caret.trail) {
            if step.index < row.nodes.len() {
                row.nodes.remove(step.index);
            }
        }
        caret.index = step.index;
    } else {
        caret.trail.push(step);
    }
    clamp_caret(formula, caret);
}

pub(crate) fn move_left(formula: &Formula, caret: &mut Caret) {
    if caret.index > 0 {
        let entered = formula
            .row(&caret.trail)
            .and_then(|row| row.nodes.get(caret.index - 1))
            .and_then(Node::last_slot);
        if let Some(slot) = entered {
            caret.trail.push(Step {
                index: caret.index - 1,
                slot,
            });
            let len = formula.row(&caret.trail).map_or(0, |row| row.nodes.len());
            caret.index = len;
        } else {
            caret.index -= 1;
        }
        return;
    }
    if let Some(slot) = beside_operator(formula, caret, false) {
        enter_slot(formula, caret, slot, true);
        return;
    }
    if let Some(slot) = neighbor_cell(formula, caret, false) {
        if let Some(step) = caret.trail.last_mut() {
            step.slot = slot;
        }
        let len = formula.row(&caret.trail).map_or(0, |row| row.nodes.len());
        caret.index = len;
        return;
    }
    if let Some(step) = caret.trail.pop() {
        caret.index = step.index;
    }
}

pub(crate) fn move_right(formula: &Formula, caret: &mut Caret) {
    let len = formula.row(&caret.trail).map_or(0, |row| row.nodes.len());
    if caret.index < len {
        let entered = formula
            .row(&caret.trail)
            .and_then(|row| row.nodes.get(caret.index))
            .and_then(Node::first_slot);
        if let Some(slot) = entered {
            caret.trail.push(Step {
                index: caret.index,
                slot,
            });
            caret.index = 0;
        } else {
            caret.index += 1;
        }
        return;
    }
    if let Some(slot) = beside_operator(formula, caret, true) {
        enter_slot(formula, caret, slot, false);
        return;
    }
    if let Some(slot) = neighbor_cell(formula, caret, true) {
        if let Some(step) = caret.trail.last_mut() {
            step.slot = slot;
        }
        caret.index = 0;
        return;
    }
    if let Some(step) = caret.trail.pop() {
        caret.index = step.index + 1;
    }
}

pub(crate) fn move_vertical(formula: &Formula, caret: &mut Caret, up: bool) {
    let Some(step) = caret.trail.last().copied() else {
        return;
    };
    let parent: Vec<Step> = caret.trail[..caret.trail.len() - 1].to_vec();
    let Some(node) = formula
        .row(&parent)
        .and_then(|row| row.nodes.get(step.index))
    else {
        return;
    };
    let Some(slot) = vertical_slot(node, step.slot, up) else {
        return;
    };
    let len = node.slot(slot).map_or(0, |row| row.nodes.len());
    if let Some(last) = caret.trail.last_mut() {
        last.slot = slot;
    }
    caret.index = if up { len } else { 0 };
}

fn enter_slot(formula: &Formula, caret: &mut Caret, slot: Slot, end: bool) {
    if let Some(step) = caret.trail.last_mut() {
        step.slot = slot;
    }
    let len = formula.row(&caret.trail).map_or(0, |row| row.nodes.len());
    caret.index = if end { len } else { 0 };
}

/// The operand sits to the right of a sum, integral, product or limit.
fn beside_operator(formula: &Formula, caret: &Caret, forward: bool) -> Option<Slot> {
    let step = caret.trail.last().copied()?;
    let parent = &caret.trail[..caret.trail.len() - 1];
    let Node::Limits {
        body: Some(_),
        under,
        over,
        ..
    } = formula.row(parent)?.nodes.get(step.index)?
    else {
        return None;
    };
    if forward {
        (step.slot != Slot::Body).then_some(Slot::Body)
    } else if step.slot == Slot::Body {
        if under.is_some() {
            Some(Slot::Under)
        } else if over.is_some() {
            Some(Slot::Over)
        } else {
            Some(Slot::Base)
        }
    } else {
        None
    }
}

fn neighbor_cell(formula: &Formula, caret: &Caret, forward: bool) -> Option<Slot> {
    let step = caret.trail.last().copied()?;
    let Slot::Cell(row_index, column) = step.slot else {
        return None;
    };
    let parent: Vec<Step> = caret.trail[..caret.trail.len() - 1].to_vec();
    let Node::Matrix { cells, .. } = formula.row(&parent)?.nodes.get(step.index)? else {
        return None;
    };
    let row = cells.get(row_index)?;
    if forward {
        if column + 1 < row.len() {
            return Some(Slot::Cell(row_index, column + 1));
        }
        let next = cells.get(row_index + 1)?;
        if next.is_empty() {
            return None;
        }
        return Some(Slot::Cell(row_index + 1, 0));
    }
    if column > 0 {
        return Some(Slot::Cell(row_index, column - 1));
    }
    if row_index == 0 {
        return None;
    }
    let previous = cells.get(row_index - 1)?;
    Some(Slot::Cell(row_index - 1, previous.len().checked_sub(1)?))
}

fn vertical_slot(node: &Node, slot: Slot, up: bool) -> Option<Slot> {
    match (node, slot, up) {
        (Node::Fraction { .. }, Slot::Denominator, true) => Some(Slot::Numerator),
        (Node::Fraction { .. }, Slot::Numerator, false) => Some(Slot::Denominator),
        (
            Node::Radical {
                degree: Some(_), ..
            },
            Slot::Radicand,
            true,
        ) => Some(Slot::Degree),
        (
            Node::Radical {
                degree: Some(_), ..
            },
            Slot::Degree,
            false,
        ) => Some(Slot::Radicand),
        (Node::Script { sup: Some(_), .. }, Slot::Base | Slot::Subscript, true) => {
            Some(Slot::Superscript)
        }
        (Node::Script { sub: Some(_), .. }, Slot::Base | Slot::Superscript, false) => {
            Some(Slot::Subscript)
        }
        (Node::Limits { over: Some(_), .. }, Slot::Base | Slot::Under, true) => Some(Slot::Over),
        (Node::Limits { under: Some(_), .. }, Slot::Base | Slot::Over, false) => Some(Slot::Under),
        (Node::Script { .. }, Slot::Superscript, false)
        | (Node::Script { .. }, Slot::Subscript, true)
        | (Node::Limits { .. }, Slot::Over, false)
        | (Node::Limits { .. }, Slot::Under, true) => Some(Slot::Base),
        (Node::Matrix { cells, .. }, Slot::Cell(row, column), true) if row > 0 => {
            let above = cells.get(row - 1)?;
            Some(Slot::Cell(
                row - 1,
                column.min(above.len().saturating_sub(1)),
            ))
        }
        (Node::Matrix { cells, .. }, Slot::Cell(row, column), false) => {
            let below = cells.get(row + 1)?;
            Some(Slot::Cell(
                row + 1,
                column.min(below.len().saturating_sub(1)),
            ))
        }
        _ => None,
    }
}

pub(crate) fn insert_fraction(formula: &mut Formula, caret: &mut Caret) {
    insert_plain(
        formula,
        caret,
        Node::Fraction {
            numerator: Row::default(),
            denominator: Row::default(),
        },
    );
    enter(formula, caret, Slot::Numerator, false);
}

pub(crate) fn insert_sqrt(formula: &mut Formula, caret: &mut Caret) {
    insert_plain(
        formula,
        caret,
        Node::Radical {
            degree: None,
            radicand: Row::default(),
        },
    );
    enter(formula, caret, Slot::Radicand, false);
}

pub(crate) fn insert_root(formula: &mut Formula, caret: &mut Caret) {
    insert_plain(
        formula,
        caret,
        Node::Radical {
            degree: Some(Row::default()),
            radicand: Row::default(),
        },
    );
    enter(formula, caret, Slot::Degree, false);
}

pub(crate) fn insert_script(formula: &mut Formula, caret: &mut Caret, sub: bool, sup: bool) {
    if extend_script(formula, caret, sub, sup) {
        return;
    }
    insert_plain(
        formula,
        caret,
        Node::Script {
            base: Row::default(),
            sub: sub.then(Row::default),
            sup: sup.then(Row::default),
        },
    );
    let slot = if sup {
        Slot::Superscript
    } else if sub {
        Slot::Subscript
    } else {
        Slot::Base
    };
    enter(formula, caret, slot, false);
}

pub(crate) fn insert_fenced(formula: &mut Formula, caret: &mut Caret, open: &str, close: &str) {
    insert_plain(
        formula,
        caret,
        Node::Fenced {
            open: open.to_owned(),
            close: close.to_owned(),
            body: Row::default(),
        },
    );
    enter(formula, caret, Slot::Body, false);
}

pub(crate) fn insert_matrix(
    formula: &mut Formula,
    caret: &mut Caret,
    rows: usize,
    columns: usize,
    open: &str,
    close: &str,
) {
    let rows = rows.clamp(1, 8);
    let columns = columns.clamp(1, 8);
    let cells: Vec<Vec<Row>> = (0..rows)
        .map(|_| (0..columns).map(|_| Row::default()).collect())
        .collect();
    insert_plain(
        formula,
        caret,
        Node::Matrix {
            open: open.to_owned(),
            close: close.to_owned(),
            cells,
        },
    );
    enter(formula, caret, Slot::Cell(0, 0), false);
}

pub(crate) fn insert_limits(
    formula: &mut Formula,
    caret: &mut Caret,
    base: &str,
    under: bool,
    over: bool,
) {
    let kind = if base.chars().count() > 1 && base.chars().all(char::is_alphabetic) {
        TokenKind::Identifier
    } else {
        TokenKind::Operator
    };
    insert_plain(
        formula,
        caret,
        Node::Limits {
            base: Row::token(kind, base, true),
            under: under.then(Row::default),
            over: over.then(Row::default),
            body: Some(Row::default()),
        },
    );
    let slot = if under { Slot::Under } else { Slot::Over };
    enter(formula, caret, slot, false);
}

pub(crate) fn insert_function(formula: &mut Formula, caret: &mut Caret, name: &str) {
    let Some(row) = formula.row_mut(&caret.trail) else {
        return;
    };
    row.nodes
        .insert(caret.index, Node::token(TokenKind::Identifier, name, true));
    caret.index += 1;
}

pub(crate) fn insert_symbol(formula: &mut Formula, caret: &mut Caret, symbol: &str) {
    let kind = if symbol.chars().all(char::is_alphabetic) {
        TokenKind::Identifier
    } else {
        TokenKind::Operator
    };
    let upright = kind == TokenKind::Identifier && symbol.chars().count() > 1;
    insert_plain(formula, caret, Node::token(kind, symbol, upright));
    caret.index += 1;
}

pub(crate) fn insert_over(formula: &mut Formula, caret: &mut Caret, symbol: &str) {
    insert_plain(
        formula,
        caret,
        Node::Limits {
            base: Row::default(),
            under: None,
            over: Some(Row::token(TokenKind::Operator, symbol, true)),
            body: None,
        },
    );
    enter(formula, caret, Slot::Base, false);
}

/// Put `node` at the caret. The neighbours stay where they are.
fn insert_plain(formula: &mut Formula, caret: &mut Caret, node: Node) {
    let Some(row) = formula.row_mut(&caret.trail) else {
        return;
    };
    row.nodes.insert(caret.index, node);
}

fn enter(formula: &Formula, caret: &mut Caret, slot: Slot, end: bool) {
    caret.trail.push(Step {
        index: caret.index,
        slot,
    });
    let len = formula.row(&caret.trail).map_or(0, |row| row.nodes.len());
    caret.index = if end { len } else { 0 };
}

fn extend_script(formula: &mut Formula, caret: &mut Caret, sub: bool, sup: bool) -> bool {
    let Some(row) = formula.row_mut(&caret.trail) else {
        return false;
    };
    if caret.index == 0 {
        return false;
    }
    let Node::Script {
        sub: lower,
        sup: upper,
        ..
    } = &mut row.nodes[caret.index - 1]
    else {
        return false;
    };
    if sub && lower.is_none() {
        *lower = Some(Row::default());
        caret.trail.push(Step {
            index: caret.index - 1,
            slot: Slot::Subscript,
        });
        caret.index = 0;
        return true;
    }
    if sup && upper.is_none() {
        *upper = Some(Row::default());
        caret.trail.push(Step {
            index: caret.index - 1,
            slot: Slot::Superscript,
        });
        caret.index = 0;
        return true;
    }
    false
}

/// A range of sibling nodes in one row. `end` is exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) trail: Vec<Step>,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// The nodes between two carets in the same row.
pub(crate) fn selection_between(anchor: &Caret, head: &Caret) -> Option<Selection> {
    if anchor.trail != head.trail || anchor.index == head.index {
        return None;
    }
    let (start, end) = if anchor.index < head.index {
        (anchor.index, head.index)
    } else {
        (head.index, anchor.index)
    };
    Some(Selection {
        trail: anchor.trail.clone(),
        start,
        end,
    })
}

/// Remove the selected nodes and leave the caret where they were.
pub(crate) fn delete_selection(formula: &mut Formula, selection: &Selection, caret: &mut Caret) {
    if let Some(row) = formula.row_mut(&selection.trail) {
        let end = selection.end.min(row.nodes.len());
        let start = selection.start.min(end);
        if start < end {
            row.nodes.drain(start..end);
        }
    }
    caret.trail.clone_from(&selection.trail);
    caret.index = selection.start;
    clamp_caret(formula, caret);
}

/// Insert `nodes` at the caret.
pub(crate) fn insert_nodes(formula: &mut Formula, caret: &mut Caret, nodes: Vec<Node>) {
    let count = nodes.len();
    let Some(row) = formula.row_mut(&caret.trail) else {
        return;
    };
    for (offset, node) in nodes.into_iter().enumerate() {
        row.nodes.insert(caret.index + offset, node);
    }
    caret.index += count;
}

#[cfg(test)]
mod tests {
    use super::insert_str;
    use crate::mathml::{parse, write};
    use crate::model::{Caret, Formula};

    #[test]
    fn letters_join_into_one_identifier() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        assert!(insert_str(&mut formula, &mut caret, "sin"));
        let mathml = write(&formula);
        assert!(
            mathml.contains("<mi mathvariant=\"normal\">sin</mi>"),
            "{mathml}"
        );
    }

    #[test]
    fn a_superscript_is_inserted_at_the_caret() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        insert_str(&mut formula, &mut caret, "x^2");
        let mathml = write(&formula);
        assert!(mathml.contains("<mi>x</mi>"), "{mathml}");
        assert!(
            mathml.contains("<msup><mrow></mrow><mn>2</mn></msup>"),
            "{mathml}"
        );
        let again = parse(&mathml).unwrap();
        assert_eq!(mathml, write(&again));
    }

    #[test]
    fn a_sum_does_not_eat_the_symbol_before_the_caret() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        insert_str(&mut formula, &mut caret, "x");
        super::insert_limits(&mut formula, &mut caret, "∑", true, true);
        let mathml = write(&formula);
        assert!(mathml.contains("<mi>x</mi>"), "{mathml}");
        assert!(mathml.contains("∑"), "{mathml}");
    }

    #[test]
    fn a_symbol_is_its_own_token() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        insert_str(&mut formula, &mut caret, "x");
        super::insert_symbol(&mut formula, &mut caret, "α");
        let mathml = write(&formula);
        assert!(mathml.contains("<mi>x</mi><mi>α</mi>"), "{mathml}");
    }

    #[test]
    fn deleting_a_selection_removes_only_that_range() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        insert_str(&mut formula, &mut caret, "ab+c");
        let selection = super::Selection {
            trail: Vec::new(),
            start: 0,
            end: 2,
        };
        super::delete_selection(&mut formula, &selection, &mut caret);
        let mathml = write(&formula);
        assert!(mathml.contains("<mi>c</mi>"), "{mathml}");
        assert!(!mathml.contains('+'), "{mathml}");
    }

    #[test]
    fn a_sum_keeps_a_slot_on_its_right() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        insert_str(&mut formula, &mut caret, "x");
        super::insert_limits(&mut formula, &mut caret, "∑", true, true);
        let mathml = write(&formula);
        assert!(mathml.contains("<mi>x</mi>"), "{mathml}");
        assert!(
            mathml.contains(
                "<munderover><mo>∑</mo><mrow></mrow><mrow></mrow></munderover><mrow></mrow>"
            ),
            "{mathml}"
        );
        assert_eq!(mathml, write(&parse(&mathml).unwrap()));
    }

    #[test]
    fn an_accent_is_an_empty_template_at_the_caret() {
        let mut formula = Formula::default();
        let mut caret = Caret::default();
        insert_str(&mut formula, &mut caret, "a+b");
        super::insert_over(&mut formula, &mut caret, "ˆ");
        let mathml = write(&formula);
        assert!(mathml.contains("<mi>a</mi>"), "{mathml}");
        assert!(mathml.contains("<mi>b</mi>"), "{mathml}");
        assert!(
            mathml.contains("<mover><mrow></mrow><mo>ˆ</mo></mover>"),
            "{mathml}"
        );
    }
}
