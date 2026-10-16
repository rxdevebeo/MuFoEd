//! A revision-scoped interaction map from the shared renderer's placement.
use crate::{Address, EditError, Editor};
use std::borrow::Borrow;
use std::collections::{BTreeMap, HashSet};
use strict_ooxml_core::error::StrictError;
use strict_ooxml_render_svg::{
    style::{apply_caps, chosen_family},
    Item, PlacedPage, TextItem,
};
use strict_ooxml_render_svg::{MediaSource, RenderOptions};
use strict_ooxml_wml::model::{Block, Color, Document, DrawingKind, Graphic, Inline, RunContent};
use unicode_segmentation::UnicodeSegmentation;
/// A position in a modeled text node, using Unicode scalar offsets.
///
/// Beside a tab or a break with no text next to it - a tab that ends a
/// paragraph, a line of tabs only - there is no text node to stand in, and
/// the position names the tab itself: `inline` and `content` as for a text
/// node of its run (for a tab between runs, its inline path and content 0),
/// `offset` 0 before it and 1 after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPosition {
    /// Paragraph block address.
    pub paragraph: Address,
    /// Inline wrapper indices ending at a run. An empty path denotes the
    /// virtual caret of an empty paragraph (edit it with `Edit::Text`).
    pub inline: Vec<usize>,
    /// Text node index within the run.
    pub content: usize,
    /// Scalar offset within that text node.
    pub offset: usize,
}
/// Modeled drawing selected by its placed geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawingPosition {
    /// Paragraph block address.
    pub paragraph: Address,
    /// Inline wrapper indices ending at the drawing or its containing run.
    pub inline: Vec<usize>,
    /// Content index for a drawing in a run, otherwise None.
    pub content: Option<usize>,
}
/// Page-space rectangle in renderer pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualRect {
    /// Zero-based page index.
    pub page: usize,
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}
/// Interaction-map failure.
#[derive(Debug)]
pub enum VisualError {
    /// Placement failed.
    Render(StrictError),
    /// Model or revision is invalid.
    Edit(EditError),
    /// Attribution would change placement or cannot be proved.
    Attribution,
}
impl std::fmt::Display for VisualError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for VisualError {}
/// Caret/selection geometry tied to one exact model revision.
pub struct VisualMap {
    revision: u64,
    spans: Vec<Span>,
    objects: Vec<(DrawingPosition, VisualRect)>,
    nodes: Vec<(TextPosition, Vec<usize>)>,
}
struct Span {
    position: TextPosition,
    rect: VisualRect,
    stops: Vec<(usize, f64)>,
}
struct Node {
    position: TextPosition,
    text: String,
    cursor: usize,
    drawing: Option<DrawingPosition>,
    /// A tab: its caret anchor spans it, offset 0 before it and 1 after it.
    tab: bool,
}
impl VisualMap {
    /// Select across text nodes in logical story order. Reversed endpoints are
    /// accepted; endpoints in different stories or inside graphemes are rejected.
    pub fn selection_between(
        &self,
        revision: u64,
        anchor: &TextPosition,
        focus: &TextPosition,
    ) -> Result<Vec<VisualRect>, EditError> {
        self.check(revision)?;
        if anchor.paragraph.story != focus.paragraph.story {
            return Err(EditError::InvalidRange);
        }
        let locate = |p: &TextPosition| {
            self.nodes
                .iter()
                .position(|(node, boundaries)| same_node(node, p) && boundaries.contains(&p.offset))
                .ok_or(EditError::InvalidRange)
        };
        let a = locate(anchor)?;
        let b = locate(focus)?;
        let (first, last, start, end) = if (a, anchor.offset) <= (b, focus.offset) {
            (a, b, anchor.offset, focus.offset)
        } else {
            (b, a, focus.offset, anchor.offset)
        };
        let mut out = vec![];
        for index in first..=last {
            let Some((position, boundaries)) = self.nodes.get(index) else {
                continue;
            };
            if position.paragraph.story != anchor.paragraph.story {
                continue;
            }
            let mut position = position.clone();
            position.offset = if index == first { start } else { 0 };
            let end = if index == last {
                end
            } else {
                *boundaries.last().unwrap_or(&0)
            };
            out.extend(self.selection(revision, &position, end)?);
        }
        Ok(out)
    }
    /// All placed rectangles for a drawing or grouped graphic.
    pub fn drawing_rects(
        &self,
        revision: u64,
        position: &DrawingPosition,
    ) -> Result<Vec<VisualRect>, EditError> {
        self.check(revision)?;
        Ok(self
            .objects
            .iter()
            .filter(|(p, _)| p == position)
            .map(|(_, r)| *r)
            .collect())
    }
    /// Select the last painted drawing whose box contains this point.
    pub fn hit_drawing(
        &self,
        revision: u64,
        page: usize,
        x: f64,
        y: f64,
    ) -> Result<Option<DrawingPosition>, EditError> {
        self.check(revision)?;
        if !x.is_finite() || !y.is_finite() {
            return Err(EditError::InvalidRange);
        }
        Ok(self
            .objects
            .iter()
            .rev()
            .find(|(_, r)| {
                r.page == page && x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
            })
            .map(|(p, _)| p.clone()))
    }
    /// Find the selectable text boundary a click at this point means, as a
    /// word processor places a caret: the nearest line vertically (distance
    /// zero inside a line's box); on it, the stretch of text the click is in
    /// or to the right of (the one starting nearest to its left, or the
    /// leftmost when it is left of them all); in that stretch, the nearest
    /// boundary.
    ///
    /// Nearest-point-overall sent a click below a table to its last row, kept
    /// the down arrow inside a cell, and made the right part of an empty
    /// cell - whose only boundary sits at its left edge - belong to the next
    /// cell, so the bottom right cell could not be reached at all.
    pub fn hit_test(
        &self,
        revision: u64,
        page: usize,
        x: f64,
        y: f64,
    ) -> Result<Option<TextPosition>, EditError> {
        self.check(revision)?;
        if !x.is_finite() || !y.is_finite() {
            return Err(EditError::InvalidRange);
        }
        let distance = |s: &Span| {
            if y < s.rect.y {
                s.rect.y - y
            } else {
                (y - s.rect.y - s.rect.height).max(0.0)
            }
        };
        let on_page = || self.spans.iter().filter(|s| s.rect.page == page);
        let Some(nearest) = on_page().map(distance).min_by(f64::total_cmp) else {
            return Ok(None);
        };
        // Lines within a hundredth of a pixel of the nearest are as near.
        let line: Vec<&Span> = on_page().filter(|s| distance(s) - nearest < 1e-2).collect();
        let start = |s: &&Span| {
            s.stops
                .iter()
                .map(|(_, px)| *px)
                .fold(f64::INFINITY, f64::min)
        };
        let chosen = line
            .iter()
            .filter(|s| start(s) <= x + 0.5)
            .max_by(|a, b| start(a).total_cmp(&start(b)))
            .or_else(|| line.iter().min_by(|a, b| start(a).total_cmp(&start(b))));
        Ok(chosen.and_then(|s| {
            s.stops
                .iter()
                .min_by(|a, b| (x - a.1).abs().total_cmp(&(x - b.1).abs()))
                .map(|(offset, _)| {
                    let mut p = s.position.clone();
                    p.offset = *offset;
                    p
                })
        }))
    }
    /// Geometry for one logical caret, including repeated header occurrences.
    pub fn caret(
        &self,
        revision: u64,
        position: &TextPosition,
    ) -> Result<Vec<VisualRect>, EditError> {
        self.check(revision)?;
        let mut out = vec![];
        for s in &self.spans {
            if same_node(&s.position, position) {
                for (offset, x) in &s.stops {
                    if *offset == position.offset {
                        out.push(VisualRect {
                            x: *x,
                            width: 1.0,
                            ..s.rect
                        });
                    }
                }
            }
        }
        out.dedup_by(|a, b| {
            a.page == b.page && (a.x - b.x).abs() < 1e-7 && (a.y - b.y).abs() < 1e-7
        });
        Ok(out)
    }
    /// Rectangles covering a text-node scalar interval.
    pub fn selection(
        &self,
        revision: u64,
        position: &TextPosition,
        end: usize,
    ) -> Result<Vec<VisualRect>, EditError> {
        self.check(revision)?;
        if position.offset > end {
            return Err(EditError::InvalidRange);
        }
        let mut out = vec![];
        for s in &self.spans {
            if same_node(&s.position, position) {
                for pair in s.stops.windows(2) {
                    let &[(from, x0), (to, x1)] = pair else {
                        continue;
                    };
                    if from >= position.offset && to <= end {
                        out.push(VisualRect {
                            x: x0,
                            width: (x1 - x0).max(0.0),
                            ..s.rect
                        });
                    }
                }
            }
        }
        Ok(out)
    }
    fn check(&self, revision: u64) -> Result<(), EditError> {
        if revision == self.revision {
            Ok(())
        } else {
            Err(EditError::StaleRevision)
        }
    }
}
fn same_node(a: &TextPosition, b: &TextPosition) -> bool {
    a.paragraph == b.paragraph && a.inline == b.inline && a.content == b.content
}
impl Editor<'_> {
    /// Places the current model using shared renderer metrics and pagination.
    pub fn visual_map(
        &self,
        revision: u64,
        options: &RenderOptions,
        media: Option<&dyn MediaSource>,
    ) -> Result<VisualMap, VisualError> {
        if revision != self.revision() {
            return Err(VisualError::Edit(EditError::StaleRevision));
        }
        let anchored = options.clone().caret_anchors(true);
        let original = strict_ooxml_render_svg::place_pages(self.document, &anchored, media)
            .map_err(VisualError::Render)?;
        self.visual_map_placed(revision, options, media, &original)
    }

    /// [`Self::visual_map`] over a placement the caller already has, saving
    /// one layout: a viewer that paints from
    /// [`strict_ooxml_render_svg::place_document`] maps the same pages.
    ///
    /// `original` must be the current model placed under `options` with
    /// [`RenderOptions::caret_anchors`] on; any other placement fails with
    /// [`VisualError::Attribution`], never with a wrong map.
    #[allow(clippy::too_many_lines)]
    pub fn visual_map_placed<P: Borrow<PlacedPage>>(
        &self,
        revision: u64,
        options: &RenderOptions,
        media: Option<&dyn MediaSource>,
        original: &[P],
    ) -> Result<VisualMap, VisualError> {
        if revision != self.revision() {
            return Err(VisualError::Edit(EditError::StaleRevision));
        }
        self.validate().map_err(VisualError::Edit)?;
        // A disposable attribution copy changes text colour only. Neither text,
        // fonts, geometry nor the live document changes. Verify every placement
        // against the unmodified model before trusting attribution.
        // Both layouts place caret anchors (an empty paragraph's line, each
        // tab), so they compare item for item.
        let anchored = options.clone().caret_anchors(true);
        let unused: HashSet<String> = original
            .iter()
            .flat_map(|p| &p.borrow().items)
            .flat_map(Item::leaves)
            .flat_map(|i| match i {
                Item::Text(t) => vec![t.run.color.clone(), t.run.highlight.clone()],
                Item::Path(p) => vec![p.fill.clone(), p.stroke.clone()],
                Item::Rect(p) => vec![p.fill.clone(), p.stroke.clone()],
                Item::Line(p) => vec![Some(p.color.clone())],
                Item::Image(p) => vec![Some(p.alt.clone())],
                Item::Group(_) => Vec::new(),
            })
            .flatten()
            .collect();
        // The copy differs only in colours.
        let mut used = unused;
        let mut tagged = self.document.clone();
        let mut nodes = BTreeMap::new();
        stamp_document(&mut tagged, &mut used, &mut nodes)?;
        let placement = strict_ooxml_render_svg::place_pages(&tagged, &anchored, media)
            .map_err(VisualError::Render)?;
        if !same_placement(original, &placement) {
            return Err(VisualError::Attribution);
        }
        let provider = options.font_provider.make();
        let mut spans = vec![];
        let mut objects = vec![];
        for (page, p) in placement.iter().enumerate() {
            for item in &p.items {
                let object = match item {
                    Item::Group(group) => {
                        group.items.iter().flat_map(Item::leaves).find_map(|child| {
                            let key = match child {
                                Item::Image(image) => Some(image.alt.as_str()),
                                Item::Path(path) => path.fill.as_deref(),
                                Item::Text(text) => text.run.color.as_deref(),
                                _ => None,
                            }?;
                            let at = nodes.get(key)?.drawing.clone()?;
                            Some((
                                at,
                                VisualRect {
                                    page,
                                    x: group.bounds[0],
                                    y: group.bounds[1],
                                    width: group.bounds[2],
                                    height: group.bounds[3],
                                },
                            ))
                        })
                    }
                    Item::Image(i) => nodes.get(&i.alt).and_then(|n| n.drawing.clone()).map(|at| {
                        (
                            at,
                            VisualRect {
                                page,
                                x: i.x,
                                y: i.y,
                                width: i.w,
                                height: i.h,
                            },
                        )
                    }),
                    Item::Path(i) => i
                        .fill
                        .as_ref()
                        .and_then(|key| nodes.get(key))
                        .and_then(|n| n.drawing.clone())
                        .map(|at| {
                            (
                                at,
                                VisualRect {
                                    page,
                                    x: i.x,
                                    y: i.y,
                                    width: i.w,
                                    height: i.h,
                                },
                            )
                        }),
                    _ => None,
                };
                if let Some(object) = object {
                    objects.push(object);
                }
                let Item::Text(text) = item else {
                    continue;
                };
                let Some(node) = text.run.color.as_ref().and_then(|key| nodes.get_mut(key)) else {
                    continue;
                };
                if node.drawing.is_some() {
                    continue;
                }
                // Text that took an anchor's colour without being the anchor:
                // a list number of an empty paragraph, say.
                if (node.tab || node.position.inline.is_empty()) && !text.text.is_empty() {
                    continue;
                }
                if text.field.is_some() {
                    continue;
                }
                spans.push(text_span(text, node, page, provider.as_ref())?);
            }
        }
        Ok(VisualMap {
            revision,
            spans,
            objects,
            nodes: logical_nodes(&nodes),
        })
    }
}
fn logical_nodes(nodes: &BTreeMap<String, Node>) -> Vec<(TextPosition, Vec<usize>)> {
    nodes
        .values()
        .filter(|n| n.drawing.is_none())
        .map(|n| {
            if n.tab {
                return (n.position.clone(), vec![0, 1]);
            }
            let mut boundaries = vec![0];
            let mut offset = 0;
            for grapheme in n.text.graphemes(true) {
                offset += grapheme.chars().count();
                boundaries.push(offset);
            }
            (n.position.clone(), boundaries)
        })
        .collect()
}
/// Text as the layout shows it: capitals for `w:caps`, and for `w:smallCaps`
/// too, whose lower case letters are drawn as smaller capitals.
fn shown(text: &str, run: &strict_ooxml_render_svg::style::ComputedRun) -> String {
    if run.small_caps {
        text.to_uppercase()
    } else {
        apply_caps(text, run)
    }
}

