//! The formula the editor draws and changes: a small Presentation `MathML` tree.

/// One formula. The root is the inside of the `math` element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Formula {
    pub(crate) root: Row,
}

/// A horizontal sequence of nodes. An empty row is an editable slot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) nodes: Vec<Node>,
}

/// A piece of a formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    /// A run of digits, letters or an operator.
    Token(Token),
    /// A visible gap, written as `mspace`.
    Space,
    /// Numerator over denominator.
    Fraction { numerator: Row, denominator: Row },
    /// Square root, or an nth root when `degree` is set.
    Radical { degree: Option<Row>, radicand: Row },
    /// A base with an optional subscript and superscript.
    Script {
        base: Row,
        sub: Option<Row>,
        sup: Option<Row>,
    },
    /// A base with limits under it, over it, or both. `body` is the slot to the right of a sum, integral, product or limit.
    Limits {
        base: Row,
        under: Option<Row>,
        over: Option<Row>,
        body: Option<Row>,
    },
    /// Fences around one row, such as parentheses.
    Fenced {
        open: String,
        close: String,
        body: Row,
    },
    /// A grid, optionally wrapped in fences.
    Matrix {
        open: String,
        close: String,
        cells: Vec<Vec<Row>>,
    },
}

/// One `MathML` token element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) text: String,
    /// Draw upright. Function names and `mathvariant="normal"` use this.
    pub(crate) upright: bool,
}

/// Which token element a run becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// `mi`
    Identifier,
    /// `mn`
    Number,
    /// `mo`
    Operator,
    /// `mtext`
    Text,
}

/// Where the caret sits: a row, and a gap in that row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Caret {
    pub(crate) trail: Vec<Step>,
    pub(crate) index: usize,
}

/// One step from a parent row into a child slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    /// Index of the node in the parent row.
    pub(crate) index: usize,
    pub(crate) slot: Slot,
}

/// A row inside a structured node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Numerator of a fraction.
    Numerator,
    /// Denominator of a fraction.
    Denominator,
    /// Degree of a root.
    Degree,
    /// Content of a root.
    Radicand,
    /// Base of a script or a limit.
    Base,
    /// Subscript.
    Subscript,
    /// Superscript.
    Superscript,
    /// Limit under a base.
    Under,
    /// Limit over a base.
    Over,
    /// Inside fences.
    Body,
    /// One cell of a matrix, row then column.
    Cell(usize, usize),
}

impl Formula {
    pub(crate) fn row(&self, trail: &[Step]) -> Option<&Row> {
        let mut row = &self.root;
        for step in trail {
            row = row.nodes.get(step.index)?.slot(step.slot)?;
        }
        Some(row)
    }

    pub(crate) fn row_mut(&mut self, trail: &[Step]) -> Option<&mut Row> {
        let mut row = &mut self.root;
        for step in trail {
            row = row.nodes.get_mut(step.index)?.slot_mut(step.slot)?;
        }
        Some(row)
    }
}

impl Row {
    pub(crate) fn token(kind: TokenKind, text: &str, upright: bool) -> Self {
        Self {
            nodes: vec![Node::token(kind, text, upright)],
        }
    }
}

impl Node {
    pub(crate) fn token(kind: TokenKind, text: &str, upright: bool) -> Self {
        Self::Token(Token {
            kind,
            text: text.to_owned(),
            upright,
        })
    }

    pub(crate) fn slot(&self, slot: Slot) -> Option<&Row> {
        match (self, slot) {
            (Self::Fraction { numerator, .. }, Slot::Numerator) => Some(numerator),
            (Self::Fraction { denominator, .. }, Slot::Denominator) => Some(denominator),
            (
                Self::Radical {
                    degree: Some(degree),
                    ..
                },
                Slot::Degree,
            ) => Some(degree),
            (Self::Radical { radicand, .. }, Slot::Radicand) => Some(radicand),
            (Self::Script { base, .. } | Self::Limits { base, .. }, Slot::Base) => Some(base),
            (Self::Script { sub: Some(sub), .. }, Slot::Subscript) => Some(sub),
            (Self::Script { sup: Some(sup), .. }, Slot::Superscript) => Some(sup),
            (
                Self::Limits {
                    under: Some(under), ..
                },
                Slot::Under,
            ) => Some(under),
            (
                Self::Limits {
                    over: Some(over), ..
                },
                Slot::Over,
            ) => Some(over),
            (
                Self::Limits {
                    body: Some(body), ..
                }
                | Self::Fenced { body, .. },
                Slot::Body,
            ) => Some(body),
            (Self::Matrix { cells, .. }, Slot::Cell(row, column)) => cells.get(row)?.get(column),
            _ => None,
        }
    }

