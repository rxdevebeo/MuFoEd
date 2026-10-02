//! Where the ink is (`STAGE-4-RENDER-FIDELITY.md` S4F.5 B-3/R-1).
//!
//! **SSIM is nearly blind to sparse text that moved.** A line of text is a few
//! percent of a page's pixels; lose it, shift it, or leave a strip of it out and
//! the mean SSIM map barely moves, because most of every 11×11 window is white
//! in both images. So the gate has a second half, and this is it: ink coverage,
//! the best whole-page alignment, the ink centroid, the ink extent and the
//! row/column ink profiles.
//!
//! These are *rasterizer-independent* in the sense that matters: they are
//! computed from a gray level (`INK_LEVEL`), not from anything a renderer does
//! at the edge of a glyph. That is why the same bounds serve the SVG gate
//! (`resvg`) and the PDF gate (`hayro`) — what they compare is the ink's place
//! on the page, not the shape of the ink.

use crate::gray::{Gray, INK_LEVEL};

/// Ink pixels counted from a page edge before the content is considered to
/// have started.
///
/// The extent is where the page's content *begins*, not where the first
/// antialiased pixel lands, so a single stray pixel one row outside the real
/// content must not move it — a bound that tight would measure rasterizer
/// noise instead of layout.
///
/// The threshold is accumulated *across* rows rather than required within one.
/// A per-row threshold is blind to a vertical rule: a 3 px page border
/// contributes about three ink pixels to each of the thousand rows it runs
/// down, so a per-row filter discards it and a border running off the bottom of
/// the page looks exactly like one that stops at the right place. That is not
/// hypothetical — it is how `strict-stage5b` shipped a page border whose
/// vertical rules ran the full page height, past the horizontal ones, with every
/// structural bound satisfied. Forty pixels is a few glyph strokes: too much for
/// antialiasing, far less than any real rule or text line.
pub const MIN_EXTENT_INK_PX: usize = 40;

/// Minimum ink fraction for a page to count as having content.
pub const MIN_INK_FRACTION: f64 = 0.001;
/// Allowed candidate/reference ink ratio (catches a blank or over-inked page).
pub const MIN_INK_RATIO: f64 = 0.5;
/// Upper bound of the candidate/reference ink ratio.
pub const MAX_INK_RATIO: f64 = 2.0;
/// Maximum allowed alignment shift in px on either axis (2 passes, 3 fails).
pub const MAX_ALIGNMENT_SHIFT_PX: isize = 2;
/// Minimum Pearson correlation of the row-ink profiles.
pub const MIN_ROW_CORRELATION: f64 = 0.9;
/// Minimum Pearson correlation of the column-ink profiles.
pub const MIN_COLUMN_CORRELATION: f64 = 0.85;
/// Maximum allowed shift of the ink centroid in px (either axis).
pub const MAX_CENTROID_SHIFT_PX: f64 = 2.5;
/// Maximum allowed shift of the trimmed ink centroid in px (either axis).
pub const MAX_CONTENT_CENTROID_SHIFT_PX: f64 = 2.5;
/// Maximum allowed drift of the page's ink extent in px (either edge).
pub const MAX_EXTENT_DRIFT_PX: f64 = 2.0;

/// Extra pixels searched beyond [`StructuralLimits::max_shift_px`] when
/// looking for the best profile alignment.
///
/// The search range must strictly exceed the bound it is checked against,
/// otherwise the reported shift is clamped to the search window and a page that
/// drifted *further* than the bound can report a shift *inside* it — the check
/// then cannot fail. This margin is what makes the bound a real bound, and each
/// gate pins the invariant with a test over every limit set it holds to.
pub const ALIGNMENT_SEARCH_MARGIN_PX: isize = 8;

/// The alignment search range for `limits`: the bound plus the margin.
///
/// The reason is the reason for the margin: a drift beyond the bound has to
/// come out as a *rejection*, and it can only do that if the search that finds
/// the drift was allowed to look that far.
#[must_use]
pub fn alignment_search_range(limits: &StructuralLimits) -> isize {
    limits.max_shift_px + ALIGNMENT_SEARCH_MARGIN_PX
}

/// Structural (rasterizer-independent) fidelity of one page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructuralFidelity {
    /// Best vertical shift of the candidate relative to the reference.
    pub shift_y_px: isize,
    /// Best horizontal shift of the candidate relative to the reference.
    pub shift_x_px: isize,
    /// Correlation of the row-ink profiles at `shift_y_px`.
    pub correlation_y: f64,
    /// Correlation of the column-ink profiles at `shift_x_px`.
    pub correlation_x: f64,
    /// Signed horizontal shift of the ink centroid, in px.
    pub centroid_x_delta: f64,
    /// Signed vertical shift of the ink centroid, in px.
    pub centroid_y_delta: f64,
    /// Signed horizontal shift of the **trimmed** ink centroid, in px.
    ///
    /// See [`content_centroid`]: the plain centroid averages a page border in, and
    /// on a framed page that is the measurement rather than the layout.
    pub content_centroid_x_delta: f64,
    /// Signed vertical shift of the trimmed ink centroid, in px.
    pub content_centroid_y_delta: f64,
    /// Signed drift of the first substantial ink row, in px.
    pub top_delta: f64,
    /// Signed drift of the last substantial ink row, in px.
    pub bottom_delta: f64,
    /// Reference ink fraction.
    pub reference_ink: f64,
    /// Candidate ink fraction.
    pub candidate_ink: f64,
}

