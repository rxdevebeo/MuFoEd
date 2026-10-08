//! A revision-scoped interaction map from the shared renderer's placement.
use crate::{Address, EditError, Editor};
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
    /// Find the closest selectable text boundary on a page.
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
        let closest = self
            .spans
            .iter()
            .filter(|s| s.rect.page == page)
            .filter_map(|s| {
                let dy = if y < s.rect.y {
                    s.rect.y - y
                } else {
                    (y - s.rect.y - s.rect.height).max(0.0)
                };
                s.stops
                    .iter()
                    .map(|(offset, px)| {
                        let mut p = s.position.clone();
                        p.offset = *offset;
                        ((x - px).powi(2) + dy.powi(2), p)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        Ok(closest.map(|(_, p)| p))
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
        self.validate().map_err(VisualError::Edit)?;
        // A disposable attribution copy changes text colour only. Neither text,
        // fonts, geometry nor the live document changes. Verify every placement
        // against the unmodified model before trusting attribution.
        let original = strict_ooxml_render_svg::place_pages(self.document, options, media)
            .map_err(VisualError::Render)?;
        let mut unused: HashSet<String> = original
            .iter()
            .flat_map(|p| &p.items)
            .flat_map(|i| match i {
                Item::Text(t) => vec![t.run.color.clone(), t.run.highlight.clone()],
                Item::Path(p) => vec![p.fill.clone(), p.stroke.clone()],
                Item::Rect(p) => vec![p.fill.clone(), p.stroke.clone()],
                Item::Line(p) => vec![Some(p.color.clone())],
                Item::Image(p) => vec![Some(p.alt.clone())],
            })
            .flatten()
            .collect();
        let mut tagged = self.document.clone();
        let mut nodes = BTreeMap::new();
        stamp_document(&mut tagged, &mut unused, &mut nodes)?;
        let placement = strict_ooxml_render_svg::place_pages(&tagged, options, media)
            .map_err(VisualError::Render)?;
        let mut comparable = placement.clone();
        for page in &mut comparable {
            page.items.retain(|item| {
                !matches!(item, Item::Text(t) if
                t.run.color.as_ref().and_then(|key| nodes.get(key))
                    .is_some_and(|n| n.drawing.is_none() && n.position.inline.is_empty()))
            });
        }
        if !same_placement(&original, &comparable) {
            return Err(VisualError::Attribution);
        }
        let provider = options.font_provider.make();
        let mut spans = vec![];
        let mut objects = vec![];
        for (page, p) in placement.iter().enumerate() {
            for item in &p.items {
                let object = match item {
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
    let rendered = apply_caps(&node.text, &text.run);
    if node.cursor >= rendered.len() {
        node.cursor = 0;
    }
    let start = rendered
        .get(node.cursor..)
        .and_then(|tail| tail.find(&text.text))
        .map(|i| i + node.cursor)
        .ok_or(VisualError::Attribution)?;
    let end = start + text.text.len();
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
        bytes += apply_caps(grapheme, &text.run).len();
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
fn same_placement(a: &[PlacedPage], b: &[PlacedPage]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.width_px == b.width_px
                && a.height_px == b.height_px
                && a.items.len() == b.items.len()
                && a.items.iter().zip(&b.items).all(|(a, b)| match (a, b) {
                    (Item::Text(a), Item::Text(b)) => {
                        a.text == b.text
                            && a.x == b.x
                            && a.baseline == b.baseline
                            && a.width == b.width
                            && a.size_px == b.size_px
                    }
                    (Item::Rect(a), Item::Rect(b)) => {
                        a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h
                    }
                    (Item::Line(a), Item::Line(b)) => {
                        a.x1 == b.x1 && a.x2 == b.x2 && a.y1 == b.y1 && a.y2 == b.y2
                    }
                    (Item::Image(a), Item::Image(b)) => {
                        a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h
                    }
                    (Item::Path(a), Item::Path(b)) => {
                        a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h && a.d == b.d
                    }
                    _ => false,
                })
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
    use strict_ooxml_wml::model::{Run, RunProperties, Space, TextNode};
    let key = next_color(used, nodes)?;
    // A zero-width space locates the empty line through shared layout.
    // It exists only in the attribution copy; width and every other placement
    // are checked before its coordinate can be exposed as a caret.
    paragraph.inlines.push(Inline::Run(Run {
        props: RunProperties {
            color: Some(Color::new(hex_digits(&key))),
            ..RunProperties::default()
        },
        content: vec![RunContent::Text(TextNode {
            text: "\u{200b}".into(),
            space: Space::Preserve,
        })],
        revision: None,
        location: paragraph.location.clone(),
    }));
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
                            },
                        );
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
        },
    );
    walk(graphic, address, inline, content, &[], used, nodes)
}
