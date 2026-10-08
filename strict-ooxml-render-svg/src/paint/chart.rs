//! Charts drawn from their cached data (`c:chartSpace` caches read by
//! `strict-ooxml-wml`).
//!
//! The chart is laid out inside the drawing's box: an optional title on top,
//! an optional legend on the side `c:legendPos` names, and the plot area with
//! a "nice number" value axis. Bars, areas, lines, pie and doughnut sectors and
//! scatter markers are ordinary paint items, so the SVG and PDF backends draw
//! them without knowing what a chart is; arcs are cubic Béziers (at most a
//! quarter turn each), which both backends support. Text is measured with the
//! layout's font provider, like every other text item.
//!
//! A chart never emits more than [`MAX_CHART_MARKS`] items; a larger one is
//! drawn with its first marks and a page warning says so.

use std::fmt::Write as _;

use strict_ooxml_wml::model::chart::{
    BarGrouping, ChartData, ChartKind, ChartSeries, LegendPosition,
};

use crate::layout::{Item, LayoutContext, LineItem, PathItem, RectItem, TextAdvanceKind, TextItem};
use crate::style::ComputedRun;
use crate::units::{finite, fmt_num};

/// The Office 2013+ default accent order, used when a series has no colour.
const PALETTE: [&str; 6] = [
    "#4472c4", "#ed7d31", "#a5a5a5", "#ffc000", "#5b9bd5", "#70ad47",
];

/// Most paint items one chart may emit.
pub(crate) const MAX_CHART_MARKS: usize = 20_000;

/// Most legend entries drawn.
const MAX_LEGEND_ENTRIES: usize = 64;

/// Most characters of a label that are measured and drawn.
const MAX_LABEL_CHARS: usize = 64;

/// Data values are clamped into `±VALUE_LIMIT` so scale arithmetic stays finite.
const VALUE_LIMIT: f64 = 1e12;

/// Text face of chart labels.
const FAMILY: &str = "Calibri";
const TEXT_COLOR: &str = "#595959";
const GRID_COLOR: &str = "#d9d9d9";
const AXIS_COLOR: &str = "#bfbfbf";

/// Relative gap between bar groups (`c:gapWidth` default 150 %).
const GAP_WIDTH: f64 = 1.5;
/// Doughnut hole as a fraction of the outer radius.
const HOLE: f64 = 0.5;
/// Markers are drawn on lines with at most this many categories.
const MAX_MARKED_POINTS: usize = 50;

/// An axis-aligned box in page px.
#[derive(Clone, Copy, Debug)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Frame {
    fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            x: finite(x),
            y: finite(y),
            w: finite(w).max(0.0),
            h: finite(h).max(0.0),
        }
    }

    #[must_use]
    fn inset(self, left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self::new(
            self.x + left,
            self.y + top,
            self.w - left - right,
            self.h - top - bottom,
        )
    }

    fn right(self) -> f64 {
        self.x + self.w
    }

    fn bottom(self) -> f64 {
        self.y + self.h
    }
}

/// Horizontal anchoring of a label.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Start,
    Middle,
    End,
}

/// A value axis: `lo..=hi` in steps of `step`.
#[derive(Clone, Copy, Debug)]
struct Scale {
    lo: f64,
    hi: f64,
    step: f64,
}

impl Scale {
    /// A "nice number" scale over `min..=max` with four to six ticks.
    fn nice(min: f64, max: f64) -> Self {
        let (mut min, mut max) = (clamp_value(min), clamp_value(max));
        if max < min {
            std::mem::swap(&mut min, &mut max);
        }
        if (max - min).abs() < f64::EPSILON {
            if min.abs() < f64::EPSILON {
                max = 1.0;
            } else if min > 0.0 {
                min = 0.0;
            } else {
                max = 0.0;
            }
        }
        let step = nice_step((max - min) / 5.0);
        let lo = (min / step).floor() * step;
        let mut hi = (max / step).ceil() * step;
        if hi - lo < step {
            hi = lo + step;
        }
        Self { lo, hi, step }
    }

    /// The scale over the finite values, with zero included when asked to or
    /// when the data is close enough to it (Excel's one-sixth rule).
    fn over(values: impl Iterator<Item = f64>, force_zero: bool) -> Self {
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for value in values.filter(|value| value.is_finite()) {
            let value = clamp_value(value);
            min = min.min(value);
            max = max.max(value);
        }
        if min > max {
            return Self::nice(0.0, 1.0);
        }
        let near_zero = (min >= 0.0 && max > 0.0 && (max - min) / max >= 1.0 / 6.0)
            || (max <= 0.0 && min < 0.0 && (max - min) / -min >= 1.0 / 6.0);
        if force_zero || near_zero {
            min = min.min(0.0);
            max = max.max(0.0);
        }
        Self::nice(min, max)
    }

