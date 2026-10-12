//! Paragraph layout: inline flattening, line breaking, alignment and spacing
//! (`STAGE-4-TASK.md` §5.4).

use std::collections::VecDeque;

use strict_ooxml_wml::model::values::{
    BreakKind, FieldCharType, LineSpacingRule, Space, TabAlignment, TabLeader, VertAlign,
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
    /// Line break that clears floating objects on the selected side.
    ClearingBreak(strict_ooxml_wml::model::values::BreakClear),
    /// A hard page break.
    PageBreak,
    /// A hard column break.
    ColumnBreak,
    /// An inline image and its `wp:effectExtent` margins (left, top, right,
    /// bottom px).
    Image(ImageItem, [f64; 4]),
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
    NoteSeparator(bool, ComputedRun),
    /// A computed field result (PAGE/NUMPAGES/SECTIONPAGES/SECTION).
    FieldResult(
        crate::fields::FieldKind,
        Option<crate::notes::NumberFormat>,
        ComputedRun,
    ),
}

/// Section grid applies to layout and intrinsic measurement alike. Direct
/// run snapToGrid overrides are applied later by the run cascade.
fn apply_character_grid(ctx: &LayoutContext<'_>, computed: &mut ComputedParagraph) {
    use strict_ooxml_wml::model::values::DocGridType;
    computed.default_run.font_programs = std::sync::Arc::clone(&ctx.font_programs);
    let section = ctx
        .field_env
        .get()
        .map_or(ctx.current_section.get(), |env| {
            env.section_index.saturating_sub(1)
        });
    let grid = ctx
        .document
        .sections
        .get(section)
        .and_then(|section| section.properties.doc_grid.as_ref())
        .filter(|grid| {
            matches!(
                grid.grid_type,
                Some(DocGridType::LinesAndChars | DocGridType::SnapToChars)
            )
        });
    computed.default_run.grid_extra_pt = grid
        .and_then(|grid| grid.character_space)
        .map_or(0.0, |space| f64::from(space) / 4096.0);
    let mut normal = ComputedParagraph::default();
    if let Some(defaults) = ctx.document.styles.defaults() {
        crate::style::apply_run_props(
            &mut normal.default_run,
            &defaults.run,
            ctx.document.theme.as_ref(),
        );
    }
    if let Some(id) = ctx
        .document
        .styles
        .default_for(strict_ooxml_wml::model::values::StyleType::Paragraph)
    {
        // A paragraph without direct formatting selects the default style.
        let paragraph = Paragraph {
            props: strict_ooxml_wml::model::props::ParagraphProperties::default(),
            inlines: Vec::new(),
            rsids: strict_ooxml_wml::model::values::Rsids::default(),
            revision: None,
            para_id: None,
            text_id: None,
            location: strict_ooxml_core::error::SourceLocation::default(),
        };
        let _ = id;
        normal = compute_paragraph(ctx.document, &paragraph);
    }
    computed.grid_character_pitch_pt =
        grid.map(|_| normal.default_run.size_pt + computed.default_run.grid_extra_pt);
    if !computed.snap_to_grid {
        computed.default_run.snap_to_grid = false;
    }
    let default_run = &computed.default_run;
    if let Some(mark_run) = computed.mark_run.as_deref_mut() {
        mark_run.font_programs = std::sync::Arc::clone(&default_run.font_programs);
        mark_run.grid_extra_pt = default_run.grid_extra_pt;
        if !computed.snap_to_grid {
            mark_run.snap_to_grid = false;
        }
    }
}

