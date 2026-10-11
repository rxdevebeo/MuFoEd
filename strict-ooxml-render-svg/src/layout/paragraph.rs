//! Paragraph layout: inline flattening, line breaking, alignment and spacing
//! (`STAGE-4-TASK.md` §5.4).

use std::collections::VecDeque;

use strict_ooxml_wml::model::values::{
    BreakKind, FieldCharType, LineSpacingRule, TabAlignment, TabLeader, VertAlign,
};
use strict_ooxml_wml::model::{
    AnchorDrawing, Drawing, DrawingKind, Inline, Paragraph, Revision, Run, RunContent,
};

use crate::layout::floating::{wrap_exclusion, WrapExclusion, WrapSide};
use crate::layout::{Flow, Geometry, ImageItem, Item, LayoutContext, TextItem, TextLine};
use crate::paint::image::layout_inline_image;
use crate::style::{apply_caps, compute_paragraph, compute_run, ComputedParagraph, ComputedRun};
use crate::units::{pt_to_px, twips_to_px};
use crate::RevisionView;

/// Whether a run with the given revision marker is visible under `view` (ADR-0018).
fn revision_visible(view: RevisionView, revision: Option<&Revision>) -> bool {
    match revision.map(|revision| revision.kind) {
        Some(kind) if kind.is_deletion() => view == RevisionView::Original,
        Some(kind) if kind.is_insertion() => view == RevisionView::Final,
        None | Some(_) => true,
    }
}

/// A flattened inline segment.
enum Seg {
    /// A text fragment with its effective run.
    Text(String, ComputedRun),
    /// A tab.
    Tab,
    /// A soft line break.
    Break,
    /// A hard page break.
    PageBreak,
    /// A hard column break.
    ColumnBreak,
    /// An inline image.
    Image(ImageItem),
    /// An inline block object (shape/group), already rendered.
    Object(Vec<Item>, f64, f64),
    /// A floating (anchored) drawing.
    Anchor(Box<AnchorDrawing>),
    /// An inline formula (`m:oMath`), laid out on the text baseline.
    Math(crate::math::layout::MathBox, ComputedRun),
    /// A display formula (`m:oMathPara`), laid out as its own centred block.
    MathParagraph(crate::math::layout::MathBox),
    /// A footnote reference marker (the referenced note id).
    FootnoteMarker(u32, ComputedRun),
    /// An endnote reference marker (the referenced note id).
    EndnoteMarker(u32, ComputedRun),
    /// The note number marker inside a note body (`w:footnoteRef`).
    NoteNumber(ComputedRun),
    /// A computed field result (PAGE/NUMPAGES/SECTIONPAGES/SECTION).
    FieldResult(
        crate::fields::FieldKind,
        Option<crate::notes::NumberFormat>,
        ComputedRun,
    ),
}

/// A laid-out paragraph plus its surrounding spacing.
pub(crate) struct ParagraphFlow {
    /// Flows in order.
    pub flows: Vec<Flow>,
    /// Floating (anchored) drawings attached to this paragraph.
    pub anchors: Vec<AnchorDrawing>,
    /// Space before in px (`w:spacing/@w:before` only).
    pub space_before: f64,
    /// Space after in px (`w:spacing/@w:after` only).
    pub space_after: f64,
    /// Top `w:pBdr` pad in px (`space` + stroke width).
    pub border_before: f64,
    /// Bottom `w:pBdr` pad in px (`space` + stroke width).
    pub border_after: f64,
    /// Keep the paragraph on one page.
    pub keep_lines: bool,
    /// Keep at least two lines on each side of a page break (`w:widowControl`).
    pub widow_control: bool,
}

/// Default tab stop in twips (0.5 inch) when settings omit one.
const DEFAULT_TAB_TWIPS: i32 = 720;

/// Compatibility limit for complex inline math pending complete compact layout.
const MAX_MATH_LINE_GROWTH: f64 = 1.60;

struct MetricAdvanceGuard<'a> {
    cell: &'a std::cell::Cell<bool>,
    previous: bool,
}

impl Drop for MetricAdvanceGuard<'_> {
    fn drop(&mut self) {
        self.cell.set(self.previous);
    }
}

/// `note_marker` is the formatted number used to replace a `w:footnoteRef`/
/// `w:endnoteRef` marker when laying out a note body; body paragraphs pass
/// `None`.
///
/// `page_exclusions` are wrap rectangles already converted to paragraph-local
/// coordinates (from earlier floating objects on the page). `host` supplies the
/// page geometry and paragraph origin used to resolve the current paragraph's
/// own anchors with the same extent rules as paint.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
#[must_use]
pub(crate) fn layout_paragraph(
    ctx: &LayoutContext<'_>,
    para: &Paragraph,
    content_left: f64,
    content_width: f64,
    grid_line_pitch: Option<f64>,
    note_marker: Option<&str>,
    page_exclusions: &[WrapExclusion],
    host: Option<(&Geometry, f64, f64)>,
    neighbors: ParagraphNeighbors,
) -> ParagraphFlow {
    let mut computed = compute_paragraph(ctx.document, para);
    let justify = matches!(
        computed.alignment,
        strict_ooxml_wml::model::values::Justification::Both
            | strict_ooxml_wml::model::values::Justification::Justify
            | strict_ooxml_wml::model::values::Justification::Distribute
    );
    let previous_advances = ctx.metric_advances.get();
    ctx.metric_advances.set(justify);
    let _restore_advances = MetricAdvanceGuard {
        cell: &ctx.metric_advances,
        previous: previous_advances,
    };
    // Numbering supplies the fields the paragraph did not set. An explicit 0
    // stays 0; a missing hanging/firstLine still comes from the level (A16).
    if let Some(marker) = ctx.numbering.get(para) {
        let direct = para.props.indentation.as_ref();
        if direct.and_then(|indent| indent.start).is_none() {
            if let Some(start) = marker.indent_start_pt {
                computed.indent_start_pt = start;
            }
        }
        let direct_line =
            direct.is_some_and(|indent| indent.hanging.is_some() || indent.first_line.is_some());
        if !direct_line {
            if let Some(first_line) = marker.first_line_pt {
                computed.first_line_pt = first_line;
            }
        }
    }
    let edges = apply_neighbors(&mut computed, &neighbors);
    // `w:pBdr` pads are separate from `w:spacing`: consecutive bordered
    // paragraphs collapse their shared edge to zero in `layout_frame_contents`.
    let space_before = pt_to_px(computed.space_before_pt, ctx.options.scale);
    let space_after = pt_to_px(computed.space_after_pt, ctx.options.scale);
    let border_before = pt_to_px(computed.border_before_pt, ctx.options.scale);
    let border_after = pt_to_px(computed.border_after_pt, ctx.options.scale);

    let mut segments = Vec::new();
    let mut field_state = FieldState::default();
    flatten_inlines(
        ctx,
        &computed,
        &para.inlines,
        content_left,
        content_width,
        &mut segments,
        &mut field_state,
    );

    let mut flows = build_lines(
        ctx,
        para,
        &computed,
        content_left,
        content_width,
        grid_line_pitch,
        note_marker,
        segments,
        page_exclusions,
        host,
    );
    if computed.borders.any() {
        let scale = ctx.options.scale;
        let indent_start = pt_to_px(computed.indent_start_pt, scale);
        let first = pt_to_px(computed.first_line_pt, scale).min(0.0);
        let box_left = content_left + indent_start + if computed.bidi { 0.0 } else { first };
        let box_right = content_left + content_width - pt_to_px(computed.indent_end_pt, scale)
            - if computed.bidi { first } else { 0.0 };
        let reach_above = if neighbors.joins_prev {
            pt_to_px(neighbors.prev_after_pt + computed.space_before_pt, scale)
        } else {
            0.0
        };
        add_border_strokes(
            &mut flows.0,
            &computed.borders,
            &edges,
            (box_left, box_right),
            (border_before, border_after, reach_above),
            pt_to_px(1.0, scale),
        );
    }

    // A paragraph whose only content is a display formula (`m:oMathPara`) is a
    // *math paragraph*: Word lays it out as one self-contained block and does
    // not add the paragraph's `w:spacing` around it — the block's own ascent
    // and descent already separate it from the surrounding lines. Adding the
    // spacing a second time is what pushed the text below every display
    // formula down by a full line (STAGE-5C-REWORK-1 C1).
    let math_paragraph = ctx.options.math
        && para
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::MathParagraph(_)))
        && para.inlines.iter().all(|inline| {
            matches!(
                inline,
                Inline::MathParagraph(_) | Inline::BookmarkStart(_) | Inline::BookmarkEnd(_)
            )
        });

    ParagraphFlow {
        flows: flows.0,
        anchors: flows.1,
        space_before: if math_paragraph
            && !para
                .props
                .spacing
                .as_ref()
                .is_some_and(|spacing| spacing.before.is_some() || spacing.before_lines.is_some())
        {
            0.0
        } else {
            space_before
        },
        // `w:after` is **kept** for a math paragraph, and the reference says so: the
        // gap between the last display formula on `06-strict-math-display` and the
        // paragraph below it is 15 px in the reference and 5 px in ours, and
        // 10.667 px is exactly the document's `w:after="160"`. A formula's own
        // descent separates it from what comes *before* it; what comes after it is
        // ordinary paragraph spacing, and dropping it is how the text under a
        // formula once ended up a line too high.
        space_after,
        border_before: if math_paragraph { 0.0 } else { border_before },
        border_after: if math_paragraph { 0.0 } else { border_after },
        keep_lines: computed.keep_lines,
        widow_control: computed.widow_control,
    }
}

/// How a paragraph relates to the blocks next to it in its container.
///
/// The layout of one paragraph needs this for `w:contextualSpacing`, for
/// paragraphs that share one `w:pBdr` box, and for autospacing at the edge
/// of a cell. The loops that walk a block list work it out with
/// [`paragraph_neighbors`]; the default is a paragraph with no neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ParagraphNeighbors {
    /// The previous block is a paragraph with the same style.
    pub prev_same_style: bool,
    /// The next block is a paragraph with the same style.
    pub next_same_style: bool,
    /// The previous paragraph has the same borders: one box holds both.
    pub joins_prev: bool,
    /// The next paragraph has the same borders.
    pub joins_next: bool,
    /// The previous paragraph's space after, in pt, when joined.
    pub prev_after_pt: f64,
    /// First block of a cell, header, footer or text box.
    pub first_in_container: bool,
    /// Last block of a cell, header, footer or text box.
    pub last_in_container: bool,
}

