//! Two-column reading order for semantic conversion (AUD-85 / CORE-QUEUE P-7).
//!
//! A PDF has no column markers. The only honest signal is a **measured gutter**:
//! a horizontal band of empty page between two clusters of lines that overlap
//! vertically. Without that gap the page stays in draw order — inventing a
//! split from line spacing alone would shatter ordinary single-column pages —
//! except for a run of lines drawn later that stands wholly above the rest
//! ([`single_order`]).

use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_pdf::{Item, PdfPage};

use crate::report::{ConversionReport, Severity};

/// Minimum gutter width in points. Narrower gaps are ordinary word/indent space.
const MIN_GUTTER_PT: f64 = 18.0;
/// Gutter must also be at least this fraction of the page width.
const MIN_GUTTER_RATIO: f64 = 0.04;
/// Each column needs at least this many lines or the split is noise.
const MIN_LINES_PER_COLUMN: usize = 2;
/// Columns must share at least this fraction of the shorter column's height.
const MIN_VERTICAL_OVERLAP: f64 = 0.45;
/// Width ratio above this (wider/narrower) is "uneven" → unsupported.
const MAX_WIDTH_RATIO: f64 = 1.55;

/// What the geometry of a page's lines says about columns.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ColumnPlan {
    /// One column, or no clear gutter: keep the reader's line order.
    Single,
    /// Exactly two columns with a measured gutter; reorder left-then-right.
    Two {
        /// Index into the original `lines` slice for the left column, top→bottom.
        left: Vec<usize>,
        /// Index into the original `lines` slice for the right column, top→bottom.
        right: Vec<usize>,
        /// Lines that cross the gutter (titles, full-width headings, a centred
        /// page number), top→bottom. Each one closes a band: the columns above
        /// it are read before it, the columns below it after it.
        spanning: Vec<usize>,
        /// Left edge of the measured gutter, page points.
        #[allow(dead_code)]
        gutter_start: f64,
        /// Right edge of the measured gutter, page points.
        #[allow(dead_code)]
        gutter_end: f64,
    },
    /// Three-plus columns, uneven widths, or a floating picture in a column band.
    /// The page is left in draw order and the report carries the reason.
    Unsupported,
}

/// Analyses one page and records Unsupported / Inferred cases. Does not reorder.
pub(crate) fn plan(
    page: &PdfPage,
    lines: &[GlyphLine],
    report: &mut ConversionReport,
) -> ColumnPlan {
    let page_number = page.number;
    let (page_width, _) = page.geometry.displayed();
    if lines.len() < MIN_LINES_PER_COLUMN * 2 {
        return ColumnPlan::Single;
    }

    let Some(gutters) = find_gutters(lines, page_width) else {
        return ColumnPlan::Single;
    };

    if gutters.len() >= 2 {
        report.record(
            "convert.columns",
            Severity::Unsupported,
            format!(
                "page {page_number}: {} column gutters detected; only two-column pages are reordered",
                gutters.len() + 1
            ),
        );
        return ColumnPlan::Unsupported;
    }

    let Some(&(gutter_start, gutter_end)) = gutters.first() else {
        return ColumnPlan::Single;
    };
    let mut left: Vec<usize> = Vec::new();
    let mut right: Vec<usize> = Vec::new();
    let mut spanning: Vec<usize> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let right_edge = line.x + line.width;
        if right_edge <= gutter_start + 0.5 {
            left.push(index);
        } else if line.x >= gutter_end - 0.5 {
            right.push(index);
        } else {
            spanning.push(index);
        }
    }

    if left.len() < MIN_LINES_PER_COLUMN || right.len() < MIN_LINES_PER_COLUMN {
        return ColumnPlan::Single;
    }

    if !vertical_overlap_ok(lines, &left, &right) {
        return ColumnPlan::Single;
    }

    let left_width = column_width(lines, &left);
    let right_width = column_width(lines, &right);
    let wider = left_width.max(right_width);
    let narrower = left_width.min(right_width).max(1.0);
    if wider / narrower > MAX_WIDTH_RATIO {
        report.record(
            "convert.columns",
            Severity::Unsupported,
            format!(
                "page {page_number}: column widths {left_width:.1} pt and {right_width:.1} pt are uneven"
            ),
        );
        return ColumnPlan::Unsupported;
    }

    if page_has_floating_image(page, gutter_start, gutter_end) {
        report.record(
            "convert.columns",
            Severity::Unsupported,
            format!(
                "page {page_number}: a floating image sits in a two-column band; reading order is not rewritten"
            ),
        );
        return ColumnPlan::Unsupported;
    }

    sort_by_baseline(lines, &mut left);
    sort_by_baseline(lines, &mut right);
    sort_by_baseline(lines, &mut spanning);

    report.record(
        "convert.columns",
        Severity::Inferred,
        format!(
            "page {page_number}: two columns with a {gap:.1} pt gutter at x={gutter_start:.1}..{gutter_end:.1}",
            gap = gutter_end - gutter_start
        ),
    );

    ColumnPlan::Two {
        left,
        right,
        spanning,
        gutter_start,
        gutter_end,
    }
}