impl StructuralFidelity {
    /// The ink edges of this measurement, as `(top, bottom)`.
    #[must_use]
    pub fn extents(&self) -> (f64, f64) {
        (self.top_delta, self.bottom_delta)
    }
}

/// Structural bounds for [`structural_fidelity_with`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructuralLimits {
    /// Maximum tolerated best-alignment shift, in px.
    pub max_shift_px: isize,
    /// Maximum tolerated ink-centroid drift, in px.
    pub max_centroid_px: f64,
    /// Maximum tolerated **trimmed** ink-centroid drift, in px.
    ///
    /// A bound of its own, not a copy of [`StructuralLimits::max_centroid_px`]:
    /// the two measures disagree exactly where the plain one is least useful (a
    /// page whose ink is a frame), and that is the whole reason both exist.
    pub max_content_centroid_px: f64,
    /// Minimum row-ink profile correlation.
    pub min_row_correlation: f64,
    /// Minimum column-ink profile correlation.
    pub min_column_correlation: f64,
    /// Maximum tolerated drift of the ink extent, in px (either edge).
    pub max_extent_px: f64,
}

impl Default for StructuralLimits {
    fn default() -> Self {
        Self {
            max_shift_px: MAX_ALIGNMENT_SHIFT_PX,
            max_centroid_px: MAX_CENTROID_SHIFT_PX,
            max_content_centroid_px: MAX_CONTENT_CENTROID_SHIFT_PX,
            min_row_correlation: MIN_ROW_CORRELATION,
            min_column_correlation: MIN_COLUMN_CORRELATION,
            max_extent_px: MAX_EXTENT_DRIFT_PX,
        }
    }
}

impl StructuralLimits {
    /// These bounds with every one of them lifted.
    ///
    /// A gate that has to prove *which* bound rejects a page needs a way to turn
    /// the others off; otherwise the demonstration passes for the wrong reason,
    /// which is how a bound gets tightened back to being decorative.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            max_shift_px: 4096,
            max_centroid_px: f64::MAX,
            max_content_centroid_px: f64::MAX,
            min_row_correlation: 0.0,
            min_column_correlation: 0.0,
            max_extent_px: f64::MAX,
        }
    }
}

/// Checks a page structurally: similar ink coverage, no vertical or horizontal
/// drift and well-correlated row/column ink profiles.
///
/// Complements SSIM, which barely penalises a missing or shifted sparse text
/// layer (`S4F-REWORK-2` B-3/R-1). Deterministic and rasterizer independent.
///
/// # Errors
///
/// Returns a human-readable reason when any structural bound is violated, with
/// every measurement that produced it.
pub fn structural_fidelity(
    reference: &Gray,
    candidate: &Gray,
) -> Result<StructuralFidelity, String> {
    structural_fidelity_with(reference, candidate, &StructuralLimits::default())
}