fn text_span(
    text: &TextItem,
    node: &mut Node,
    page: usize,
    provider: &dyn strict_ooxml_render_svg::FontProvider,
) -> Result<Span, VisualError> {
    let family = chosen_family(&text.run, &text.text);
    let metrics = provider.metrics(&family, text.run.bold, text.run.italic);
    let rect = VisualRect {
        page,
        x: text.x,
        y: text.baseline - metrics.ascender * text.size_px,
        width: text.width,
        height: (metrics.ascender + metrics.descender) * text.size_px,
    };
    if node.tab {
        return Ok(Span {
            position: node.position.clone(),
            rect,
            stops: vec![(0, text.x), (1, text.x + text.width)],
        });
    }
    if node.position.inline.is_empty() {
        if text.width.abs() > 1e-7 {
            return Err(VisualError::Attribution);
        }
        return Ok(Span {
            position: node.position.clone(),
            rect,
            stops: vec![(0, text.x)],
        });
    }
    let rendered = shown(&node.text, &text.run);
    if node.cursor >= rendered.len() {
        node.cursor = 0;
    }
    let find = |needle: &str| {
        rendered
            .get(node.cursor..)
            .and_then(|tail| tail.find(needle))
            .map(|i| (i + node.cursor, needle.len()))
    };
    // A line broken by hyphenation shows a hyphen the text does not have.
    let (start, len) = find(&text.text)
        .or_else(|| {
            text.text
                .strip_suffix('-')
                .filter(|s| !s.is_empty())
                .and_then(find)
        })
        .ok_or(VisualError::Attribution)?;
    let end = start + len;
    node.cursor = end;
    let mut stops = vec![];
    let mut scalar = 0;
    let mut bytes = 0;
    for grapheme in node.text.graphemes(true) {
        if bytes >= start && bytes <= end {
            let prefix = rendered.get(start..bytes).ok_or(VisualError::Attribution)?;
            let width = measure(prefix, text, provider);
            stops.push((scalar, text.x + width));
        }
        scalar += grapheme.chars().count();
        bytes += shown(grapheme, &text.run).len();
    }
    if bytes >= start && bytes <= end {
        let prefix = rendered.get(start..bytes).ok_or(VisualError::Attribution)?;
        stops.push((scalar, text.x + measure(prefix, text, provider)));
    }
    Ok(Span {
        position: node.position.clone(),
        rect,
        stops,
    })
}
/// Converts a UTF-16 boundary; surrogate interiors are rejected.
pub fn utf16_to_scalar(text: &str, offset: usize) -> Result<usize, EditError> {
    let mut units = 0;
    for (scalar, ch) in text.chars().enumerate() {
        if units == offset {
            return Ok(scalar);
        }
        units += ch.len_utf16();
        if units > offset {
            return Err(EditError::InvalidRange);
        }
    }
    if units == offset {
        Ok(text.chars().count())
    } else {
        Err(EditError::InvalidRange)
    }
}
/// Converts a scalar boundary to UTF-16 code units.
pub fn scalar_to_utf16(text: &str, offset: usize) -> Result<usize, EditError> {
    if offset > text.chars().count() {
        return Err(EditError::InvalidRange);
    }
    Ok(text.chars().take(offset).map(char::len_utf16).sum())
}
fn measure(text: &str, item: &TextItem, font: &dyn strict_ooxml_render_svg::FontProvider) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let family = chosen_family(&item.run, &item.text);
    let shown = strict_ooxml_render_svg::font::present_text(&item.run.family, text);
    match item.advance {
        strict_ooxml_render_svg::TextAdvanceKind::Metric => {
            shown
                .chars()
                .map(|ch| font.advance_em(&family, ch, item.run.bold, item.run.italic))
                .sum::<f64>()
                * item.size_px
        }
        strict_ooxml_render_svg::TextAdvanceKind::Shaped => {
            strict_ooxml_render_svg::font::shape_text(
                &shown,
                &family,
                item.run.bold,
                item.run.italic,
                font,
            )
            .total_advance_em
                * item.size_px
        }
    }
}
// Exact equality is intentional: attribution must reproduce the same layout.
#[allow(clippy::float_cmp)]
fn same_placement<P: Borrow<PlacedPage>>(a: &[P], b: &[PlacedPage]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            let a = a.borrow();
            a.width_px == b.width_px && a.height_px == b.height_px && same_items(&a.items, &b.items)
        })
}

