//! Pages laid out as a design rather than as running text.
//!
//! A brochure, a cover, a web page saved as PDF: blocks of text placed side
//! by side, in boxes and grids. Read in order they do not run down the page:
//! a line beside another stands on the same baseline, a column starts back
//! at the top. Set as one flow they stack, each beside-line a line lower,
//! until a two-page brochure takes eleven. Such a page keeps its place
//! instead: its lines are grouped into the blocks they form, and each block
//! is a text box anchored to the page where the block stood, one paragraph
//! per line of the source, unwrapped, with the line's own leading.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, DocPr, Drawing, DrawingKind, Extent, Graphic, Position, Shape, ShapeFill,
    ShapeGeometry, ShapeStroke, TextBox, TextBoxBody, Wrap, WrapKind,
};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::props::ParagraphProperties;
use strict_ooxml_wml::model::values::{Emu, Indentation, LineSpacingRule, Rsids, Spacing, Twips};

use crate::to_twips;

const EMU_PER_POINT: f64 = 12_700.0;
/// Where the renderer puts an exact line's baseline, as a share of the line.
const EXACT_LINE_ASCENT: f64 = 0.8;
/// The share of the page's height whose lines may stand beside others before
/// the page is a design: a running page has a signature row, a design has
/// columns and grids.
const DESIGNED_SHARE: f64 = 0.15;

/// Whether the page's lines, read in their order, would stack: the height of
/// the lines whose height on the page some earlier line already holds - a
/// second column beside the first, the third and later cells of a grid's
/// row - against the page's height. One line beside another on the same
/// baseline is a list item's marker and text, or a pair of signatures, which a
/// flow holds; a column drawn from the bottom up holds no height twice.
pub(crate) fn is_designed(lines: &[GlyphLine], page_height: f64) -> bool {
    if !(page_height.is_finite() && page_height > 0.0) {
        return false;
    }
    let inked: Vec<&GlyphLine> = lines
        .iter()
        .filter(|line| !line.text().trim().is_empty() && line.size > 0.0)
        .take(4000)
        .collect();
    let mut stacked = 0.0;
    for (index, line) in inked.iter().enumerate() {
        let mut sharing = 0usize;
        let mut same_row = true;
        for earlier in inked.iter().take(index) {
            let apart = (earlier.baseline - line.baseline).abs();
            if apart < line.size * 0.8 {
                sharing = sharing.saturating_add(1);
                same_row &= apart < line.size * 0.3;
            }
        }
        if sharing >= 2 || (sharing == 1 && !same_row) {
            stacked += line.size * 1.2;
        }
    }
    stacked > page_height * DESIGNED_SHARE
}

/// Lines that stand together: one under the other, overlapping across.
struct Frame<'l> {
    lines: Vec<&'l GlyphLine>,
    left: f64,
    right: f64,
}

/// The page's text as anchored text boxes, one per block of lines.
pub(crate) fn text_frames(
    lines: &[GlyphLine],
    ratio: f64,
    push_runs: fn(&mut Paragraph, &GlyphLine),
) -> Vec<Inline> {
    let mut inked: Vec<&GlyphLine> = lines
        .iter()
        .filter(|line| !line.text().trim().is_empty() && line.size > 0.0)
        .collect();
    inked.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.x.total_cmp(&b.x)));
    let mut frames: Vec<Frame<'_>> = Vec::new();
    for line in inked {
        let right = line.x + line.width;
        let joined = frames.iter_mut().rev().find(|frame| {
            let Some(last) = frame.lines.last() else {
                return false;
            };
            let gap = line.baseline - last.baseline;
            gap > line.size * 0.5
                && gap <= line.size.max(last.size) * 2.0
                && line.x < frame.right
                && right > frame.left
        });
        match joined {
            Some(frame) => {
                frame.left = frame.left.min(line.x);
                frame.right = frame.right.max(right);
                frame.lines.push(line);
            }
            None => frames.push(Frame {
                lines: vec![line],
                left: line.x,
                right,
            }),
        }
    }
    let ratio = if ratio.is_finite() {
        ratio.clamp(1.0, 1.6)
    } else {
        1.2
    };
    frames
        .iter()
        .enumerate()
        .filter_map(|(index, frame)| frame_anchor(index, frame, ratio, push_runs))
        .collect()
}