/// Like [`structural_fidelity`] but with explicit [`StructuralLimits`].
///
/// # Errors
///
/// See [`structural_fidelity`].
/// Checks a page structurally: similar ink coverage, no vertical or horizontal
/// drift and well-correlated row/column ink profiles.
///
/// Complements SSIM, which barely penalises a missing or shifted sparse text
/// layer (`S4F-REWORK-2` B-3/R-1). Deterministic and rasterizer independent.
///
/// The position bounds and the correlation bounds are checked in two private
/// helpers, so that neither list is lost inside a hundred-line function; this one
/// takes every measurement first, so that whichever bound rejects a page, the
/// failure carries all the numbers that produced it.
///
/// # Errors
///
/// Returns the first violated bound, with every measurement in the message.
pub fn structural_fidelity_with(
    reference: &Gray,
    candidate: &Gray,
    limits: &StructuralLimits,
) -> Result<StructuralFidelity, String> {
    let reference_ink = ink_fraction(reference.pixels());
    let candidate_ink = ink_fraction(candidate.pixels());
    if reference_ink <= MIN_INK_FRACTION {
        return Err(format!("reference ink {reference_ink:.5} is empty"));
    }
    if candidate_ink < reference_ink * MIN_INK_RATIO {
        return Err(format!(
            "candidate ink {candidate_ink:.5} << reference {reference_ink:.5}"
        ));
    }
    if candidate_ink > reference_ink * MAX_INK_RATIO {
        return Err(format!(
            "candidate ink {candidate_ink:.5} >> reference {reference_ink:.5}"
        ));
    }
    let (shift_y, correlation_y) = best_profile_alignment(
        &row_ink_profile(reference),
        &row_ink_profile(candidate),
        alignment_search_range(limits),
    );
    let correlation_y = correlation_y.ok_or_else(|| "no row structure to compare".to_owned())?;
    let (shift_x, correlation_x) = best_profile_alignment(
        &column_ink_profile(reference),
        &column_ink_profile(candidate),
        alignment_search_range(limits),
    );
    let correlation_x = correlation_x.ok_or_else(|| "no column structure to compare".to_owned())?;
    let (centroid_x_delta, centroid_y_delta) = centroid_delta(reference, candidate);
    let (content_x_delta, content_y_delta) = content_centroid_delta(reference, candidate);
    // The extent is the outermost *substantial* ink on each edge. Unlike the
    // centroid it does not average over the page: a block that is a whole line
    // too tall moves the last ink row without moving the mean, which is exactly
    // the defect the centroid tolerates.
    let top_delta = extent_delta(reference, candidate, Edge::Top);
    let bottom_delta = extent_delta(reference, candidate, Edge::Bottom);
    // Every measurement is taken before any bound is judged, so a failure
    // carries the numbers that produced it. Naming only the violated bound is
    // what left the Stage-5C display-block defect invisible: the class limits
    // were fitted to whichever page failed first, and the log never said how
    // far the others were out.
    let measured = |violation: String| {
        format!(
            "{violation} [measured: top {top_delta:+.0}px, bottom {bottom_delta:+.0}px, \
             centroid ({centroid_x_delta:.2},{centroid_y_delta:.2}), \
             content centroid ({content_x_delta:.2},{content_y_delta:.2}), \
             shift ({shift_x}px,{shift_y}px), corr_y {correlation_y:.3}, corr_x {correlation_x:.3}]"
        )
    };
    if let Some(violation) = check_correlation(
        (shift_y, shift_x, correlation_y, correlation_x),
        limits,
        &measured,
    ) {
        return Err(violation);
    }
    if let Some(violation) = check_position(
        Position {
            top_delta,
            bottom_delta,
            centroid: (centroid_x_delta, centroid_y_delta),
            content: (content_x_delta, content_y_delta),
        },
        limits,
        &measured,
    ) {
        return Err(violation);
    }
    Ok(StructuralFidelity {
        shift_y_px: shift_y,
        shift_x_px: shift_x,
        correlation_y,
        correlation_x,
        centroid_x_delta,
        centroid_y_delta,
        content_centroid_x_delta: content_x_delta,
        content_centroid_y_delta: content_y_delta,
        top_delta,
        bottom_delta,
        reference_ink,
        candidate_ink,
    })
}

/// One line of the numbers a gate prints per page, whatever produced them.
///
/// Both gates print this, so a regression is read the same way whether the
/// candidate came from the SVG backend or from a rasterized PDF.
#[must_use]
pub fn measurement_line(page: usize, structure: &StructuralFidelity) -> String {
    format!(
        "  page {page}: dy={}px corr_y={:.3} | dx={}px corr_x={:.3} | \
         centroid d=({:.2},{:.2}) trimmed d=({:.2},{:.2}) | edge d=({:+.0},{:+.0}) | \
         ink ref={:.5} cand={:.5}",
        structure.shift_y_px,
        structure.correlation_y,
        structure.shift_x_px,
        structure.correlation_x,
        structure.centroid_x_delta,
        structure.centroid_y_delta,
        structure.content_centroid_x_delta,
        structure.content_centroid_y_delta,
        structure.top_delta,
        structure.bottom_delta,
        structure.reference_ink,
        structure.candidate_ink
    )
}

/// Where the page's ink ended up, in px, candidate relative to reference.
#[derive(Clone, Copy, Debug)]
struct Position {
    top_delta: f64,
    bottom_delta: f64,
    centroid: (f64, f64),
    content: (f64, f64),
}

/// The alignment and correlation bounds.
///
/// # Errors
///
/// Returns the first violated bound, already carrying `measured`'s numbers.
fn check_correlation(
    (shift_y, shift_x, correlation_y, correlation_x): (isize, isize, f64, f64),
    limits: &StructuralLimits,
    measured: &impl Fn(String) -> String,
) -> Option<String> {
    if shift_y.abs() > limits.max_shift_px {
        return Some(measured(format!(
            "vertical shift {shift_y}px exceeds tolerance"
        )));
    }
    if correlation_y < limits.min_row_correlation {
        return Some(measured(format!(
            "row-ink correlation {correlation_y:.3} too low"
        )));
    }
    if shift_x.abs() > limits.max_shift_px {
        return Some(measured(format!(
            "horizontal shift {shift_x}px exceeds tolerance"
        )));
    }
    if correlation_x < limits.min_column_correlation {
        return Some(measured(format!(
            "column-ink correlation {correlation_x:.3} too low"
        )));
    }
    None
}