    /// The tick values, at most twelve.
    fn ticks(self) -> Vec<f64> {
        let mut ticks = Vec::new();
        let mut value = self.lo;
        while value <= self.hi + self.step / 2.0 && ticks.len() < 12 {
            ticks.push(if value.abs() < self.step / 1e6 {
                0.0
            } else {
                value
            });
            value += self.step;
        }
        ticks
    }

    /// `value`'s fraction of the way from `lo` to `hi`, clamped to `0..=1`.
    fn fraction(self, value: f64) -> f64 {
        let span = self.hi - self.lo;
        if span.abs() < f64::EPSILON || !span.is_finite() {
            return 0.0;
        }
        let fraction = (clamp_value(value) - self.lo) / span;
        if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// A tick label with as many decimals as the step needs.
    fn label(self, value: f64, percent: bool) -> String {
        let decimals = if self.step >= 1.0 {
            0
        } else {
            let digits = -self.step.log10().floor();
            if digits >= 6.0 {
                6
            } else if digits >= 1.0 {
                usize::try_from(saturating_round(digits)).unwrap_or(0)
            } else {
                0
            }
        };
        let mut text = format!("{value:.decimals$}");
        if text.starts_with('-') && text.trim_start_matches(['-', '0', '.']).is_empty() {
            text.remove(0);
        }
        if percent {
            text.push('%');
        }
        text
    }
}

/// Rounds a finite `f64` into `i64`, saturating.
fn saturating_round(value: f64) -> i64 {
    crate::units::to_i64_saturating(value.round())
}

/// 1, 2, 5 or 10 times a power of ten, at least `raw` (so tick labels need no
/// more decimals than the step itself).
fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    let fraction = raw / power;
    let nice = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = nice * power;
    if step.is_finite() && step > 0.0 {
        step
    } else {
        1.0
    }
}

fn clamp_value(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-VALUE_LIMIT, VALUE_LIMIT)
    } else {
        0.0
    }
}

/// The colour of series `index` (`#rrggbb`).
fn series_color(series: &ChartSeries, index: usize) -> String {
    series.color.as_deref().map_or_else(
        || palette(index).to_owned(),
        |hex| format!("#{}", hex.to_ascii_lowercase()),
    )
}

/// The colour of point `index` of a pie series.
fn point_color(series: Option<&ChartSeries>, index: usize) -> String {
    series
        .and_then(|series| {
            series
                .point_colors
                .iter()
                .find(|(point, _)| *point == index)
        })
        .map_or_else(
            || palette(index).to_owned(),
            |(_, hex)| format!("#{}", hex.to_ascii_lowercase()),
        )
}

fn palette(index: usize) -> &'static str {
    PALETTE
        .get(index % PALETTE.len())
        .copied()
        .unwrap_or("#4472c4")
}

/// The name shown for series `index`.
fn series_label(series: &ChartSeries, index: usize) -> String {
    series
        .name
        .clone()
        .unwrap_or_else(|| format!("Series {}", index + 1))
}

/// A category label (its first [`MAX_LABEL_CHARS`] characters), or its
/// one-based number.
fn category_label(chart: &ChartData, index: usize) -> String {
    chart
        .categories
        .get(index)
        .filter(|label| !label.is_empty())
        .map_or_else(
            || (index + 1).to_string(),
            |label| label.chars().take(MAX_LABEL_CHARS).collect(),
        )
}

/// The legend entries: categories for a pie, series otherwise.
fn legend_entries(chart: &ChartData) -> Vec<(String, String)> {
    if matches!(chart.kind, ChartKind::Pie { .. }) {
        let first = chart.series.first();
        let count = chart
            .categories
            .len()
            .max(first.map_or(0, |series| series.values.len()));
        return (0..count.min(MAX_LEGEND_ENTRIES))
            .map(|index| (category_label(chart, index), point_color(first, index)))
            .collect();
    }
    chart
        .series
        .iter()
        .take(MAX_LEGEND_ENTRIES)
        .enumerate()
        .map(|(index, series)| (series_label(series, index), series_color(series, index)))
        .collect()
}

