//! Text positions inside a paragraph's inline tree.
//!
//! The editing kernel addresses text by `(paragraph, inline path, content
//! index, scalar offset)`. Moving a caret through a paragraph needs its pieces
//! in reading order - text nodes, and the tabs and breaks that each take one
//! caret position - which is what [`pieces`] lists; the wrappers it descends
//! are the ones the visual map descends (hyperlinks, inline content controls
//! and directional runs).

use strict_ooxml::edit::InlinePoint;
use strict_ooxml::model::{Inline, Paragraph, Run, RunContent};
use unicode_segmentation::UnicodeSegmentation;

/// What a piece of a paragraph is, for addressing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PieceKind {
    /// A text node of a run.
    Text,
    /// A tab, a break or another one-character item inside a run.
    RunAtom,
    /// A tab or a break between runs (`Inline::Tab`, `Inline::Break`),
    /// addressed by its top-level index.
    InlineAtom,
}

/// The character a drawing stands for in a paragraph's text: one caret
/// position, like a tab.
pub(crate) const OBJECT: char = '\u{FFFC}';

/// One piece of a paragraph in reading order: a text node, or an item that
/// takes one caret position (a tab, a line break).
#[derive(Clone, Debug)]
pub(crate) struct Piece {
    pub(crate) kind: PieceKind,
    pub(crate) inline: Vec<usize>,
    pub(crate) content: usize,
    /// The text, or the one character an atom stands for (`\t`, `\n`).
    pub(crate) text: String,
}

impl Piece {
    pub(crate) fn len(&self) -> usize {
        self.text.chars().count()
    }
}

/// The paragraph's pieces in reading order.
pub(crate) fn pieces(paragraph: &Paragraph) -> Vec<Piece> {
    let mut out = Vec::new();
    walk_pieces(&paragraph.inlines, &mut Vec::new(), &mut out);
    out
}

fn atom_char(content: &RunContent) -> Option<char> {
    match content {
        RunContent::Tab | RunContent::Ptab { .. } => Some('\t'),
        RunContent::Break(_) | RunContent::ClearingBreak(_) | RunContent::CarriageReturn => {
            Some('\n')
        }
        RunContent::NoBreakHyphen => Some('\u{2011}'),
        RunContent::Drawing(_) => Some(OBJECT),
        _ => None,
    }
}

fn walk_pieces(inlines: &[Inline], path: &mut Vec<usize>, out: &mut Vec<Piece>) {
    for (index, inline) in inlines.iter().enumerate() {
        path.push(index);
        match inline {
            Inline::Run(run) => {
                for (content, item) in run.content.iter().enumerate() {
                    if let RunContent::Text(node) = item {
                        out.push(Piece {
                            kind: PieceKind::Text,
                            inline: path.clone(),
                            content,
                            text: node.text.clone(),
                        });
                    } else if let Some(c) = atom_char(item) {
                        out.push(Piece {
                            kind: PieceKind::RunAtom,
                            inline: path.clone(),
                            content,
                            text: c.to_string(),
                        });
                    }
                }
            }
            Inline::Tab
            | Inline::Break(_)
            | Inline::Drawing(_)
            | Inline::Math(_)
            | Inline::MathParagraph(_)
                if path.len() == 1 =>
            {
                out.push(Piece {
                    kind: PieceKind::InlineAtom,
                    inline: path.clone(),
                    content: 0,
                    text: match inline {
                        Inline::Tab => "\t".to_owned(),
                        Inline::Drawing(_) | Inline::Math(_) | Inline::MathParagraph(_) => {
                            OBJECT.to_string()
                        }
                        _ => "\n".to_owned(),
                    },
                });
            }
            Inline::Hyperlink(link) => walk_pieces(&link.inlines, path, out),
            Inline::SdtInline(sdt) => walk_pieces(&sdt.inlines, path, out),
            Inline::Directional(wrapper) => walk_pieces(&wrapper.inlines, path, out),
            _ => {}
        }
        path.pop();
    }
}