/// The extent and centroid bounds.
///
/// # Errors
///
/// Returns the first violated bound, already carrying `measured`'s numbers.
fn check_position(
    position: Position,
    limits: &StructuralLimits,
    measured: &impl Fn(String) -> String,
) -> Option<String> {
    let Position {
        top_delta,
        bottom_delta,
        centroid: (centroid_x, centroid_y),
        content: (content_x, content_y),
    } = position;
    if top_delta.abs() > limits.max_extent_px {
        return Some(measured(format!(
            "top ink edge drifted {top_delta:.2}px (bound {:.2})",
            limits.max_extent_px
        )));
    }
    if bottom_delta.abs() > limits.max_extent_px {
        return Some(measured(format!(
            "bottom ink edge drifted {bottom_delta:.2}px (bound {:.2})",
            limits.max_extent_px
        )));
    }
    if centroid_x.abs() > limits.max_centroid_px {
        return Some(measured(format!(
            "horizontal centroid shift {centroid_x:.2}px exceeds tolerance"
        )));
    }
    if centroid_y.abs() > limits.max_centroid_px {
        return Some(measured(format!(
            "vertical centroid shift {centroid_y:.2}px exceeds tolerance"
        )));
    }
    if content_x.abs() > limits.max_content_centroid_px {
        return Some(measured(format!(
            "horizontal content centroid shift {content_x:.2}px exceeds tolerance \
             (bound {:.2})",
            limits.max_content_centroid_px
        )));
    }
    if content_y.abs() > limits.max_content_centroid_px {
        return Some(measured(format!(
            "vertical content centroid shift {content_y:.2}px exceeds tolerance \
             (bound {:.2})",
            limits.max_content_centroid_px
        )));
    }
    None
}

/// Which page edge an [`extent_delta`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The first substantial ink row.
    Top,
    /// The last substantial ink row.
    Bottom,
}

/// The first and last row where a page's content starts, or `None` for a blank
/// page.
///
/// Rows are counted cumulatively from each edge; see [`MIN_EXTENT_INK_PX`]. A
/// page too sparse to reach the threshold falls back to its outermost ink pixel
/// rather than reporting no extent.
#[must_use]
pub fn ink_rows(gray: &Gray) -> Option<(usize, usize)> {
    let (width, height) = (gray.width(), gray.height());
    let row_ink: Vec<usize> = (0..height)
        .map(|row| {
            (0..width)
                .filter(|&col| gray.at(col, row) < INK_LEVEL)
                .count()
        })
        .collect();
    let outermost = || {
        let first = row_ink.iter().position(|&ink| ink > 0)?;
        let last = row_ink.iter().rposition(|&ink| ink > 0)?;
        Some((first, last))
    };
    let mut from_top = 0usize;
    let mut top = None;
    let mut from_bottom = 0usize;
    let mut bottom = None;
    for row in 0..height {
        if top.is_none() {
            from_top += row_ink[row];
            if from_top >= MIN_EXTENT_INK_PX {
                top = Some(row);
            }
        }
        let reversed = height - 1 - row;
        if bottom.is_none() {
            from_bottom += row_ink[reversed];
            if from_bottom >= MIN_EXTENT_INK_PX {
                bottom = Some(reversed);
            }
        }
        if top.is_some() && bottom.is_some() {
            break;
        }
    }
    match (top, bottom) {
        (Some(top), Some(bottom)) => Some((top, bottom)),
        _ => outermost(),
    }
}

/// Signed drift of one ink edge, `candidate - reference`, in px.
///
/// Zero when either page has no substantial ink; the ink-ratio check in
/// [`structural_fidelity_with`] has already rejected a blank page by this point.
#[must_use]
pub fn extent_delta(reference: &Gray, candidate: &Gray, edge: Edge) -> f64 {
    let pick = |page: &Gray| match edge {
        Edge::Top => ink_rows(page).map(|rows| rows.0),
        Edge::Bottom => ink_rows(page).map(|rows| rows.1),
    };
    match (pick(reference), pick(candidate)) {
        (Some(reference_edge), Some(candidate_edge)) => {
            candidate_edge as f64 - reference_edge as f64
        }
        _ => 0.0,
    }
}

/// Mean `(x, y)` of the ink pixels, or `None` when the page has no ink.
#[must_use]
pub fn ink_centroid(gray: &Gray) -> Option<(f64, f64)> {
    let (width, height) = (gray.width(), gray.height());
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;
    for row in 0..height {
        for col in 0..width {
            if gray.at(col, row) < INK_LEVEL {
                sum_x += col as f64;
                sum_y += row as f64;
                count += 1;
            }
        }
    }
    (count > 0).then(|| (sum_x / count as f64, sum_y / count as f64))
}