/// Reorders lines into reading order, or returns them unchanged.
///
/// Spanning lines cut the page into horizontal bands; within a band the left
/// column is read before the right one. A title therefore stays on top, a
/// full-width heading between two column blocks stays between them, and a page
/// number under the columns stays last.
#[must_use]
pub(crate) fn apply(lines: Vec<GlyphLine>, column_plan: &ColumnPlan) -> Vec<GlyphLine> {
    match column_plan {
        // A page whose columns are not read here keeps its draw order too,
        // except for what stands wholly above the rest: a contents page's
        // numbers made it "two uneven columns", and its title, drawn last,
        // came after its entries.
        ColumnPlan::Single | ColumnPlan::Unsupported => single_order(lines),
        ColumnPlan::Two {
            left,
            right,
            spanning,
            ..
        } => {
            let baseline = |index: usize| lines.get(index).map_or(f64::INFINITY, |l| l.baseline);
            let mut order = Vec::with_capacity(lines.len());
            let mut left_rest = left.iter().copied().peekable();
            let mut right_rest = right.iter().copied().peekable();
            for &cut in spanning {
                let limit = baseline(cut);
                while let Some(index) = left_rest.next_if(|&index| baseline(index) < limit) {
                    order.push(index);
                }
                while let Some(index) = right_rest.next_if(|&index| baseline(index) < limit) {
                    order.push(index);
                }
                order.push(cut);
            }
            order.extend(left_rest);
            order.extend(right_rest);

            let mut claimed = vec![false; lines.len()];
            let mut out = Vec::with_capacity(lines.len());
            for index in order {
                if let (Some(line), Some(slot)) = (lines.get(index), claimed.get_mut(index)) {
                    if !*slot {
                        *slot = true;
                        out.push(line.clone());
                    }
                }
            }
            for (index, line) in lines.into_iter().enumerate() {
                if !claimed.get(index).copied().unwrap_or(true) {
                    out.push(line);
                }
            }
            out
        }
    }
}