/// Works out the neighbours of the paragraph at `blocks[index]`.
pub(crate) fn paragraph_neighbors(
    document: &strict_ooxml_wml::model::Document,
    blocks: &[strict_ooxml_wml::model::block::Block],
    index: usize,
    in_container: bool,
) -> ParagraphNeighbors {
    use strict_ooxml_wml::model::block::Block;
    let paragraph_at = |at: Option<usize>| match at.and_then(|at| blocks.get(at)) {
        Some(Block::Paragraph(para)) => Some(compute_paragraph(document, para)),
        _ => None,
    };
    let Some(current) = paragraph_at(Some(index)) else {
        return ParagraphNeighbors::default();
    };
    let prev = paragraph_at(index.checked_sub(1));
    let next = paragraph_at(index.checked_add(1));
    let same_style = |other: &Option<ComputedParagraph>| {
        other
            .as_ref()
            .is_some_and(|other| other.style == current.style)
    };
    let joins = |other: &Option<ComputedParagraph>| {
        other.as_ref().is_some_and(|other| {
            current.borders.any()
                && other.borders == current.borders
                && (other.indent_start_pt - current.indent_start_pt).abs() < 1e-6
                && (other.indent_end_pt - current.indent_end_pt).abs() < 1e-6
        })
    };
    let prev_same_style = same_style(&prev);
    let joins_prev = joins(&prev);
    let prev_after_pt = match &prev {
        Some(prev) if joins_prev => {
            if prev.contextual_spacing && prev_same_style {
                0.0
            } else {
                prev.space_after_pt
            }
        }
        _ => 0.0,
    };
    ParagraphNeighbors {
        prev_same_style,
        next_same_style: same_style(&next),
        joins_prev,
        joins_next: joins(&next),
        prev_after_pt,
        first_in_container: in_container && index == 0,
        last_in_container: in_container && index + 1 == blocks.len(),
    }
}

/// The edges drawn above and below a paragraph once its neighbours are known.
#[derive(Clone, Debug, Default)]
struct DrawnEdges {
    /// `w:top`, or `w:between` inside a shared box, or nothing.
    above: Option<crate::layout::border::ResolvedBorder>,
    /// `w:bottom` at the end of a box, or nothing.
    below: Option<crate::layout::border::ResolvedBorder>,
}

/// Applies `w:contextualSpacing`, autospacing at a container edge and a
/// shared border box to `computed`'s spacing and border pads.
fn apply_neighbors(computed: &mut ComputedParagraph, neighbors: &ParagraphNeighbors) -> DrawnEdges {
    // Each paragraph's own flag governs its own spacing (§17.3.1.9).
    if computed.contextual_spacing && neighbors.prev_same_style {
        computed.space_before_pt = 0.0;
    }
    if computed.contextual_spacing && neighbors.next_same_style {
        computed.space_after_pt = 0.0;
    }
    // HTML spacing does not push the first line of a cell down, nor the
    // last one up.
    if computed.before_autospacing && neighbors.first_in_container {
        computed.space_before_pt = 0.0;
    }
    if computed.after_autospacing && neighbors.last_in_container {
        computed.space_after_pt = 0.0;
    }
    let above = if neighbors.joins_prev {
        computed.borders.between.clone()
    } else {
        computed.borders.top.clone()
    };
    let below = if neighbors.joins_next {
        None
    } else {
        computed.borders.bottom.clone()
    };
    let pad = |edge: &Option<crate::layout::border::ResolvedBorder>| {
        edge.as_ref()
            .map_or(0.0, |edge| edge.space_pt + edge.band_pt())
    };
    computed.border_before_pt = pad(&above);
    computed.border_after_pt = pad(&below);
    DrawnEdges { above, below }
}

/// Adds the strokes of a paragraph border to the paragraph's flows.
///
/// Each line carries the side edges for its own height, so the box follows
/// the lines across a page break. The first line also carries the edge
/// above it (in the pad the paginator leaves there) and the last the edge
/// below. `bounds` is the text box, `pads` the pads above and below and how
/// far the side edges reach up to meet the previous paragraph of a shared
/// box, all in px.
fn add_border_strokes(
    flows: &mut [Flow],
    borders: &crate::style::ParagraphBorders,
    edges: &DrawnEdges,
    bounds: (f64, f64),
    pads: (f64, f64, f64),
    px_per_pt: f64,
) {
    use crate::layout::border::{edge_items, Side};
    let (box_left, box_right) = bounds;
    let (pad_above, pad_below, reach_above) = pads;
    let positions: Vec<usize> = flows
        .iter()
        .enumerate()
        .filter(|(_, flow)| matches!(flow, Flow::Line(_) | Flow::Block { .. }))
        .map(|(index, _)| index)
        .collect();
    let (Some(&first), Some(&last)) = (positions.first(), positions.last()) else {
        return;
    };
    let left_at = borders
        .left
        .as_ref()
        .map(|edge| box_left - edge.space_pt * px_per_pt);
    let right_at = borders
        .right
        .as_ref()
        .map(|edge| box_right + edge.space_pt * px_per_pt);
    let outer_left = match (&borders.left, left_at) {
        (Some(edge), Some(at)) => at - edge.band_pt() * px_per_pt,
        _ => box_left,
    };
    let outer_right = match (&borders.right, right_at) {
        (Some(edge), Some(at)) => at + edge.band_pt() * px_per_pt,
        _ => box_right,
    };
    for &index in &positions {
        let Some(flow) = flows.get_mut(index) else {
            continue;
        };
        let height = match flow {
            Flow::Line(line) => line.height,
            Flow::Block { height, .. } => *height,
            _ => continue,
        };
        let top = if index == first {
            -(pad_above + reach_above)
        } else {
            0.0
        };
        let bottom = if index == last {
            height + pad_below
        } else {
            height
        };
        let mut strokes = Vec::new();
        if let (Some(edge), Some(at)) = (&borders.left, left_at) {
            strokes.extend(edge_items(edge, Side::Left, at, top, bottom, px_per_pt));
        }
        if let (Some(edge), Some(at)) = (&borders.right, right_at) {
            strokes.extend(edge_items(edge, Side::Right, at, top, bottom, px_per_pt));
        }
        if index == first {
            if let Some(edge) = &edges.above {
                let at = -edge.space_pt * px_per_pt;
                strokes.extend(edge_items(
                    edge,
                    Side::Top,
                    at,
                    outer_left,
                    outer_right,
                    px_per_pt,
                ));
            }
        }
        if index == last {
            if let Some(edge) = &edges.below {
                let at = height + edge.space_pt * px_per_pt;
                strokes.extend(edge_items(
                    edge,
                    Side::Bottom,
                    at,
                    outer_left,
                    outer_right,
                    px_per_pt,
                ));
            }
        }
        match flow {
            Flow::Line(line) => line.graphics.extend(strokes),
            Flow::Block { items, .. } => items.extend(strokes),
            _ => {}
        }
    }
}

/// Returns an effective run format forced to superscript (for note markers).
fn superscript(mut run: ComputedRun) -> ComputedRun {
    run.vert_align = VertAlign::Superscript;
    run
}

/// The stage of a complex field while flattening a paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum FieldStage {
    /// Not inside a field.
    #[default]
    Outside,
    /// Between `begin` and `separate`: collecting the instruction.
    Instruction,
    /// Between `separate` and `end`: the cached result region.
    Result,
}

/// Tracks complex-field (`fldChar`/`instrText`) state across runs.
#[derive(Default)]
struct FieldState {
    stage: FieldStage,
    instruction: String,
    computed: Option<(crate::fields::FieldKind, Option<crate::notes::NumberFormat>)>,
}

impl FieldState {
    /// Whether the current position is inside a computed field's result.
    fn suppressed(&self) -> bool {
        self.stage == FieldStage::Result && self.computed.is_some()
    }

    /// Handles a `w:fldChar`.
    fn on_char(&mut self, kind: FieldCharType, out: &mut Vec<Seg>, run: &ComputedRun) {
        match kind {
            FieldCharType::Begin => {
                self.stage = FieldStage::Instruction;
                self.instruction.clear();
                self.computed = None;
            }
            FieldCharType::Separate => {
                if self.stage == FieldStage::Instruction {
                    let (kind, format) = crate::fields::parse_instruction(&self.instruction);
                    self.computed = kind.map(|kind| (kind, format));
                    self.stage = FieldStage::Result;
                    if let Some((kind, format)) = self.computed {
                        out.push(Seg::FieldResult(kind, format, run.clone()));
                    }
                }
            }
            FieldCharType::End => {
                if self.stage == FieldStage::Instruction {
                    let (kind, format) = crate::fields::parse_instruction(&self.instruction);
                    if let Some(kind) = kind {
                        out.push(Seg::FieldResult(kind, format, run.clone()));
                    }
                }
                self.stage = FieldStage::Outside;
                self.computed = None;
                self.instruction.clear();
            }
        }
    }

    /// Handles a `w:instrText` chunk.
    fn on_instr(&mut self, text: &str) {
        if self.stage == FieldStage::Instruction {
            self.instruction.push_str(text);
        }
    }
}

/// Recursively flattens inline content into segments.
#[allow(clippy::too_many_arguments)]
fn flatten_inlines(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    inlines: &[Inline],
    content_left: f64,
    content_width: f64,
    out: &mut Vec<Seg>,
    field: &mut FieldState,
) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => flatten_run(ctx, computed, run, out, field),
            _ if field.suppressed() => {}
            Inline::Hyperlink(link) => flatten_inlines(
                ctx,
                computed,
                &link.inlines,
                content_left,
                content_width,
                out,
                field,
            ),
            Inline::Field(simple) => {
                let instruction = simple.instruction.as_deref().unwrap_or("");
                let (kind, format) = crate::fields::parse_instruction(instruction);
                if let Some(kind) = kind {
                    out.push(Seg::FieldResult(kind, format, computed.default_run.clone()));
                } else {
                    flatten_inlines(
                        ctx,
                        computed,
                        &simple.inlines,
                        content_left,
                        content_width,
                        out,
                        field,
                    );
                }
            }
            Inline::Drawing(drawing) => flatten_drawing(ctx, drawing, out),
            Inline::Break(kind) => match kind {
                BreakKind::Page => out.push(Seg::PageBreak),
                BreakKind::Column => out.push(Seg::ColumnBreak),
                BreakKind::TextWrapping => out.push(Seg::Break),
            },
            Inline::Tab => out.push(Seg::Tab),
            Inline::SdtInline(sdt) => flatten_inlines(
                ctx,
                computed,
                &sdt.inlines,
                content_left,
                content_width,
                out,
                field,
            ),
            Inline::Directional(dir) => {
                // AUD-42: apply direction like `w:rtl` on nested runs.
                if dir.val == strict_ooxml_wml::model::inline::DirectionalVal::Rtl {
                    flatten_directional_rtl(
                        ctx,
                        computed,
                        &dir.inlines,
                        content_left,
                        content_width,
                        out,
                        field,
                    );
                } else {
                    flatten_inlines(
                        ctx,
                        computed,
                        &dir.inlines,
                        content_left,
                        content_width,
                        out,
                        field,
                    );
                }
            }
            Inline::FootnoteRef(id) => out.push(Seg::FootnoteMarker(
                *id,
                superscript(computed.default_run.clone()),
            )),
            Inline::EndnoteRef(id) => out.push(Seg::EndnoteMarker(
                *id,
                superscript(computed.default_run.clone()),
            )),
            // Formulas (Stage 5C, §3.1.1/§3.1.5). An inline `m:oMath` joins the
            // text baseline; a display `m:oMathPara` becomes its own block.
            Inline::Math(expression) if ctx.options.math => {
                flatten_math(ctx, computed, expression, content_width, out);
            }
            Inline::MathParagraph(paragraph) if ctx.options.math => {
                out.push(Seg::MathParagraph(crate::math::layout_display(
                    ctx,
                    paragraph,
                    &computed.default_run,
                    content_left,
                    content_width,
                )));
            }
            Inline::BookmarkStart(_)
            | Inline::BookmarkEnd(_)
            | Inline::CommentRangeStart(_)
            | Inline::CommentRangeEnd(_)
            | Inline::CommentReference(_)
            | Inline::Math(_)
            | Inline::MathParagraph(_)
            | Inline::Opaque(_) => {}
        }
    }
}