/// Builds the paint items of a chart at the given box.
pub(crate) fn chart_items(
    ctx: &LayoutContext<'_>,
    chart: &ChartData,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<Item> {
    let frame = Frame::new(x, y, w, h);
    let mut painter = Painter::new(ctx, frame);
    painter.rect(frame, Some("#ffffff"), Some(GRID_COLOR));
    let pad = (frame.w.min(frame.h) * 0.04).clamp(2.0, 12.0);
    let mut inner = frame.inset(pad, pad, pad, pad);
    if let Some(title) = chart.title.as_deref() {
        inner = painter.title(inner, title);
    }
    let entries = legend_entries(chart);
    if chart.legend && !entries.is_empty() {
        inner = painter.legend(inner, &entries, chart.legend_position);
    }
    match &chart.kind {
        ChartKind::Bar {
            horizontal,
            grouping,
        } => painter.bars(inner, chart, *horizontal, *grouping),
        ChartKind::Line => painter.lines(inner, chart, false),
        ChartKind::Area => painter.lines(inner, chart, true),
        ChartKind::Pie { doughnut } => painter.pie(inner, chart, *doughnut),
        ChartKind::Scatter => painter.scatter(inner, chart),
        ChartKind::Other(_) => painter.rect(inner, Some("#f2f2f2"), Some("#999999")),
    }
    if painter.capped {
        ctx.warn(format!(
            "render.chart-capped: a chart was drawn with its first {MAX_CHART_MARKS} marks only"
        ));
    }
    painter.items
}

/// Collects one chart's items under the mark budget.
struct Painter<'c, 'a> {
    ctx: &'c LayoutContext<'a>,
    items: Vec<Item>,
    marks_left: usize,
    capped: bool,
    /// Label size in px.
    label_px: f64,
    /// Title size in px.
    title_px: f64,
    /// Line width of a data line in px.
    line_px: f64,
}

impl<'c, 'a> Painter<'c, 'a> {
    fn new(ctx: &'c LayoutContext<'a>, frame: Frame) -> Self {
        let scale = if ctx.options.scale.is_finite() && ctx.options.scale > 0.0 {
            ctx.options.scale
        } else {
            96.0
        };
        let shrink = (frame.w.min(frame.h) / 240.0).clamp(0.5, 1.0);
        let px_per_pt = scale / 72.0 * shrink;
        Self {
            ctx,
            items: Vec::new(),
            marks_left: MAX_CHART_MARKS,
            capped: false,
            label_px: 9.0 * px_per_pt,
            title_px: 14.0 * px_per_pt,
            line_px: 2.25 * px_per_pt,
        }
    }

    fn push(&mut self, item: Item) {
        if self.marks_left == 0 {
            self.capped = true;
            return;
        }
        self.marks_left -= 1;
        self.items.push(item);
    }

    fn full(&self) -> bool {
        self.marks_left == 0
    }

    fn rect(&mut self, frame: Frame, fill: Option<&str>, stroke: Option<&str>) {
        self.push(Item::Rect(RectItem {
            x: frame.x,
            y: frame.y,
            w: frame.w,
            h: frame.h,
            fill: fill.map(str::to_owned),
            stroke: stroke.map(str::to_owned),
            stroke_w: if stroke.is_some() { 0.75 } else { 0.0 },
        }));
    }

    fn line(&mut self, from: (f64, f64), to: (f64, f64), color: &str) {
        self.push(Item::Line(LineItem {
            x1: finite(from.0),
            y1: finite(from.1),
            x2: finite(to.0),
            y2: finite(to.1),
            color: color.to_owned(),
            width: 0.75,
            dashed: false,
        }));
    }

    /// A path whose `d` is in `frame`-local coordinates.
    fn path(&mut self, frame: Frame, d: String, fill: Option<String>, stroke: Option<String>) {
        let stroke_w = if stroke.is_some() { self.line_px } else { 0.0 };
        self.push(Item::Path(PathItem {
            x: frame.x,
            y: frame.y,
            w: frame.w,
            h: frame.h,
            d,
            fill,
            stroke,
            stroke_w,
            dash: None,
            rotate_deg: 0.0,
            flip_h: false,
            flip_v: false,
        }));
    }

    fn text_width(&self, text: &str, size_px: f64) -> f64 {
        let em: f64 = text
            .chars()
            .map(|ch| finite(self.ctx.font.advance_em(FAMILY, ch, false, false)))
            .sum();
        finite(em * size_px)
    }

    /// `text` cut to `max_w` px with an ellipsis; empty when nothing fits.
    fn fit(&self, text: &str, size_px: f64, max_w: f64) -> String {
        let text: String = text.chars().take(MAX_LABEL_CHARS).collect();
        if self.text_width(&text, size_px) <= max_w {
            return text;
        }
        let mut kept = String::new();
        let budget = max_w - self.text_width("\u{2026}", size_px);
        for ch in text.chars() {
            kept.push(ch);
            if self.text_width(&kept, size_px) > budget {
                kept.pop();
                break;
            }
        }
        if kept.is_empty() {
            return kept;
        }
        kept.push('\u{2026}');
        kept
    }

    /// Draws a label; returns its width.
    fn text(&mut self, x: f64, baseline: f64, text: &str, size_px: f64, align: Align) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let width = self.text_width(text, size_px);
        let left = match align {
            Align::Start => x,
            Align::Middle => x - width / 2.0,
            Align::End => x - width,
        };
        let scale = self.ctx.options.scale;
        let point_size = if scale > 0.0 {
            size_px * 72.0 / scale
        } else {
            9.0
        };
        self.push(Item::Text(TextItem {
            x: finite(left),
            baseline: finite(baseline),
            width,
            text: text.to_owned(),
            run: ComputedRun {
                family: FAMILY.to_owned(),
                point_size: finite(point_size),
                color: Some(TEXT_COLOR.to_owned()),
                ..ComputedRun::default()
            },
            size_px,
            advance: TextAdvanceKind::Shaped,
            field: None,
            compress_punctuation: false,
            following_non_space: None,
            plain_space_factor: 1.0,
        }));
        width
    }

    /// Draws the title on top; returns the remaining box.
    fn title(&mut self, inner: Frame, title: &str) -> Frame {
        let size = self.title_px;
        let shown = self.fit(title, size, inner.w);
        self.text(
            inner.x + inner.w / 2.0,
            inner.y + size,
            &shown,
            size,
            Align::Middle,
        );
        let used = (size * 1.6).min(inner.h);
        inner.inset(0.0, used, 0.0, 0.0)
    }

    /// Draws the legend; returns the remaining box.
    fn legend(
        &mut self,
        inner: Frame,
        entries: &[(String, String)],
        position: LegendPosition,
    ) -> Frame {
        let size = self.label_px;
        let swatch = size * 0.7;
        let gap = size * 0.4;
        match position {
            LegendPosition::Right | LegendPosition::Left => {
                let max_label = inner.w * 0.3 - swatch - gap;
                let labels: Vec<String> = entries
                    .iter()
                    .map(|(label, _)| self.fit(label, size, max_label))
                    .collect();
                let widest = labels
                    .iter()
                    .map(|label| self.text_width(label, size))
                    .fold(0.0, f64::max);
                let width = swatch + gap + widest;
                let row = size * 1.4;
                let rows = labels.len().min(rows_that_fit(inner.h, row));
                let top = inner.y + (inner.h - row * rows as f64) / 2.0;
                let left = if position == LegendPosition::Right {
                    inner.right() - width
                } else {
                    inner.x
                };
                for (index, (label, (_, color))) in
                    labels.iter().zip(entries).take(rows).enumerate()
                {
                    let y = top + row * index as f64;
                    self.legend_entry(left, y, row, label, color);
                }
                if position == LegendPosition::Right {
                    inner.inset(0.0, 0.0, width + gap * 2.0, 0.0)
                } else {
                    inner.inset(width + gap * 2.0, 0.0, 0.0, 0.0)
                }
            }
            LegendPosition::Top | LegendPosition::Bottom => {
                self.legend_row(inner, entries, position == LegendPosition::Top)
            }
        }
    }

    /// One horizontal legend row; returns the remaining box.
    fn legend_row(&mut self, inner: Frame, entries: &[(String, String)], top: bool) -> Frame {
        let size = self.label_px;
        let swatch = size * 0.7;
        let gap = size * 0.4;
        let row = size * 1.4;
        let mut shown = Vec::new();
        let mut total = 0.0;
        for (label, color) in entries {
            let label = self.fit(label, size, inner.w * 0.5);
            let width = swatch + gap + self.text_width(&label, size) + size;
            if total + width > inner.w {
                break;
            }
            total += width;
            shown.push((label, color, width));
        }
        let y = if top { inner.y } else { inner.bottom() - row };
        let mut x = inner.x + (inner.w - total) / 2.0;
        for (label, color, width) in shown {
            self.legend_entry(x, y, row, &label, color);
            x += width;
        }
        if top {
            inner.inset(0.0, row + gap, 0.0, 0.0)
        } else {
            inner.inset(0.0, 0.0, 0.0, row + gap)
        }
    }

    fn legend_entry(&mut self, x: f64, y: f64, row: f64, label: &str, color: &str) {
        let size = self.label_px;
        let swatch = size * 0.7;
        let swatch_top = y + (row - swatch) / 2.0;
        self.rect(Frame::new(x, swatch_top, swatch, swatch), Some(color), None);
        let baseline = y + row / 2.0 + size * 0.35;
        self.text(x + swatch + size * 0.4, baseline, label, size, Align::Start);
    }
}