/// Finds clear horizontal gutters between text clusters.
///
/// A gutter is a band at least `min_gap` wide that almost no line crosses.
/// "Almost": a title, a full-width heading or a centred page number crosses
/// the gutter of a two-column page, and merging every line's extent (the
/// previous rule) let any one of them close the gutter, so such pages were
/// never reordered. Up to [`crossing_allowance`] lines may cross; they become
/// the plan's `spanning` lines.
fn find_gutters(lines: &[GlyphLine], page_width: f64) -> Option<Vec<(f64, f64)>> {
    let min_gap = MIN_GUTTER_PT.max(page_width * MIN_GUTTER_RATIO);
    let intervals: Vec<(f64, f64)> = lines
        .iter()
        .filter(|line| line.width > 0.0 && line.x.is_finite() && line.width.is_finite())
        .map(|line| (line.x, line.x + line.width))
        .collect();
    if intervals.len() < 2 {
        return None;
    }
    let allowance = crossing_allowance(intervals.len());

    // Coverage as a step function: (from, to, lines covering that span).
    let mut events: Vec<(f64, i32)> = Vec::with_capacity(intervals.len() * 2);
    for &(start, end) in &intervals {
        events.push((start, 1));
        events.push((end, -1));
    }
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut spans: Vec<(f64, f64, i32)> = Vec::new();
    let mut coverage = 0_i32;
    let mut at = events.first()?.0;
    for (x, delta) in events {
        if x > at {
            spans.push((at, x, coverage));
            at = x;
        }
        coverage += delta;
    }

    // A gutter is a run of low-coverage spans with text on both sides; the
    // low runs before the first and after the last text are page margins.
    let mut gutters = Vec::new();
    let mut run_start: Option<f64> = None;
    let mut seen_text = false;
    for (from, _to, count) in spans {
        if count > allowance {
            if let Some(start) = run_start.take() {
                if seen_text {
                    push_gutter(&mut gutters, start, from, min_gap, page_width);
                }
            }
            seen_text = true;
        } else if run_start.is_none() {
            run_start = Some(from);
        }
    }
    if gutters.is_empty() {
        None
    } else {
        Some(gutters)
    }
}

/// How many lines may cross a gutter: about one in eight. A short page (under
/// eight lines) allows none, which is the strict rule it always had; a full
/// two-column page of 40 lines tolerates a title, a heading and a page number.
fn crossing_allowance(line_count: usize) -> i32 {
    i32::try_from(line_count / 8).unwrap_or(i32::MAX)
}

fn push_gutter(gutters: &mut Vec<(f64, f64)>, start: f64, end: f64, min_gap: f64, page_width: f64) {
    if end - start < min_gap {
        return;
    }
    // Gutters hugging the page edge are margins, not column splits.
    if start < page_width * 0.12 || end > page_width * 0.88 {
        return;
    }
    gutters.push((start, end));
}

fn column_width(lines: &[GlyphLine], indices: &[usize]) -> f64 {
    let mut left = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    for line in indices.iter().filter_map(|&index| lines.get(index)) {
        left = left.min(line.x);
        right = right.max(line.x + line.width);
    }
    (right - left).max(0.0)
}

fn vertical_overlap_ok(lines: &[GlyphLine], left: &[usize], right: &[usize]) -> bool {
    let Some((l0, l1)) = baseline_span(lines, left) else {
        return false;
    };
    let Some((r0, r1)) = baseline_span(lines, right) else {
        return false;
    };
    let overlap = (l1.min(r1) - l0.max(r0)).max(0.0);
    let shorter = (l1 - l0).min(r1 - r0).max(1.0);
    overlap / shorter >= MIN_VERTICAL_OVERLAP
}

fn baseline_span(lines: &[GlyphLine], indices: &[usize]) -> Option<(f64, f64)> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for line in indices.iter().filter_map(|&index| lines.get(index)) {
        let y = line.baseline;
        min = min.min(y);
        max = max.max(y);
    }
    if min.is_finite() && max.is_finite() {
        Some((min, max))
    } else {
        None
    }
}