fn math_spacing(ctx: &LayoutContext<'_>, value: Option<&str>, name: &str) -> f64 {
    match value {
        None => 0.0,
        Some(value) => match value.parse::<i32>() {
            Ok(twips) if twips >= 0 => twips_to_px(twips, ctx.options.scale),
            _ => {
                ctx.warn(format!(
                    "render.math-spacing: invalid m:{name} value {value:?}; using zero"
                ));
                0.0
            }
        },
    }
}

fn flatten_math(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    expression: &strict_ooxml_wml::model::math::MathExpression,
    content_width: f64,
    out: &mut Vec<Seg>,
) {
    let width = content_width
        - pt_to_px(
            computed.indent_start_pt + computed.indent_end_pt,
            ctx.options.scale,
        );
    for (index, boxed) in
        crate::math::layout_inline_wrapped(ctx, expression, &computed.default_run, width)
            .into_iter()
            .enumerate()
    {
        if index > 0 {
            out.push(Seg::Break);
        }
        out.push(Seg::Math(boxed, computed.default_run.clone()));
    }
}

/// Flattens directional content after stamping `w:rtl` onto nested runs (AUD-42).
fn flatten_directional_rtl(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    inlines: &[Inline],
    content_left: f64,
    content_width: f64,
    out: &mut Vec<Seg>,
    field: &mut FieldState,
) {
    let stamped: Vec<Inline> = inlines.iter().map(stamp_rtl).collect();
    flatten_inlines(
        ctx,
        computed,
        &stamped,
        content_left,
        content_width,
        out,
        field,
    );
}

fn stamp_rtl(inline: &Inline) -> Inline {
    match inline {
        Inline::Run(run) => {
            let mut run = run.clone();
            run.props.rtl = strict_ooxml_wml::model::values::TriState::On;
            Inline::Run(run)
        }
        Inline::Hyperlink(link) => {
            let mut link = link.clone();
            link.inlines = link.inlines.iter().map(stamp_rtl).collect();
            Inline::Hyperlink(link)
        }
        Inline::Field(field) => {
            let mut field = field.clone();
            field.inlines = field.inlines.iter().map(stamp_rtl).collect();
            Inline::Field(field)
        }
        Inline::SdtInline(sdt) => {
            let mut sdt = sdt.clone();
            sdt.inlines = sdt.inlines.iter().map(stamp_rtl).collect();
            Inline::SdtInline(sdt)
        }
        Inline::Directional(dir) => {
            let mut dir = dir.clone();
            dir.inlines = dir.inlines.iter().map(stamp_rtl).collect();
            Inline::Directional(dir)
        }
        other => other.clone(),
    }
}

fn flatten_run(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    run: &Run,
    out: &mut Vec<Seg>,
    field: &mut FieldState,
) {
    if !revision_visible(ctx.options.revisions, run.revision.as_ref()) {
        return;
    }
    let run_style = compute_run(ctx.document, computed, run);
    if run_style.vanish {
        return;
    }
    for content in &run.content {
        match content {
            RunContent::FieldChar(field_char) => field.on_char(field_char.kind, out, &run_style),
            RunContent::InstrText(text) => field.on_instr(text),
            _ if field.suppressed() => {}
            RunContent::Text(text) => {
                if text.text.is_empty() {
                    continue;
                }
                if run_style.small_caps && !run_style.caps {
                    push_small_caps(&text.text, &run_style, out);
                } else {
                    out.push(Seg::Text(
                        apply_caps(&text.text, &run_style),
                        run_style.clone(),
                    ));
                }
            }
            RunContent::Tab => out.push(Seg::Tab),
            RunContent::Break(BreakKind::Page) => out.push(Seg::PageBreak),
            RunContent::Break(BreakKind::Column) => out.push(Seg::ColumnBreak),
            RunContent::Break(BreakKind::TextWrapping)
            | RunContent::ClearingBreak(_)
            | RunContent::CarriageReturn => out.push(Seg::Break),
            RunContent::Drawing(drawing) => flatten_drawing(ctx, drawing, out),
            RunContent::Symbol(symbol) => {
                // `w:sym` names its own face (Symbol, Wingdings): the
                // character is a private-use code in that face, not in the
                // run's.
                let mut symbol_style = run_style.clone();
                if !symbol.font.trim().is_empty() {
                    symbol.font.as_ref().clone_into(&mut symbol_style.family);
                    symbol_style.family_fallback = None;
                    symbol_style.complex_family = None;
                    symbol_style.east_asia_family = None;
                }
                out.push(Seg::Text(
                    symbol_character(&symbol.font, symbol.character).to_string(),
                    symbol_style,
                ));
            }
            RunContent::NoBreakHyphen => {
                out.push(Seg::Text("\u{2011}".to_owned(), run_style.clone()));
            }
            RunContent::FootnoteRef(id) => {
                out.push(Seg::FootnoteMarker(*id, superscript(run_style.clone())));
            }
            RunContent::EndnoteRef(id) => {
                out.push(Seg::EndnoteMarker(*id, superscript(run_style.clone())));
            }
            RunContent::NoteRef => out.push(Seg::NoteNumber(superscript(run_style.clone()))),
            // An anchor, not a character. Word draws a marker here and this
            // renderer draws nothing, which is a visible difference rather than
            // a silent one: the choice is between a gap in the line and a glyph
            // nobody asked for, and the gap is the honest one until the marker
            // exists as a segment of its own.
            RunContent::CommentReference(_)
            | RunContent::SoftHyphen
            | RunContent::LastRenderedPageBreak
            // A `w:ptab` moves the following text to a margin or an indent. The
            // renderer has no notion of either, and drawing the leader glyphs
            // would be a character run the document did not ask for. It is named
            // here rather than ignored so the gap is visible where it is: the same
            // shape as Н-3, where a wrap contour is a rectangle standing in for a
            // curve this project cannot describe.
            | RunContent::Ptab { .. }
            | RunContent::Opaque(_) => {}
        }
    }
}

/// The character a `w:sym` stands for in its face.
///
/// The parser folds `F0xx` codes to `00xx`, which is the byte the symbol face
/// stores; the stand-in table of [`crate::font::present_text`] is keyed by
/// the private-use form Word writes. A byte with a stand-in goes back to that
/// form, so a Symbol `a` is painted as `α` rather than as a Latin `a`.
fn symbol_character(font: &str, character: char) -> char {
    let code = u32::from(character);
    if code > 0xFF {
        return character;
    }
    let Some(private) = char::from_u32(0xF000 + code) else {
        return character;
    };
    let mut buffer = [0u8; 4];
    let private_text: &str = private.encode_utf8(&mut buffer);
    if crate::font::present_text(font, private_text) == private_text {
        character
    } else {
        private
    }
}

/// Size of a small-capital letter relative to the run size (`w:smallCaps`).
///
/// Word synthesises small capitals by scaling capitals, as LibreOffice does
/// at the same ratio.
const SMALL_CAPS_SCALE: f64 = 0.8;

/// Splits a `w:smallCaps` run into full-size and small-capital pieces.
///
/// Lowercase letters become capitals at [`SMALL_CAPS_SCALE`]; everything
/// else keeps the run's size. The pieces are consecutive segments, so a word
/// stays a word for wrapping the same way a word with mixed formatting does.
fn push_small_caps(text: &str, run: &ComputedRun, out: &mut Vec<Seg>) {
    let mut small = run.clone();
    small.size_pt = run.size_pt * SMALL_CAPS_SCALE;
    let mut piece = String::new();
    let mut piece_is_small = false;
    for ch in text.chars() {
        let is_small = ch.is_lowercase();
        if is_small != piece_is_small && !piece.is_empty() {
            let style = if piece_is_small { &small } else { run };
            out.push(Seg::Text(std::mem::take(&mut piece), style.clone()));
        }
        piece_is_small = is_small;
        if is_small {
            piece.extend(ch.to_uppercase());
        } else {
            piece.push(ch);
        }
    }
    if !piece.is_empty() {
        let style = if piece_is_small { &small } else { run };
        out.push(Seg::Text(piece, style.clone()));
    }
}

fn flatten_drawing(ctx: &LayoutContext<'_>, drawing: &Drawing, out: &mut Vec<Seg>) {
    let doc_pr = match &drawing.kind {
        DrawingKind::Anchor(anchor) => anchor.doc_pr.as_ref(),
        DrawingKind::Inline(inline) => inline.doc_pr.as_ref(),
        DrawingKind::Opaque(_) => None,
    };
    if doc_pr.is_some_and(|properties| properties.hidden == Some(true)) {
        return;
    }
    match &drawing.kind {
        // An independently hidden anchor also contributes no paint or wrapping.
        DrawingKind::Anchor(anchor) if anchor.hidden == Some(true) => {}
        DrawingKind::Anchor(anchor) => out.push(Seg::Anchor(Box::new(anchor.clone()))),
        DrawingKind::Inline(_) => {
            if let Some(image) = layout_inline_image(ctx, drawing, 0.0, 0.0) {
                out.push(Seg::Image(image));
            } else if let Some((items, width, height)) =
                crate::paint::graphics::inline_items(ctx, drawing)
            {
                out.push(Seg::Object(items, width, height));
            }
        }
        DrawingKind::Opaque(_) => {}
    }
}

/// Mutable line being assembled.
struct LineBuilder {
    inline_fraction: bool,
    items: Vec<TextItem>,
    graphics: Vec<Item>,
    footnote_refs: Vec<u32>,
    /// Ink extent above the baseline contributed by formulas, in px.
    ink_height: f64,
    /// Ink extent below the baseline contributed by formulas, in px.
    ink_depth: f64,
    /// Height of the tallest inline drawing on the line, in px.
    object_height: f64,
}