/// The kernel point at a scalar offset into the paragraph's pieces, when one
/// exists there: inside or at either end of a text node, or beside an atom
/// inside a run. Beside atoms between runs only, there is none.
pub(crate) fn point_at(pieces: &[Piece], offset: usize) -> Option<InlinePoint> {
    let mut before = 0;
    for piece in pieces {
        let end = before + piece.len();
        match piece.kind {
            PieceKind::Text if (before..=end).contains(&offset) => {
                return Some(InlinePoint {
                    inline: piece.inline.clone(),
                    content: piece.content,
                    offset: offset - before,
                });
            }
            PieceKind::RunAtom if offset == before => {
                return Some(InlinePoint {
                    inline: piece.inline.clone(),
                    content: piece.content,
                    offset: 0,
                });
            }
            PieceKind::RunAtom if offset == end => {
                return Some(InlinePoint {
                    inline: piece.inline.clone(),
                    content: piece.content + 1,
                    offset: 0,
                });
            }
            _ => {}
        }
        before = end;
    }
    None
}

/// The run at an inline path, descending the same wrappers.
pub(crate) fn run_at<'a>(paragraph: &'a Paragraph, path: &[usize]) -> Option<&'a Run> {
    let (first, rest) = path.split_first()?;
    let mut inline = paragraph.inlines.get(*first)?;
    for index in rest {
        let children = match inline {
            Inline::Hyperlink(link) => &link.inlines,
            Inline::SdtInline(sdt) => &sdt.inlines,
            Inline::Directional(wrapper) => &wrapper.inlines,
            _ => return None,
        };
        inline = children.get(*index)?;
    }
    match inline {
        Inline::Run(run) => Some(run),
        _ => None,
    }
}

/// Scalar offset of the grapheme boundary before `offset`.
pub(crate) fn previous_boundary(text: &str, offset: usize) -> usize {
    let mut scalars = 0;
    for grapheme in text.graphemes(true) {
        let next = scalars + grapheme.chars().count();
        if next >= offset {
            return scalars;
        }
        scalars = next;
    }
    scalars
}

/// Scalar offset of the grapheme boundary after `offset`.
pub(crate) fn next_boundary(text: &str, offset: usize) -> usize {
    let mut scalars = 0;
    for grapheme in text.graphemes(true) {
        scalars += grapheme.chars().count();
        if scalars > offset {
            return scalars;
        }
    }
    scalars
}

/// The word around `offset`, as a scalar range, when there is one.
pub(crate) fn word_at(text: &str, offset: usize) -> Option<std::ops::Range<usize>> {
    let mut scalars = 0;
    for word in text.split_word_bounds() {
        let len = word.chars().count();
        let range = scalars..scalars + len;
        let is_word = word.chars().any(char::is_alphanumeric);
        if is_word && range.start <= offset && offset <= range.end {
            return Some(range);
        }
        scalars += len;
    }
    None
}

/// The text between two scalar offsets.
pub(crate) fn slice(text: &str, range: std::ops::Range<usize>) -> String {
    text.chars()
        .skip(range.start)
        .take(range.end.saturating_sub(range.start))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grapheme_boundaries_step_over_combining_marks() {
        let text = "ае\u{301}б";
        assert_eq!(next_boundary(text, 0), 1);
        assert_eq!(next_boundary(text, 1), 3);
        assert_eq!(previous_boundary(text, 3), 1);
        assert_eq!(previous_boundary(text, 4), 3);
        assert_eq!(previous_boundary(text, 0), 0);
        assert_eq!(next_boundary(text, 4), 4);
    }

    #[test]
    fn a_word_is_found_from_inside_or_at_its_edges() {
        let text = "один два, три";
        assert_eq!(word_at(text, 0), Some(0..4));
        assert_eq!(word_at(text, 6), Some(5..8));
        assert_eq!(word_at(text, 8), Some(5..8));
        assert_eq!(word_at(text, 4), Some(0..4));
        assert_eq!(slice(text, 5..8), "два");
    }
}
