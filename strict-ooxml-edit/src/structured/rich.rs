//! K06 (second part): splitting a paragraph at a point of its inline tree,
//! and the measure that lets paragraphs join whatever their inlines hold.
use crate::{EditError, Unsupported};
use strict_ooxml_wml::model::{FieldCharType, Inline, Paragraph, Run, RunContent, Space, TextNode};
use unicode_segmentation::UnicodeSegmentation;

/// A point in a paragraph: before content `content` of the run at the inline
/// path `inline`, `offset` scalars into it when that content is text.
pub(super) struct Point<'a> {
    pub(super) inline: &'a [usize],
    pub(super) content: usize,
    pub(super) offset: usize,
}

/// Splits `paragraph` at `point`: it keeps the inlines before the point and
/// the inlines after it are returned. Wrappers around the point (hyperlinks,
/// bidi overrides) go to both halves with their properties.
pub(super) fn split(
    paragraph: &mut Paragraph,
    point: &Point<'_>,
    graphemes: bool,
) -> Result<Vec<Inline>, EditError> {
    let right = split_inlines(&mut paragraph.inlines, point.inline, point, graphemes)?;
    // A complex field begun before the point and not ended by it would have
    // its instruction or result cut across two paragraphs.
    if field_depth(&paragraph.inlines) > 0 {
        return Err(EditError::UnsupportedContent(Unsupported::FieldResult));
    }
    Ok(right)
}

fn split_inlines(
    inlines: &mut Vec<Inline>,
    path: &[usize],
    point: &Point<'_>,
    graphemes: bool,
) -> Result<Vec<Inline>, EditError> {
    let (&index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    if index >= inlines.len() {
        return Err(EditError::InvalidParagraph);
    }
    let mut tail = inlines.split_off(index + 1);
    let item = inlines.pop().ok_or(EditError::InvalidParagraph)?;
    let (left, right) = match (item, rest.is_empty()) {
        (Inline::Run(run), true) => split_run(run, point, graphemes)?,
        (Inline::Hyperlink(mut wrapper), false) => {
            let inner = split_inlines(&mut wrapper.inlines, rest, point, graphemes)?;
            let mut other = wrapper.clone();
            other.inlines = inner;
            let keep = (!wrapper.inlines.is_empty(), !other.inlines.is_empty());
            pair(keep, Inline::Hyperlink(wrapper), Inline::Hyperlink(other))
        }
        (Inline::Directional(mut wrapper), false) => {
            let inner = split_inlines(&mut wrapper.inlines, rest, point, graphemes)?;
            let mut other = wrapper.clone();
            other.inlines = inner;
            let keep = (!wrapper.inlines.is_empty(), !other.inlines.is_empty());
            pair(
                keep,
                Inline::Directional(wrapper),
                Inline::Directional(other),
            )
        }
        // A content control has one identity, which two halves cannot share.
        (Inline::SdtInline(_), false) => {
            return Err(EditError::UnsupportedContent(Unsupported::Indivisible));
        }
        (Inline::Field(_), _) => {
            return Err(EditError::UnsupportedContent(Unsupported::FieldResult))
        }
        _ => return Err(EditError::UnsupportedContent(Unsupported::Target)),
    };
    inlines.extend(left);
    let mut after: Vec<Inline> = right.into_iter().collect();
    after.append(&mut tail);
    Ok(after)
}

/// The halves of a divided wrapper, without the empty ones.
fn pair((left, right): (bool, bool), a: Inline, b: Inline) -> (Option<Inline>, Option<Inline>) {
    (left.then_some(a), right.then_some(b))
}

fn split_run(
    mut run: Run,
    point: &Point<'_>,
    graphemes: bool,
) -> Result<(Option<Inline>, Option<Inline>), EditError> {
    if point.content > run.content.len() {
        return Err(EditError::InvalidRange);
    }
    let mut after = run.content.split_off(point.content);
    match after.first_mut() {
        Some(RunContent::Text(node)) => {
            let byte = byte_offset(&node.text, point.offset)?;
            if graphemes && !on_grapheme_boundary(&node.text, byte) {
                return Err(EditError::InvalidRange);
            }
            let rest = node.text.split_off(byte);
            let before = std::mem::replace(&mut node.text, rest);
            node.space = Space::Preserve;
            if !before.is_empty() {
                run.content.push(RunContent::Text(TextNode {
                    text: before,
                    space: Space::Preserve,
                }));
            }
            if node.text.is_empty() {
                after.remove(0);
            }
        }
        // Anything but text is one unit: the point is before it.
        _ if point.offset != 0 => return Err(EditError::InvalidRange),
        _ => {}
    }
    // A tracked run stays whole until the change is accepted or rejected.
    if run.revision.is_some() && !run.content.is_empty() && !after.is_empty() {
        return Err(EditError::UnsupportedContent(Unsupported::TrackedChange));
    }
    let mut other = run.clone();
    other.content = after;
    let keep = (!run.content.is_empty(), !other.content.is_empty());
    Ok(pair(keep, Inline::Run(run), Inline::Run(other)))
}

fn byte_offset(text: &str, scalars: usize) -> Result<usize, EditError> {
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .nth(scalars)
        .ok_or(EditError::InvalidRange)
}

/// Whether `byte` starts a grapheme of `text`, or ends it. Only this text node
/// is looked at: a cluster continued in the next run is not seen.
fn on_grapheme_boundary(text: &str, byte: usize) -> bool {
    byte == text.len() || text.grapheme_indices(true).any(|(start, _)| start == byte)
}

/// How many complex fields begun in `inlines` are still open at their end.
fn field_depth(inlines: &[Inline]) -> usize {
    fn walk(inlines: &[Inline], depth: &mut usize) {
        for inline in inlines {
            match inline {
                Inline::Run(run) => {
                    for content in &run.content {
                        if let RunContent::FieldChar(c) = content {
                            if c.kind == FieldCharType::Begin {
                                *depth += 1;
                            } else if c.kind == FieldCharType::End {
                                *depth = depth.saturating_sub(1);
                            }
                        }
                    }
                }
                Inline::Hyperlink(v) => walk(&v.inlines, depth),
                Inline::SdtInline(v) => walk(&v.inlines, depth),
                Inline::Directional(v) => walk(&v.inlines, depth),
                Inline::Field(v) => walk(&v.inlines, depth),
                _ => {}
            }
        }
    }
    let mut depth = 0;
    walk(inlines, &mut depth);
    depth
}

/// The scalars of every text node under `inlines`, wrappers included.
pub(super) fn scalars(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Run(run) => run
                .content
                .iter()
                .map(|content| match content {
                    RunContent::Text(node) => node.text.chars().count(),
                    _ => 0,
                })
                .fold(0, usize::saturating_add),
            Inline::Hyperlink(v) => scalars(&v.inlines),
            Inline::SdtInline(v) => scalars(&v.inlines),
            Inline::Directional(v) => scalars(&v.inlines),
            Inline::Field(v) => scalars(&v.inlines),
            _ => 0,
        })
        .fold(0, usize::saturating_add)
}