impl LineBuilder {
    fn new() -> Self {
        Self {
            inline_fraction: false,
            items: Vec::new(),
            graphics: Vec::new(),
            footnote_refs: Vec::new(),
            ink_height: 0.0,
            ink_depth: 0.0,
            object_height: 0.0,
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty() && self.graphics.is_empty()
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_lines(
    ctx: &LayoutContext<'_>,
    para: &Paragraph,
    computed: &ComputedParagraph,
    content_left: f64,
    content_width: f64,
    grid_line_pitch: Option<f64>,
    note_marker: Option<&str>,
    segments: Vec<Seg>,
    page_exclusions: &[WrapExclusion],
    host: Option<(&Geometry, f64, f64)>,
) -> (Vec<Flow>, Vec<AnchorDrawing>) {
    let scale = ctx.options.scale;
    let indent_start = pt_to_px(computed.indent_start_pt, scale);
    let indent_end = pt_to_px(computed.indent_end_pt, scale);
    let normal_x = content_left + indent_start;
    let first_offset = pt_to_px(computed.first_line_pt, scale);
    let first_x = content_left + indent_start + if computed.bidi { 0.0 } else { first_offset };
    // Every line ends on this edge. The first line may start further in
    // (or further out, for a hanging indent); that changes its length, not
    // this edge (A20).
    let right_edge = content_left + content_width - indent_end;

    // AUD-71: Word's default when `defaultTabStop` is missing or non-positive
    // is 720 twips (0.5"); a zero step would make `relative / tab_step` NaN.
    let default_tab = match ctx.document.settings.default_tab_stop {
        Some(twips) if twips.value() > 0 => twips.value(),
        Some(_) => {
            ctx.warn(
                "render.default-tab-stop: non-positive defaultTabStop; using 720 twips".to_owned(),
            );
            DEFAULT_TAB_TWIPS
        }
        None => DEFAULT_TAB_TWIPS,
    };

    let marker = ctx.numbering.get(para).map(|marker| {
        (
            marker.text.clone(),
            marker.run.clone(),
            marker.suffix.clone(),
            marker.pic_bullet,
            marker.justification,
        )
    });

    let mut exclusions = page_exclusions.to_vec();
    exclusions.extend(anchor_exclusions(
        ctx,
        &segments,
        host,
        content_left,
        content_width,
    ));
    let mut sink = LineSink {
        ctx,
        computed,
        right_edge: right_edge - if computed.bidi { first_offset } else { 0.0 },
        normal_right_edge: right_edge,
        shade: computed.shading.clone().map(|fill| {
            let width = (right_edge - normal_x).max(0.0);
            (normal_x, width, fill)
        }),
        grid_line_pitch,
        flows: Vec::new(),
        anchors: Vec::new(),
        math_block: false,
        exclusions,
        line_y: 0.0,
    };
    let mut current = LineBuilder::new();
    let mut x = if marker.is_some() { normal_x } else { first_x };
    for stop in &computed.tabs {
        if stop.alignment != TabAlignment::Bar {
            continue;
        }
        let bar_x = content_left + twips_to_px(stop.position.value(), scale);
        current.graphics.push(Item::Line(crate::layout::LineItem {
            x1: bar_x,
            y1: -12.0,
            x2: bar_x,
            y2: 2.0,
            color: "#000000".to_owned(),
            width: 0.75,
            dashed: false,
        }));
    }

    let mut pending: VecDeque<Seg> = segments.into();
    while let Some(segment) = pending.pop_front() {
        match segment {
            Seg::PageBreak => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                sink.flows.push(Flow::PageBreak);
                // The square wrap stays on the page where the anchor was placed.
                sink.exclusions.clear();
                sink.line_y = 0.0;
                x = normal_x;
            }
            Seg::ColumnBreak => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                sink.flows.push(Flow::ColumnBreak);
                sink.exclusions.clear();
                sink.line_y = 0.0;
                x = normal_x;
            }
            Seg::Break => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                x = normal_x;
            }
            Seg::Tab => {
                place_tab(
                    ctx,
                    computed,
                    &mut current,
                    &mut x,
                    &pending,
                    default_tab,
                    scale,
                    content_left,
                );
            }
            Seg::Image(image) => {
                // An empty line here is not a line the paragraph asked for. A
                // picture that is the whole paragraph (a field result under a
                // `w:sz="0"` mark) was pushed down by that line: the mark's size
                // collapses to 1 px, which is 15 twips, and the picture followed
                // it. Text that actually precedes the picture still ends its line.
                if !current.is_empty() {
                    sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                }
                let mut image = image;
                image.x = normal_x;
                sink.flows.push(Flow::Image(image));
                x = normal_x;
            }
            Seg::Object(items, width, height) => {
                // An inline drawing sits in the text line with its bottom on the
                // baseline, as Word places `wp:inline` (STAGE-5C-REWORK-1 D3).
                let line_end = sink.right_edge;
                if x + width > line_end + 1e-9 && !current.is_empty() {
                    sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                    x = normal_x;
                }
                for item in items {
                    current.graphics.push(shift_item(item, x, -height));
                }
                current.object_height = current.object_height.max(height);
                x += width;
            }
            Seg::Anchor(anchor) => sink.anchors.push(*anchor),
            Seg::Math(boxed, run) => {
                place_formula(&mut sink, &mut current, &mut x, normal_x, boxed);
                let _ = run;
            }
            Seg::MathParagraph(boxed) => {
                // A display formula owns its line, as Word/WPS lay it out. The
                // line it interrupts is emitted only when it carries content: an
                // empty placeholder line would add a whole empty line of
                // leading above and below the block, which is what pushed the
                // following text a full line down (STAGE-5C-REWORK-1 C1).
                let pending = std::mem::replace(&mut current, LineBuilder::new());
                if !pending.is_empty() {
                    sink.emit(pending, false);
                }
                let properties = ctx.document.settings.math_properties.as_ref();
                let before = math_spacing(
                    ctx,
                    properties.and_then(|props| props.pre_space.as_deref()),
                    "preSp",
                );
                let after = math_spacing(
                    ctx,
                    properties.and_then(|props| props.post_space.as_deref()),
                    "postSp",
                );
                let height = before + boxed.size.height + boxed.size.depth + after;
                let items = boxed
                    .items
                    .into_iter()
                    .map(|item| shift_item(item, 0.0, before + boxed.size.height))
                    .collect();
                sink.flows.push(Flow::Block { items, height });
                sink.math_block = true;
                x = normal_x;
            }
            Seg::Text(text, run) => {
                let tokens = tokenize(&text);
                for (index, token) in tokens.iter().enumerate() {
                    let following = tokens
                        .get(index + 1)
                        .and_then(|next| next.chars().find(|ch| !ch.is_whitespace()));
                    place_token(
                        &mut sink,
                        &mut current,
                        &mut x,
                        normal_x,
                        token,
                        &run,
                        following,
                    );
                }
            }
            Seg::FootnoteMarker(id, run) => {
                if let Some((number, format)) = ctx.note_numbers.footnote_number(id) {
                    push_note_marker(ctx, &mut current, &mut x, &format.format(number), &run);
                    current.footnote_refs.push(id);
                }
            }
            Seg::EndnoteMarker(id, run) => {
                if let Some((number, format)) = ctx.note_numbers.endnote_number(id) {
                    push_note_marker(ctx, &mut current, &mut x, &format.format(number), &run);
                }
            }
            Seg::NoteNumber(run) => {
                if let Some(text) = note_marker {
                    push_note_marker(ctx, &mut current, &mut x, text, &run);
                }
            }
            Seg::FieldResult(kind, format, run) => {
                push_field_marker(
                    ctx,
                    &mut current,
                    &mut x,
                    &run,
                    crate::fields::FieldMarker { kind, format },
                );
            }
        }
    }
    let current_is_empty = current.is_empty();
    if !(sink.math_block && current_is_empty) {
        sink.emit(current, true);
    }
    let mut flows = sink.flows;
    let anchors = sink.anchors;

    // Prepend the numbering marker to the first line, if any.
    if let Some((marker_text, marker_run, suffix, pic_bullet, justification)) = &marker {
        if let Some(Flow::Line(first)) = flows.iter_mut().find(|flow| matches!(flow, Flow::Line(_)))
        {
            let size_px = ctx.size_px(marker_run.size_pt);
            // A picture bullet is drawn at the marker's font size, its bottom on
            // the baseline and its width by the picture's own proportions - the
            // size Word gives it whatever the picture's declared extent.
            let picture = pic_bullet
                .and_then(|id| ctx.document.numbering.pic_bullet(id))
                .and_then(|bullet| layout_inline_image(ctx, &bullet.drawing, 0.0, 0.0))
                .map(|image| {
                    let width = if image.h > 0.0 {
                        image.w * size_px / image.h
                    } else {
                        size_px
                    };
                    (image, width)
                });
            let width = picture
                .as_ref()
                .map_or_else(|| ctx.measure(marker_text, marker_run), |(_, width)| *width);
            let clearance = marker_clearance(ctx, marker_run, suffix.as_deref());
            // `w:lvlJc`: a right-aligned marker ends on the number position
            // and a centred one straddles it.
            let mut marker_x = match justification {
                Some(strict_ooxml_wml::model::values::Justification::End) => first_x - width,
                Some(strict_ooxml_wml::model::values::Justification::Center) => {
                    first_x - width / 2.0
                }
                _ => first_x,
            };
            // The text anchor stays where the paragraph declared it. A marker
            // that would share that anchor, or that is wider than the hanging
            // gap, moves left by the suffix clearance.
            if marker_x + width + clearance > normal_x + 1e-6 {
                marker_x = normal_x - clearance - width;
            }
            if let Some((image, width)) = picture {
                first.graphics.push(Item::Image(ImageItem {
                    x: marker_x,
                    y: -size_px,
                    w: width,
                    h: size_px,
                    ..image
                }));
            } else {
                first.items.insert(
                    0,
                    TextItem {
                        x: marker_x,
                        baseline: first.ascent,
                        width,
                        text: marker_text.clone(),
                        run: marker_run.clone(),
                        size_px,
                        advance: ctx.advance_kind_for(marker_text, marker_run),
                        field: None,
                        compress_punctuation: false,
                        following_non_space: None,
                        plain_space_factor: 1.0,
                    },
                );
            }
        }
    }

    let _ = para;
    (flows, anchors)
}

/// The gap a numbering suffix keeps between the marker and the text.
fn marker_clearance(ctx: &LayoutContext<'_>, run: &ComputedRun, suffix: Option<&str>) -> f64 {
    match suffix.unwrap_or("tab") {
        "space" => ctx.measure(" ", run),
        // `nothing` and `tab` do not insert a measured glyph. A tab lands on
        // the text indent, which is already the next stop.
        _ => 0.0,
    }
}

/// Splits text into wrap tokens (words keep a single trailing space).
fn tokenize(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split_inclusive(' ').map(str::to_owned).collect()
}

