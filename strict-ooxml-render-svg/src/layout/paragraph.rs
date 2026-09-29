//! Paragraph layout: inline flattening, line breaking, alignment and spacing
//! (`STAGE-4-TASK.md` §5.4).

use strict_ooxml_wml::model::values::{
    BreakKind, FieldCharType, LineSpacingRule, TabAlignment, VertAlign,
};
use strict_ooxml_wml::model::{
    AnchorDrawing, Drawing, DrawingKind, Inline, Paragraph, Run, RunContent,
};

use crate::layout::{Flow, ImageItem, Item, LayoutContext, TextItem, TextLine};
use crate::paint::image::layout_inline_image;
use crate::style::{apply_caps, compute_paragraph, compute_run, ComputedParagraph, ComputedRun};
use crate::units::{pt_to_px, twips_to_px};

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
    /// An inline image.
    Image(ImageItem),
    /// An inline block object (shape/group), already rendered.
    Object(Vec<Item>, f64),
    /// A floating (anchored) drawing.
    Anchor(Box<AnchorDrawing>),
    /// A footnote reference marker (the referenced note id).
    FootnoteMarker(u32, ComputedRun),
    /// An endnote reference marker (the referenced note id).
    EndnoteMarker(u32, ComputedRun),
    /// The note number marker inside a note body (`w:footnoteRef`).
    NoteNumber(ComputedRun),
    /// A computed field result (PAGE/NUMPAGES/SECTIONPAGES).
    FieldResult(
        crate::fields::FieldKind,
        crate::notes::NumberFormat,
        ComputedRun,
    ),
}

/// A laid-out paragraph plus its surrounding spacing.
pub(crate) struct ParagraphFlow {
    /// Flows in order.
    pub flows: Vec<Flow>,
    /// Floating (anchored) drawings attached to this paragraph.
    pub anchors: Vec<AnchorDrawing>,
    /// Space before in px.
    pub space_before: f64,
    /// Space after in px.
    pub space_after: f64,
    /// Keep the paragraph on one page.
    pub keep_lines: bool,
}

/// Default tab stop in twips (0.5 inch) when settings omit one.
const DEFAULT_TAB_TWIPS: i32 = 720;

/// Lays out one paragraph within a content box.
///
/// `note_marker` is the formatted number used to replace a `w:footnoteRef`/
/// `w:endnoteRef` marker when laying out a note body; body paragraphs pass
/// `None`.
#[must_use]
pub(crate) fn layout_paragraph(
    ctx: &LayoutContext<'_>,
    para: &Paragraph,
    content_left: f64,
    content_width: f64,
    grid_line_pitch: Option<f64>,
    note_marker: Option<&str>,
) -> ParagraphFlow {
    let mut computed = compute_paragraph(ctx.document, para);
    // A numbered paragraph without its own indentation inherits the level's.
    if para.props.indentation.is_none() {
        if let Some(marker) = ctx.numbering.get(&para.location) {
            if let Some(start) = marker.indent_start_pt {
                computed.indent_start_pt = start;
            }
            if let Some(first_line) = marker.first_line_pt {
                computed.first_line_pt = first_line;
            }
        }
    }
    let space_before = pt_to_px(computed.space_before_pt, ctx.options.scale);
    let space_after = pt_to_px(computed.space_after_pt, ctx.options.scale);

    let mut segments = Vec::new();
    let mut field_state = FieldState::default();
    flatten_inlines(
        ctx,
        &computed,
        &para.inlines,
        &mut segments,
        &mut field_state,
    );

    let flows = build_lines(
        ctx,
        para,
        &computed,
        content_left,
        content_width,
        grid_line_pitch,
        note_marker,
        segments,
    );

    ParagraphFlow {
        flows: flows.0,
        anchors: flows.1,
        space_before,
        space_after,
        keep_lines: computed.keep_lines,
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
    computed: Option<(crate::fields::FieldKind, crate::notes::NumberFormat)>,
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
fn flatten_inlines(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    inlines: &[Inline],
    out: &mut Vec<Seg>,
    field: &mut FieldState,
) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => flatten_run(ctx, computed, run, out, field),
            _ if field.suppressed() => {}
            Inline::Hyperlink(link) => flatten_inlines(ctx, computed, &link.inlines, out, field),
            Inline::Field(simple) => {
                let instruction = simple.instruction.as_deref().unwrap_or("");
                let (kind, format) = crate::fields::parse_instruction(instruction);
                if let Some(kind) = kind {
                    out.push(Seg::FieldResult(kind, format, computed.default_run.clone()));
                } else {
                    flatten_inlines(ctx, computed, &simple.inlines, out, field);
                }
            }
            Inline::Drawing(drawing) => flatten_drawing(ctx, drawing, out),
            Inline::Break(kind) => match kind {
                BreakKind::Page => out.push(Seg::PageBreak),
                BreakKind::Column | BreakKind::TextWrapping => out.push(Seg::Break),
            },
            Inline::Tab => out.push(Seg::Tab),
            Inline::SdtInline(sdt) => {
                flatten_inlines(ctx, computed, &sdt.inlines, out, field);
            }
            Inline::FootnoteRef(id) => out.push(Seg::FootnoteMarker(
                *id,
                superscript(computed.default_run.clone()),
            )),
            Inline::EndnoteRef(id) => out.push(Seg::EndnoteMarker(
                *id,
                superscript(computed.default_run.clone()),
            )),
            Inline::BookmarkStart(_)
            | Inline::BookmarkEnd(_)
            | Inline::CommentRangeStart(_)
            | Inline::CommentRangeEnd(_)
            | Inline::CommentReference(_)
            | Inline::Opaque(_) => {}
        }
    }
}