/// How many rows of height `row` fit in `height`.
fn rows_that_fit(height: f64, row: f64) -> usize {
    if row <= 0.0 || !height.is_finite() || height <= 0.0 {
        return 0;
    }
    usize::try_from(saturating_round((height / row).floor())).unwrap_or(0)
}

/// Value-space segments `[from, to]` of every bar, by series then category.
fn bar_segments(chart: &ChartData, grouping: BarGrouping) -> Vec<Vec<Option<(f64, f64)>>> {
    let count = chart.category_count();
    let value = |series: &ChartSeries, index: usize| {
        series
            .values
            .get(index)
            .copied()
            .flatten()
            .filter(|value| value.is_finite())
            .map(clamp_value)
    };
    let mut positive = vec![0.0; count];
    let mut negative = vec![0.0; count];
    let totals: Vec<f64> = (0..count)
        .map(|index| {
            chart
                .series
                .iter()
                .filter_map(|series| value(series, index))
                .map(f64::abs)
                .sum()
        })
        .collect();
    chart
        .series
        .iter()
        .map(|series| {
            (0..count)
                .map(|index| {
                    let raw = value(series, index)?;
                    let amount = match grouping {
                        BarGrouping::PercentStacked => {
                            let total = totals.get(index).copied().unwrap_or(0.0);
                            if total > 0.0 {
                                raw / total * 100.0
                            } else {
                                0.0
                            }
                        }
                        BarGrouping::Clustered | BarGrouping::Stacked => raw,
                    };
                    if grouping == BarGrouping::Clustered {
                        return Some((0.0, amount));
                    }
                    let stack = if amount >= 0.0 {
                        positive.get_mut(index)?
                    } else {
                        negative.get_mut(index)?
                    };
                    let from = *stack;
                    *stack += amount;
                    Some((from, *stack))
                })
                .collect()
        })
        .collect()
}