/// The fraction of the ink box one side of a page frame must carry.
///
/// Nine tenths. This is the only threshold in the measure, and the four other
/// conditions — a solid top, a solid bottom, a solid left, a solid right, and a
/// **hollow** interior — are what make it safe: measured, a threshold alone
/// deletes every line of a full-width paragraph, and no paragraph has a leftmost
/// ink column that is solid from its first line to its last.
///
/// On a page with no frame the hollow test fails, nothing is excluded, and this
/// measure is [`ink_centroid`] — which
/// the unit test `a_page_without_a_frame_measures_the_same_as_a_plain_centroid`
/// pins.
pub const RULE_COVERAGE: f64 = 0.9;

/// The centroid of a page's **content**: its ink with the page's rules left out, a
/// rule being an ink row or ink column carrying [`RULE_COVERAGE`] of the page.
///
/// Returns `None` when nothing but rules remains.
///
/// **Why this exists, and why the plain centroid is not the fix.**
/// [`ink_centroid`] averages every ink pixel, so on a page whose ink is mostly a
/// page border the mean says where the *border* is. The border is symmetric, so
/// its centroid sits near the middle of the page, and two rasterizers that
/// antialias a long thin stroke differently disagree about one pixel of it — with
/// a lever arm of half a page. On `strict-stage5b` that is the whole of the
/// disagreement: the plain centroids differ by (25, 39) px, and the ink inside the
/// border agrees to (3.5, 0.8).
///
/// Two cheaper fixes were tried and **measured not to work**, which is why they
/// are named here rather than proposed again:
///
/// - a *rank* trim, dropping the outermost 5 % of the ink on each axis, makes it
///   worse: (27.7, 43.0). The rules are 30 % of that page's ink, not 5 %, so a 5 %
///   trim removes none of them while still eating real glyphs — and a 30 % trim
///   does remove the rules, by which point it is averaging three shapes and
///   calling it a page.
/// - cropping a fixed margin off the page works only at about 40 px on that
///   document, which is that document's margin rather than a property of anything.
///
/// Dropping the rules needs no margin and no threshold on a fraction of ink: it
/// asks whether a row is a rule, which is a question about that row. On a page
/// with no rules the answer is no everywhere and this measure is
/// [`ink_centroid`] again.
#[must_use]
pub fn content_centroid(gray: &Gray) -> Option<(f64, f64)> {
    let (width, height) = (gray.width(), gray.height());
    let inked = |row: usize, col: usize| gray.at(col, row) < INK_LEVEL;
    let mut kept = vec![false; width * height];
    for row in 0..height {
        for col in 0..width {
            kept[row * width + col] = inked(row, col);
        }
    }

    let (top, bottom, left, right) = ink_bounds(gray)?;
    let columns = right - left + 1;
    let rows = bottom - top + 1;
    let solid_row = |row: usize| {
        is_solid(
            (left..=right).filter(|&col| inked(row, col)).count(),
            columns,
        )
    };
    // A column is solid over what the rows left, so a column that is only long
    // because a horizontal rule crosses it is not itself a frame side.
    let solid_column = |col: usize, kept: &[bool]| {
        is_solid(
            (top..=bottom)
                .filter(|&row| kept[row * width + col])
                .count(),
            rows,
        )
    };

    // A frame is a **hollow rectangle**: all four sides solid, the inside not.
    //
    // Any weaker test takes text with it, and the measurements are why. "The
    // widest lines are rules" excludes every full-width line of a paragraph —
    // measured: a text page's rows covered 100 % of the ink box and all its
    // content vanished. "The outermost lines are rules" does not help either,
    // because a paragraph's first and last line *are* its outermost. What a
    // paragraph cannot have is a leftmost ink column that is solid from the top
    // line to the bottom, together with a hollow interior: a table of rules is
    // what that is, and a page border is the degenerate table.
    let middle = top + rows / 2;
    let hollow = middle > top && middle < bottom && !solid_row(middle);
    if !(hollow
        && solid_row(top)
        && solid_row(bottom)
        && solid_column(left, &kept)
        && solid_column(right, &kept))
    {
        return ink_centroid(gray);
    }

    for (from, inward) in [(top, true), (bottom, false)] {
        let mut row = from;
        loop {
            if !solid_row(row) {
                break;
            }
            for col in 0..width {
                kept[row * width + col] = false;
            }
            if inward {
                if row + 1 > bottom {
                    break;
                }
                row += 1;
            } else {
                if row == 0 || row <= top {
                    break;
                }
                row -= 1;
            }
        }
    }
    for (from, inward) in [(left, true), (right, false)] {
        let mut col = from;
        loop {
            if !solid_column(col, &kept) {
                break;
            }
            for row in 0..height {
                kept[row * width + col] = false;
            }
            if inward {
                if col + 1 > right {
                    break;
                }
                col += 1;
            } else {
                if col == 0 || col <= left {
                    break;
                }
                col -= 1;
            }
        }
    }

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;
    for row in 0..height {
        for col in 0..width {
            if kept[row * width + col] {
                sum_x += col as f64;
                sum_y += row as f64;
                count += 1;
            }
        }
    }
    (count > 0).then(|| (sum_x / count as f64, sum_y / count as f64))
}