fn flatten_run(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    run: &Run,
    out: &mut Vec<Seg>,
    field: &mut FieldState,
) {
    let run_style = compute_run(ctx.document, computed, run);
    for content in &run.content {
        match content {
            RunContent::FieldChar(field_char) => field.on_char(field_char.kind, out, &run_style),
            RunContent::InstrText(text) => field.on_instr(text),
            _ if field.suppressed() => {}
            RunContent::Text(text) => {
                if !text.text.is_empty() {
                    out.push(Seg::Text(
                        apply_caps(&text.text, &run_style),
                        run_style.clone(),
                    ));
                }
            }
            RunContent::Tab => out.push(Seg::Tab),
            RunContent::Break(BreakKind::Page) => out.push(Seg::PageBreak),
            RunContent::Break(BreakKind::Column | BreakKind::TextWrapping)
            | RunContent::CarriageReturn => out.push(Seg::Break),
            RunContent::Drawing(drawing) => flatten_drawing(ctx, drawing, out),
            RunContent::Symbol(symbol) => {
                out.push(Seg::Text(symbol.character.to_string(), run_style.clone()));
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
            RunContent::SoftHyphen | RunContent::LastRenderedPageBreak | RunContent::Opaque(_) => {}
        }
    }
}

fn flatten_drawing(ctx: &LayoutContext<'_>, drawing: &Drawing, out: &mut Vec<Seg>) {
    match &drawing.kind {
        DrawingKind::Anchor(anchor) => out.push(Seg::Anchor(Box::new(anchor.clone()))),
        DrawingKind::Inline(_) => {
            if let Some(image) = layout_inline_image(ctx, drawing, 0.0, 0.0) {
                out.push(Seg::Image(image));
            } else if let Some((items, _, height)) =
                crate::paint::graphics::inline_items(ctx, drawing)
            {
                out.push(Seg::Object(items, height));
            }
        }
        DrawingKind::Opaque(_) => {}
    }
}

/// Mutable line being assembled.
struct LineBuilder {
    items: Vec<TextItem>,
    footnote_refs: Vec<u32>,
}

impl LineBuilder {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            footnote_refs: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
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
) -> (Vec<Flow>, Vec<AnchorDrawing>) {
    let scale = ctx.options.scale;
    let indent_start = pt_to_px(computed.indent_start_pt, scale);
    let indent_end = pt_to_px(computed.indent_end_pt, scale);
    let normal_x = content_left + indent_start;
    let first_offset = pt_to_px(computed.first_line_pt, scale);
    let first_x = content_left + indent_start + first_offset;
    let line_width = (content_width - indent_start - indent_end).max(1.0);

    let default_tab = ctx.document.settings.default_tab_stop.map_or(
        DEFAULT_TAB_TWIPS,
        strict_ooxml_wml::model::values::Twips::value,
    );

    let marker = ctx
        .numbering
        .get(&para.location)
        .map(|marker| (marker.text.clone(), marker.run.clone()));

    let mut sink = LineSink {
        ctx,
        computed,
        line_width,
        grid_line_pitch,
        flows: Vec::new(),
        anchors: Vec::new(),
    };
    let mut current = LineBuilder::new();
    let mut x = if marker.is_some() { normal_x } else { first_x };
    let mut line_start = x;
    let mut first_line = true;

    for segment in segments {
        match segment {
            Seg::PageBreak => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                sink.flows.push(Flow::PageBreak);
                x = normal_x;
                line_start = x;
                first_line = true;
            }
            Seg::Break => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                x = normal_x;
                line_start = x;
                first_line = false;
            }
            Seg::Tab => {
                x = next_tab_x(x, computed, default_tab, scale, content_left);
            }
            Seg::Image(image) => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                let mut image = image;
                image.x = normal_x;
                sink.flows.push(Flow::Image(image));
                x = normal_x;
                line_start = x;
                first_line = false;
            }
            Seg::Object(items, height) => {
                sink.emit(std::mem::replace(&mut current, LineBuilder::new()), false);
                sink.flows.push(Flow::Block { items, height });
                x = normal_x;
                line_start = x;
                first_line = false;
            }
            Seg::Anchor(anchor) => sink.anchors.push(*anchor),
            Seg::Text(text, run) => {
                for token in tokenize(&text) {
                    place_token(
                        &mut sink,
                        &mut current,
                        &mut x,
                        line_start,
                        first_x,
                        normal_x,
                        &token,
                        &run,
                        &mut first_line,
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
    sink.emit(current, true);
    let mut flows = sink.flows;
    let anchors = sink.anchors;

    // Prepend the numbering marker to the first line, if any.
    if let Some((marker_text, marker_run)) = &marker {
        if let Some(Flow::Line(first)) = flows.iter_mut().find(|flow| matches!(flow, Flow::Line(_)))
        {
            let size_px = ctx.size_px(marker_run.size_pt);
            let width = ctx.measure(marker_text, marker_run);
            first.items.insert(
                0,
                TextItem {
                    x: first_x,
                    baseline: first.ascent,
                    width,
                    text: marker_text.clone(),
                    run: marker_run.clone(),
                    size_px,
                    field: None,
                },
            );
        }
    }

    let _ = para;
    (flows, anchors)
}

/// Splits text into wrap tokens (words keep a single trailing space).
fn tokenize(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split_inclusive(' ').map(str::to_owned).collect()
}

/// Accumulates finished lines and break/image flows in order.
struct LineSink<'a, 'b> {
    ctx: &'b LayoutContext<'a>,
    computed: &'b ComputedParagraph,
    line_width: f64,
    grid_line_pitch: Option<f64>,
    flows: Vec<Flow>,
    anchors: Vec<AnchorDrawing>,
}

impl LineSink<'_, '_> {
    fn emit(&mut self, line: LineBuilder, last: bool) {
        let finished = finish_line(
            self.ctx,
            self.computed,
            line,
            self.line_width,
            self.grid_line_pitch,
            last,
        );
        self.flows.push(Flow::Line(finished));
    }
}

#[allow(clippy::too_many_arguments)]
fn place_token(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    line_start: f64,
    first_x: f64,
    normal_x: f64,
    token: &str,
    run: &ComputedRun,
    first_line: &mut bool,
) {
    let width = sink.ctx.measure(token, run);
    let line_end = line_start + sink.line_width;
    if *x + width > line_end + 1e-9 && !current.is_empty() {
        sink.emit(std::mem::replace(current, LineBuilder::new()), false);
        *x = if *first_line { first_x } else { normal_x };
        *first_line = false;
    }
    // Skip a leading space at the start of a line.
    if current.is_empty() && token.trim().is_empty() {
        *x += width;
        return;
    }
    if *x + width > line_end + 1e-9 && current.is_empty() {
        // A single token wider than the line: break by characters.
        place_long_token(
            sink, current, x, line_end, first_x, normal_x, token, run, first_line,
        );
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
        field: None,
    });
    *x += width;
}

#[allow(clippy::too_many_arguments)]
fn place_long_token(
    sink: &mut LineSink<'_, '_>,
    current: &mut LineBuilder,
    x: &mut f64,
    line_end: f64,
    first_x: f64,
    normal_x: f64,
    token: &str,
    run: &ComputedRun,
    first_line: &mut bool,
) {
    let size_px = sink.ctx.size_px(run.size_pt);
    for ch in token.chars() {
        let width = sink.ctx.measure(&ch.to_string(), run);
        if *x + width > line_end + 1e-9 && !current.is_empty() {
            sink.emit(std::mem::replace(current, LineBuilder::new()), false);
            *x = if *first_line { first_x } else { normal_x };
            *first_line = false;
        }
        current.items.push(TextItem {
            x: *x,
            baseline: 0.0,
            width,
            text: ch.to_string(),
            run: run.clone(),
            size_px,
            field: None,
        });
        *x += width;
    }
}

/// Finalizes a line: baseline, justification and alignment.
fn finish_line(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    mut line: LineBuilder,
    line_width: f64,
    grid_line_pitch: Option<f64>,
    last: bool,
) -> TextLine {
    let used = line_extent(&line.items);
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

    let (height, ascent) = resolve_line_metrics(ctx, computed, line.items.first(), grid_line_pitch);
    for item in &mut line.items {
        item.baseline = ascent;
        apply_vertical_align(item);
    }
    TextLine {
        items: line.items,
        height,
        ascent,
        footnote_refs: line.footnote_refs,
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
        field: None,
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
    let text = marker.format.format(1);
    let width = ctx.measure(&text, run);
    let size_px = ctx.size_px(run.size_pt);
    current.items.push(TextItem {
        x: *x,
        baseline: 0.0,
        width,
        text,
        run: run.clone(),
        size_px,
        field: Some(marker),
    });
    *x += width;
}

/// Adjusts a superscript/subscript item's size and baseline.
fn apply_vertical_align(item: &mut TextItem) {
    let base = item.size_px;
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
fn line_extent(items: &[TextItem]) -> f64 {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for item in items {
        min = min.min(item.x);
        max = max.max(item.x + item.width);
    }
    if min.is_finite() && max.is_finite() {
        (max - min).max(0.0)
    } else {
        0.0
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
    let metrics = ctx.font.metrics(&run.family, run.bold, run.italic);
    let size_px = ctx.size_px(run.size_pt);
    let natural_height = (metrics.line_height_em() * size_px).max(1.0);
    let natural_ascent = metrics.ascent_em() * size_px;

    let (height, ascent) = match computed.line_rule {
        LineSpacingRule::Exact => {
            let exact = computed
                .line_pt
                .map_or(natural_height, |pt| pt_to_px(pt, ctx.options.scale));
            let exact = exact.max(1.0);
            (exact, exact * 0.8)
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

/// Returns the next tab-stop x (absolute px), or a default step if none.
fn next_tab_x(
    x: f64,
    computed: &ComputedParagraph,
    default_tab: i32,
    scale: f64,
    content_left: f64,
) -> f64 {
    let relative = x - content_left;
    let mut stop_positions: Vec<f64> = computed
        .tabs
        .iter()
        .filter(|candidate| !matches!(candidate.alignment, TabAlignment::Clear | TabAlignment::Bar))
        .map(|candidate| twips_to_px(candidate.position.value(), scale))
        .collect();
    stop_positions
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(position) = stop_positions
        .into_iter()
        .find(|position| *position > relative + 1e-6)
    {
        return content_left + position;
    }
    let tab_step = twips_to_px(default_tab, scale);
    let steps = (relative / tab_step).floor() + 1.0;
    content_left + steps * tab_step
}