/// Where the bars of a bar chart go.
struct BarLayout {
    plot: Frame,
    scale: Scale,
    /// Width of one category slot along the category axis.
    slot: f64,
    /// Width of the bars of one category together.
    group: f64,
    /// Bars side by side in one category.
    lanes: usize,
    horizontal: bool,
}

impl BarLayout {
    /// The rectangle of the bar of category `index` in `lane`.
    fn bar(&self, index: usize, lane: usize, (from, to): (f64, f64)) -> Frame {
        let thickness = self.group / self.lanes.max(1) as f64;
        let start =
            self.slot * index as f64 + (self.slot - self.group) / 2.0 + thickness * lane as f64;
        let low = self.scale.fraction(from.min(to));
        let high = self.scale.fraction(from.max(to));
        let plot = self.plot;
        if self.horizontal {
            Frame::new(
                plot.x + low * plot.w,
                plot.bottom() - start - thickness,
                (high - low) * plot.w,
                thickness,
            )
        } else {
            Frame::new(
                plot.x + start,
                plot.bottom() - high * plot.h,
                thickness,
                (high - low) * plot.h,
            )
        }
    }
}

impl Painter<'_, '_> {
    /// Splits `inner` into the plot area and draws a value axis for `scale`
    /// along it (`values_across` puts the values on the horizontal axis).
    fn value_axis(
        &mut self,
        inner: Frame,
        scale: Scale,
        values_across: bool,
        percent: bool,
    ) -> Frame {
        let size = self.label_px;
        let ticks = scale.ticks();
        let labels: Vec<String> = ticks
            .iter()
            .map(|tick| scale.label(*tick, percent))
            .collect();
        if values_across {
            let plot = inner.inset(0.0, 0.0, size, size * 1.6);
            for (tick, label) in ticks.iter().zip(&labels) {
                let x = plot.x + scale.fraction(*tick) * plot.w;
                self.line((x, plot.y), (x, plot.bottom()), GRID_COLOR);
                self.text(x, plot.bottom() + size * 1.2, label, size, Align::Middle);
            }
            return plot;
        }
        let widest = labels
            .iter()
            .map(|label| self.text_width(label, size))
            .fold(0.0, f64::max)
            .min(inner.w * 0.3);
        let plot = inner.inset(widest + size * 0.5, size * 0.5, 0.0, size * 1.6);
        for (tick, label) in ticks.iter().zip(&labels) {
            let y = plot.bottom() - scale.fraction(*tick) * plot.h;
            self.line((plot.x, y), (plot.right(), y), GRID_COLOR);
            self.text(
                plot.x - size * 0.4,
                y + size * 0.35,
                label,
                size,
                Align::End,
            );
        }
        plot
    }

    /// Category labels at the given centres along the bottom, or along the
    /// left within `left` px of the plot area.
    fn category_labels(
        &mut self,
        chart: &ChartData,
        centres: &[f64],
        plot: Frame,
        left: Option<f64>,
    ) {
        let size = self.label_px;
        let mut last_edge = f64::NEG_INFINITY;
        for (index, centre) in centres.iter().enumerate() {
            if self.full() {
                return;
            }
            let label = category_label(chart, index);
            if let Some(room) = left {
                if centre - size / 2.0 < last_edge {
                    continue;
                }
                let shown = self.fit(&label, size, room);
                self.text(
                    plot.x - size * 0.4,
                    centre + size * 0.35,
                    &shown,
                    size,
                    Align::End,
                );
                last_edge = centre + size;
            } else {
                let shown: String = label.chars().take(MAX_LABEL_CHARS).collect();
                let width = self.text_width(&shown, size);
                if centre - width / 2.0 < last_edge {
                    continue;
                }
                self.text(
                    *centre,
                    plot.bottom() + size * 1.2,
                    &shown,
                    size,
                    Align::Middle,
                );
                last_edge = centre + width / 2.0 + size * 0.5;
            }
        }
    }