    pub(crate) fn slot_mut(&mut self, slot: Slot) -> Option<&mut Row> {
        match (self, slot) {
            (Self::Fraction { numerator, .. }, Slot::Numerator) => Some(numerator),
            (Self::Fraction { denominator, .. }, Slot::Denominator) => Some(denominator),
            (
                Self::Radical {
                    degree: Some(degree),
                    ..
                },
                Slot::Degree,
            ) => Some(degree),
            (Self::Radical { radicand, .. }, Slot::Radicand) => Some(radicand),
            (Self::Script { base, .. } | Self::Limits { base, .. }, Slot::Base) => Some(base),
            (Self::Script { sub: Some(sub), .. }, Slot::Subscript) => Some(sub),
            (Self::Script { sup: Some(sup), .. }, Slot::Superscript) => Some(sup),
            (
                Self::Limits {
                    under: Some(under), ..
                },
                Slot::Under,
            ) => Some(under),
            (
                Self::Limits {
                    over: Some(over), ..
                },
                Slot::Over,
            ) => Some(over),
            (
                Self::Limits {
                    body: Some(body), ..
                }
                | Self::Fenced { body, .. },
                Slot::Body,
            ) => Some(body),
            (Self::Matrix { cells, .. }, Slot::Cell(row, column)) => {
                cells.get_mut(row)?.get_mut(column)
            }
            _ => None,
        }
    }

    /// The slot the caret enters when moving right into this node.
    pub(crate) fn first_slot(&self) -> Option<Slot> {
        match self {
            Self::Fraction { .. } => Some(Slot::Numerator),
            Self::Radical {
                degree: Some(_), ..
            } => Some(Slot::Degree),
            Self::Radical { .. } => Some(Slot::Radicand),
            Self::Limits {
                under: Some(_),
                body: Some(_),
                ..
            } => Some(Slot::Under),
            Self::Limits {
                over: Some(_),
                body: Some(_),
                ..
            } => Some(Slot::Over),
            Self::Limits { body: Some(_), .. } | Self::Fenced { .. } => Some(Slot::Body),
            Self::Script { .. } | Self::Limits { .. } => Some(Slot::Base),
            Self::Matrix { cells, .. } => cells
                .first()
                .is_some_and(|row| !row.is_empty())
                .then_some(Slot::Cell(0, 0)),
            Self::Token(_) | Self::Space => None,
        }
    }

    /// The slot the caret enters when moving left into this node.
    pub(crate) fn last_slot(&self) -> Option<Slot> {
        match self {
            Self::Token(_) | Self::Space => None,
            Self::Fraction { .. } => Some(Slot::Denominator),
            Self::Radical { .. } => Some(Slot::Radicand),
            Self::Script { sup: Some(_), .. } => Some(Slot::Superscript),
            Self::Script { sub: Some(_), .. } => Some(Slot::Subscript),
            Self::Limits { body: Some(_), .. } | Self::Fenced { .. } => Some(Slot::Body),
            Self::Limits { over: Some(_), .. } => Some(Slot::Over),
            Self::Limits { under: Some(_), .. } => Some(Slot::Under),
            Self::Script { .. } | Self::Limits { .. } => Some(Slot::Base),
            Self::Matrix { cells, .. } => {
                let row = cells.len().checked_sub(1)?;
                let column = cells.get(row)?.len().checked_sub(1)?;
                Some(Slot::Cell(row, column))
            }
        }
    }
}

/// Keep the caret on a row that exists.
pub(crate) fn clamp_caret(formula: &Formula, caret: &mut Caret) {
    let mut row = &formula.root;
    let mut valid = 0;
    for step in &caret.trail {
        let Some(next) = row
            .nodes
            .get(step.index)
            .and_then(|node| node.slot(step.slot))
        else {
            break;
        };
        row = next;
        valid += 1;
    }
    caret.trail.truncate(valid);
    caret.index = caret.index.min(row.nodes.len());
}