/// The first and last row and column carrying any ink, as `(top, bottom, left,
/// right)`.
///
/// Min and max, not first-seen: the walk is row-major, so the last row with ink
/// gives `bottom` but gives a *smaller* `right` than the truth whenever a lower row
/// has narrower ink than one above it — which is exactly what a frame thickened on
/// its right edge looks like, and it left that edge outside the box.
fn ink_bounds(gray: &Gray) -> Option<(usize, usize, usize, usize)> {
    let (width, height) = (gray.width(), gray.height());
    let mut top: Option<usize> = None;
    let mut bottom = 0usize;
    let mut left: Option<usize> = None;
    let mut right = 0usize;
    for row in 0..height {
        for col in 0..width {
            if gray.at(col, row) >= INK_LEVEL {
                continue;
            }
            top.get_or_insert(row);
            bottom = row;
            left = Some(left.map_or(col, |seen| seen.min(col)));
            right = right.max(col);
        }
    }
    Some((top?, bottom, left?, right))
}

/// Whether `inked` of `of` pixels make this line solid enough to be a rule.
fn is_solid(inked: usize, of: usize) -> bool {
    f64::from(u32::try_from(inked).unwrap_or(u32::MAX)) >= RULE_COVERAGE * of as f64
}

/// Signed **content** centroid shift `candidate - reference` on `(x, y)`.
///
/// Zero when either page has no content once its rules are dropped; the ink-ratio
/// check in [`structural_fidelity_with`] has already rejected a blank page by this
/// point.
#[must_use]
pub fn content_centroid_delta(reference: &Gray, candidate: &Gray) -> (f64, f64) {
    match (content_centroid(reference), content_centroid(candidate)) {
        (Some((rx, ry)), Some((cx, cy))) => (cx - rx, cy - ry),
        _ => (0.0, 0.0),
    }
}

/// Signed centroid shift `candidate - reference` on `(x, y)`.
#[must_use]
pub fn centroid_delta(reference: &Gray, candidate: &Gray) -> (f64, f64) {
    match (ink_centroid(reference), ink_centroid(candidate)) {
        (Some((rx, ry)), Some((cx, cy))) => (cx - rx, cy - ry),
        _ => (0.0, 0.0),
    }
}

/// Fraction of the page covered by ink.
#[must_use]
pub fn ink_fraction(gray: &[f64]) -> f64 {
    let dark = gray.iter().filter(|value| **value < INK_LEVEL).count();
    dark as f64 / gray.len() as f64
}

/// Per-row ink fraction, used for vertical-alignment comparison.
#[must_use]
pub fn row_ink_profile(gray: &Gray) -> Vec<f64> {
    let (width, height) = (gray.width(), gray.height());
    (0..height)
        .map(|row| {
            let dark = (0..width)
                .filter(|&col| gray.at(col, row) < INK_LEVEL)
                .count();
            dark as f64 / width as f64
        })
        .collect()
}

/// Per-column ink fraction, used for horizontal-alignment comparison.
#[must_use]
pub fn column_ink_profile(gray: &Gray) -> Vec<f64> {
    let (width, height) = (gray.width(), gray.height());
    (0..width)
        .map(|col| {
            let dark = (0..height)
                .filter(|&row| gray.at(col, row) < INK_LEVEL)
                .count();
            dark as f64 / height as f64
        })
        .collect()
}

/// Pearson correlation of two equal-length samples, or `None` if either is
/// constant (no structure to correlate).
fn pearson(left: &[f64], right: &[f64]) -> Option<f64> {
    let n = left.len() as f64;
    let mean_left = left.iter().sum::<f64>() / n;
    let mean_right = right.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_left = 0.0;
    let mut var_right = 0.0;
    for (a, b) in left.iter().zip(right) {
        let da = a - mean_left;
        let db = b - mean_right;
        cov += da * db;
        var_left += da * da;
        var_right += db * db;
    }
    if var_left <= 0.0 || var_right <= 0.0 {
        return None;
    }
    Some(cov / (var_left.sqrt() * var_right.sqrt()))
}

/// Finds the shift of `candidate` relative to `reference` that best aligns their
/// ink profiles, returning `(shift_px, correlation)`.
///
/// A positive shift means the candidate content sits lower/further right. Pass
/// [`row_ink_profile`] for the vertical axis and [`column_ink_profile`] for the
/// horizontal axis. Returns `(0, None)` when either profile has no structure.
#[must_use]
pub fn best_profile_alignment(
    reference: &[f64],
    candidate: &[f64],
    max_shift: isize,
) -> (isize, Option<f64>) {
    let n = reference.len() as isize;
    let mut best_shift = 0isize;
    let mut best_corr: Option<f64> = None;
    for shift in -max_shift..=max_shift {
        let start = (-shift).max(0);
        let end = n - shift.max(0);
        if start >= end {
            continue;
        }
        let left = &reference[start as usize..end as usize];
        let right = &candidate[(start + shift) as usize..(end + shift) as usize];
        if let Some(corr) = pearson(left, right) {
            let better = match best_corr {
                None => true,
                Some(current) => corr > current,
            };
            if better {
                best_corr = Some(corr);
                best_shift = shift;
            }
        }
    }
    (best_shift, best_corr)
}