fn sort_by_baseline(lines: &[GlyphLine], indices: &mut [usize]) {
    let baseline = |index: usize| lines.get(index).map_or(f64::INFINITY, |line| line.baseline);
    indices.sort_by(|a, b| {
        baseline(*a)
            .partial_cmp(&baseline(*b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
}

/// An image whose box is not confined to the gutter is a floating picture that
/// blocks reorder (CORE-QUEUE P-7 §3).
fn page_has_floating_image(page: &PdfPage, gutter_start: f64, gutter_end: f64) -> bool {
    for item in page.items() {
        let Item::Image(image) = item else {
            continue;
        };
        let image_left = image.x;
        let image_right = image.x + image.width;
        if image_right <= gutter_end && image_left >= gutter_start {
            continue;
        }
        return true;
    }
    false
}

/// A page's lines in draw order, with what was drawn later but stands wholly
/// above moved up to where it is read: a one-column page's, or a page whose
/// columns this module does not reorder.
///
/// The draw order is cut into runs that go down the page; a run starts where a
/// line stands more than a line above the one drawn before it. A run is moved
/// before the runs it lies **entirely** above - a title drawn after the table
/// of contents under it, a heading Google Docs drew last - and is otherwise
/// left in draw order: runs that share a height are blocks side by side that
/// the column pass did not split, and draw order is how they are read.
fn single_order(lines: Vec<GlyphLine>) -> Vec<GlyphLine> {
    let mut runs: Vec<Vec<GlyphLine>> = Vec::new();
    for line in lines {
        let climbs = runs
            .last()
            .and_then(|run| run.last())
            .is_some_and(|previous| {
                previous.baseline - line.baseline > line.size.max(previous.size)
            });
        match runs.last_mut() {
            Some(run) if !climbs => run.push(line),
            _ => runs.push(vec![line]),
        }
    }
    if runs.len() < 2 {
        return runs.into_iter().flatten().collect();
    }
    // (top, bottom) of a run, in points from the page's top.
    let extent = |run: &[GlyphLine]| {
        run.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(top, bottom), line| {
                (
                    top.min(line.baseline - line.ascent),
                    bottom.max(line.baseline),
                )
            })
    };
    let mut placed: Vec<(f64, f64, Vec<GlyphLine>)> = Vec::with_capacity(runs.len());
    for run in runs {
        let (top, bottom) = extent(&run);
        // Before the first placed run that lies wholly below this one, when
        // every run after that point does too: one that shares its height
        // keeps the draw order.
        let at = placed
            .iter()
            .position(|(other_top, _, _)| *other_top > bottom)
            .filter(|&from| {
                placed
                    .get(from..)
                    .is_some_and(|rest| rest.iter().all(|(other_top, _, _)| *other_top > bottom))
            })
            .unwrap_or(placed.len());
        placed.insert(at, (top, bottom, run));
    }
    placed.into_iter().flat_map(|(_, _, run)| run).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A title drawn after the list under it is read first; blocks side by
    /// side keep the order they were drawn in.
    #[test]
    fn a_line_drawn_last_but_standing_above_is_read_first() {
        let order = |lines: Vec<GlyphLine>| -> Vec<f64> {
            single_order(lines)
                .iter()
                .map(|line| line.baseline)
                .collect()
        };
        // Contents entries at 200..260, then their title at 150.
        let toc = vec![
            line(72.0, 300.0, 200.0),
            line(72.0, 300.0, 220.0),
            line(72.0, 300.0, 240.0),
            line(72.0, 300.0, 260.0),
            line(72.0, 200.0, 150.0),
        ];
        assert_eq!(order(toc), [150.0, 200.0, 220.0, 240.0, 260.0]);
        // A left block 100..140 drawn before a right block 100..140.
        let blocks = vec![
            line(50.0, 200.0, 100.0),
            line(50.0, 200.0, 120.0),
            line(50.0, 200.0, 140.0),
            line(300.0, 200.0, 100.0),
            line(300.0, 200.0, 120.0),
            line(300.0, 200.0, 140.0),
        ];
        assert_eq!(order(blocks), [100.0, 120.0, 140.0, 100.0, 120.0, 140.0]);
        // A superscript drawn after its line, a little higher, stays after it.
        let sup = vec![line(72.0, 300.0, 200.0), line(372.0, 5.0, 196.0)];
        assert_eq!(order(sup), [200.0, 196.0]);
    }

    fn line(x: f64, width: f64, baseline: f64) -> GlyphLine {
        GlyphLine {
            x,
            baseline,
            size: 12.0,
            ascent: 9.0,
            width,
            glyphs: Vec::new(),
        }
    }

    #[test]
    fn a_clear_gutter_is_found() {
        let lines = vec![
            line(50.0, 200.0, 100.0),
            line(350.0, 200.0, 100.0),
            line(50.0, 200.0, 120.0),
            line(350.0, 200.0, 120.0),
            line(50.0, 200.0, 140.0),
            line(350.0, 200.0, 140.0),
        ];
        let gutters = find_gutters(&lines, 600.0).expect("gutter");
        assert_eq!(gutters.len(), 1);
        assert!(gutters[0].1 - gutters[0].0 >= MIN_GUTTER_PT);
    }

    #[test]
    fn ordinary_line_spacing_is_not_a_gutter() {
        let lines = vec![
            line(72.0, 400.0, 100.0),
            line(72.0, 400.0, 114.0),
            line(90.0, 380.0, 128.0),
            line(72.0, 400.0, 142.0),
        ];
        assert!(find_gutters(&lines, 612.0).is_none());
    }

    fn two_column_page_with(extra: &[GlyphLine]) -> Vec<GlyphLine> {
        let mut lines = Vec::new();
        for row in 0..8 {
            let baseline = 150.0 + f64::from(row) * 20.0;
            lines.push(line(50.0, 200.0, baseline));
            lines.push(line(350.0, 200.0, baseline));
        }
        lines.extend_from_slice(extra);
        lines
    }

    /// AUD-85 review: a full-width title used to merge both columns into one
    /// interval, so the "spanning" branch could never run.
    #[test]
    fn a_title_and_a_page_number_do_not_close_the_gutter() {
        let lines = two_column_page_with(&[
            line(50.0, 500.0, 100.0), // title across both columns
            line(290.0, 20.0, 780.0), // centred page number in the gutter
        ]);
        let gutters = find_gutters(&lines, 600.0).expect("gutter despite spanning lines");
        assert_eq!(gutters.len(), 1, "{gutters:?}");
        let (start, end) = gutters[0];
        assert!(start >= 250.0 - 1e-9 && end <= 350.0 + 1e-9, "{gutters:?}");
    }

    #[test]
    fn spanning_lines_cut_the_page_into_bands() {
        // Title, two rows of columns, a full-width heading, two more rows,
        // then a page number.
        let lines = vec![
            line(50.0, 500.0, 100.0),  // 0 title
            line(50.0, 200.0, 120.0),  // 1 L
            line(350.0, 200.0, 120.0), // 2 R
            line(50.0, 200.0, 140.0),  // 3 L
            line(350.0, 200.0, 140.0), // 4 R
            line(50.0, 500.0, 170.0),  // 5 heading
            line(50.0, 200.0, 190.0),  // 6 L
            line(350.0, 200.0, 190.0), // 7 R
            line(290.0, 20.0, 780.0),  // 8 page number
        ];
        let column_plan = ColumnPlan::Two {
            left: vec![1, 3, 6],
            right: vec![2, 4, 7],
            spanning: vec![0, 5, 8],
            gutter_start: 250.0,
            gutter_end: 350.0,
        };
        let order: Vec<(f64, f64)> = apply(lines, &column_plan)
            .iter()
            .map(|l| (l.x, l.baseline))
            .collect();
        assert_eq!(
            order,
            [
                (50.0, 100.0),
                (50.0, 120.0),
                (50.0, 140.0),
                (350.0, 120.0),
                (350.0, 140.0),
                (50.0, 170.0),
                (50.0, 190.0),
                (350.0, 190.0),
                (290.0, 780.0),
            ]
        );
    }

    #[test]
    fn apply_puts_left_column_before_right() {
        let lines = vec![
            line(50.0, 200.0, 100.0),
            line(350.0, 200.0, 100.0),
            line(50.0, 200.0, 120.0),
            line(350.0, 200.0, 120.0),
        ];
        let column_plan = ColumnPlan::Two {
            left: vec![0, 2],
            right: vec![1, 3],
            spanning: vec![],
            gutter_start: 250.0,
            gutter_end: 350.0,
        };
        let ordered = apply(lines, &column_plan);
        assert!((ordered[0].x - 50.0).abs() < f64::EPSILON);
        assert!((ordered[1].x - 50.0).abs() < f64::EPSILON);
        assert!((ordered[2].x - 350.0).abs() < f64::EPSILON);
        assert!((ordered[3].x - 350.0).abs() < f64::EPSILON);
    }
}