/// Two coordinates of one placement computed along different paths.
///
/// The attribution copy adds runs, so the layout sums the same line heights
/// in a different order and a coordinate can differ in its last bit (a table
/// of six empty rows put its rules 2.8e-14 px apart, and exact equality turned
/// every caret off). A millionth of a pixel is far below anything that moves
/// a glyph and far above that rounding.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

fn same_items(a: &[Item], b: &[Item]) -> bool {
    let all = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(a, b)| close(*a, *b));
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Item::Group(a), Item::Group(b)) => {
                all(&a.matrix, &b.matrix)
                    && all(&a.bounds, &b.bounds)
                    && match (a.clip, b.clip) {
                        (Some(a), Some(b)) => all(&a, &b),
                        (None, None) => true,
                        _ => false,
                    }
                    && same_items(&a.items, &b.items)
            }
            (Item::Text(a), Item::Text(b)) => {
                a.text == b.text
                    && all(
                        &[a.x, a.baseline, a.width, a.size_px],
                        &[b.x, b.baseline, b.width, b.size_px],
                    )
            }
            (Item::Rect(a), Item::Rect(b)) => all(&[a.x, a.y, a.w, a.h], &[b.x, b.y, b.w, b.h]),
            (Item::Line(a), Item::Line(b)) => {
                all(&[a.x1, a.x2, a.y1, a.y2], &[b.x1, b.x2, b.y1, b.y2])
            }
            (Item::Image(a), Item::Image(b)) => all(&[a.x, a.y, a.w, a.h], &[b.x, b.y, b.w, b.h]),
            (Item::Path(a), Item::Path(b)) => {
                all(&[a.x, a.y, a.w, a.h], &[b.x, b.y, b.w, b.h]) && a.d == b.d
            }
            _ => false,
        })
}
fn stamp_document(
    document: &mut Document,
    used: &mut HashSet<String>,
    nodes: &mut BTreeMap<String, Node>,
) -> Result<(), VisualError> {
    use crate::Story;
    stamp_blocks(&mut document.body.blocks, &Story::Body, &[], used, nodes)?;
    for h in &mut document.headers_footers {
        stamp_blocks(
            &mut h.blocks,
            &Story::HeaderFooter(h.part.clone()),
            &[],
            used,
            nodes,
        )?;
    }
    let mut foot = document.footnotes.iter().cloned().collect::<Vec<_>>();
    for n in &mut foot {
        stamp_blocks(&mut n.blocks, &Story::Footnote(n.id), &[], used, nodes)?;
        document.footnotes.insert(n.clone());
    }
    let mut end = document.endnotes.iter().cloned().collect::<Vec<_>>();
    for n in &mut end {
        stamp_blocks(&mut n.blocks, &Story::Endnote(n.id), &[], used, nodes)?;
        document.endnotes.insert(n.clone());
    }
    Ok(())
}
fn stamp_blocks(
    blocks: &mut [Block],
    story: &crate::Story,
    parents: &[crate::Container],
    used: &mut HashSet<String>,
    nodes: &mut BTreeMap<String, Node>,
) -> Result<(), VisualError> {
    for (block, b) in blocks.iter_mut().enumerate() {
        match b {
            Block::Paragraph(p) if p.revision.is_none() => {
                let address = Address {
                    story: story.clone(),
                    containers: parents.to_vec(),
                    block,
                };
                if !crate::structured::complex_fields(&p.inlines) {
                    let empty = p.inlines.iter().all(|i| {
                        matches!(i, Inline::Run(r)
                        if r.revision.is_none() && r.content.iter().all(|c|
                            matches!(c, RunContent::Text(t) if t.text.is_empty())))
                    });
                    if empty {
                        stamp_empty(p, &address, used, nodes)?;
                        continue;
                    }
                    stamp_inlines(&mut p.inlines, &address, &[], used, nodes)?;
                }
            }
            Block::Table(t) => {
                for (row, r) in t.rows.iter_mut().enumerate() {
                    for (cell, c) in r.cells.iter_mut().enumerate() {
                        let mut child = parents.to_vec();
                        child.push(crate::Container::Cell {
                            table: block,
                            row,
                            cell,
                        });
                        stamp_blocks(&mut c.blocks, story, &child, used, nodes)?;
                    }
                }
            }
            Block::SdtBlock(s) => {
                let mut child = parents.to_vec();
                child.push(crate::Container::Sdt(block));
                stamp_blocks(&mut s.blocks, story, &child, used, nodes)?;
            }
            _ => {}
        }
    }
    Ok(())
}
fn stamp_empty(
    paragraph: &mut strict_ooxml_wml::model::Paragraph,
    address: &Address,
    used: &mut HashSet<String>,
    nodes: &mut BTreeMap<String, Node>,
) -> Result<(), VisualError> {
    let key = next_color(used, nodes)?;
    // The paragraph mark carries the key; layout places a caret anchor for
    // the empty line in the mark's style. Only the colour changes.
    let mark = paragraph
        .props
        .run_props
        .get_or_insert_with(Default::default);
    mark.color = Some(Color::new(hex_digits(&key)));
    mark.color_theme = None;
    nodes.insert(
        key,
        Node {
            position: TextPosition {
                paragraph: address.clone(),
                inline: vec![],
                content: 0,
                offset: 0,
            },
            text: String::new(),
            cursor: 0,
            drawing: None,
            tab: false,
        },
    );
    Ok(())
}
/// The hex digits of an attribution key; `next_color` always prefixes `#`.
fn hex_digits(key: &str) -> &str {
    key.get(1..).unwrap_or_default()
}
fn next_color(
    used: &mut HashSet<String>,
    nodes: &BTreeMap<String, Node>,
) -> Result<String, VisualError> {
    for n in nodes.len() + 1..0x0100_0000 {
        let key = format!("#{n:06x}");
        if used.insert(key.clone()) {
            return Ok(key);
        }
    }
    Err(VisualError::Edit(EditError::LimitExceeded))
}
fn stamp_inlines(
    inlines: &mut Vec<Inline>,
    address: &Address,
    parents: &[usize],
    used: &mut HashSet<String>,
    nodes: &mut BTreeMap<String, Node>,
) -> Result<(), VisualError> {
    let original = std::mem::take(inlines);
    for (index, mut inline) in original.into_iter().enumerate() {
        let mut path = parents.to_vec();
        path.push(index);
        match &mut inline {
            Inline::Run(r) if r.revision.is_none() => {
                let complex = r
                    .content
                    .iter()
                    .any(|c| matches!(c, RunContent::FieldChar(_) | RunContent::InstrText(_)));
                if complex {
                    inlines.push(inline);
                    continue;
                }
                for (content, c) in r.content.iter().enumerate() {
                    let mut run = r.clone();
                    run.content = vec![c.clone()];
                    let tab = matches!(c, RunContent::Tab);
                    if let RunContent::Text(t) = c {
                        let key = next_color(used, nodes)?;
                        run.props.color = Some(Color::new(hex_digits(&key).to_owned()));
                        run.props.color_theme = None;
                        nodes.insert(
                            key,
                            Node {
                                position: TextPosition {
                                    paragraph: address.clone(),
                                    inline: path.clone(),
                                    content,
                                    offset: 0,
                                },
                                text: t.text.clone(),
                                cursor: 0,
                                drawing: None,
                                tab: false,
                            },
                        );
                    } else if tab {
                        let key = next_color(used, nodes)?;
                        run.props.color = Some(Color::new(hex_digits(&key).to_owned()));
                        run.props.color_theme = None;
                        nodes.insert(key, tab_node(address, &path, content));
                    }
                    if let Some(RunContent::Drawing(d)) = run.content.first_mut() {
                        stamp_drawing(d, address, &path, Some(content), used, nodes)?;
                    }
                    inlines.push(Inline::Run(run));
                }
                if r.content.is_empty() {
                    inlines.push(inline);
                }
                continue;
            }
            Inline::Hyperlink(v) => stamp_inlines(&mut v.inlines, address, &path, used, nodes)?,
            Inline::SdtInline(v) => stamp_inlines(&mut v.inlines, address, &path, used, nodes)?,
            Inline::Directional(v) => stamp_inlines(&mut v.inlines, address, &path, used, nodes)?,
            Inline::Drawing(d) => stamp_drawing(d, address, &path, None, used, nodes)?,
            Inline::Tab => {
                // A tab between runs is laid out as a tab in a run is; in the
                // copy it gets a run of its own to carry the key.
                let key = next_color(used, nodes)?;
                nodes.insert(key.clone(), tab_node(address, &path, 0));
                inlines.push(Inline::Run(strict_ooxml_wml::model::Run {
                    props: strict_ooxml_wml::model::props::RunProperties {
                        color: Some(Color::new(hex_digits(&key).to_owned())),
                        ..Default::default()
                    },
                    content: vec![RunContent::Tab],
                    revision: None,
                    location: strict_ooxml_core::error::SourceLocation::unknown(),
                }));
                continue;
            }
            _ => {}
        }
        // Drawings embedded in runs are traversed separately, preserving their index.
        if let Inline::Run(r) = &mut inline {
            for (content, c) in r.content.iter_mut().enumerate() {
                if let RunContent::Drawing(d) = c {
                    stamp_drawing(d, address, &path, Some(content), used, nodes)?;
                }
            }
        }
        inlines.push(inline);
    }
    Ok(())
}

