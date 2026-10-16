//! Text positions inside a paragraph's inline tree.
//!
//! The editing kernel addresses text by `(paragraph, inline path, content
//! index, scalar offset)`. Moving a caret through a paragraph needs the text
//! nodes in reading order, which is what [`text_nodes`] lists; the wrappers it
//! descends are the ones the visual map descends (hyperlinks, inline content
//! controls and directional runs).

use strict_ooxml::edit::{InlinePoint, TextPosition};
use strict_ooxml::model::{Inline, Paragraph, Run, RunContent};
use unicode_segmentation::UnicodeSegmentation;

/// One text node: where it is and what it says.
#[derive(Clone, Debug)]
pub(crate) struct TextNodeRef {
    pub(crate) inline: Vec<usize>,
    pub(crate) content: usize,
    pub(crate) text: String,
}

impl TextNodeRef {
    /// Length in Unicode scalars, the kernel's unit.
    pub(crate) fn len(&self) -> usize {
        self.text.chars().count()
    }
}

/// The paragraph's text nodes in reading order.
pub(crate) fn text_nodes(paragraph: &Paragraph) -> Vec<TextNodeRef> {
    let mut out = Vec::new();
    walk(&paragraph.inlines, &mut Vec::new(), &mut out);
    out
}

fn walk(inlines: &[Inline], path: &mut Vec<usize>, out: &mut Vec<TextNodeRef>) {
    for (index, inline) in inlines.iter().enumerate() {
        path.push(index);
        match inline {
            Inline::Run(run) => {
                for (content, item) in run.content.iter().enumerate() {
                    if let RunContent::Text(node) = item {
                        out.push(TextNodeRef {
                            inline: path.clone(),
                            content,
                            text: node.text.clone(),
                        });
                    }
                }
            }
            Inline::Hyperlink(link) => walk(&link.inlines, path, out),
            Inline::SdtInline(sdt) => walk(&sdt.inlines, path, out),
            Inline::Directional(wrapper) => walk(&wrapper.inlines, path, out),
            _ => {}
        }
        path.pop();
    }
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

/// The first run of the paragraph in reading order, with its path.
fn first_run(inlines: &[Inline], path: &mut Vec<usize>) -> bool {
    for (index, inline) in inlines.iter().enumerate() {
        path.push(index);
        let found = match inline {
            Inline::Run(_) => true,
            Inline::Hyperlink(link) => first_run(&link.inlines, path),
            Inline::SdtInline(sdt) => first_run(&sdt.inlines, path),
            Inline::Directional(wrapper) => first_run(&wrapper.inlines, path),
            _ => false,
        };
        if found {
            return true;
        }
        path.pop();
    }
    false
}

/// The last run of the paragraph in reading order, with its path.
fn last_run<'a>(inlines: &'a [Inline], path: &mut Vec<usize>) -> Option<&'a Run> {
    for (index, inline) in inlines.iter().enumerate().rev() {
        path.push(index);
        let found = match inline {
            Inline::Run(run) => Some(run),
            Inline::Hyperlink(link) => last_run(&link.inlines, path),
            Inline::SdtInline(sdt) => last_run(&sdt.inlines, path),
            Inline::Directional(wrapper) => last_run(&wrapper.inlines, path),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
        path.pop();
    }
    None
}

/// The point before everything in the paragraph, when it has a run.
pub(crate) fn start_point(paragraph: &Paragraph) -> Option<InlinePoint> {
    let mut path = Vec::new();
    first_run(&paragraph.inlines, &mut path).then_some(InlinePoint {
        inline: path,
        content: 0,
        offset: 0,
    })
}

/// The point after everything in the paragraph, when it has a run.
pub(crate) fn end_point(paragraph: &Paragraph) -> Option<InlinePoint> {
    let mut path = Vec::new();
    let run = last_run(&paragraph.inlines, &mut path)?;
    Some(InlinePoint {
        inline: path,
        content: run.content.len(),
        offset: 0,
    })
}

/// The kernel point a caret stands on.
pub(crate) fn point_of(position: &TextPosition) -> InlinePoint {
    InlinePoint {
        inline: position.inline.clone(),
        content: position.content,
        offset: position.offset,
    }
}

/// Index of the node a caret stands in.
pub(crate) fn node_index(nodes: &[TextNodeRef], position: &TextPosition) -> Option<usize> {
    nodes
        .iter()
        .position(|node| node.inline == position.inline && node.content == position.content)
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