    /// Bar and column charts.
    fn bars(&mut self, inner: Frame, chart: &ChartData, horizontal: bool, grouping: BarGrouping) {
        let segments = bar_segments(chart, grouping);
        let bounds = segments
            .iter()
            .flatten()
            .flatten()
            .flat_map(|(from, to)| [*from, *to]);
        let scale = Scale::over(bounds, true);
        let count = chart.category_count().clamp(1, MAX_CHART_MARKS);
        let room = horizontal.then(|| {
            (0..count)
                .map(|index| self.text_width(&category_label(chart, index), self.label_px))
                .fold(0.0, f64::max)
                .min(inner.w * 0.3)
        });
        let inner = room.map_or(inner, |room| {
            inner.inset(room + self.label_px * 0.5, 0.0, 0.0, 0.0)
        });
        let plot = self.value_axis(
            inner,
            scale,
            horizontal,
            grouping == BarGrouping::PercentStacked,
        );
        let along = if horizontal { plot.h } else { plot.w };
        let slot = along / count as f64;
        let centres: Vec<f64> = (0..count)
            .map(|index| {
                if horizontal {
                    plot.bottom() - slot * (index as f64 + 0.5)
                } else {
                    plot.x + slot * (index as f64 + 0.5)
                }
            })
            .collect();
        self.category_labels(chart, &centres, plot, room);
        let layout = BarLayout {
            plot,
            scale,
            slot,
            group: slot / (1.0 + GAP_WIDTH),
            lanes: if grouping == BarGrouping::Clustered {
                chart.series.len().max(1)
            } else {
                1
            },
            horizontal,
        };
        for (series_index, (series, bars)) in chart.series.iter().zip(&segments).enumerate() {
            let color = series_color(series, series_index);
            let lane = if grouping == BarGrouping::Clustered {
                series_index
            } else {
                0
            };
            for (index, segment) in bars.iter().enumerate() {
                let Some(segment) = *segment else {
                    continue;
                };
                if self.full() {
                    self.capped = true;
                    return;
                }
                self.rect(layout.bar(index, lane, segment), Some(&color), None);
            }
        }
        self.axis_line(plot, scale, horizontal);
    }

    /// The category axis line, at zero.
    fn axis_line(&mut self, plot: Frame, scale: Scale, values_across: bool) {
        let zero = scale.fraction(0.0);
        if values_across {
            let x = plot.x + zero * plot.w;
            self.line((x, plot.y), (x, plot.bottom()), AXIS_COLOR);
        } else {
            let y = plot.bottom() - zero * plot.h;
            self.line((plot.x, y), (plot.right(), y), AXIS_COLOR);
        }
    }

    /// Line and area charts.
    fn lines(&mut self, inner: Frame, chart: &ChartData, area: bool) {
        let values = chart
            .series
            .iter()
            .flat_map(|series| series.values.iter().copied().flatten());
        let scale = Scale::over(values, area);
        let plot = self.value_axis(inner, scale, false, false);
        let count = chart.category_count().clamp(1, MAX_CHART_MARKS);
        let xs: Vec<f64> = (0..count)
            .map(|index| {
                if area && count > 1 {
                    plot.w * index as f64 / (count - 1) as f64
                } else {
                    plot.w * (index as f64 + 0.5) / count as f64
                }
            })
            .collect();
        let centres: Vec<f64> = xs.iter().map(|x| plot.x + x).collect();
        self.category_labels(chart, &centres, plot, None);
        let ys = |series: &ChartSeries| -> Vec<Option<f64>> {
            padded(&series.values)
                .take(count)
                .map(|value| value.map(|value| plot.h - scale.fraction(value) * plot.h))
                .collect()
        };
        let base = plot.h - scale.fraction(0.0) * plot.h;
        for (series_index, series) in chart.series.iter().enumerate() {
            if self.full() {
                self.capped = true;
                return;
            }
            let color = series_color(series, series_index);
            let points = ys(series);
            if area {
                self.path(plot, area_path(&xs, &points, base), Some(color), None);
                continue;
            }
            let d = line_path(&xs, &points);
            if !d.is_empty() {
                self.path(plot, d, None, Some(color.clone()));
            }
            if count <= MAX_MARKED_POINTS {
                let radius = self.line_px * 1.5;
                for (x, y) in xs.iter().zip(&points) {
                    if let Some(y) = y {
                        self.path(plot, circle(*x, *y, radius), Some(color.clone()), None);
                    }
                }
            }
        }
        self.axis_line(plot, scale, false);
    }