/// The node of a tab's caret anchor: the tab's own position.
fn tab_node(address: &Address, path: &[usize], content: usize) -> Node {
    Node {
        position: TextPosition {
            paragraph: address.clone(),
            inline: path.to_vec(),
            content,
            offset: 0,
        },
        text: String::new(),
        cursor: 0,
        drawing: None,
        tab: true,
    }
}

fn stamp_drawing(
    d: &mut strict_ooxml_wml::model::Drawing,
    address: &Address,
    inline: &[usize],
    content: Option<usize>,
    used: &mut HashSet<String>,
    nodes: &mut BTreeMap<String, Node>,
) -> Result<(), VisualError> {
    fn mark_graphic(graphic: &mut Graphic, key: &str) {
        match graphic {
            Graphic::Picture(p) => p.descr = Some(key.into()),
            Graphic::Shape(s) => {
                s.fill = Some(strict_ooxml_wml::model::ShapeFill::Solid {
                    color: strict_ooxml_wml::model::ShapeColor {
                        value: Some(Color::new(hex_digits(key).to_owned())),
                        theme: None,
                        markup: None,
                    },
                });
            }
            Graphic::Group(g) => {
                for child in &mut g.children {
                    mark_graphic(child, key);
                }
            }
            _ => {}
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn walk(
        g: &mut Graphic,
        address: &Address,
        inline: &[usize],
        content: Option<usize>,
        graphics: &[usize],
        used: &mut HashSet<String>,
        nodes: &mut BTreeMap<String, Node>,
    ) -> Result<(), VisualError> {
        match g {
            Graphic::Shape(s) => {
                if let Some(text) = &mut s.text {
                    let mut parents = address.containers.clone();
                    parents.push(crate::Container::TextBox {
                        paragraph: address.block,
                        inline: inline.to_vec(),
                        content,
                        graphics: graphics.to_vec(),
                    });
                    stamp_blocks(&mut text.blocks, &address.story, &parents, used, nodes)?;
                }
            }
            Graphic::Group(g) => {
                for (index, child) in g.children.iter_mut().enumerate() {
                    let mut path = graphics.to_vec();
                    path.push(index);
                    walk(child, address, inline, content, &path, used, nodes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let key = next_color(used, nodes)?;
    let graphic = match &mut d.kind {
        DrawingKind::Inline(v) => {
            v.doc_pr
                .get_or_insert_with(strict_ooxml_wml::model::DocPr::default)
                .descr = Some(key.clone().into());
            v.graphic.as_mut()
        }
        DrawingKind::Anchor(v) => v.graphic.as_mut(),
        DrawingKind::Opaque(_) => return Ok(()),
    };
    mark_graphic(graphic, &key);
    nodes.insert(
        key,
        Node {
            position: TextPosition {
                paragraph: address.clone(),
                inline: inline.to_vec(),
                content: content.unwrap_or(usize::MAX),
                offset: 0,
            },
            text: String::new(),
            cursor: 0,
            drawing: Some(DrawingPosition {
                paragraph: address.clone(),
                inline: inline.to_vec(),
                content,
            }),
            tab: false,
        },
    );
    walk(graphic, address, inline, content, &[], used, nodes)
}