/// One block as a text box at its place on the page.
fn frame_anchor(
    index: usize,
    frame: &Frame<'_>,
    ratio: f64,
    push_runs: fn(&mut Paragraph, &GlyphLine),
) -> Option<Inline> {
    let first = frame.lines.first()?;
    let last = frame.lines.last()?;
    let height_of = |line: &GlyphLine| line.size * ratio;
    let top = first.baseline - EXACT_LINE_ASCENT * height_of(first);
    let bottom = last.baseline + (1.0 - EXACT_LINE_ASCENT) * height_of(last);
    let mut blocks = Vec::with_capacity(frame.lines.len());
    let mut previous: Option<&GlyphLine> = None;
    for line in &frame.lines {
        let height = height_of(line);
        let before = previous.map_or(0.0, |previous| {
            let gap = line.baseline - previous.baseline;
            gap - (1.0 - EXACT_LINE_ASCENT) * height_of(previous) - EXACT_LINE_ASCENT * height
        });
        let mut paragraph = Paragraph {
            props: ParagraphProperties {
                indentation: Some(Indentation {
                    start: Some(Twips(to_twips(line.x - frame.left))),
                    ..Indentation::default()
                }),
                spacing: Some(Spacing {
                    before: (before > 0.25).then(|| Twips(to_twips(before))),
                    after: Some(Twips(0)),
                    line: Some(Twips(to_twips(height))),
                    line_rule: Some(LineSpacingRule::Exact),
                    ..Spacing::default()
                }),
                ..ParagraphProperties::default()
            },
            inlines: Vec::new(),
            rsids: Rsids::default(),
            para_id: None,
            text_id: None,
            revision: None,
            location: SourceLocation::unknown(),
        };
        push_runs(&mut paragraph, line);
        blocks.push(Block::Paragraph(paragraph));
        previous = Some(line);
    }
    // Room for a substitute font that sets a little wider than the source's:
    // the box does not wrap, and a clipped right edge is no loss.
    Some(text_box_at(
        Arc::from(format!("text-{index}")),
        [
            frame.left,
            top,
            (frame.right - frame.left).max(1.0) + 6.0,
            (bottom - top).max(1.0),
        ],
        blocks,
        "none",
    ))
}

/// A text box anchored to the page: `area` is its left, top, width and
/// height in points from the page's corner, `blocks` its content, `wrap` the
/// box's own wrapping of its text (`none` keeps a source line one line,
/// `square` wraps a paragraph in the box's width).
pub(crate) fn text_box_at(
    name: Arc<str>,
    area: [f64; 4],
    blocks: Vec<Block>,
    wrap: &str,
) -> Inline {
    let emu = |points: f64| Emu((points * EMU_PER_POINT).round() as i64);
    let [left, top, width, height] = area;
    let extent = Extent {
        cx: emu(width.max(1.0)),
        cy: emu(height.max(1.0)),
    };
    let zero = Some(Emu(0));
    let shape = Shape {
        name: Some(Arc::clone(&name)),
        descr: None,
        nv_id: None,
        bw_mode: None,
        tx_box: Some(true),
        sp_locks: None,
        connector: false,
        effects: None,
        geometry: ShapeGeometry::Preset(Arc::from("rect")),
        xfrm: None,
        offset: Some((Emu(0), Emu(0))),
        extent: Some(extent),
        fill: Some(ShapeFill::None),
        stroke: Some(ShapeStroke {
            none: true,
            ..ShapeStroke::default()
        }),
        text: Some(TextBox {
            body: Some(TextBoxBody {
                left_inset: zero,
                top_inset: zero,
                right_inset: zero,
                bottom_inset: zero,
                wrap: Some(Arc::from(wrap)),
                ..TextBoxBody::default()
            }),
            blocks,
            location: SourceLocation::unknown(),
        }),
        style: None,
        location: SourceLocation::unknown(),
    };
    let position = |offset: f64| Position {
        relative_from: Some(Arc::from("page")),
        align: None,
        offset: Some(emu(offset)),
        percent_offset: None,
    };
    Inline::Drawing(Drawing {
        kind: DrawingKind::Anchor(AnchorDrawing {
            extent: Some(extent),
            effect_extent: None,
            doc_pr: Some(DocPr {
                hidden: None,
                id: None,
                name: Some(name),
                descr: None,
                title: None,
            }),
            simple_pos: false,
            simple_pos_point: None,
            hidden: None,
            position_h: Some(position(left)),
            position_v: Some(position(top)),
            wrap: Some(Wrap {
                kind: WrapKind::None,
                wrap_text: None,
                dist_left: None,
                dist_right: None,
                dist_top: None,
                dist_bottom: None,
                polygon: Vec::new(),
                polygon_edited: None,
            }),
            behind_doc: false,
            relative_height: Some(1),
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            allow_overlap: true,
            layout_in_cell: true,
            locked: false,
            graphic_uri: None,
            graphic: Box::new(Graphic::Shape(shape)),
            size_rel_h: None,
            size_rel_v: None,
            location: SourceLocation::unknown(),
        }),
        location: SourceLocation::unknown(),
    })
}