/// Intrinsic advances use the same flattening, scripts and objects as layout.
#[allow(clippy::too_many_lines)]
pub(crate) fn intrinsic_widths(ctx: &LayoutContext<'_>, paragraph: &Paragraph) -> (f64, f64) {
    let mut computed = compute_paragraph(ctx.document, paragraph);
    apply_character_grid(ctx, &mut computed);
    let mut segments = Vec::new();
    flatten_inlines(
        ctx,
        &computed,
        &paragraph.inlines,
        0.0,
        100_000.0,
        &mut segments,
        &mut FieldState::default(),
    );
    let mut minimum = 0.0_f64;
    let mut maximum = 0.0_f64;
    let mut word = 0.0_f64;
    let mut line = 0.0_f64;
    for segment in segments {
        match segment {
            Seg::Text(text, run) => {
                line += ctx.measure(&text, &run);
                let mut token = String::new();
                for ch in text.chars() {
                    let cjk = matches!(ch, '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}');
                    let break_space =
                        ch.is_whitespace() && !matches!(ch, '\u{00a0}' | '\u{202f}' | '\u{feff}');
                    if break_space || cjk {
                        word += ctx.measure(&token, &run);
                        minimum = minimum.max(word);
                        word = 0.0;
                        token.clear();
                        if cjk {
                            minimum = minimum.max(ctx.measure(&ch.to_string(), &run));
                        }
                    } else {
                        token.push(ch);
                        if ch == '-' || ch == '\u{200b}' {
                            word += ctx.measure(&token, &run);
                            minimum = minimum.max(word);
                            word = 0.0;
                            token.clear();
                        }
                    }
                }
                word += ctx.measure(&token, &run);
            }
            Seg::Image(image, margins) => {
                let width = image.w + margins[0] + margins[2];
                minimum = minimum.max(word).max(width);
                word = 0.0;
                line += width;
            }
            Seg::Object(_, width, _) => {
                minimum = minimum.max(word).max(width);
                word = 0.0;
                line += width;
            }
            Seg::Math(boxed, _) | Seg::MathParagraph(boxed) => {
                minimum = minimum.max(word).max(boxed.size.width);
                word = 0.0;
                line += boxed.size.width;
            }
            Seg::Break | Seg::ClearingBreak(_) | Seg::PageBreak | Seg::ColumnBreak => {
                minimum = minimum.max(word);
                maximum = maximum.max(line);
                word = 0.0;
                line = 0.0;
            }
            Seg::Tab => {
                minimum = minimum.max(word);
                word = 0.0;
                line += ctx.size_px(computed.default_run.size_pt) * 4.0;
            }
            Seg::FootnoteMarker(id, run) => {
                if let Some((number, format)) = ctx.note_numbers.footnote_number(id) {
                    let width = ctx.measure(&format.format(number), &run);
                    word += width;
                    line += width;
                }
            }
            Seg::EndnoteMarker(id, run) => {
                if let Some((number, format)) = ctx.note_numbers.endnote_number(id) {
                    let width = ctx.measure(&format.format(number), &run);
                    word += width;
                    line += width;
                }
            }
            Seg::FieldResult(kind, format, run) => {
                let marker = crate::fields::FieldMarker { kind, format };
                let text = ctx.field_env.get().map_or_else(
                    || {
                        format
                            .unwrap_or(crate::notes::NumberFormat::Decimal)
                            .format(1)
                    },
                    |environment| environment.format(marker),
                );
                let width = ctx.measure(&text, &run);
                word += width;
                line += width;
            }
            _ => {}
        }
    }
    (minimum.max(word), maximum.max(line))
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
    use strict_ooxml_wml::model::values::TextDirection;
    let computed = compute_paragraph(ctx.document, para);
    let section = ctx
        .field_env
        .get()
        .map_or(ctx.current_section.get(), |env| {
            env.section_index.saturating_sub(1)
        });
    let parent = crate::table_style::current_text_flow()
        .map(|flow| flow.0)
        .or_else(|| {
            ctx.document
                .sections
                .get(section)
                .and_then(|s| s.properties.text_direction)
        })
        .unwrap_or(TextDirection::LrTb);
    let desired = computed.text_direction.unwrap_or(parent);
    let angle = |direction| match direction {
        TextDirection::BtLr => -90.0_f64,
        TextDirection::TbRl | TextDirection::TbRlV | TextDirection::TbLrV => 90.0,
        _ => 0.0,
    };
    let rotation = angle(desired) - angle(parent);
    if rotation == 0.0 {
        let mut flow = layout_paragraph_horizontal(
            ctx,
            para,
            content_left,
            content_width,
            grid_line_pitch,
            note_marker,
            page_exclusions,
            host,
            neighbors,
        );
        if desired == TextDirection::LrTbV && parent != desired {
            for item in &mut flow.flows {
                if let Flow::Line(line) = item {
                    let items = std::mem::take(&mut line.items)
                        .into_iter()
                        .map(Item::Text)
                        .collect();
                    line.graphics
                        .extend(crate::layout::table::rotate_east_asian_items(ctx, items));
                }
            }
        }
        return flow;
    }
    let available = if is_half_turn(rotation) {
        content_width
    } else {
        crate::table_style::current_text_flow()
            .and_then(|flow| flow.1)
            .or_else(|| {
                host.map(|(geometry, _, y)| (geometry.height - geometry.bottom - y).max(1.0))
            })
            .unwrap_or_else(|| ctx.frame_anchor.get().content_height.max(content_width))
            .max(1.0)
    };
    let _direction = crate::table_style::enter_text_flow(desired, Some(available));
    let mut flow = layout_paragraph_horizontal(
        ctx,
        para,
        0.0,
        available,
        grid_line_pitch,
        note_marker,
        &[],
        None,
        neighbors,
    );
    let mut items = Vec::new();
    let mut y = 0.0;
    for item in std::mem::take(&mut flow.flows) {
        match item {
            Flow::Line(mut line) => {
                let height = line.height;
                line.offset(0.0, y);
                items.extend(line.items.into_iter().map(Item::Text));
                items.extend(line.graphics);
                y += height;
            }
            Flow::Block {
                items: block,
                height,
            } => {
                items.extend(block.iter().map(|item| shift_item(item.clone(), 0.0, y)));
                y += height;
            }
            Flow::TableRow(row) => {
                items.extend(
                    row.items
                        .iter()
                        .map(|item| shift_item(item.clone(), 0.0, y)),
                );
                y += row.height;
            }
            Flow::PageBreak | Flow::ColumnBreak => {}
        }
    }
    if matches!(
        desired,
        TextDirection::TbRlV | TextDirection::TbLrV | TextDirection::LrTbV
    ) {
        items = crate::layout::table::rotate_east_asian_items(ctx, items);
    }
    let measured = crate::paint::graphics::transform_items(items, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let bounds = match measured.first() {
        Some(Item::Group(group)) => group.bounds,
        _ => [0.0; 4],
    };
    if is_half_turn(rotation) {
        let items = crate::paint::graphics::transform_items(
            measured,
            [-1.0, 0.0, 0.0, -1.0, content_left + content_width, y],
        );
        flow.flows.push(Flow::Block { items, height: y });
        return flow;
    }
    let height = bounds[2].max(0.0);
    let (sin, cos) = rotation.to_radians().sin_cos();
    let offset_x = if sin > 0.0 { y } else { 0.0 };
    let offset_y = if sin < 0.0 {
        bounds[0] + height
    } else {
        -bounds[0]
    };
    let items = crate::paint::graphics::transform_items(
        measured,
        [cos, sin, -sin, cos, content_left + offset_x, offset_y],
    );
    flow.flows.push(Flow::Block { items, height });
    flow
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn layout_paragraph_horizontal(
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
    apply_character_grid(ctx, &mut computed);
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
        if direct.and_then(|indent| indent.start).is_none()
            && computed.character_indents[0].is_none()
        {
            if let Some(start) = marker.indent_start_pt {
                computed.indent_start_pt = start;
            }
        }
        let direct_line =
            direct.is_some_and(|indent| indent.hanging.is_some() || indent.first_line.is_some());
        if !direct_line && computed.character_indents[2].is_none() {
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

    let segments = apply_east_asian_spacing(segments, &computed, ctx.options.scale);
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
        let box_right = content_left + content_width
            - pt_to_px(computed.indent_end_pt, scale)
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
        && crate::math::layout::use_display_defaults(ctx)
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
    /// First block of the body, a cell, header, footer or text box.
    pub first_in_container: bool,
    /// Last block of a cell, header, footer or text box.
    pub last_in_container: bool,
    /// This paragraph and the previous one are both list items.
    pub prev_list_item: bool,
    /// This paragraph and the next one are both list items.
    pub next_list_item: bool,
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
        // The document body counts too: HTML spacing does not push the
        // first line of the document down either (WPS, CC0/078).
        first_in_container: index == 0,
        last_in_container: in_container && index + 1 == blocks.len(),
        prev_list_item: current.numbering.is_some()
            && prev.as_ref().is_some_and(|prev| prev.numbering.is_some()),
        next_list_item: current.numbering.is_some()
            && next.as_ref().is_some_and(|next| next.numbering.is_some()),
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
    // Nor does it separate the items of a list, as `<li>` has no margins
    // (WPS, CC0/098).
    if computed.before_autospacing && (neighbors.first_in_container || neighbors.prev_list_item) {
        computed.space_before_pt = 0.0;
    }
    if computed.after_autospacing && (neighbors.last_in_container || neighbors.next_list_item) {
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
#[allow(clippy::too_many_lines)]
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
                let first = out.len();
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
                for segment in out.iter_mut().skip(first) {
                    let run = match segment {
                        Seg::Text(_, run)
                        | Seg::Math(_, run)
                        | Seg::FootnoteMarker(_, run)
                        | Seg::EndnoteMarker(_, run)
                        | Seg::NoteNumber(run)
                        | Seg::NoteSeparator(_, run)
                        | Seg::FieldResult(_, _, run) => Some(run),
                        _ => None,
                    };
                    if let Some(run) = run {
                        if run.bidi_embeddings.is_empty()
                            && dir.kind == strict_ooxml_wml::model::inline::DirectionalKind::Bdo
                        {
                            run.bidi_override = true;
                            run.rtl = Some(
                                dir.val == strict_ooxml_wml::model::inline::DirectionalVal::Rtl,
                            );
                        } else if run.bidi_embeddings.is_empty() && run.rtl.is_none() {
                            run.rtl = Some(
                                dir.val == strict_ooxml_wml::model::inline::DirectionalVal::Rtl,
                            );
                        }
                        run.bidi_embeddings.insert(
                            0,
                            (
                                std::ptr::from_ref(dir) as usize,
                                dir.val == strict_ooxml_wml::model::inline::DirectionalVal::Rtl,
                                dir.kind == strict_ooxml_wml::model::inline::DirectionalKind::Bdo,
                            ),
                        );
                    }
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
                    computed,
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

/// Keep additional inter-script spacing separate from document text.
fn apply_east_asian_spacing(
    segments: Vec<Seg>,
    computed: &ComputedParagraph,
    scale: f64,
) -> Vec<Seg> {
    if !computed.auto_space_de && !computed.auto_space_dn {
        return segments;
    }
    let cjk = |ch: char| matches!(ch, '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{ff66}'..='\u{ff9f}');
    let mut previous = None;
    let mut out = Vec::new();
    for segment in segments {
        if let Seg::Text(text, run) = segment {
            let mut piece = String::new();
            for ch in text.chars() {
                let gap = previous.is_some_and(|prev| {
                    if cjk(prev) == cjk(ch) {
                        return false;
                    }
                    let western = if cjk(ch) { prev } else { ch };
                    (computed.auto_space_de && western.is_alphabetic())
                        || (computed.auto_space_dn && western.is_ascii_digit())
                });
                if gap {
                    if !piece.is_empty() {
                        out.push(Seg::Text(std::mem::take(&mut piece), run.clone()));
                    }
                    out.push(Seg::Object(
                        Vec::new(),
                        pt_to_px(run.size_pt * 0.25, scale),
                        0.0,
                    ));
                }
                piece.push(ch);
                previous = Some(ch);
            }
            if !piece.is_empty() {
                out.push(Seg::Text(piece, run));
            }
        } else {
            previous = None;
            out.push(segment);
        }
    }
    out
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
        Inline::Directional(dir) => Inline::Directional(dir.clone()),
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
                // Keep the source text in the model, but honor the Word text
                // node's whitespace policy when measuring and painting it.
                let visible = if text.space == Space::Preserve {
                    text.text.as_str()
                } else {
                    text.text.trim_matches([' ', '\t', '\r', '\n'])
                };
                if visible.is_empty() {
                    continue;
                }
                if ctx.document.settings.compat_flags.do_not_leave_backslash_alone && visible.contains('\\') {
                    push_script_segments(&visible.replace('\\', "¥"), &run_style, out);
                } else {
                    push_script_segments(visible, &run_style, out);
                }
            }
            RunContent::Tab => out.push(Seg::Tab),
            RunContent::Break(BreakKind::Page) => out.push(Seg::PageBreak),
            RunContent::Break(BreakKind::Column) => out.push(Seg::ColumnBreak),
            RunContent::ClearingBreak(clear) => out.push(Seg::ClearingBreak(*clear)),
            RunContent::Break(BreakKind::TextWrapping)
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
            RunContent::NoteSeparator(continuation) => out.push(Seg::NoteSeparator(*continuation, run_style.clone())),
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
fn push_script_segments(text: &str, run: &ComputedRun, out: &mut Vec<Seg>) {
    let emit = |piece: String, channel: u8, out: &mut Vec<Seg>| {
        let mut style = run.clone();
        if channel == 1 || style.force_complex || style.rtl == Some(true) {
            style.bold = style.complex_bold;
            style.italic = style.complex_italic;
            style.size_pt = style.complex_size_pt;
        } else {
            style.bold = style.latin_bold;
            style.italic = style.latin_italic;
            style.size_pt = style.latin_size_pt;
        }
        if style.small_caps && !style.caps {
            push_small_caps(&piece, &style, out);
        } else {
            out.push(Seg::Text(apply_caps(&piece, &style), style));
        }
    };
    let mut channel = 0u8;
    let mut piece = String::new();
    for ch in text.chars() {
        let next = if crate::style::is_complex_script(ch) {
            1
        } else if matches!(ch, '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}')
        {
            2
        } else if ch.is_alphabetic() || ch.is_ascii_digit() {
            0
        } else {
            channel
        };
        if next != channel && !piece.is_empty() {
            emit(std::mem::take(&mut piece), channel, out);
        }
        channel = next;
        piece.push(ch);
    }
    if !piece.is_empty() {
        emit(piece, channel, out);
    }
}

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
                let margins = crate::paint::image::inline_effect_margins(ctx, drawing);
                out.push(Seg::Image(image, margins));
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
    mut segments: Vec<Seg>,
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
    let mut right_edge = content_left + content_width - indent_end;
    if computed.adjust_right_ind && computed.snap_to_grid {
        if let Some(pitch) = computed
            .grid_character_pitch_pt
            .map(|pt| pt_to_px(pt, scale))
        {
            if pitch.is_finite() && pitch > 0.0 {
                right_edge = normal_x + ((right_edge - normal_x) / pitch).floor().max(1.0) * pitch;
            }
        }
    }

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
        let mut run = marker.run.clone();
        run.font_programs = std::sync::Arc::clone(&ctx.font_programs);
        run.chapter_heading = computed
            .chapter_level
            .filter(|level| *level < 9)
            .zip(marker.number)
            .map(|(level, (value, format))| (level, value, format));
        (
            marker.text.clone(),
            run,
            marker.suffix.clone(),
            marker.pic_bullet,
            marker.justification,
        )
    });

    let mut exclusions = page_exclusions.to_vec();
    if let Some((geometry, host_x, host_y)) = host {
        let mut occupied: Vec<_> = page_exclusions
            .iter()
            .map(|e| {
                (
                    e.left + host_x - content_left,
                    e.right + host_x - content_left,
                    e.top + host_y,
                    e.bottom + host_y,
                )
            })
            .collect();
        for segment in &mut segments {
            let Seg::Anchor(anchor) = segment else {
                continue;
            };
            if !crate::layout::table::cell_anchor_wrap_enabled(anchor) {
                continue;
            }
            if anchor.hidden == Some(true)
                || anchor
                    .doc_pr
                    .as_ref()
                    .is_some_and(|p| p.hidden == Some(true))
            {
                continue;
            }
            let Some((w, h)) = crate::layout::floating::resolved_extent(ctx, anchor, geometry)
            else {
                continue;
            };
            let (x, original_y) = crate::layout::floating::anchor_position(
                anchor, geometry, host_x, host_y, w, h, scale,
            );
            let mut y = original_y;
            if !anchor.allow_overlap {
                for _ in 0..=occupied.len() {
                    let next = occupied
                        .iter()
                        .filter(|(l, r, t, b)| x < *r && x + w > *l && y < *b && y + h > *t)
                        .map(|(_, _, _, bottom)| *bottom)
                        .fold(y, f64::max);
                    if next <= y + 0.001 {
                        break;
                    }
                    y = next;
                }
                if (y - original_y).abs() > 1e-9 {
                    anchor.simple_pos = true;
                    anchor.simple_pos_point = Some((
                        (x / scale * 9525.0).round() as i64,
                        (y / scale * 9525.0).round() as i64,
                    ));
                }
            }
            occupied.push((x, x + w, y, y + h));
        }
    }
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
                sink.emit(
                    std::mem::replace(&mut current, LineBuilder::new()),
                    ctx.document
                        .settings
                        .compat_flags
                        .do_not_expand_shift_return,
                );
                x = normal_x;
            }
            Seg::ClearingBreak(clear) => {
                sink.emit(
                    std::mem::replace(&mut current, LineBuilder::new()),
                    ctx.document
                        .settings
                        .compat_flags
                        .do_not_expand_shift_return,
                );
                let center = content_left + content_width / 2.0;
                let bottom = sink
                    .exclusions
                    .iter()
                    .filter(|exclusion| {
                        exclusion.bottom > sink.line_y
                            && match clear {
                                strict_ooxml_wml::model::values::BreakClear::All => true,
                                strict_ooxml_wml::model::values::BreakClear::Left => {
                                    exclusion.left < center
                                }
                                strict_ooxml_wml::model::values::BreakClear::Right => {
                                    exclusion.right > center
                                }
                            }
                    })
                    .map(|exclusion| exclusion.bottom)
                    .fold(sink.line_y, f64::max);
                if bottom > sink.line_y {
                    sink.flows.push(Flow::Block {
                        items: Vec::new(),
                        height: bottom - sink.line_y,
                    });
                    sink.line_y = bottom;
                }
                x = normal_x;
            }
            Seg::Tab => {
                if let Some(stop) = place_tab(
                    ctx,
                    computed,
                    &mut current,
                    &mut x,
                    &pending,
                    default_tab,
                    scale,
                    content_left,
                ) {
                    // A right, centre or decimal stop past the right indent
                    // is kept, as Word 2013+ does: the text it aligns may run
                    // into the margin instead of wrapping.
                    sink.right_edge = sink.right_edge.max(stop);
                }
            }
            Seg::Image(image, [left, top, right, bottom]) => {
                // A `wp:inline` picture is a character of its line: it sits on
                // the baseline next to text and other pictures, wraps like a
                // word and follows the paragraph's alignment (ISO/IEC 29500-1
                // §20.4.2.8). The line takes the picture's height.
                let mut image = image;
                let (width, height) = (image.w + left + right, image.h + top + bottom);
                if !computed.no_wrap && x + width > sink.right_edge + 1e-9 && !current.is_empty() {
                    sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                    x = normal_x;
                }
                image.x = x + left;
                image.y = top - height;
                current.graphics.push(Item::Image(image));
                current.object_height = current.object_height.max(height);
                x += width;
            }
            Seg::Object(items, width, height) => {
                // An inline drawing sits in the text line with its bottom on the
                // baseline, as Word places `wp:inline` (STAGE-5C-REWORK-1 D3).
                let line_end = sink.right_edge;
                if !computed.no_wrap && x + width > line_end + 1e-9 && !current.is_empty() {
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
                let properties = ctx
                    .document
                    .settings
                    .math_properties
                    .as_ref()
                    .filter(|_| crate::math::layout::use_display_defaults(ctx));
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
                let mut chunks = vec![(text, run)];
                while matches!(pending.front(), Some(Seg::Text(_, _))) {
                    if let Some(Seg::Text(text, run)) = pending.pop_front() {
                        chunks.push((text, run));
                    }
                }
                let joined: String = chunks.iter().map(|(text, _)| text.as_str()).collect();
                let tokens = tokenize(
                    &joined,
                    ctx.document.settings.compat_flags.apply_breaking_rules,
                );
                let mut chunk_index = 0;
                let mut chunk_offset = 0;
                for (index, token) in tokens.iter().enumerate() {
                    let following = tokens
                        .get(index + 1)
                        .and_then(|next| next.chars().find(|ch| !ch.is_whitespace()));
                    let mut remaining = token.len();
                    let mut pieces = Vec::new();
                    while remaining > 0 {
                        let Some((text, run)) = chunks.get(chunk_index) else {
                            break;
                        };
                        let count = remaining.min(text.len() - chunk_offset);
                        if let Some(piece) = text
                            .get(chunk_offset..chunk_offset + count)
                            .filter(|_| count > 0)
                        {
                            pieces.push((piece.to_owned(), run.clone()));
                        }
                        chunk_offset += count;
                        remaining -= count;
                        if chunk_offset == text.len() {
                            chunk_index += 1;
                            chunk_offset = 0;
                        }
                    }
                    if let [(text, run)] = pieces.as_slice() {
                        place_token(
                            &mut sink,
                            &mut current,
                            &mut x,
                            normal_x,
                            text,
                            run,
                            following,
                        );
                    } else if !pieces.is_empty() {
                        place_formatted_word(
                            &mut sink,
                            &mut current,
                            &mut x,
                            normal_x,
                            pieces,
                            following,
                        );
                    }
                }
            }
            Seg::FootnoteMarker(id, run) => {
                if let Some((number, format)) = ctx.note_numbers.footnote_number(id) {
                    let mut run = run;
                    run.footnote_id = Some(id);
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
            Seg::NoteSeparator(continuation, run) => {
                let width = if continuation {
                    content_width
                } else {
                    (content_width / 3.0).max(8.0)
                };
                current.graphics.push(Item::Line(crate::layout::LineItem {
                    x1: x,
                    y1: -ctx.size_px(run.size_pt) * 0.3,
                    x2: x + width,
                    y2: -ctx.size_px(run.size_pt) * 0.3,
                    color: run.color.unwrap_or_else(|| "#000000".to_owned()),
                    // WPS strokes the separator at 0.75 pt, one pixel at 96
                    // dpi, as the built-in separator line is.
                    width: pt_to_px(0.75, scale),
                    dashed: false,
                }));
                x += width;
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
fn tokenize(text: &str, legacy_ethiopic: bool) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let ethiopic_separator = ('\u{1361}'..='\u{1368}').contains(&ch);
        if legacy_ethiopic && ethiopic_separator && index > start {
            tokens.push(text[start..index].to_owned());
            start = index;
        }
        if ch == ' ' || (ethiopic_separator && !legacy_ethiopic) {
            let end = index + ch.len_utf8();
            tokens.push(text[start..end].to_owned());
            start = end;
        }
    }
    if start < text.len() {
        tokens.push(text[start..].to_owned());
    }
    tokens
}

/// Shifts a paint item vertically (used to place a display formula's ink).
fn shift_item(item: Item, dx: f64, dy: f64) -> Item {
    match item {
        Item::Group(mut group) => {
            group.offset(dx, dy);
            Item::Group(group)
        }
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
    if !sink.computed.no_wrap && *x + width > line_end + 1e-9 && !current.is_empty() {
        sink.emit(std::mem::replace(current, LineBuilder::new()), false);
        *x = normal_x;
    }
    current.inline_fraction |= boxed.inline_fraction;
    current
        .graphics
        .extend(crate::paint::graphics::transform_items(
            boxed.items,
            [1.0, 0.0, 0.0, 1.0, *x, 0.0],
        ));
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
            // A translated group (an inline formula placed on the line) is
            // measured by its own ink: its `bounds` are the loose font boxes of
            // its runs, which made every formula line ~5 px taller than WPS.
            Item::Group(group)
                if is_translation(group.matrix)
                    && group.clip.is_none()
                    && group.clip_path.is_none() =>
            {
                let (above, below) = formula_ink(ctx, &group.items);
                (group.matrix[5] - above, group.matrix[5] + below)
            }
            Item::Group(group) => (group.bounds[1], group.bounds[1] + group.bounds[3]),
            Item::Text(text) => {
                let family = crate::style::chosen_family(&text.run, &text.text);
                let metrics = crate::font::embedded::run_metrics(&text.run, &family, ctx.font);
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
    /// The end of the free interval that holds all of `line`, when wrap
    /// exclusions narrow it.
    fn wrapped_right_edge(&self, line: &LineBuilder) -> Option<f64> {
        if self.exclusions.is_empty() || line.items.is_empty() {
            return None;
        }
        let left = line
            .items
            .iter()
            .map(|item| item.x)
            .fold(f64::INFINITY, f64::min);
        let right = line
            .items
            .iter()
            .map(|item| item.x + item.width)
            .fold(f64::NEG_INFINITY, f64::max);
        let spans = free_spans(self, f64::NEG_INFINITY, provisional_line_height(self));
        spans
            .into_iter()
            .find(|(start, end)| *start <= left + 0.5 && right <= *end + 0.5)
            .map(|(_, end)| end)
            .filter(|end| *end < self.right_edge - 0.5)
    }

    fn emit(&mut self, line: LineBuilder, last: bool) {
        // Beside a wrapped object the line ends where its free interval ends:
        // justification and alignment work up to the object, not under it
        // (WQ-012). A line whose text crosses the object keeps the column edge.
        let right_edge = self.wrapped_right_edge(&line).unwrap_or(self.right_edge);
        let mut finished = finish_line(
            self.ctx,
            self.computed,
            line,
            right_edge,
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
                if !crate::layout::table::cell_anchor_wrap_enabled(anchor) {
                    return None;
                }
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
            if !crate::layout::table::cell_anchor_wrap_enabled(anchor) {
                return None;
            }
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
/// The longest prefix of `token` ending in a hyphen between two letters that
/// fits in `available` px, as `(cut, prefix)`. Before a digit a hyphen is a
/// minus sign and offers no break.
fn hard_hyphen_prefix(
    sink: &LineSink<'_, '_>,
    token: &str,
    run: &ComputedRun,
    available: f64,
) -> Option<(usize, String)> {
    let mut best = None;
    let mut previous: Option<char> = None;
    let mut chars = token.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let breaks = matches!(ch, '-' | '\u{2010}')
            && previous.is_some_and(char::is_alphabetic)
            && chars.peek().is_some_and(|(_, next)| next.is_alphabetic());
        if breaks {
            let cut = index + ch.len_utf8();
            let prefix = token.get(..cut)?;
            if sink.ctx.measure(prefix, run) > available + 1e-9 {
                break;
            }
            best = Some((cut, prefix.to_owned()));
        }
        previous = Some(ch);
    }
    best
}

fn place_token(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    normal_x: f64,
    token: &str,
    run: &ComputedRun,
    following: Option<char>,
) {
    let mut token = token;
    // Hyphenation runs before moving a whole word to the following line.
    // Use the free interval containing the pen so floating objects constrain
    // the candidate just as they constrain an ordinary text token.
    if !sink.computed.no_wrap {
        loop {
            let spans = free_spans(sink, normal_x, provisional_line_height(sink));
            let Some((_, line_end)) = spans
                .iter()
                .copied()
                .find(|(start, end)| *start <= *x + 1e-9 && *x < *end)
            else {
                break;
            };
            if *x + sink.ctx.measure(token, run) <= line_end + 1e-9 {
                break;
            }
            // A hyphen already in the word is a break opportunity after it
            // (UAX #14 class HY): "cross- / sections" (WQ-021).
            let Some((cut, displayed)) = hard_hyphen_prefix(sink, token, run, line_end - *x)
                .or_else(|| hyphenated_prefix(sink, current, token, run, line_end - *x))
            else {
                break;
            };
            let width = sink.ctx.measure(&displayed, run);
            push_piece(
                sink,
                current,
                &displayed,
                *x,
                width,
                run,
                sink.ctx.size_px(run.size_pt),
                false,
                1.0,
            );
            sink.emit(std::mem::replace(current, LineBuilder::new()), false);
            *x = normal_x;
            token = &token[cut..];
        }
    }
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
    if sink.computed.no_wrap {
        // A cell's noWrap retains authored hard breaks and lets the whole
        // word advance beyond the cell when its fixed width is insufficient.
    } else if sink.exclusions.is_empty() {
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

/// A word can span multiple runs; line breaking uses its complete width and
/// language while painting each substring with its authored formatting.
#[allow(clippy::too_many_lines)]
fn place_formatted_word(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    normal_x: f64,
    mut pieces: Vec<(String, ComputedRun)>,
    following: Option<char>,
) {
    loop {
        let word: String = pieces.iter().map(|(text, _)| text.as_str()).collect();
        let Some(first_run) = pieces.first().map(|(_, run)| run.clone()) else {
            return;
        };
        if word.is_empty() {
            return;
        }
        let total: f64 = pieces
            .iter()
            .map(|(text, run)| sink.ctx.measure(text, run))
            .sum();
        let end = free_spans(sink, normal_x, provisional_line_height(sink))
            .into_iter()
            .find(|(left, right)| *left <= *x + 0.001 && *x < *right)
            .map_or(sink.right_edge, |(_, right)| right);
        let mut cut = word.len();
        let mut hyphen = false;
        if !sink.computed.no_wrap && *x + total > end + 0.001 {
            let settings = &sink.ctx.document.settings;
            let zone = settings.hyphenation_zone.map_or(0.0, |v| {
                twips_to_px(v.value().max(0), sink.ctx.options.scale)
            });
            if settings.auto_hyphenation
                && !settings.auto_hyphenation_off
                && !sink.computed.suppress_auto_hyphens
                && word.len() <= 4096
                && (*x <= normal_x + 0.001 || end - *x > zone)
                && !(settings.do_not_hyphenate_caps
                    && word
                        .chars()
                        .all(|ch| !ch.is_alphabetic() || ch.is_uppercase()))
            {
                let language = &first_run.language;
                let tag = if word.chars().any(crate::style::is_complex_script) {
                    language.bidi.as_deref().or(language.val.as_deref())
                } else {
                    language.val.as_deref()
                }
                .unwrap_or("en")
                .split(['-', '_'])
                .next()
                .unwrap_or("en");
                let code = tag.as_bytes();
                if let [a, b] = code {
                    if let Some(language) =
                        hypher::Lang::from_iso([a.to_ascii_lowercase(), b.to_ascii_lowercase()])
                    {
                        let mut candidate = 0;
                        let mut best = None;
                        let word_end = word
                            .char_indices()
                            .find(|(_, ch)| !ch.is_alphabetic())
                            .map_or(word.len(), |(index, _)| index);
                        for syllable in hypher::hyphenate(&word[..word_end], language) {
                            candidate += syllable.len();
                            if candidate >= word_end {
                                break;
                            }
                            let mut remaining = candidate;
                            let mut width = 0.0;
                            for (text, run) in &pieces {
                                let count = remaining.min(text.len());
                                width += sink.ctx.measure(&text[..count], run);
                                remaining -= count;
                                if remaining == 0 {
                                    width += sink.ctx.measure("-", run);
                                    break;
                                }
                            }
                            if *x + width <= end + 0.001 {
                                best = Some(candidate);
                            }
                        }
                        if let Some(best) = best {
                            cut = best;
                            hyphen = true;
                        }
                    }
                }
            }
            if !hyphen && !current.is_empty() {
                sink.emit(std::mem::replace(current, LineBuilder::new()), false);
                *x = normal_x;
                continue;
            }
        }
        let mut remaining = cut;
        let mut tail = Vec::new();
        let mut last_run = first_run;
        for (text, run) in pieces {
            let count = remaining.min(text.len());
            remaining -= count;
            if count > 0 {
                let part = &text[..count];
                let width = sink.ctx.measure(part, &run);
                last_run = run.clone();
                if !hyphen
                    && !sink.computed.no_wrap
                    && (sink.computed.word_wrap
                        || word.chars().any(crate::font::metrics::is_wide_character))
                    && *x + width > end
                {
                    if !current.is_empty() {
                        sink.emit(std::mem::replace(current, LineBuilder::new()), false);
                        *x = normal_x;
                    }
                    place_long_token(sink, current, x, end, normal_x, part, &run);
                } else {
                    push_piece(
                        sink,
                        current,
                        part,
                        *x,
                        width,
                        &run,
                        sink.ctx.size_px(run.size_pt),
                        false,
                        1.0,
                    );
                    *x += width;
                }
            }
            if count < text.len() {
                tail.push((text[count..].to_owned(), run));
            }
        }
        if !hyphen {
            if let Some(last) = current.items.last_mut() {
                last.following_non_space = following;
            }
            return;
        }
        let width = sink.ctx.measure("-", &last_run);
        push_piece(
            sink,
            current,
            "-",
            *x,
            width,
            &last_run,
            sink.ctx.size_px(last_run.size_pt),
            false,
            1.0,
        );
        sink.emit(std::mem::replace(current, LineBuilder::new()), false);
        *x = normal_x;
        pieces = tail;
    }
}

/// Longest language-approved prefix fitting the current line, including its
/// visible hyphen. The model and the DOCX writer retain the original word.
fn hyphenated_prefix(
    sink: &LineSink<'_, '_>,
    current: &LineBuilder,
    token: &str,
    run: &ComputedRun,
    available: f64,
) -> Option<(usize, String)> {
    let settings = &sink.ctx.document.settings;
    if !settings.auto_hyphenation
        || settings.auto_hyphenation_off
        || sink.computed.suppress_auto_hyphens
        || available <= 0.0
        || token.len() > 4096
    {
        return None;
    }
    let zone = settings.hyphenation_zone.map_or(0.0, |zone| {
        twips_to_px(zone.value().max(0), sink.ctx.options.scale)
    });
    if !current.is_empty() && available <= zone {
        return None;
    }
    let end = token
        .char_indices()
        .find(|(_, ch)| !ch.is_alphabetic())
        .map_or(token.len(), |(index, _)| index);
    let word = &token[..end];
    if word.chars().count() < 4
        || (settings.do_not_hyphenate_caps && word.chars().all(char::is_uppercase))
    {
        return None;
    }
    let tag = if word.chars().any(crate::style::is_complex_script) {
        run.language.bidi.as_deref().or(run.language.val.as_deref())
    } else {
        run.language.val.as_deref()
    }
    .unwrap_or("en");
    let &[first, second] = tag.split(['-', '_']).next()?.as_bytes() else {
        return None;
    };
    let language =
        hypher::Lang::from_iso([first.to_ascii_lowercase(), second.to_ascii_lowercase()])?;
    let hyphen = if language.hyphenation_character().is_some() {
        "-"
    } else {
        ""
    };
    let mut cut = 0;
    let mut best = None;
    for syllable in hypher::hyphenate(word, language) {
        cut += syllable.len();
        if cut >= word.len() {
            break;
        }
        let displayed = format!("{}{hyphen}", &word[..cut]);
        if sink.ctx.measure(&displayed, run) <= available + 1e-9 {
            best = Some((cut, displayed));
        }
    }
    best
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
    let cjk = token.chars().any(|ch| {
        matches!(u32::from(ch),
        0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff)
    });
    if !sink.computed.word_wrap && !cjk {
        let width = sink.ctx.measure(token, run);
        push_piece(
            sink,
            current,
            token,
            *x,
            width,
            run,
            size_px,
            compress,
            plain_space_factor,
        );
        *x += width;
        return;
    }
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
            Item::Group(_) => "group",
            Item::Image(_) => "image",
        };
        ctx.warn(format!(
            "render.object-overflow: {kind}, right={:.3}, limit={right_edge:.3}",
            item_right(item)
        ));
    }
}

/// One bidi-reorderable object of a line: text fragments and inline graphics
/// that overlap horizontally move together.
#[derive(Clone)]
struct Unit {
    left: f64,
    right: f64,
    texts: Vec<usize>,
    graphics: Vec<usize>,
}

/// Order text fragments by Unicode embedding levels while preserving glyph
/// order inside each already-shaped fragment.
#[allow(clippy::too_many_lines)]
fn reorder_bidi_line(ctx: &LayoutContext<'_>, line: &mut LineBuilder, rtl: bool) {
    if !rtl
        && !line
            .items
            .iter()
            .any(|item| item.run.rtl.is_some() || crate::font::needs_complex_script(&item.text))
    {
        return;
    }
    let base = if rtl {
        unicode_bidi::Level::rtl()
    } else {
        unicode_bidi::Level::ltr()
    };
    let mut logical = String::new();
    let mut starts = Vec::new();
    let mut active_scopes = Vec::new();
    for item in &line.items {
        let shared = active_scopes
            .iter()
            .zip(&item.run.bidi_embeddings)
            .take_while(|(a, b)| a == b)
            .count();
        for _ in shared..active_scopes.len() {
            logical.push('\u{202c}');
        }
        active_scopes.truncate(shared);
        for scope in item.run.bidi_embeddings.iter().skip(shared) {
            logical.push(match (scope.1, scope.2) {
                (true, true) => '\u{202e}',
                (false, true) => '\u{202d}',
                (true, false) => '\u{202b}',
                (false, false) => '\u{202a}',
            });
            active_scopes.push(*scope);
        }
        if let Some(direction) = item.run.rtl {
            logical.push(match (direction, item.run.bidi_override) {
                (true, true) => '\u{202e}',
                (false, true) => '\u{202d}',
                (true, false) => '\u{202b}',
                (false, false) => '\u{202a}',
            });
        }
        starts.push(logical.len());
        logical.push_str(&item.text);
        if item.run.rtl.is_some() {
            logical.push('\u{202c}');
        }
    }
    for _ in &active_scopes {
        logical.push('\u{202c}');
    }
    let bidi = unicode_bidi::BidiInfo::new(&logical, Some(base));
    let levels = bidi.paragraphs.first().map_or_else(
        || bidi.levels.clone(),
        |paragraph| bidi.reordered_levels(paragraph, 0..logical.len()),
    );
    let mut fragments = Vec::new();
    let mut fragment_levels = Vec::new();
    for (item, start) in line.items.iter().zip(starts) {
        let mut begin = 0;
        let mut level = levels.get(start).copied().unwrap_or(base);
        let mut x = item.x;
        for (byte, _) in item
            .text
            .char_indices()
            .chain(std::iter::once((item.text.len(), '\0')))
        {
            let next = levels.get(start + byte).copied().unwrap_or(level);
            if byte == item.text.len() || next != level {
                if byte > begin {
                    let mut fragment = item.clone();
                    item.text[begin..byte].clone_into(&mut fragment.text);
                    fragment.x = x;
                    fragment.run.rtl = Some(level.is_rtl());
                    fragment.width = ctx.measure(&fragment.text, &fragment.run);
                    fragment.advance = ctx.advance_kind_for(&fragment.text, &fragment.run);
                    x += fragment.width;
                    fragments.push(fragment);
                    fragment_levels.push(level);
                }
                begin = byte;
                level = next;
            }
        }
    }
    line.items = fragments;
    let mut units: Vec<_> = line
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| Unit {
            left: item.x,
            right: item.x + item.width,
            texts: vec![index],
            graphics: Vec::new(),
        })
        .collect();
    for (index, item) in line.graphics.iter().enumerate() {
        // Background fills and frame edges span the whole line, so they follow
        // alignment rather than acting as inline bidi objects.
        if matches!(item, Item::Rect(_) | Item::Line(_)) {
            continue;
        }
        let (left, right) = match item {
            Item::Image(image) => (image.x, image.x + image.w),
            Item::Path(path) => (path.x, path.x + path.w),
            Item::Group(group) => (group.bounds[0], group.bounds[0] + group.bounds[2]),
            _ => continue,
        };
        units.push(Unit {
            left,
            right,
            texts: Vec::new(),
            graphics: vec![index],
        });
    }
    units.sort_by(|a, b| a.left.total_cmp(&b.left));
    let mut groups: Vec<Unit> = Vec::new();
    for unit in units {
        if let Some(group) = groups
            .last_mut()
            .filter(|group| unit.left < group.right - 0.01)
        {
            group.right = group.right.max(unit.right);
            group.texts.extend(unit.texts);
            group.graphics.extend(unit.graphics);
        } else {
            groups.push(unit);
        }
    }
    // Retain the resolved embedding depth, including nested dir/bdo scopes.
    // Neutral objects inherit the surrounding resolved level.
    let mut prior_level = base;
    let levels: Vec<_> = groups
        .iter()
        .map(|group| {
            if let Some(level) = group
                .texts
                .first()
                .and_then(|index| fragment_levels.get(*index))
            {
                prior_level = *level;
            }
            prior_level
        })
        .collect();
    let order = unicode_bidi::BidiInfo::reorder_visual(&levels);
    let mut gaps: Vec<_> = groups
        .windows(2)
        .map(|pair| match pair {
            [left, right] => (right.left - left.right).max(0.0),
            _ => 0.0,
        })
        .collect();
    gaps.push(0.0);
    let mut x = groups.first().map_or(0.0, |group| group.left);
    for index in order {
        let Some(group) = groups.get(index) else {
            continue;
        };
        let dx = x - group.left;
        for index in &group.texts {
            if let Some(item) = line.items.get_mut(*index) {
                item.x += dx;
            }
        }
        for index in &group.graphics {
            if let Some(item) = line.graphics.get_mut(*index) {
                *item = crate::layout::table::offset_item(item, dx, 0.0);
            }
        }
        x += group.right - group.left + gaps.get(index).copied().unwrap_or(0.0);
    }
}

/// Finalizes a line: baseline, justification and alignment.
#[allow(clippy::too_many_lines)]
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
    reorder_bidi_line(ctx, &mut line, computed.bidi);
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
    if computed.line_rule != LineSpacingRule::Exact {
        let extra_above = line
            .items
            .iter()
            .filter(|item| {
                item.run
                    .emphasis
                    .as_deref()
                    .is_some_and(|mark| mark != "underDot")
            })
            .map(|item| item.size_px * 0.25)
            .fold(0.0, f64::max);
        let extra_below = line
            .items
            .iter()
            .filter(|item| item.run.emphasis.as_deref() == Some("underDot"))
            .map(|item| item.size_px * 0.25)
            .fold(0.0, f64::max);
        ascent += extra_above;
        height += extra_above + extra_below;
    }
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
    for item in line.items.iter_mut().rev() {
        item.run.underline_trailing_space =
            ctx.document.settings.compat_flags.underline_trailing_space;
        if item.text.chars().any(|ch| !ch.is_whitespace()) {
            break;
        }
    }
    if computed.line_rule != LineSpacingRule::Exact
        && ctx.document.settings.compat_flags.space_for_underline
    {
        let underline_depth = line
            .items
            .iter()
            .filter(|item| item.run.underline.is_some())
            .map(|item| item.size_px * 0.2)
            .fold(0.0, f64::max);
        height = height.max(ascent + underline_depth);
    }
    for item in &mut line.items {
        item.baseline = ascent;
        apply_vertical_align(item);
    }
    // The graphics were placed relative to the baseline, which is only now
    // known.
    for item in &mut line.graphics {
        *item = shift_item(item.clone(), 0.0, ascent);
    }
    super::decoration::merge_run_borders(&mut line.items, &mut line.graphics);
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
    let text = ctx.field_env.get().map_or_else(
        || {
            marker
                .format
                .unwrap_or(crate::notes::NumberFormat::Decimal)
                .format(1)
        },
        |env| env.format(marker),
    );
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
        Item::Group(group) => group.bounds[0],
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
        Item::Group(group) => group.bounds[0] + group.bounds[2],
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
    let run = item.map_or(computed.mark_run(), |item| &item.run);
    let grid_line_pitch = grid_line_pitch.filter(|_| computed.snap_to_grid && run.snap_to_grid);
    let family = crate::style::chosen_family(run, item.map_or("", |text| text.text.as_str()));
    let metrics = crate::font::embedded::run_metrics(run, &family, ctx.font);
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
            // When the document declares a line grid, a single line occupies
            // the smallest whole number of grid units that fits its natural
            // height, as Word/WPS do even with single spacing (`w:line="240"`).
            // The multiplier then scales that snapped height: 1.5 lines on a
            // 15.6 pt grid is 23.4 pt, not two units (WQ-024). The extra
            // leading goes above the baseline so the block keeps its vertical
            // position within the grid.
            let (height, extra) = match grid_line_pitch {
                Some(pitch) if pitch > 0.0 => {
                    let single = (natural_height / pitch).ceil().max(1.0) * pitch;
                    let height = single * multiplier.max(0.1);
                    (height, (height - scaled).max(0.0))
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
                let text = ctx.field_env.get().map_or_else(
                    || {
                        format
                            .unwrap_or(crate::notes::NumberFormat::Decimal)
                            .format(1)
                    },
                    |env| env.format(marker),
                );
                width += ctx.measure(&text, run);
            }
            Seg::Tab
            | Seg::Break
            | Seg::ClearingBreak(_)
            | Seg::PageBreak
            | Seg::ColumnBreak
            | Seg::Image(..)
            | Seg::MathParagraph(_) => break,
            Seg::Anchor(_)
            | Seg::FootnoteMarker(_, _)
            | Seg::EndnoteMarker(_, _)
            | Seg::NoteNumber(_)
            | Seg::NoteSeparator(_, _) => {}
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
            Seg::Tab | Seg::Break | Seg::ClearingBreak(_) | Seg::PageBreak | Seg::ColumnBreak => {
                break
            }
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
) -> Option<f64> {
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
        return None;
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
    (!matches!(tab.alignment, TabAlignment::Start | TabAlignment::Clear)).then_some(tab.x)
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

/// Whether an affine matrix only translates.
fn is_translation(matrix: [f64; 6]) -> bool {
    let [a, b, c, d, _, _] = matrix;
    (a - 1.0).abs() < 1e-9 && b.abs() < 1e-9 && c.abs() < 1e-9 && (d - 1.0).abs() < 1e-9
}

/// Whether a rotation is a half turn, which keeps the block's width and height.
pub(crate) fn is_half_turn(rotation: f64) -> bool {
    (rotation.abs() - 180.0).abs() < 1e-9
}