/// Shifts a paint item vertically (used to place a display formula's ink).
fn shift_item(item: Item, dx: f64, dy: f64) -> Item {
    match item {
        Item::Text(mut text) => {
            text.x += dx;
            text.baseline += dy;
            Item::Text(text)
        }
        Item::Rect(mut rect) => {
            rect.x += dx;
            rect.y += dy;
            Item::Rect(rect)
        }
        Item::Line(mut line) => {
            line.x1 += dx;
            line.y1 += dy;
            line.x2 += dx;
            line.y2 += dy;
            Item::Line(line)
        }
        Item::Path(mut path) => {
            path.x += dx;
            path.y += dy;
            Item::Path(path)
        }
        Item::Image(mut image) => {
            image.x += dx;
            image.y += dy;
            Item::Image(image)
        }
    }
}

/// Places an inline formula on the current line, wrapping when it does not fit.
#[allow(clippy::too_many_arguments)]
fn place_formula(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    normal_x: f64,
    boxed: crate::math::layout::MathBox,
) {
    let width = boxed.size.width;
    let line_end = sink.right_edge;
    if *x + width > line_end + 1e-9 && !current.is_empty() {
        sink.emit(std::mem::replace(current, LineBuilder::new()), false);
        *x = normal_x;
    }
    current.inline_fraction |= boxed.inline_fraction;
    for item in boxed.items {
        current.graphics.push(shift_item(item, *x, 0.0));
    }
    current.ink_height = current.ink_height.max(boxed.size.height);
    current.ink_depth = current.ink_depth.max(boxed.size.depth);
    *x += width;
}

/// Keep the construction's nominal metrics while reserving its painted extent
/// in the surrounding paragraph. Font ascent is not the height of every glyph.
pub(crate) fn formula_ink(ctx: &LayoutContext<'_>, items: &[Item]) -> (f64, f64) {
    let mut above: f64 = 0.0;
    let mut below: f64 = 0.0;
    for item in items {
        let (top, bottom) = match item {
            Item::Text(text) => {
                let family = crate::style::chosen_family(&text.run, &text.text);
                let metrics = ctx.font.metrics(&family, text.run.bold, text.run.italic);
                let bounds = text
                    .text
                    .chars()
                    .filter_map(|ch| {
                        ctx.font
                            .glyph_vertical_em(&family, ch, text.run.bold, text.run.italic)
                    })
                    .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))
                    .unwrap_or((metrics.ascent_em(), metrics.descender));
                (
                    text.baseline - bounds.0 * text.size_px,
                    text.baseline + bounds.1 * text.size_px,
                )
            }
            Item::Line(line) => (
                line.y1.min(line.y2) - line.width / 2.0,
                line.y1.max(line.y2) + line.width / 2.0,
            ),
            Item::Rect(rect) => (
                rect.y - rect.stroke_w / 2.0,
                rect.y + rect.h + rect.stroke_w / 2.0,
            ),
            Item::Path(path) => (
                path.y - path.stroke_w / 2.0,
                path.y + path.h + path.stroke_w / 2.0,
            ),
            Item::Image(image) => (image.y, image.y + image.h),
        };
        above = above.max(-top);
        below = below.max(bottom);
    }
    (above, below)
}

fn resolve_math_line_metrics(
    line: &mut LineBuilder,
    mut height: f64,
    mut ascent: f64,
) -> (f64, f64) {
    let natural = height.max(1.0);
    if line.ink_height > 0.0 || line.ink_depth > 0.0 {
        ascent = ascent.max(line.ink_height);
        height = height.max(ascent + line.ink_depth);
        let allowed = natural * MAX_MATH_LINE_GROWTH;
        if height > allowed && !line.inline_fraction {
            let excess = height - natural;
            let factor = ((allowed - natural) / excess).clamp(0.0, 1.0);
            line.ink_height *= factor;
            line.ink_depth *= factor;
            ascent = ascent.max(line.ink_height);
            height = allowed.max(ascent + line.ink_depth);
        }
    }
    (height, ascent)
}

fn warn_inline_vertical_overflow(
    ctx: &LayoutContext<'_>,
    line: &LineBuilder,
    ascent: f64,
    height: f64,
) {
    if line.ink_height == 0.0 && line.ink_depth == 0.0 {
        return;
    }
    let (above, below) = formula_ink(ctx, &line.graphics);
    let excess = (above - ascent).max(ascent + below - height);
    if excess > 0.25 {
        ctx.warn(format!("render.math-line-overflow: inline formula extends {excess:.3} px beyond its reserved vertical line box"));
    }
}

/// Accumulates finished lines and break/image flows in order.
struct LineSink<'a, 'b> {
    ctx: &'b LayoutContext<'a>,
    computed: &'b ComputedParagraph,
    /// Absolute right edge. The first line starts further in and still ends here.
    right_edge: f64,
    /// Edge of subsequent lines after the direction-dependent first-line indent.
    normal_right_edge: f64,
    /// Paragraph shading: x, width, `#rrggbb`. Drawn behind each line.
    shade: Option<(f64, f64, String)>,
    grid_line_pitch: Option<f64>,
    flows: Vec<Flow>,
    anchors: Vec<AnchorDrawing>,
    /// Whether the last flow is a display-formula block (so a trailing empty
    /// line must not be synthesised after it).
    math_block: bool,
    /// Square-wrap rectangles in paragraph-local coordinates.
    exclusions: Vec<WrapExclusion>,
    /// Top of the line currently being filled, in paragraph-local px.
    line_y: f64,
}

impl LineSink<'_, '_> {
    fn emit(&mut self, line: LineBuilder, last: bool) {
        let mut finished = finish_line(
            self.ctx,
            self.computed,
            line,
            self.right_edge,
            self.grid_line_pitch,
            last,
        );
        if let Some((x, width, fill)) = &self.shade {
            finished.graphics.insert(
                0,
                Item::Rect(crate::layout::RectItem {
                    x: *x,
                    y: 0.0,
                    w: *width,
                    h: finished.height,
                    fill: Some(fill.clone()),
                    stroke: None,
                    stroke_w: 0.0,
                }),
            );
        }
        let height = finished.height;
        self.flows.push(Flow::Line(finished));
        self.right_edge = self.normal_right_edge;
        self.line_y += height;
        self.math_block = false;
    }
}

/// Wrap boxes from anchors in this paragraph (Square / TopAndBottom).
fn anchor_exclusions(
    ctx: &LayoutContext<'_>,
    segments: &[Seg],
    host: Option<(&Geometry, f64, f64)>,
    content_left: f64,
    content_width: f64,
) -> Vec<WrapExclusion> {
    let Some((geometry, host_x, host_y)) = host else {
        // Without page geometry only paragraph-relative anchors can be placed; use
        // a synthetic geometry so extent resolution still matches the content box.
        let geometry = Geometry {
            width: content_left + content_width,
            height: content_width.max(1.0) * 2.0,
            left: content_left,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            grid_line_pitch: None,
            columns: crate::layout::ColumnLayout::single(content_width),
            active_column: 0,
        };
        return segments
            .iter()
            .filter_map(|segment| {
                let Seg::Anchor(anchor) = segment else {
                    return None;
                };
                wrap_exclusion(ctx, anchor, &geometry, content_left, 0.0, 0.0)
            })
            .collect();
    };
    let _ = content_width;
    // The paginator lays every column out from `content_left` and paints it at
    // its column's left edge, `host_x`.
    let column_dx = host_x - content_left;
    segments
        .iter()
        .filter_map(|segment| {
            let Seg::Anchor(anchor) = segment else {
                return None;
            };
            wrap_exclusion(ctx, anchor, geometry, host_x, host_y, column_dx)
        })
        .collect()
}

/// Provisional height of the line currently being filled.
fn provisional_line_height(sink: &LineSink<'_, '_>) -> f64 {
    let (height, _) = resolve_line_metrics(sink.ctx, sink.computed, None, sink.grid_line_pitch);
    height.max(1.0)
}

/// Lowest bottom edge of exclusions that fully block the current provisional line.
fn blocked_line_bottom(sink: &LineSink<'_, '_>, line_height: f64) -> Option<f64> {
    let line_top = sink.line_y;
    let line_bottom = sink.line_y + line_height;
    let mut bottom = None;
    for exclusion in &sink.exclusions {
        if line_bottom <= exclusion.top || line_top >= exclusion.bottom {
            continue;
        }
        // A full-width band (TopAndBottom) or any exclusion that leaves no free
        // span advances past its bottom rather than placing ink inside it.
        let edge = if exclusion.contour.is_some() {
            line_top + line_height
        } else {
            exclusion.bottom
        };
        bottom = Some(bottom.map_or(edge, |value: f64| value.max(edge)));
    }
    bottom
}

/// Skips vertical space covered by exclusions that leave no free interval.
fn advance_past_blocked(
    sink: &mut LineSink<'_, '_>,
    current: &LineBuilder,
    x: &mut f64,
    normal_x: f64,
) -> bool {
    if !current.is_empty() {
        return false;
    }
    let mut advanced = false;
    for _ in 0..64 {
        let line_height = provisional_line_height(sink);
        let spans = free_spans(sink, normal_x, line_height);
        if !spans.is_empty() {
            break;
        }
        let Some(bottom) = blocked_line_bottom(sink, line_height) else {
            break;
        };
        let skip = (bottom - sink.line_y).max(0.0);
        if skip <= 0.0 {
            break;
        }
        sink.flows.push(Flow::Block {
            items: Vec::new(),
            height: skip,
        });
        sink.line_y += skip;
        *x = normal_x;
        advanced = true;
    }
    advanced
}

/// Moves `x` onto a free interval wide enough for `width`.
///
/// Returns false when no interval on an empty line can hold it.
fn reserve(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    normal_x: f64,
    width: f64,
) -> bool {
    for _ in 0..24 {
        let line_height = provisional_line_height(sink);
        let spans = free_spans(sink, normal_x, line_height);
        if spans.is_empty() {
            if advance_past_blocked(sink, current, x, normal_x) {
                continue;
            }
            if current.is_empty() {
                return false;
            }
            sink.emit(std::mem::replace(current, LineBuilder::new()), false);
            *x = normal_x;
            continue;
        }
        let Some((index, &(start, end))) = spans
            .iter()
            .enumerate()
            .find(|(_, (start, end))| *x < *end - 1e-6 && *end - start.max(*x) + 1e-9 >= 0.0)
        else {
            if advance_past_blocked(sink, current, x, normal_x) {
                continue;
            }
            if current.is_empty() {
                return false;
            }
            sink.emit(std::mem::replace(current, LineBuilder::new()), false);
            *x = normal_x;
            continue;
        };
        if *x < start {
            *x = start;
        }
        if *x + width <= end + 1e-9 {
            return true;
        }
        if let Some((next, _)) = spans.get(index + 1) {
            *x = *next;
            continue;
        }
        if advance_past_blocked(sink, current, x, normal_x) {
            continue;
        }
        if current.is_empty() {
            return false;
        }
        sink.emit(std::mem::replace(current, LineBuilder::new()), false);
        *x = normal_x;
    }
    false
}