#[cfg(test)]
mod tests {
    use super::{
        best_profile_alignment, centroid_delta, extent_delta, ink_centroid, ink_rows,
        structural_fidelity, structural_fidelity_with, StructuralLimits, MAX_ALIGNMENT_SHIFT_PX,
    };
    use crate::gray::Gray;

    /// Builds a tiny text-like page: two dark rows of "ink", shifted by
    /// `(dy, dx)` px.
    fn text_like(width: usize, height: usize, rows: [usize; 2], dx: isize) -> Gray {
        let mut pixels = vec![1.0; width * height];
        for row in rows {
            for col in 0..width {
                let source = col as isize - dx;
                if source >= 10 && (source as usize) < width - 10 {
                    pixels[row * width + col] = 0.0;
                }
            }
        }
        Gray::new(width, height, pixels)
    }

    #[test]
    fn the_default_bounds_reject_blank_and_shifted_pages() {
        let (width, height) = (64, 96);
        let reference = text_like(width, height, [20, 60], 0);
        // Identical candidate passes.
        assert!(structural_fidelity(&reference, &reference).is_ok());
        // A blank page is rejected (ink ratio).
        let blank = Gray::filled(width, height, 1.0);
        assert!(structural_fidelity(&reference, &blank).is_err());
        // A 3px vertical shift is rejected, a 2px one is not.
        assert!(structural_fidelity(&reference, &text_like(width, height, [23, 63], 0)).is_err());
        assert!(structural_fidelity(&reference, &text_like(width, height, [22, 62], 0)).is_ok());
        // The same horizontally (R-1).
        assert!(structural_fidelity(&reference, &text_like(width, height, [20, 60], 3)).is_err());
        assert!(structural_fidelity(&reference, &text_like(width, height, [20, 60], 2)).is_ok());
    }

    #[test]
    fn the_shift_bound_is_two_pixels_by_default() {
        assert_eq!(
            StructuralLimits::default().max_shift_px,
            MAX_ALIGNMENT_SHIFT_PX
        );
    }

    #[test]
    fn a_failure_carries_the_numbers_that_produced_it() {
        let reference = text_like(64, 96, [20, 60], 0);
        let error = structural_fidelity(&reference, &text_like(64, 96, [30, 70], 0))
            .expect_err("a 10px drift is out of bounds");
        assert!(error.contains("measured:"), "{error}");
        assert!(error.contains("corr_y"), "{error}");
    }

    #[test]
    fn the_extent_check_sees_drift_the_centroid_does_not() {
        let reference = text_like(64, 96, [20, 70], 0);
        // Stretched about the middle: the two halves cancel in the centroid,
        // both ink edges move.
        let stretched = reference.stretched_vertically(5);
        let (_delta_x, delta_y) = centroid_delta(&reference, &stretched);
        assert!(delta_y.abs() < 1.0, "the centroid barely moves: {delta_y}");
        assert!(
            extent_delta(&reference, &stretched, super::Edge::Top).abs() >= 5.0,
            "the top edge moved"
        );
        assert!(
            extent_delta(&reference, &stretched, super::Edge::Bottom).abs() >= 5.0,
            "the bottom edge moved"
        );
        // With every bound lifted the page passes, and with only the extent
        // bound in force it is that bound which rejects it.
        let unbounded = StructuralLimits::unbounded();
        assert!(structural_fidelity_with(&reference, &stretched, &unbounded).is_ok());
        let just_extent = StructuralLimits {
            max_extent_px: 2.0,
            ..unbounded
        };
        let error = structural_fidelity_with(&reference, &stretched, &just_extent)
            .expect_err("the extent bound must reject the stretch");
        assert!(error.contains("ink edge"), "{error}");
    }

    #[test]
    fn a_vertical_rule_is_found_by_the_extent() {
        // One narrow column of ink repeated down every row: no single row is
        // ever substantial on its own, so a per-row filter would call the page
        // blank at both ends and a border running off the bottom of the sheet
        // would look like one that stopped at the right place. Accumulated
        // across rows, forty of them are enough — and the extent starts where
        // they add up rather than where the first antialiased pixel is.
        let (width, height) = (64, 96);
        let mut pixels = vec![1.0; width * height];
        for row in 0..height {
            pixels[row * width + 30] = 0.0;
        }
        let rule = Gray::new(width, height, pixels);
        assert_eq!(
            ink_rows(&rule),
            Some((
                super::MIN_EXTENT_INK_PX - 1,
                height - super::MIN_EXTENT_INK_PX
            ))
        );
    }

    #[test]
    fn a_page_with_no_ink_has_no_centroid() {
        assert!(ink_centroid(&Gray::filled(8, 8, 1.0)).is_none());
    }

