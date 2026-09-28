//! Paragraph layout: inline flattening, line breaking, alignment and spacing
//! (`STAGE-4-TASK.md` §5.4).

use strict_ooxml_wml::model::values::{BreakKind, LineSpacingRule, TabAlignment};
use strict_ooxml_wml::model::{Document, Drawing, Ilvl, Inline, NumId, Paragraph, Run, RunContent};

use crate::layout::{Flow, ImageItem, LayoutContext, TextItem, TextLine};
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
}

/// A laid-out paragraph plus its surrounding spacing.
pub(crate) struct ParagraphFlow {
    /// Flows in order.
    pub flows: Vec<Flow>,
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
#[must_use]
pub(crate) fn layout_paragraph(
    ctx: &LayoutContext<'_>,
    para: &Paragraph,
    content_left: f64,
    content_width: f64,
    grid_line_pitch: Option<f64>,
) -> ParagraphFlow {
    let computed = compute_paragraph(ctx.document, para);
    let space_before = pt_to_px(computed.space_before_pt, ctx.options.scale);
    let space_after = pt_to_px(computed.space_after_pt, ctx.options.scale);

    let mut segments = Vec::new();
    flatten_inlines(ctx, &computed, &para.inlines, &mut segments);

    let flows = build_lines(
        ctx,
        para,
        &computed,
        content_left,
        content_width,
        grid_line_pitch,
        segments,
    );

    ParagraphFlow {
        flows,
        space_before,
        space_after,
        keep_lines: computed.keep_lines,
    }
}

/// Recursively flattens inline content into segments.
fn flatten_inlines(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    inlines: &[Inline],
    out: &mut Vec<Seg>,
) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => flatten_run(ctx, computed, run, out),
            Inline::Hyperlink(link) => flatten_inlines(ctx, computed, &link.inlines, out),
            Inline::Field(field) => flatten_inlines(ctx, computed, &field.inlines, out),
            Inline::Drawing(drawing) => flatten_drawing(ctx, drawing, out),
            Inline::Break(kind) => match kind {
                BreakKind::Page => out.push(Seg::PageBreak),
                BreakKind::Column | BreakKind::TextWrapping => out.push(Seg::Break),
            },
            Inline::Tab => out.push(Seg::Tab),
            Inline::SdtInline(sdt) => flatten_inlines(ctx, computed, &sdt.inlines, out),
            Inline::BookmarkStart(_)
            | Inline::BookmarkEnd(_)
            | Inline::CommentRangeStart(_)
            | Inline::CommentRangeEnd(_)
            | Inline::CommentReference(_)
            | Inline::FootnoteRef(_)
            | Inline::EndnoteRef(_)
            | Inline::Opaque(_) => {}
        }
    }
}

fn flatten_run(
    ctx: &LayoutContext<'_>,
    computed: &ComputedParagraph,
    run: &Run,
    out: &mut Vec<Seg>,
) {
    let run_style = compute_run(ctx.document, computed, run);
    for content in &run.content {
        match content {
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
            RunContent::SoftHyphen
            | RunContent::InstrText(_)
            | RunContent::FieldChar(_)
            | RunContent::FootnoteRef(_)
            | RunContent::EndnoteRef(_)
            | RunContent::LastRenderedPageBreak
            | RunContent::Opaque(_) => {}
        }
    }
}

fn flatten_drawing(ctx: &LayoutContext<'_>, drawing: &Drawing, out: &mut Vec<Seg>) {
    if let Some(image) = layout_inline_image(ctx, drawing, 0.0, 0.0) {
        out.push(Seg::Image(image));
    }
}

/// Mutable line being assembled.
struct LineBuilder {
    items: Vec<TextItem>,
}

impl LineBuilder {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[allow(clippy::too_many_arguments)]
fn build_lines(
    ctx: &LayoutContext<'_>,
    para: &Paragraph,
    computed: &ComputedParagraph,
    content_left: f64,
    content_width: f64,
    grid_line_pitch: Option<f64>,
    segments: Vec<Seg>,
) -> Vec<Flow> {
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

    let marker = computed
        .numbering
        .and_then(|numbering| numbering_marker(ctx.document, numbering.num_id, numbering.ilvl));

    let mut sink = LineSink {
        ctx,
        computed,
        line_width,
        grid_line_pitch,
        flows: Vec::new(),
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
        }
    }
    sink.emit(current, true);
    let mut flows = sink.flows;

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
                },
            );
        }
    }

    let _ = para;
    flows
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
    }
    TextLine {
        items: line.items,
        height,
        ascent,
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
            // With no explicit line spacing, Word/WPS use the document grid's
            // line pitch as the line height; the extra leading goes above the
            // baseline so text keeps its vertical position within the grid.
            let height = match grid_line_pitch {
                Some(pitch) if computed.line_pt.is_none() => scaled.max(pitch),
                _ => scaled,
            };
            let extra = (height - scaled).max(0.0);
            (height, (natural_ascent + extra).min(height))
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

/// Resolves a numbering marker text for `(num_id, ilvl)`.
fn numbering_marker(document: &Document, num_id: u32, ilvl: u8) -> Option<(String, ComputedRun)> {
    let abstract_num = document.numbering.resolved_abstract(NumId(num_id))?;
    let level = abstract_num.level(Ilvl(ilvl))?;
    let text = level.text.as_ref()?;
    let start = level.start.unwrap_or(1);
    let mut rendered = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(digit) = chars.peek().and_then(|value| value.to_digit(10)) {
                chars.next();
                rendered.push_str(&start.to_string());
                let _ = digit;
                continue;
            }
        }
        rendered.push(ch);
    }
    if rendered.is_empty() {
        return None;
    }
    let mut run = ComputedRun::default();
    crate::style::apply_run_props(&mut run, &level.run);
    Some((rendered, run))
}