/// Free intervals of the current line, left to right.
fn free_spans(sink: &LineSink<'_, '_>, left: f64, line_height: f64) -> Vec<(f64, f64)> {
    let mut spans = vec![(left, sink.right_edge)];
    for exclusion in &sink.exclusions {
        if sink.line_y + line_height <= exclusion.top || sink.line_y >= exclusion.bottom {
            continue;
        }
        if let Some(contour) = &exclusion.contour {
            let occupied: Vec<(f64, f64)> = contour
                .intervals(
                    sink.line_y + exclusion.page_y,
                    sink.line_y + line_height + exclusion.page_y,
                )
                .into_iter()
                .map(|(left, right)| (left - exclusion.page_dx, right - exclusion.page_dx))
                .collect();
            if exclusion.side == WrapSide::Both {
                for (left, right) in occupied {
                    subtract(&mut spans, left, right);
                }
            } else if let (Some(first), Some(last)) = (occupied.first(), occupied.last()) {
                let bounds = WrapExclusion {
                    left: first.0,
                    right: last.1,
                    ..exclusion.clone()
                };
                apply_side(&mut spans, &bounds);
            }
        } else {
            apply_side(&mut spans, exclusion);
        }
    }
    spans.retain(|(start, end)| *end - *start > 0.5);
    spans
}

fn apply_side(spans: &mut Vec<(f64, f64)>, exclusion: &WrapExclusion) {
    match exclusion.side {
        WrapSide::Both => subtract(spans, exclusion.left, exclusion.right),
        WrapSide::Left => clip_end(spans, exclusion.left),
        WrapSide::Right => clip_start(spans, exclusion.right),
        WrapSide::Largest => {
            let mut left_spans = spans.clone();
            clip_end(&mut left_spans, exclusion.left);
            let mut right_spans = spans.clone();
            clip_start(&mut right_spans, exclusion.right);
            let width =
                |side: &[(f64, f64)]| side.iter().map(|(start, end)| end - start).sum::<f64>();
            *spans = if width(&right_spans) > width(&left_spans) {
                right_spans
            } else {
                left_spans
            };
        }
    }
}

fn subtract(spans: &mut Vec<(f64, f64)>, left: f64, right: f64) {
    let mut next = Vec::new();
    for (start, end) in spans.drain(..) {
        if right <= start || left >= end {
            next.push((start, end));
            continue;
        }
        if start < left {
            next.push((start, left.min(end)));
        }
        if end > right {
            next.push((right.max(start), end));
        }
    }
    *spans = next;
}

fn clip_end(spans: &mut Vec<(f64, f64)>, limit: f64) {
    spans.retain(|(start, _)| *start < limit);
    for (start, end) in spans.iter_mut() {
        *end = (*end).min(limit).max(*start);
    }
    spans.retain(|(start, end)| end - start > 0.5);
}

fn clip_start(spans: &mut Vec<(f64, f64)>, limit: f64) {
    spans.retain(|(_, end)| *end > limit);
    for (start, end) in spans.iter_mut() {
        *start = (*start).max(limit).min(*end);
    }
    spans.retain(|(start, end)| end - start > 0.5);
}

#[allow(clippy::too_many_arguments)]
fn place_token(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    normal_x: f64,
    token: &str,
    run: &ComputedRun,
    following: Option<char>,
) {
    let compress = crate::style::compress_punctuation(
        sink.ctx
            .document
            .settings
            .character_spacing_control
            .as_deref(),
    );
    let plain = crate::style::plain_space_factor(sink.computed.alignment, run.size_pt);
    let width = sink
        .ctx
        .measure_with_next_factor(token, run, following, plain);
    if sink.exclusions.is_empty() {
        let line_end = sink.right_edge;
        if *x + width > line_end + 1e-9 && !current.is_empty() {
            sink.emit(std::mem::replace(current, LineBuilder::new()), false);
            *x = normal_x;
        }
        if current.is_empty() && token.trim().is_empty() {
            *x += width;
            return;
        }
        if *x + width > line_end + 1e-9 && current.is_empty() {
            place_long_token(sink, current, x, line_end, normal_x, token, run);
            return;
        }
    } else if !reserve(sink, current, x, normal_x, width) {
        if current.is_empty() && token.trim().is_empty() {
            *x += width;
            return;
        }
        place_long_token(sink, current, x, sink.right_edge, normal_x, token, run);
        return;
    } else if current.is_empty() && token.trim().is_empty() {
        *x += width;
        return;
    }
    let size_px = sink.ctx.size_px(run.size_pt);
    current.items.push(TextItem {
        x: *x,
        baseline: 0.0,
        width,
        text: token.to_owned(),
        run: run.clone(),
        size_px,
        advance: sink.ctx.advance_kind_for(token, run),
        field: None,
        compress_punctuation: compress,
        following_non_space: following,
        plain_space_factor: plain,
    });
    *x += width;
}