    /// Scatter charts: markers at `(x, y)`.
    fn scatter(&mut self, inner: Frame, chart: &ChartData) {
        let pairs: Vec<Vec<(f64, f64)>> = chart.series.iter().map(scatter_points).collect();
        let x_scale = Scale::over(pairs.iter().flatten().map(|(x, _)| *x), false);
        let y_scale = Scale::over(pairs.iter().flatten().map(|(_, y)| *y), false);
        let plot = self.value_axis(inner, y_scale, false, false);
        let size = self.label_px;
        for tick in x_scale.ticks() {
            let x = plot.x + x_scale.fraction(tick) * plot.w;
            let label = x_scale.label(tick, false);
            self.text(x, plot.bottom() + size * 1.2, &label, size, Align::Middle);
        }
        let radius = self.line_px * 1.5;
        for (series_index, (series, points)) in chart.series.iter().zip(&pairs).enumerate() {
            let color = series_color(series, series_index);
            for (x, y) in points {
                if self.full() {
                    self.capped = true;
                    return;
                }
                let px = x_scale.fraction(*x) * plot.w;
                let py = plot.h - y_scale.fraction(*y) * plot.h;
                self.path(plot, circle(px, py, radius), Some(color.clone()), None);
            }
        }
        self.axis_line(plot, y_scale, false);
    }

    /// Pie and doughnut charts: one disc, or one ring per series.
    fn pie(&mut self, inner: Frame, chart: &ChartData, doughnut: bool) {
        let radius = inner.w.min(inner.h) / 2.0 * 0.9;
        if radius <= 0.0 {
            return;
        }
        let centre = (inner.w / 2.0, inner.h / 2.0);
        let rings: Vec<&ChartSeries> = if doughnut {
            chart.series.iter().collect()
        } else {
            chart.series.iter().take(1).collect()
        };
        let ring_count = rings.len().max(1) as f64;
        let ring_width = if doughnut {
            radius * (1.0 - HOLE) / ring_count
        } else {
            radius
        };
        for (ring, series) in rings.into_iter().enumerate() {
            let outer = radius - ring_width * ring as f64;
            let hole = (outer - ring_width).max(0.0);
            let values: Vec<f64> = series
                .values
                .iter()
                .map(|value| {
                    value
                        .filter(|value| value.is_finite())
                        .map_or(0.0, |value| clamp_value(value).abs())
                })
                .collect();
            let total: f64 = values.iter().sum();
            if total <= 0.0 {
                self.path(
                    inner,
                    circle(centre.0, centre.1, outer),
                    None,
                    Some(GRID_COLOR.to_owned()),
                );
                continue;
            }
            let mut angle = 0.0;
            for (index, value) in values.iter().enumerate() {
                if *value <= 0.0 {
                    continue;
                }
                if self.full() {
                    self.capped = true;
                    return;
                }
                let sweep = value / total * std::f64::consts::TAU;
                let d = sector(centre, outer, hole, angle, angle + sweep);
                let color = point_color(Some(series), index);
                self.push(Item::Path(PathItem {
                    x: inner.x,
                    y: inner.y,
                    w: inner.w,
                    h: inner.h,
                    d,
                    fill: Some(color),
                    stroke: Some("#ffffff".to_owned()),
                    stroke_w: 0.75,
                    dash: None,
                    rotate_deg: 0.0,
                    flip_h: false,
                    flip_v: false,
                }));
                angle += sweep;
            }
        }
    }
}

/// An area polygon from the baseline through every point (a missing point is
/// on the baseline).
fn area_path(xs: &[f64], ys: &[Option<f64>], base: f64) -> String {
    let first = xs.first().copied().unwrap_or(0.0);
    let last = xs.last().copied().unwrap_or(0.0);
    let mut d = format!("M{} {}", fmt_num(first), fmt_num(base));
    for (x, y) in xs.iter().zip(ys) {
        let _ = write!(d, " L{} {}", fmt_num(*x), fmt_num(y.unwrap_or(base)));
    }
    let _ = write!(d, " L{} {} Z", fmt_num(last), fmt_num(base));
    d
}

/// A polyline through the points, broken at every missing one.
fn line_path(xs: &[f64], ys: &[Option<f64>]) -> String {
    let mut d = String::new();
    let mut pen_down = false;
    for (x, y) in xs.iter().zip(ys) {
        match y {
            Some(y) => {
                let command = if pen_down { 'L' } else { 'M' };
                let _ = write!(d, "{command}{} {} ", fmt_num(*x), fmt_num(*y));
                pen_down = true;
            }
            None => pen_down = false,
        }
    }
    d.trim_end().to_owned()
}

/// The values of a series, then `None` for ever.
fn padded(values: &[Option<f64>]) -> impl Iterator<Item = Option<f64>> + '_ {
    values
        .iter()
        .map(|value| value.filter(|value| value.is_finite()))
        .chain(std::iter::repeat(None))
}

