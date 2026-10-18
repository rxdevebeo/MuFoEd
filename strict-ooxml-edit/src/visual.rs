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
    /// The stamped copy did not place like the original. Carets taken from
    /// it would sit on the wrong lines, so the whole map is refused. A fill
    /// that only changes colour, and one text fragment that cannot be bound,
    /// do not.
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
    formulas: Vec<(TextPosition, VisualRect)>,
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
    formula: bool,
    /// A tab: its caret anchor spans it, offset 0 before it and 1 after it.
    tab: bool,
}
impl VisualMap {
    /// Rectangles of an inline or display formula; boundary offsets are 0 and 1.
    pub fn formula_rects(
        &self,
        revision: u64,
        at: &TextPosition,
    ) -> Result<Vec<VisualRect>, EditError> {
        self.check(revision)?;
        Ok(self
            .formulas
            .iter()
            .filter(|(p, _)| same_node(p, at))
            .map(|(_, r)| *r)
            .collect())
    }

    /// The last painted inline or display formula containing a page point.
    pub fn hit_formula(
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
        Ok(self
            .formulas
            .iter()
            .rev()
            .find(|(_, r)| {
                r.page == page && x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
            })
            .map(|(p, _)| p.clone()))
    }
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
        // One source offset is the end of a fragment and the start of the
        // next. A justified line widens the space between those two x
        // positions, and both were drawn. The following glyph is the one
        // further along the line.
        let mut kept = Vec::new();
        for rect in out {
            if let Some(previous) = kept.iter_mut().find(|previous: &&mut VisualRect| {
                previous.page == rect.page && (previous.y - rect.y).abs() < 1.0
            }) {
                if rect.x > previous.x {
                    previous.x = rect.x;
                }
                continue;
            }
            kept.push(rect);
        }
        Ok(kept)
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
        // The copy differs in colour and, for a picture fill, the shape
        // description. Fill structure stays, so a gradient is still a path
        // and a picture fill is still an image.
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
        let mut formulas: Vec<(TextPosition, VisualRect)> = vec![];
        for (page, p) in placement.iter().enumerate() {
            for item in &p.items {
                // Display equations are placed as block primitives rather
                // than the single group used by inline equations. Attribute
                // their ink through the same colour-only disposable copy.
                if !matches!(item, Item::Group(_)) {
                    let key = match item {
                        Item::Text(t) => t.run.color.as_deref(),
                        Item::Line(l) => Some(l.color.as_str()),
                        Item::Path(p) => p.stroke.as_deref().or(p.fill.as_deref()),
                        _ => None,
                    };
                    if let Some(node) = key.and_then(|key| nodes.get(key)).filter(|n| n.formula) {
                        if let Some(rect) = formula_primitive_rect(item, page, provider.as_ref()) {
                            formulas.push((node.position.clone(), rect));
                        }
                    }
                }
                if let Item::Group(group) = item {
                    let formula = group.items.iter().flat_map(Item::leaves).find_map(|child| {
                        let key = match child {
                            Item::Text(t) => t.run.color.as_deref(),
                            Item::Line(l) => Some(l.color.as_str()),
                            Item::Path(p) => p.stroke.as_deref().or(p.fill.as_deref()),
                            _ => None,
                        }?;
                        nodes.get(key).filter(|node| node.formula)
                    });
                    if let Some(node) = formula {
                        let rect = VisualRect {
                            page,
                            x: group.bounds[0],
                            y: group.bounds[1],
                            width: group.bounds[2],
                            height: group.bounds[3],
                        };
                        formulas.push((node.position.clone(), rect));
                    }
                }
                let object = match item {
                    Item::Group(group) => {
                        group.items.iter().flat_map(Item::leaves).find_map(|child| {
                            let key = match child {
                                Item::Image(image) => Some(image.alt.as_str()),
                                Item::Path(path) => path.fill.as_deref().or(path.stroke.as_deref()),
                                Item::Rect(rect) => rect.fill.as_deref().or(rect.stroke.as_deref()),
                                Item::Line(line) => Some(line.color.as_str()),
                                Item::Text(text) => text.run.color.as_deref(),
                                Item::Group(_) => None,
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
                    Item::Text(text) if text.is_caret_anchor() => text
                        .run
                        .color
                        .as_ref()
                        .and_then(|key| nodes.get(key))
                        .and_then(|node| node.drawing.clone())
                        .map(|at| {
                            (
                                at,
                                VisualRect {
                                    page,
                                    x: text.x,
                                    y: text.baseline - text.size_px,
                                    width: text.width,
                                    height: text.size_px,
                                },
                            )
                        }),
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
                        .or_else(|| i.stroke.as_ref().and_then(|key| nodes.get(key)))
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
                if node.drawing.is_some() || node.formula {
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
                // One fragment that cannot be bound keeps no stops. The rest
                // of the map stays: a hyphen or a caps form the source does
                // not contain must not hide every other caret. A layout that
                // actually moved was already refused above.
                if let Some(span) = text_span(text, node, page, provider.as_ref()) {
                    spans.push(span);
                }
            }
        }
        let mut merged: Vec<(TextPosition, VisualRect)> = Vec::new();
        for (position, rect) in formulas {
            if let Some((_, old)) = merged
                .iter_mut()
                .find(|(p, r)| same_node(p, &position) && r.page == rect.page)
            {
                let right = (old.x + old.width).max(rect.x + rect.width);
                let bottom = (old.y + old.height).max(rect.y + rect.height);
                old.x = old.x.min(rect.x);
                old.y = old.y.min(rect.y);
                old.width = right - old.x;
                old.height = bottom - old.y;
            } else {
                merged.push((position, rect));
            }
        }
        let formulas = merged;
        for (position, rect) in &formulas {
            spans.push(Span {
                position: position.clone(),
                rect: *rect,
                stops: vec![(0, rect.x), (1, rect.x + rect.width)],
            });
        }
        for (position, rect) in &objects {
            spans.push(Span {
                position: TextPosition {
                    paragraph: position.paragraph.clone(),
                    inline: position.inline.clone(),
                    content: position.content.unwrap_or(0),
                    offset: 0,
                },
                rect: *rect,
                stops: vec![(0, rect.x), (1, rect.x + rect.width)],
            });
        }
        Ok(VisualMap {
            revision,
            spans,
            objects,
            formulas,
            nodes: logical_nodes(&nodes),
        })
    }
}
fn logical_nodes(nodes: &BTreeMap<String, Node>) -> Vec<(TextPosition, Vec<usize>)> {
    nodes
        .values()
        .map(|n| {
            if n.tab || n.formula || n.drawing.is_some() {
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
) -> Option<Span> {
    let family = chosen_family(&text.run, &text.text);
    let metrics = text.run.font_programs.for_resource(&family).map_or_else(
        || provider.metrics(&family, text.run.bold, text.run.italic),
        |program| program.metrics,
    );
    let rect = VisualRect {
        page,
        x: text.x,
        y: text.baseline - metrics.ascender * text.size_px,
        width: text.width,
        height: (metrics.ascender + metrics.descender) * text.size_px,
    };
    if node.tab {
        return Some(Span {
            position: node.position.clone(),
            rect,
            stops: vec![(0, text.x), (1, text.x + text.width)],
        });
    }
    if node.position.inline.is_empty() {
        // The empty-paragraph colour painted real ink. That is not a caret.
        if text.width.abs() > 1e-7 {
            return None;
        }
        return Some(Span {
            position: node.position.clone(),
            rect,
            stops: vec![(0, text.x)],
        });
    }
    let rendered = shown(&node.text, &text.run);
    let mut cursor = node.cursor;
    if cursor >= rendered.len() {
        cursor = 0;
    }
    let find = |needle: &str| {
        rendered
            .get(cursor..)
            .and_then(|tail| tail.find(needle))
            .map(|i| (i + cursor, needle.len()))
    };
    // A line broken by hyphenation shows a hyphen the text does not have.
    let (start, len) = find(&text.text).or_else(|| {
        text.text
            .strip_suffix('-')
            .filter(|s| !s.is_empty())
            .and_then(find)
    })?;
    let end = start + len;
    let positions = fragment_positions(text, provider);
    let position_at = |byte: usize| {
        positions
            .iter()
            .find(|(at, _)| *at == byte)
            .map(|(_, x)| *x)
    };
    // Spaces the line starts with are laid out as an advance before the
    // item, not drawn in it; their carets stand in that advance.
    let skipped = rendered
        .get(cursor..start)
        .filter(|gap| !gap.is_empty() && gap.chars().all(char::is_whitespace))
        .map(|_| cursor);
    let mut stops = vec![];
    let mut scalar = 0;
    let mut bytes = 0;
    for grapheme in node.text.graphemes(true) {
        if skipped.is_some_and(|from| bytes >= from && bytes < start) {
            let gap = rendered.get(bytes..start)?;
            stops.push((scalar, text.x - measure(gap, text, provider)));
        }
        if bytes >= start && bytes <= end {
            stops.push((scalar, position_at(bytes - start)?));
        }
        scalar += grapheme.chars().count();
        bytes += shown(grapheme, &text.run).len();
    }
    if bytes >= start && bytes <= end {
        stops.push((scalar, position_at(bytes - start)?));
    }
    node.cursor = end;
    Some(Span {
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
/// Origins from the whole painted fragment, not independently re-shaped
/// prefixes. This keeps font programs, kerning, ligatures and CJK spacing on
/// the same coordinates as SVG paint. Source byte boundaries remain intact
/// when a legacy symbol face maps characters to their display equivalents.
fn fragment_positions(
    item: &TextItem,
    font: &dyn strict_ooxml_render_svg::FontProvider,
) -> Vec<(usize, f64)> {
    use strict_ooxml_render_svg::{font, style, TextAdvanceKind};
    let shown = font::present_text(&item.run.family, &item.text);
    let family = chosen_family(&item.run, &item.text);
    let xs = match item.advance {
        TextAdvanceKind::Metric => {
            let chars: Vec<_> = shown.chars().collect();
            let mut cursor = item.x;
            let mut previous = None;
            chars
                .iter()
                .enumerate()
                .map(|(index, &ch)| {
                    let x = cursor;
                    let next = chars.get(index + 1).copied().or(item.following_non_space);
                    let factor = style::compressed_char_factor(
                        previous,
                        ch,
                        next,
                        item.compress_punctuation,
                        item.plain_space_factor,
                    );
                    let ordinary = font::embedded::run_advance_em(&item.run, &family, ch, font);
                    cursor += style::space_advance_em(&item.run, &family, ch, ordinary)
                        * item.size_px
                        * factor;
                    if !ch.is_whitespace() {
                        cursor += style::spacing_px(&item.run, item.size_px);
                    }
                    if !ch.is_whitespace() || factor > 0.0 {
                        previous = Some(ch);
                    }
                    x
                })
                .collect::<Vec<_>>()
        }
        TextAdvanceKind::Shaped => {
            let shaped = font::shape_run(&shown, &family, &item.run, font);
            let mut xs = font::unicode_x_positions_px(&shown, &shaped, item.x, item.size_px);
            let deltas = font::shaped_cluster_offsets_px(item, &shown, &shaped);
            let mut cluster = 0;
            for (scalar, (byte, _)) in shown.char_indices().enumerate() {
                while shaped
                    .clusters
                    .get(cluster)
                    .is_some_and(|c| c.byte_end <= byte)
                {
                    cluster += 1;
                }
                if let (Some(x), Some(delta)) = (xs.get_mut(scalar), deltas.get(cluster)) {
                    *x += delta;
                }
            }
            xs
        }
    };
    // The map's scalar intervals currently use ascending logical widths.
    // Preserve that convention for RTL fragments rather than treating their
    // decreasing visual glyph origins as negative-width selections.
    if xs
        .windows(2)
        .any(|pair| matches!(pair, [a, b] if *a > *b + 1e-7))
    {
        let mut positions: Vec<_> = item
            .text
            .char_indices()
            .map(|(byte, _)| {
                (
                    byte,
                    item.x + measure(&item.text[..byte], item, font) * item.run.char_scale,
                )
            })
            .collect();
        positions.push((item.text.len(), item.x + item.width));
        return positions;
    }
    let mut positions: Vec<_> = item
        .text
        .char_indices()
        .zip(xs)
        .map(|((byte, _), x)| (byte, item.x + (x - item.x) * item.run.char_scale))
        .collect();
    // The placed width includes justification and the trailing advance.
    positions.push((item.text.len(), item.x + item.width));
    positions
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
                .map(|ch| {
                    strict_ooxml_render_svg::font::embedded::run_advance_em(
                        &item.run, &family, ch, font,
                    )
                })
                .sum::<f64>()
                * item.size_px
        }
        strict_ooxml_render_svg::TextAdvanceKind::Shaped => {
            strict_ooxml_render_svg::font::shape_run(&shown, &family, &item.run, font)
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

/// Item tree, text and geometry. Fill colour, stroke colour, image alt and
/// gradient stops are paint: the stamp changes those and nothing else. A
/// path, image or group that appears or disappears still fails, because the
/// carets after it would be read from the wrong place.
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
                let empty = p.inlines.iter().all(|i| match i {
                    Inline::Run(r) => {
                        r.revision.is_none()
                            && r.content
                                .iter()
                                .all(|c| matches!(c, RunContent::Text(t) if t.text.is_empty()))
                    }
                    Inline::BookmarkStart(_)
                    | Inline::BookmarkEnd(_)
                    | Inline::CommentRangeStart(_)
                    | Inline::CommentRangeEnd(_)
                    | Inline::CommentReference(_) => true,
                    _ => false,
                });
                if empty {
                    stamp_empty(p, &address, used, nodes)?;
                    continue;
                }
                // A hyperlink field used to drop the whole paragraph, so the
                // caret could not stand on its ordinary text either. Field
                // runs themselves stay unmarked.
                stamp_inlines(
                    &mut p.inlines,
                    &address,
                    &[],
                    p.props.run_props.as_ref(),
                    used,
                    nodes,
                )?;
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
            formula: false,
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
#[allow(clippy::too_many_lines)]
fn stamp_inlines(
    inlines: &mut Vec<Inline>,
    address: &Address,
    parents: &[usize],
    mark_props: Option<&strict_ooxml_wml::model::props::RunProperties>,
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
                // Each text node was already its own segment. Splitting the
                // run changes addition order only; a wrap or a kerning shift
                // still fails the placement compare.
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
                                formula: false,
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
            Inline::Hyperlink(v) => {
                stamp_inlines(&mut v.inlines, address, &path, mark_props, used, nodes)?;
            }
            Inline::SdtInline(v) => {
                stamp_inlines(&mut v.inlines, address, &path, mark_props, used, nodes)?;
            }
            Inline::Directional(v) => {
                stamp_inlines(&mut v.inlines, address, &path, mark_props, used, nodes)?;
            }
            Inline::Drawing(d) => stamp_drawing(d, address, &path, None, used, nodes)?,
            Inline::Math(expression) => {
                let key = next_color(used, nodes)?;
                stamp_formula(expression, &key);
                nodes.insert(
                    key,
                    Node {
                        position: TextPosition {
                            paragraph: address.clone(),
                            inline: path.clone(),
                            content: 0,
                            offset: 0,
                        },
                        text: String::new(),
                        cursor: 0,
                        drawing: None,
                        formula: true,
                        tab: false,
                    },
                );
            }
            Inline::MathParagraph(paragraph) => {
                let key = next_color(used, nodes)?;
                for expression in &mut paragraph.equations {
                    stamp_formula(expression, &key);
                }
                nodes.insert(
                    key,
                    Node {
                        position: TextPosition {
                            paragraph: address.clone(),
                            inline: path.clone(),
                            content: 0,
                            offset: 0,
                        },
                        text: String::new(),
                        cursor: 0,
                        drawing: None,
                        formula: true,
                        tab: false,
                    },
                );
            }
            Inline::Tab => {
                // A tab between runs is laid out from the paragraph mark.
                // The copy needs a run to carry the key, and that run must
                // wear the mark's own properties or the anchor's size drifts
                // and the whole map is refused.
                let key = next_color(used, nodes)?;
                nodes.insert(key.clone(), tab_node(address, &path, 0));
                let mut props = mark_props.cloned().unwrap_or_default();
                props.color = Some(Color::new(hex_digits(&key).to_owned()));
                props.color_theme = None;
                inlines.push(Inline::Run(strict_ooxml_wml::model::Run {
                    props,
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

/// Recolours a shape in place. The attribution copy must paint the same
/// items as the original: a picture fill stays an image, a gradient stays
/// one path, and an `a:alpha` on a solid fill stays a translucent group.
/// Replacing any of those with a flat solid drops or adds an item and the
/// caret map is refused.
fn tint_shape(shape: &mut strict_ooxml_wml::model::Shape, key: &str) {
    use strict_ooxml_wml::model::{GradientStop, ShapeFill};
    // A picture fill's image uses this description as its alt text.
    shape.descr = Some(key.into());
    if let Some(stroke) = shape.stroke.as_mut().filter(|stroke| !stroke.none) {
        tint_shape_color(
            stroke.color.get_or_insert_with(|| keyed_shape_color(key)),
            key,
        );
    }
    // A missing fill and an explicit noFill have no colour for the path.
    // A solid on the copy does not add an item; an open outline still
    // carries the key on its stroke.
    let bare = matches!(shape.fill, Some(ShapeFill::None) | None);
    match &mut shape.fill {
        Some(ShapeFill::Solid { color }) => tint_shape_color(color, key),
        Some(ShapeFill::Gradient { stops, .. }) => {
            if let Some(stop) = stops.first_mut() {
                tint_shape_color(&mut stop.color, key);
            } else {
                stops.push(GradientStop {
                    position: 0,
                    color: keyed_shape_color(key),
                });
            }
        }
        Some(ShapeFill::Pattern { foreground, .. }) => match foreground {
            Some(color) => tint_shape_color(color, key),
            None => *foreground = Some(keyed_shape_color(key)),
        },
        Some(ShapeFill::Blip { .. } | ShapeFill::None) | None => {}
    }
    if bare {
        shape.fill = Some(ShapeFill::Solid {
            color: keyed_shape_color(key),
        });
    }
}

/// Sets the explicit colour the renderer paints, and leaves `a:alpha` markup
/// so a translucent solid stays wrapped in a group.
fn tint_shape_color(color: &mut strict_ooxml_wml::model::ShapeColor, key: &str) {
    color.value = Some(Color::new(hex_digits(key).to_owned()));
    color.theme = None;
}

fn keyed_shape_color(key: &str) -> strict_ooxml_wml::model::ShapeColor {
    strict_ooxml_wml::model::ShapeColor {
        value: Some(Color::new(hex_digits(key).to_owned())),
        theme: None,
        markup: None,
    }
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
        formula: false,
        tab: true,
    }
}

#[allow(clippy::too_many_lines)]
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
            Graphic::Shape(s) => tint_shape(s, key),
            Graphic::Group(g) => {
                for child in &mut g.children {
                    mark_graphic(child, key);
                }
            }
            Graphic::Chart(refs) => {
                // A chart nested in a grouped drawing has no independent
                // drawing anchor; its ink still identifies the parent group.
                if let Some(chart) = &mut refs.chart {
                    let chart = std::sync::Arc::make_mut(chart);
                    for series in &mut chart.series {
                        series.color = Some(hex_digits(key).to_owned());
                        for (_, color) in &mut series.point_colors {
                            hex_digits(key).clone_into(color);
                        }
                        if matches!(
                            chart.kind,
                            strict_ooxml_wml::model::chart::ChartKind::Pie { .. }
                        ) {
                            series.point_colors = (0..series.values.len())
                                .map(|index| (index, hex_digits(key).to_owned()))
                                .collect();
                        }
                    }
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
        DrawingKind::Anchor(v) => {
            v.doc_pr
                .get_or_insert_with(strict_ooxml_wml::model::DocPr::default)
                .descr = Some(key.clone().into());
            v.graphic.as_mut()
        }
        DrawingKind::Opaque(_) => return Ok(()),
    };
    // Standalone charts use their full-extent anchor, including empty charts.
    // Leave their cached series untouched, even on the attribution copy.
    if !matches!(graphic, Graphic::Chart(_)) {
        mark_graphic(graphic, &key);
    }
    nodes.insert(
        key,
        Node {
            position: TextPosition {
                paragraph: address.clone(),
                inline: inline.to_vec(),
                content: content.unwrap_or(0),
                offset: 0,
            },
            text: String::new(),
            cursor: 0,
            drawing: Some(DrawingPosition {
                paragraph: address.clone(),
                inline: inline.to_vec(),
                content,
            }),
            formula: false,
            tab: false,
        },
    );
    walk(graphic, address, inline, content, &[], used, nodes)
}

fn stamp_formula(expression: &mut strict_ooxml_wml::model::MathExpression, key: &str) {
    for node in &mut expression.nodes {
        use strict_ooxml_wml::model::math::MathNode;
        let control = match node {
            MathNode::Run(n) => &mut n.properties.control,
            MathNode::Fraction(n) => &mut n.control,
            MathNode::Radical(n) => &mut n.control,
            MathNode::Superscript(n) => &mut n.control,
            MathNode::Subscript(n) => &mut n.control,
            MathNode::SubSuperscript(n) => &mut n.control,
            MathNode::PreScript(n) => &mut n.control,
            MathNode::NaryOperator(n) => &mut n.control,
            MathNode::Delimiter(n) => &mut n.control,
            MathNode::Function(n) => &mut n.control,
            MathNode::Limit(n) => &mut n.control,
            MathNode::Matrix(n) => &mut n.control,
            MathNode::EquationArray(n) => &mut n.control,
            MathNode::Accent(n) => &mut n.control,
            MathNode::Bar(n) => &mut n.control,
            MathNode::GroupCharacter(n) => &mut n.control,
            MathNode::Boxed(n) => &mut n.control,
            MathNode::BorderBox(n) => &mut n.control,
            MathNode::Phantom(n) => &mut n.control,
            MathNode::Unknown(_) => continue,
        };
        let props = control.get_or_insert_with(Default::default);
        props.color = Some(Color::new(hex_digits(key)));
        props.color_theme = None;
    }
}

fn formula_primitive_rect(
    item: &Item,
    page: usize,
    font: &dyn strict_ooxml_render_svg::FontProvider,
) -> Option<VisualRect> {
    let (x, y, width, height) = match item {
        Item::Text(t) => {
            let family = chosen_family(&t.run, &t.text);
            let metrics = font.metrics(&family, t.run.bold, t.run.italic);
            (
                t.x,
                t.baseline - t.size_px * metrics.ascender,
                t.width,
                t.size_px * (metrics.ascender + metrics.descender),
            )
        }
        Item::Line(l) => (
            l.x1.min(l.x2) - l.width / 2.0,
            l.y1.min(l.y2) - l.width / 2.0,
            (l.x2 - l.x1).abs() + l.width,
            (l.y2 - l.y1).abs() + l.width,
        ),
        Item::Path(p) => (p.x, p.y, p.w, p.h),
        _ => return None,
    };
    Some(VisualRect {
        page,
        x,
        y,
        width,
        height,
    })
}