#[allow(clippy::too_many_arguments)]
fn place_long_token(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    line_end: f64,
    normal_x: f64,
    token: &str,
    run: &ComputedRun,
) {
    let size_px = sink.ctx.size_px(run.size_pt);
    let compress = crate::style::compress_punctuation(
        sink.ctx
            .document
            .settings
            .character_spacing_control
            .as_deref(),
    );
    let plain_space_factor = crate::style::plain_space_factor(sink.computed.alignment, run.size_pt);
    // One item per line-sized piece, not per character: a 1 MB unbroken token
    // used to become a million items, each with its own copy of the run. The
    // token is cut only at grapheme-cluster boundaries.
    let mut piece = String::new();
    let mut piece_x = *x;
    let mut piece_width = 0.0;
    for cluster in unicode_segmentation::UnicodeSegmentation::graphemes(token, true) {
        let width = sink.ctx.measure(cluster, run);
        let overflows = if sink.exclusions.is_empty() {
            *x + width > line_end + 1e-9 && !(current.is_empty() && piece.is_empty())
        } else {
            false
        };
        if overflows {
            if !piece.is_empty() {
                push_piece(
                    sink,
                    current,
                    &piece,
                    piece_x,
                    piece_width,
                    run,
                    size_px,
                    compress,
                    plain_space_factor,
                );
                piece.clear();
            }
            sink.emit(std::mem::replace(current, LineBuilder::new()), false);
            *x = normal_x;
            piece_x = *x;
            piece_width = 0.0;
        } else if !sink.exclusions.is_empty() {
            // Exclusions may move the pen; flush so the piece starts where
            // `reserve` puts it.
            if !piece.is_empty() {
                push_piece(
                    sink,
                    current,
                    &piece,
                    piece_x,
                    piece_width,
                    run,
                    size_px,
                    compress,
                    plain_space_factor,
                );
                piece.clear();
                piece_width = 0.0;
            }
            let _ = reserve(sink, current, x, normal_x, width);
            piece_x = *x;
        }
        piece.push_str(cluster);
        piece_width += width;
        *x += width;
    }
    if !piece.is_empty() {
        push_piece(
            sink,
            current,
            &piece,
            piece_x,
            piece_width,
            run,
            size_px,
            compress,
            plain_space_factor,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_piece(
    sink: &LineSink<'_, '_>,
    current: &mut LineBuilder,
    text: &str,
    x: f64,
    width: f64,
    run: &ComputedRun,
    size_px: f64,
    compress_punctuation: bool,
    plain_space_factor: f64,
) {
    current.items.push(TextItem {
        x,
        baseline: 0.0,
        width,
        text: text.to_owned(),
        run: run.clone(),
        size_px,
        advance: sink.ctx.advance_kind_for(text, run),
        field: None,
        compress_punctuation,
        following_non_space: None,
        plain_space_factor,
    });
}

/// Blend from identity at the line origin to `scale` by ~160 px of run-in.
///
/// Early tokens (Clio `modern.4` after a short `each `) are already within
/// 0.25 px of WPS; a flat 1.008 stretch tips them over. Mid/late tokens still
/// need the full substitute scale.
fn distance_blend_scale(rel: f64, scale: f64) -> f64 {
    if (scale - 1.0).abs() <= f64::EPSILON {
        return 1.0;
    }
    // Bold shrink must apply fully; blending left modern.2 ~1.7 px wide.
    if scale < 1.0 {
        return scale;
    }
    if rel <= 0.0 {
        return 1.0;
    }
    let t = (rel / 160.0).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    1.0 + (scale - 1.0) * t
}

/// Stretch line positions after wrap/justify so Tinos tracks WPS Times.
///
/// Wrap and justification keep unscaled advances (stable line breaks). This
/// then scales each item's distance from the line origin by the substitute
/// factor. Applying the scale *before* justify let redistributed gaps cancel
/// the correction on nearly-full `both` lines.
fn apply_substitute_width_reflow(
    items: &mut [TextItem],
    alignment: strict_ooxml_wml::model::values::Justification,
) {
    let allow_bold = matches!(
        alignment,
        strict_ooxml_wml::model::values::Justification::Both
            | strict_ooxml_wml::model::values::Justification::Justify
            | strict_ooxml_wml::model::values::Justification::Distribute
    );
    let scale_for = |item: &TextItem| -> f64 {
        let family = crate::style::chosen_family(&item.run, &item.text);
        let scale = crate::font::substitute_width_scale(&family, item.run.bold);
        if item.run.bold && !allow_bold {
            1.0
        } else {
            scale
        }
    };
    if items.len() < 2 {
        if let Some(item) = items.first_mut() {
            let scale = scale_for(item);
            if (scale - 1.0).abs() > f64::EPSILON {
                item.width *= scale;
            }
        }
        return;
    }
    let scales: Vec<f64> = items.iter().map(scale_for).collect();
    if scales.iter().all(|s| (*s - 1.0).abs() <= f64::EPSILON) {
        return;
    }
    let origin = items
        .iter()
        .map(|item| item.x)
        .fold(f64::INFINITY, f64::min);
    if !origin.is_finite() {
        return;
    }
    // When every run shares one scale, stretch uniformly (including justify gaps).
    let Some(&first) = scales.first() else {
        return;
    };
    if scales.iter().all(|s| (*s - first).abs() <= 1e-9) {
        for (item, scale) in items.iter_mut().zip(scales.iter().copied()) {
            let rel = item.x - origin;
            let factor = distance_blend_scale(rel, scale);
            item.x = origin + rel * factor;
            item.width *= factor;
        }
        return;
    }
    // Mixed bold/regular: keep justify gaps, scale each run's width, and scale
    // gaps by the following run's factor so later origins stay consistent.
    // Snapshot `(index, x, width, scale)` so the walk reads pre-scaling geometry.
    let mut order: Vec<(usize, f64, f64, f64)> = items
        .iter()
        .zip(scales.iter().copied())
        .enumerate()
        .map(|(idx, (item, scale))| (idx, item.x, item.width, scale))
        .collect();
    // `total_cmp`: a NaN origin from hostile metrics must not abort the sort.
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut cursor = origin;
    let mut prev_right = origin;
    for (idx, ox, ow, scale) in order {
        let gap = (ox - prev_right).max(0.0);
        let gap_factor = distance_blend_scale((ox - origin).max(0.0), scale);
        cursor += gap * gap_factor;
        let width_factor = distance_blend_scale((ox - origin).max(0.0), scale);
        if let Some(item) = items.get_mut(idx) {
            item.x = cursor;
            item.width = ow * width_factor;
            cursor += item.width;
        }
        prev_right = ox + ow;
    }
}

/// Carry text-origin corrections through to formulas and inline drawings.
fn reflow_with_graphics(
    line: &mut LineBuilder,
    alignment: strict_ooxml_wml::model::values::Justification,
) {
    if line.graphics.is_empty() {
        apply_substitute_width_reflow(&mut line.items, alignment);
        return;
    }
    let original: Vec<f64> = line.items.iter().map(|text| text.x + text.width).collect();
    apply_substitute_width_reflow(&mut line.items, alignment);
    for graphic in &mut line.graphics {
        let left = item_left(graphic);
        let shift = original
            .iter()
            .zip(&line.items)
            .filter(|(right, _)| **right <= left + 1e-6)
            .max_by(|(a, _), (b, _)| a.total_cmp(b))
            .map_or(0.0, |(right, text)| text.x + text.width - right);
        *graphic = shift_item(graphic.clone(), shift, 0.0);
    }
}

/// Reports advance, outline and object edges independently.
fn warn_line_overflow(ctx: &LayoutContext<'_>, line: &LineBuilder, right_edge: f64) {
    if let Some(item) = line
        .items
        .iter()
        .find(|item| item.x + item.width > right_edge + 0.25)
    {
        ctx.warn(format!(
            "render.line-overflow: text {:?}, family {:?}, x={:.3}, width={:.3}, right={:.3}, limit={:.3}",
            item.text.chars().take(96).collect::<String>(), item.run.family,
            item.x, item.width, item.x + item.width, right_edge,
        ));
    }
    for item in line
        .items
        .iter()
        .filter(|item| item.x + item.width + item.size_px * 2.0 > right_edge)
    {
        if let Some(ink_right) = crate::paint::text::text_ink_right(item) {
            if ink_right > right_edge + 0.25 {
                ctx.warn(format!(
                    "render.glyph-overflow: text {:?}, family {:?}, ink_right={ink_right:.3}, advance_right={:.3}, limit={right_edge:.3}",
                    item.text.chars().take(96).collect::<String>(), item.run.family, item.x + item.width,
                ));
                break;
            }
        }
    }
    if let Some(item) = line
        .graphics
        .iter()
        .find(|item| item_right(item) > right_edge + 0.25)
    {
        let kind = match item {
            Item::Text(_) => "formula text",
            Item::Rect(_) => "rectangle",
            Item::Line(_) => "line",
            Item::Path(_) => "path",
            Item::Image(_) => "image",
        };
        ctx.warn(format!(
            "render.object-overflow: {kind}, right={:.3}, limit={right_edge:.3}",
            item_right(item)
        ));
    }
}

/// Finalizes a line: baseline, justification and alignment.
fn finish_line(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    mut line: LineBuilder,
    right_edge: f64,
    grid_line_pitch: Option<f64>,
    last: bool,
) -> TextLine {
    let used = line_extent(&line.items, &line.graphics);
    let origin = line_origin(&line.items, &line.graphics);
    // Align inside this line's own interval. A positive first-line indent
    // shortens that interval; a hanging indent lengthens it. Both end at
    // `right_edge`. A non-positive interval is reported, not replaced by 1 px.
    let line_width = if origin.is_finite() {
        right_edge - origin
    } else {
        0.0
    };
    if used > 0.0 && line_width <= 0.0 {
        ctx.warn("render.line-interval: indent leaves no room before the right edge".to_owned());
    }
    let justify = !last
        && matches!(
            computed.alignment,
            strict_ooxml_wml::model::values::Justification::Both
                | strict_ooxml_wml::model::values::Justification::Justify
                | strict_ooxml_wml::model::values::Justification::Distribute
        );

    if justify && used < line_width {
        let gaps = line
            .items
            .iter()
            .filter(|item| item.text.ends_with(' '))
            .count();
        if gaps > 0 {
            let extra = (line_width - used) / gaps as f64;
            let boundaries: Vec<f64> = line
                .items
                .iter()
                .filter(|item| item.text.ends_with(' '))
                .map(|item| item.x + item.width)
                .collect();
            for graphic in &mut line.graphics {
                let left = item_left(graphic);
                let shift = boundaries
                    .iter()
                    .filter(|right| **right <= left + 1e-6)
                    .count() as f64
                    * extra;
                *graphic = shift_item(graphic.clone(), shift, 0.0);
            }
            let mut shift = 0.0;
            for item in &mut line.items {
                item.x += shift;
                if item.text.ends_with(' ') {
                    shift += extra;
                }
            }
        }
    }

    let used = if justify { line_width } else { used };
    let offset = match computed.alignment {
        strict_ooxml_wml::model::values::Justification::Center => {
            (line_width - used).max(0.0) / 2.0
        }
        strict_ooxml_wml::model::values::Justification::End => (line_width - used).max(0.0),
        _ => 0.0,
    };
    for item in &mut line.items {
        item.x += offset;
    }
    if offset != 0.0 {
        for item in &mut line.graphics {
            *item = shift_item(item.clone(), offset, 0.0);
        }
    }

    // After wrap + justify + alignment: stretch Tinos to WPS Times metrics.
    if ctx.options.wps_times_calibration {
        reflow_with_graphics(&mut line, computed.alignment);
    }

    warn_line_overflow(ctx, &line, right_edge);

    let (mut height, mut ascent) =
        resolve_line_metrics(ctx, computed, line.items.first(), grid_line_pitch);
    (height, ascent) = resolve_math_line_metrics(&mut line, height, ascent);
    // The nominal cap is a spacing preference, never permission to overlap
    // adjacent lines. Reserve only the actual painted extent beyond it.
    if line.ink_height > 0.0 || line.ink_depth > 0.0 {
        let (above, below) = formula_ink(ctx, &line.graphics);
        let depth = (height - ascent).max(0.0);
        ascent = ascent.max(above);
        height = height.max(ascent + depth.max(below));
    }
    // An inline drawing keeps its full extent: the line grows to hold it, as
    // Word does, instead of being capped like a formula.
    //
    // The line **keeps its depth**. A line box has a depth even when its tallest
    // item has none: the paragraph mark is still a glyph with a descent, and Word
    // keeps the room for it. Growing only the ascent made every drawing line
    // exactly as tall as its drawing, and on `07-strict-drawingml-shapes` — four
    // shapes, no formulas, so nothing here is confounded by the math-font
    // substitution — that put each block 4 px closer to the next than the
    // reference has it, which is the whole of that page's 16 px bottom-edge error.
    if line.object_height > 0.0 {
        let depth = (height - ascent).max(0.0);
        ascent = ascent.max(line.object_height);
        height = height.max(ascent + depth);
    }
    warn_inline_vertical_overflow(ctx, &line, ascent, height);
    for item in &mut line.items {
        item.baseline = ascent;
        apply_vertical_align(item);
    }
    // The graphics were placed relative to the baseline, which is only now
    // known.
    for item in &mut line.graphics {
        *item = shift_item(item.clone(), 0.0, ascent);
    }
    TextLine {
        items: line.items,
        graphics: line.graphics,
        height,
        ascent,
        footnote_refs: line.footnote_refs,
        suppress_line_number: computed.suppress_line_numbers,
    }
}

/// Appends a note marker to the line, advancing `x`.
fn push_note_marker(
    ctx: &LayoutContext<'_>,
    current: &mut LineBuilder,
    x: &mut f64,
    text: &str,
    run: &ComputedRun,
) {
    let width = ctx.measure(text, run);
    let size_px = ctx.size_px(run.size_pt);
    current.items.push(TextItem {
        x: *x,
        baseline: 0.0,
        width,
        text: text.to_owned(),
        run: run.clone(),
        size_px,
        advance: ctx.advance_kind_for(text, run),
        field: None,
        compress_punctuation: false,
        following_non_space: None,
        plain_space_factor: 1.0,
    });
    *x += width;
}

/// Appends a computed-field placeholder to the line, advancing `x`.
fn push_field_marker(
    ctx: &LayoutContext<'_>,
    current: &mut LineBuilder,
    x: &mut f64,
    run: &ComputedRun,
    marker: crate::fields::FieldMarker,
) {
    // AUD-70: headers/footers set `field_env` so PAGE/NUMPAGES resolve per page
    // during layout. The body still places a placeholder and fixes it in
    // `Paginator::resolve_fields`.
    let (value, format) = match ctx.field_env.get() {
        Some(env) => env.resolve(marker),
        None => (
            1,
            marker.format.unwrap_or(crate::notes::NumberFormat::Decimal),
        ),
    };
    let text = format.format(value);
    let width = ctx.measure(&text, run);
    let size_px = ctx.size_px(run.size_pt);
    let advance = ctx.advance_kind_for(&text, run);
    current.items.push(TextItem {
        x: *x,
        baseline: 0.0,
        width,
        text,
        run: run.clone(),
        size_px,
        advance,
        field: Some(marker),
        compress_punctuation: false,
        following_non_space: None,
        plain_space_factor: 1.0,
    });
    *x += width;
}

/// Adjusts a superscript/subscript item's size and baseline, and applies the
/// run's `w:position` raise or lowering.
fn apply_vertical_align(item: &mut TextItem) {
    let base = item.size_px;
    if item.run.position_pt != 0.0 && item.run.size_pt > 0.0 {
        // `size_px` is `size_pt` at the page scale, so the ratio converts the
        // raise to px without the layout context.
        item.baseline -= item.run.position_pt * base / item.run.size_pt;
    }
    match item.run.vert_align {
        VertAlign::Superscript => {
            item.baseline -= base * 0.33;
            item.size_px = base * 0.65;
        }
        VertAlign::Subscript => {
            item.baseline += base * 0.20;
            item.size_px = base * 0.65;
        }
        VertAlign::Baseline => {}
    }
}

/// Returns the horizontal extent covered by a line's items.
fn line_extent(items: &[TextItem], graphics: &[Item]) -> f64 {
    let min = line_origin(items, graphics);
    let mut max = f64::NEG_INFINITY;
    for item in items {
        max = max.max(item.x + item.width);
    }
    for item in graphics {
        max = max.max(item_right(item));
    }
    if min.is_finite() && max.is_finite() {
        (max - min).max(0.0)
    } else {
        0.0
    }
}

/// Left edge of the line's ink, before alignment shifts it.
fn line_origin(items: &[TextItem], graphics: &[Item]) -> f64 {
    let mut min = f64::INFINITY;
    for item in items {
        min = min.min(item.x);
    }
    for item in graphics {
        min = min.min(item_left(item));
    }
    min
}

/// The left edge of a paint item.
fn item_left(item: &Item) -> f64 {
    match item {
        Item::Text(text) => text.x,
        Item::Rect(rect) => rect.x,
        Item::Path(path) => path.x,
        Item::Line(line) => line.x1.min(line.x2),
        Item::Image(image) => image.x,
    }
}

/// The right edge of a paint item.
fn item_right(item: &Item) -> f64 {
    match item {
        Item::Text(text) => text.x + text.width,
        Item::Rect(rect) => rect.x + rect.w,
        Item::Path(path) => path.x + path.w,
        Item::Line(line) => line.x1.max(line.x2),
        Item::Image(image) => image.x + image.w,
    }
}

/// Computes line height and ascent for a line given its first item.
fn resolve_line_metrics(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    item: Option<&TextItem>,
    grid_line_pitch: Option<f64>,
) -> (f64, f64) {
    let run = item.map_or(&computed.default_run, |item| &item.run);
    let family = crate::style::chosen_family(run, item.map_or("", |text| text.text.as_str()));
    let metrics = ctx.font.metrics(&family, run.bold, run.italic);
    let size_px = ctx.size_px(run.size_pt);
    let natural_height = (metrics.line_height_em() * size_px).max(1.0);
    let natural_ascent = metrics.ascent_em() * size_px;

    let (height, ascent) = match computed.line_rule {
        LineSpacingRule::Exact => {
            let exact = computed
                .line_pt
                .map_or(natural_height, |pt| pt_to_px(pt, ctx.options.scale));
            let exact = exact.max(1.0);
            // Exact fixes the line advance, not the baseline.
            // - Font taller than the line: park on font ascent (ink sticks out).
            // - Top-bordered frame line (Clio HVR-I): 80% grid + border pad.
            // - First Exact line in a frame (Clio SNP `line=90`): keep
            //   `natural.max(exact*0.8)` so the block origin matches WPS.
            // - Later Exact lines: 80% grid, including after a taller Exact
            //   neighbour (Clio L16055 after `line=216`).
            let ascent = if natural_ascent > exact {
                natural_ascent
            } else if computed.border_before_pt > 0.0 {
                exact * 0.8
            } else {
                let after_taller = ctx
                    .frame_prior_exact
                    .get()
                    .is_some_and(|prev| prev > exact + 0.5);
                if after_taller {
                    ctx.frame_force_exact_grid.set(true);
                }
                // Clio bold Figure captions (style 50, ≈10.5pt) sit on the 80%
                // Exact grid in WPS. Small bold SNP labels (4.5pt) and regular
                // body Exact lines keep natural ascent.
                // Outside a frame every Exact line is on the grid (9fec5cb and
                // the f18 PDF baseline witness); the natural-ascent rule below
                // is the WPS frame behaviour only.
                let use_grid = !ctx.in_frame.get()
                    || ctx.frame_force_exact_grid.get()
                    || after_taller
                    || (run.bold && run.size_pt >= 9.0);
                if use_grid {
                    exact * 0.8
                } else {
                    natural_ascent.max(exact * 0.8)
                }
            };
            (exact, ascent)
        }
        LineSpacingRule::AtLeast => {
            let minimum = computed
                .line_pt
                .map_or(0.0, |pt| pt_to_px(pt, ctx.options.scale));
            let height = natural_height.max(minimum);
            (height, natural_ascent.max(height * 0.8).min(height))
        }
        LineSpacingRule::Auto => {
            let multiplier = computed.line_pt.map_or(1.0, |pt| pt / 12.0);
            let scaled = natural_height * multiplier.max(0.1);
            // When the document declares a line grid, each line occupies the
            // smallest whole number of grid units that fits the (possibly
            // explicit) auto line height, as Word/WPS do even with single
            // spacing (`w:line="240"`). The extra leading goes above the
            // baseline so the block keeps its vertical position within the grid.
            let (height, extra) = match grid_line_pitch {
                Some(pitch) if pitch > 0.0 => {
                    let height = (scaled / pitch).ceil().max(1.0) * pitch;
                    (height, height - scaled)
                }
                _ => (scaled, 0.0),
            };
            // The extra leading is centered on the natural line box so the glyph
            // block keeps its optical position within the snapped grid line.
            (height, (natural_ascent + extra / 2.0).min(height))
        }
    };
    (height, ascent)
}

/// A resolved tab stop: where it sits, how the following segment aligns, and
/// the leader drawn in the gap.
struct ResolvedTab {
    x: f64,
    alignment: TabAlignment,
    leader: Option<TabLeader>,
}

/// The next stop to the right of `x`. A custom stop wins; otherwise the
/// default step is a start-aligned stop with no leader.
fn resolve_tab(
    x: f64,
    computed: &ComputedParagraph,
    default_tab: i32,
    scale: f64,
    content_left: f64,
) -> ResolvedTab {
    let relative = x - content_left;
    let mut stops: Vec<&strict_ooxml_wml::model::values::TabStop> = computed
        .tabs
        .iter()
        .filter(|candidate| !matches!(candidate.alignment, TabAlignment::Clear | TabAlignment::Bar))
        .collect();
    stops.sort_by(|left, right| left.position.value().cmp(&right.position.value()));
    if let Some(stop) = stops
        .into_iter()
        .find(|stop| twips_to_px(stop.position.value(), scale) > relative + 1e-6)
    {
        return ResolvedTab {
            x: content_left + twips_to_px(stop.position.value(), scale),
            alignment: stop.alignment,
            leader: stop.leader,
        };
    }
    let tab_step = twips_to_px(default_tab.max(1), scale).max(1e-6);
    let advances = (relative / tab_step).floor() + 1.0;
    ResolvedTab {
        x: content_left + advances * tab_step,
        alignment: TabAlignment::Start,
        leader: None,
    }
}

/// Width of the segment that this tab aligns, up to the next tab or break.
fn following_ink(ctx: &LayoutContext<'_>, pending: &VecDeque<Seg>) -> f64 {
    let mut width = 0.0;
    for segment in pending {
        match segment {
            Seg::Text(text, run) => width += ctx.measure(text, run),
            Seg::Object(_, object_width, _) => width += object_width,
            Seg::Math(boxed, _) => width += boxed.size.width,
            Seg::FieldResult(kind, format, run) => {
                let marker = crate::fields::FieldMarker {
                    kind: *kind,
                    format: *format,
                };
                let (value, format) = ctx.field_env.get().map_or(
                    (1, format.unwrap_or(crate::notes::NumberFormat::Decimal)),
                    |env| env.resolve(marker),
                );
                width += ctx.measure(&format.format(value), run);
            }
            Seg::Tab
            | Seg::Break
            | Seg::PageBreak
            | Seg::ColumnBreak
            | Seg::Image(_)
            | Seg::MathParagraph(_) => break,
            Seg::Anchor(_)
            | Seg::FootnoteMarker(_, _)
            | Seg::EndnoteMarker(_, _)
            | Seg::NoteNumber(_) => {}
        }
    }
    width
}

/// Distance from the start of the following segment to its decimal separator.
/// Without a separator the whole segment is the anchor, as a right tab.
fn decimal_offset(ctx: &LayoutContext<'_>, pending: &VecDeque<Seg>) -> f64 {
    let mut width = 0.0;
    for segment in pending {
        match segment {
            Seg::Text(text, run) => {
                if let Some(head) = text.find(['.', ',']).and_then(|index| text.get(..index)) {
                    width += ctx.measure(head, run);
                    return width;
                }
                width += ctx.measure(text, run);
            }
            Seg::Tab | Seg::Break | Seg::PageBreak | Seg::ColumnBreak => break,
            _ => {}
        }
    }
    width
}

#[allow(clippy::too_many_arguments)]
fn place_tab(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    current: &mut LineBuilder,
    x: &mut f64,
    pending: &VecDeque<Seg>,
    default_tab: i32,
    scale: f64,
    content_left: f64,
) {
    let tab = resolve_tab(*x, computed, default_tab, scale, content_left);
    if matches!(tab.alignment, TabAlignment::Bar) {
        current.graphics.push(Item::Line(crate::layout::LineItem {
            x1: tab.x,
            y1: -12.0,
            x2: tab.x,
            y2: 2.0,
            color: "#000000".to_owned(),
            width: 0.75,
            dashed: false,
        }));
        return;
    }
    let ink = following_ink(ctx, pending);
    let text_start = match tab.alignment {
        TabAlignment::End => tab.x - ink,
        TabAlignment::Center => tab.x - ink / 2.0,
        TabAlignment::Decimal | TabAlignment::Num => tab.x - decimal_offset(ctx, pending),
        TabAlignment::Start | TabAlignment::Clear | TabAlignment::Bar => tab.x,
    };
    if text_start > *x + 0.5 {
        if let Some(leader) = tab.leader.filter(|leader| *leader != TabLeader::None) {
            push_leader(current, *x, text_start, leader);
        }
        *x = text_start;
    } else if matches!(tab.alignment, TabAlignment::Start) {
        *x = text_start.max(*x);
    }
}

/// Draws the leader in the gap. The marks are graphics, so they are not part
/// of the paragraph's extracted text.
fn push_leader(current: &mut LineBuilder, from: f64, to: f64, leader: TabLeader) {
    let start = from + 4.0;
    let end = to - 4.0;
    if end <= start {
        return;
    }
    match leader {
        TabLeader::Underscore | TabLeader::Heavy => {
            current.graphics.push(Item::Line(crate::layout::LineItem {
                x1: start,
                y1: 1.0,
                x2: end,
                y2: 1.0,
                color: "#000000".to_owned(),
                width: if leader == TabLeader::Heavy {
                    1.5
                } else {
                    0.75
                },
                dashed: false,
            }));
        }
        TabLeader::Hyphen => {
            current.graphics.push(Item::Line(crate::layout::LineItem {
                x1: start,
                y1: -2.0,
                x2: end,
                y2: -2.0,
                color: "#000000".to_owned(),
                width: 0.75,
                dashed: true,
            }));
        }
        TabLeader::Dot | TabLeader::MiddleDot | TabLeader::None => {
            let mut dot = start;
            while dot < end {
                current.graphics.push(Item::Rect(crate::layout::RectItem {
                    x: dot,
                    y: if leader == TabLeader::MiddleDot {
                        -4.0
                    } else {
                        -1.5
                    },
                    w: 1.25,
                    h: 1.25,
                    fill: Some("#000000".to_owned()),
                    stroke: None,
                    stroke_w: 0.0,
                }));
                dot += 6.0;
            }
        }
    }
}