    #[test]
    fn an_unshifted_profile_correlates_perfectly() {
        let reference = text_like(64, 96, [20, 60], 0);
        let rows = super::row_ink_profile(&reference);
        let (shift, correlation) = best_profile_alignment(&rows, &rows, 4);
        assert_eq!(shift, 0);
        assert!((correlation.expect("correlation") - 1.0).abs() < 1e-12);
    }

    /// A page with a border, and the same page with the border's outermost rule
    /// one pixel further out — the exact disagreement two rasterizers have about
    /// a long thin stroke.
    fn framed(width: usize, height: usize, inset: usize) -> Gray {
        let mut pixels = vec![1.0; width * height];
        for row in inset..=inset + 2 {
            for col in inset..width - inset {
                pixels[row * width + col] = 0.0;
            }
        }
        for row in height - inset - 2..height - inset {
            for col in inset..width - inset {
                pixels[row * width + col] = 0.0;
            }
        }
        for col in inset..=inset + 2 {
            for row in inset..height - inset {
                pixels[row * width + col] = 0.0;
            }
        }
        for col in width - inset - 2..width - inset {
            for row in inset..height - inset {
                pixels[row * width + col] = 0.0;
            }
        }
        // Something in the middle for the rules to frame.
        for row in 40..50 {
            for col in 30..40 {
                pixels[row * width + col] = 0.0;
            }
        }
        Gray::new(width, height, pixels)
    }

    #[test]
    fn a_page_border_does_not_move_the_content_centroid() {
        // The whole reason [`content_centroid`] exists, as a fact rather than as
        // an argument: the same content, with its frame one pixel further out on
        // the right and the bottom — exactly the disagreement two rasterizers have
        // about a long thin stroke — moves the *plain* centroid by tens of pixels
        // and the content centroid not at all.
        let reference = framed(400, 600, 30);
        let moved = thicken_frame(&reference, 30);
        println!(
            "DBG plain {:?} {:?}",
            super::ink_centroid(&reference),
            super::ink_centroid(&moved)
        );
        println!(
            "DBG content {:?} {:?}",
            super::content_centroid(&reference),
            super::content_centroid(&moved)
        );
        println!(
            "DBG bounds {:?} {:?}",
            super::ink_bounds(&reference),
            super::ink_bounds(&moved)
        );
        let plain = super::centroid_delta(&reference, &moved);
        assert!(
            plain.0.abs() > 1.0 || plain.1.abs() > 1.0,
            "a frame that moved must move the plain centroid, or this test proves nothing: \
             {plain:?}"
        );
        let content = super::content_centroid_delta(&reference, &moved);
        assert!(
            content.0.abs() < 0.5 && content.1.abs() < 0.5,
            "and it must not move the content centroid: {content:?}"
        );
    }

    /// One extra pixel of frame on the right and the bottom only — which is what a
    /// rasterizer's antialiasing does to a stroke, and which is **not** symmetric,
    /// so it does move a mean.
    fn thicken_frame(page: &Gray, inset: usize) -> Gray {
        let (width, height) = (page.width(), page.height());
        let mut pixels = page.pixels().to_vec();
        for row in inset..height - inset {
            pixels[row * width + width - inset] = 0.0;
        }
        for col in inset..width - inset {
            pixels[(height - inset) * width + col] = 0.0;
        }
        Gray::new(width, height, pixels)
    }

    /// Builds a tiny text-like page: `count` dark rows of "ink" starting at
    /// `first`, inset 10 px from each side.
    fn text_rows(width: usize, height: usize, first: usize, count: usize) -> Gray {
        let mut pixels = vec![1.0; width * height];
        for row in first..first + count {
            for col in 10..width - 10 {
                pixels[row * width + col] = 0.0;
            }
        }
        Gray::new(width, height, pixels)
    }

    #[test]
    fn a_page_without_a_frame_measures_the_same_as_a_plain_centroid() {
        // The measure must be a no-op where there is nothing to exclude, or every
        // text page is being judged by a different rule than the one it was
        // calibrated on.
        let page = text_rows(200, 300, 40, 8);
        assert_eq!(
            super::content_centroid(&page),
            super::ink_centroid(&page),
            "a page with no rules has no rules to exclude"
        );
    }

    #[test]
    fn a_page_that_is_only_a_frame_has_no_content_to_measure() {
        let page = framed(200, 300, 10);
        // Every row of the frame except the content block is a rule, and the
        // content block is not a rule, so there is content left.
        assert!(super::content_centroid(&page).is_some());
        // Take the content away and there is nothing but rules.
        let border_only = framed(200, 300, 10);
        let mut pixels = border_only.pixels().to_vec();
        for row in 40..50 {
            for col in 30..40 {
                pixels[row * 200 + col] = 1.0;
            }
        }
        assert_eq!(
            super::content_centroid(&Gray::new(200, 300, pixels)),
            None,
            "a page that is nothing but rules has no content centroid"
        );
    }
}
