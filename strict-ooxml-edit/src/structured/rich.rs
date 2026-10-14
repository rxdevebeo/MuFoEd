//! K06 (second part): splitting and formatting a paragraph at points of its
//! inline tree, and the measure that lets paragraphs join whatever their
//! inlines hold.
use crate::{EditError, FormatPatch, Unsupported};
use strict_ooxml_wml::model::{FieldCharType, Inline, Paragraph, Run, RunContent, Space, TextNode};
use unicode_segmentation::UnicodeSegmentation;

use super::command::InlinePoint;

/// Splits `paragraph` at `point`: it keeps the inlines before the point and
/// the inlines after it are returned. Wrappers around the point (hyperlinks,
/// bidi overrides) go to both halves with their properties.
pub(super) fn split(
    paragraph: &mut Paragraph,
    point: &InlinePoint,
    graphemes: bool,
) -> Result<Vec<Inline>, EditError> {
    let right = split_inlines(&mut paragraph.inlines, &point.inline, point, graphemes)?;
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
    point: &InlinePoint,
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
    point: &InlinePoint,
    graphemes: bool,
) -> Result<(Option<Inline>, Option<Inline>), EditError> {
    let after = cut(&mut run.content, point.content, point.offset, graphemes)?;
    // A tracked run stays whole until the change is accepted or rejected.
    if run.revision.is_some() && !run.content.is_empty() && !after.is_empty() {
        return Err(EditError::UnsupportedContent(Unsupported::TrackedChange));
    }
    let mut other = run.clone();
    other.content = after;
    let keep = (!run.content.is_empty(), !other.content.is_empty());
    Ok(pair(keep, Inline::Run(run), Inline::Run(other)))
}

/// Cuts `content` before item `index`, `offset` scalars into it when it is
/// text: `content` keeps what is before, and what is after is returned.
fn cut(
    content: &mut Vec<RunContent>,
    index: usize,
    offset: usize,
    graphemes: bool,
) -> Result<Vec<RunContent>, EditError> {
    if index > content.len() {
        return Err(EditError::InvalidRange);
    }
    let mut after = content.split_off(index);
    match after.first_mut() {
        Some(RunContent::Text(node)) => {
            let byte = byte_offset(&node.text, offset)?;
            if graphemes && !on_grapheme_boundary(&node.text, byte) {
                return Err(EditError::InvalidRange);
            }
            let rest = node.text.split_off(byte);
            let before = std::mem::replace(&mut node.text, rest);
            node.space = Space::Preserve;
            if !before.is_empty() {
                content.push(RunContent::Text(TextNode {
                    text: before,
                    space: Space::Preserve,
                }));
            }
            if node.text.is_empty() {
                after.remove(0);
            }
        }
        // Anything but text is one unit: the point is before it.
        _ if offset != 0 => return Err(EditError::InvalidRange),
        _ => {}
    }
    Ok(after)
}

/// Formats the run content between `from` and `to` with `patch`.
pub(super) fn format(
    paragraph: &mut Paragraph,
    from: &InlinePoint,
    to: &InlinePoint,
    patch: &FormatPatch,
    graphemes: bool,
) -> Result<(), EditError> {
    if from > to {
        return Err(EditError::InvalidRange);
    }
    for point in [from, to] {
        check_point(&paragraph.inlines, &point.inline, point, graphemes)?;
    }
    let mut path = Vec::new();
    format_inlines(&mut paragraph.inlines, &mut path, from, to, patch)
}

/// Whether `point` names a run and a place in it.
fn check_point(
    inlines: &[Inline],
    path: &[usize],
    point: &InlinePoint,
    graphemes: bool,
) -> Result<(), EditError> {
    let (&index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    let item = inlines.get(index).ok_or(EditError::InvalidParagraph)?;
    match (item, rest.is_empty()) {
        (Inline::Run(run), true) => {
            let mut content = run.content.clone();
            cut(&mut content, point.content, point.offset, graphemes).map(drop)
        }
        (Inline::Hyperlink(v), false) => check_point(&v.inlines, rest, point, graphemes),
        (Inline::SdtInline(v), false) => check_point(&v.inlines, rest, point, graphemes),
        (Inline::Directional(v), false) => check_point(&v.inlines, rest, point, graphemes),
        (Inline::Field(v), false) => check_point(&v.inlines, rest, point, graphemes),
        _ => Err(EditError::UnsupportedContent(Unsupported::Target)),
    }
}

/// Formats the runs under `inlines`, last first, so that the paths of the
/// points stay valid for the ones not yet visited.
fn format_inlines(
    inlines: &mut Vec<Inline>,
    path: &mut Vec<usize>,
    from: &InlinePoint,
    to: &InlinePoint,
    patch: &FormatPatch,
) -> Result<(), EditError> {
    for index in (0..inlines.len()).rev() {
        path.push(index);
        let pieces = match inlines.get_mut(index) {
            Some(Inline::Run(run)) => format_run(run, path, from, to, patch),
            Some(Inline::Hyperlink(v)) => {
                format_inlines(&mut v.inlines, path, from, to, patch).map(|()| None)
            }
            Some(Inline::SdtInline(v)) => {
                format_inlines(&mut v.inlines, path, from, to, patch).map(|()| None)
            }
            Some(Inline::Directional(v)) => {
                format_inlines(&mut v.inlines, path, from, to, patch).map(|()| None)
            }
            Some(Inline::Field(v)) => {
                format_inlines(&mut v.inlines, path, from, to, patch).map(|()| None)
            }
            _ => Ok(None),
        };
        path.pop();
        if let Some(pieces) = pieces? {
            drop(inlines.splice(index..=index, pieces));
        }
    }
    Ok(())
}

/// Where `point` falls in the run at `path` with `len` content items: its
/// start when the point is before the run, its end when after.
fn place(path: &[usize], point: &InlinePoint, len: usize) -> (usize, usize) {
    match point.inline.as_slice().cmp(path) {
        std::cmp::Ordering::Less => (0, 0),
        std::cmp::Ordering::Equal => (point.content, point.offset),
        std::cmp::Ordering::Greater => (len, 0),
    }
}

/// The pieces the run at `path` becomes, or `None` when it stays as it is.
fn format_run(
    run: &Run,
    path: &[usize],
    from: &InlinePoint,
    to: &InlinePoint,
    patch: &FormatPatch,
) -> Result<Option<Vec<Inline>>, EditError> {
    let len = run.content.len();
    let (start, end) = (place(path, from, len), place(path, to, len));
    if start >= end {
        return Ok(None);
    }
    let mut props = run.props.clone();
    crate::patch_properties(&mut props, patch);
    if props == run.props {
        return Ok(None);
    }
    // The end first: the start's place does not move.
    let mut head = run.content.clone();
    let tail = cut(&mut head, end.0, end.1, false)?;
    let middle = cut(&mut head, start.0, start.1, false)?;
    if middle.is_empty() {
        return Ok(None);
    }
    if run.revision.is_some() {
        return Err(EditError::UnsupportedContent(Unsupported::TrackedChange));
    }
    let piece = |content: Vec<RunContent>, props: &strict_ooxml_wml::model::RunProperties| {
        let mut piece = run.clone();
        piece.props = props.clone();
        piece.content = content;
        Inline::Run(piece)
    };
    let mut pieces = Vec::with_capacity(3);
    if !head.is_empty() {
        pieces.push(piece(head, &run.props));
    }
    pieces.push(piece(middle, &props));
    if !tail.is_empty() {
        pieces.push(piece(tail, &run.props));
    }
    Ok(Some(pieces))
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