/// The finite `(x, y)` points of a scatter series; `x` is the one-based index
/// when the series has no x values.
fn scatter_points(series: &ChartSeries) -> Vec<(f64, f64)> {
    series
        .values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let y = value.filter(|value| value.is_finite())?;
            let x = if series.x_values.is_empty() {
                index as f64 + 1.0
            } else {
                series
                    .x_values
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|value| value.is_finite())?
            };
            Some((clamp_value(x), clamp_value(y)))
        })
        .collect()
}

/// A point on a circle; angle 0 is twelve o'clock, increasing clockwise.
fn on_circle(centre: (f64, f64), radius: f64, angle: f64) -> (f64, f64) {
    (
        centre.0 + radius * angle.sin(),
        centre.1 - radius * angle.cos(),
    )
}

/// Cubic Bézier segments (at most a quarter turn each) along a circle from
/// `start` to `end`, appended to `d` without the initial move.
fn arc(d: &mut String, centre: (f64, f64), radius: f64, start: f64, end: f64) {
    let quarter = std::f64::consts::FRAC_PI_2;
    let pieces = ((end - start).abs() / quarter).ceil().clamp(1.0, 8.0);
    let count = usize::try_from(crate::units::to_i64_saturating(pieces)).unwrap_or(1);
    let step = (end - start) / pieces;
    let handle = 4.0 / 3.0 * (step / 4.0).tan() * radius;
    let mut from = start;
    for _ in 0..count {
        let to = from + step;
        let start_point = on_circle(centre, radius, from);
        let end_point = on_circle(centre, radius, to);
        let leaving = (
            start_point.0 + handle * from.cos(),
            start_point.1 + handle * from.sin(),
        );
        let arriving = (
            end_point.0 - handle * to.cos(),
            end_point.1 - handle * to.sin(),
        );
        let _ = write!(
            d,
            " C{} {} {} {} {} {}",
            fmt_num(leaving.0),
            fmt_num(leaving.1),
            fmt_num(arriving.0),
            fmt_num(arriving.1),
            fmt_num(end_point.0),
            fmt_num(end_point.1)
        );
        from = to;
    }
}

/// A full circle path.
fn circle(cx: f64, cy: f64, radius: f64) -> String {
    let centre = (cx, cy);
    let top = on_circle(centre, radius, 0.0);
    let mut d = format!("M{} {}", fmt_num(top.0), fmt_num(top.1));
    arc(&mut d, centre, radius, 0.0, std::f64::consts::TAU);
    d.push_str(" Z");
    d
}

/// A pie slice (`hole == 0`) or a ring segment from `start` to `end`.
fn sector(centre: (f64, f64), outer: f64, hole: f64, start: f64, end: f64) -> String {
    let full = end - start >= std::f64::consts::TAU - 1e-9;
    let mut d = String::new();
    if full && hole <= 0.0 {
        return circle(centre.0, centre.1, outer);
    }
    let first = on_circle(centre, outer, start);
    if hole <= 0.0 {
        let _ = write!(
            d,
            "M{} {} L{} {}",
            fmt_num(centre.0),
            fmt_num(centre.1),
            fmt_num(first.0),
            fmt_num(first.1)
        );
    } else {
        let _ = write!(d, "M{} {}", fmt_num(first.0), fmt_num(first.1));
    }
    arc(&mut d, centre, outer, start, end);
    if hole > 0.0 {
        let inner_end = on_circle(centre, hole, end);
        let _ = write!(d, " L{} {}", fmt_num(inner_end.0), fmt_num(inner_end.1));
        arc(&mut d, centre, hole, end, start);
    }
    d.push_str(" Z");
    d
}

#[cfg(test)]
mod tests {
    use super::{nice_step, sector, Scale};

    #[test]
    fn nice_scales_have_four_to_twelve_ticks_and_cover_the_data() {
        for (min, max) in [
            (0.0, 7.3),
            (-3.0, 12.0),
            (5.0, 5.0),
            (0.001, 0.0042),
            (1e11, 1e12),
        ] {
            let scale = Scale::nice(min, max);
            let ticks = scale.ticks();
            assert!((2..=12).contains(&ticks.len()), "{min}..{max}: {ticks:?}");
            assert!(scale.lo <= min.min(max) + 1e-12 && scale.hi >= max.max(min) - 1e-9);
        }
        assert!((nice_step(0.7) - 1.0).abs() < 1e-12);
        assert!((nice_step(f64::NAN) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn zero_is_included_when_the_data_is_near_it() {
        let scale = Scale::over([3.0, 9.0].into_iter(), false);
        assert!(scale.lo.abs() < 1e-12);
        let far = Scale::over([100.0, 101.0].into_iter(), false);
        assert!(far.lo > 50.0);
    }

    #[test]
    fn sectors_use_only_move_line_cubic_and_close() {
        let d = sector((50.0, 50.0), 40.0, 20.0, 0.0, 4.0);
        assert!(d
            .chars()
            .filter(char::is_ascii_alphabetic)
            .all(|ch| "MLCZ".contains(ch)));
    }
}
