//! Tab leaders drawn as paths: the dots of a table of contents.
//!
//! Google Docs, Chrome and many design tools do not set a contents line's dots
//! as text. They draw the title, a dotted line, and the page number, each on
//! its own: the dots are a stroked path just under the baseline, between the
//! end of the title and the start of the number. Converted as they were - a
//! shape anchored to the page - the dots stayed where the PDF had them while
//! the text they belong to reflowed, and every contents page came out with its
//! dots a line or two away from their entries.
//!
//! What they are is a tab's leader. The number already arrives by a tab
//! (`semantic::tab_to`); this module finds the paths that lead to it, so that
//! tab gets `w:leader` and the paths are not drawn a second time. A path is a
//! leader only when it lies on one horizontal line, within the gap of a line's
//! two pieces, at the baseline, and at least one of its paths is dashed: a
//! solid rule under the baseline is as often a form's blank as a leader, and it
//! stays a shape.

use std::collections::BTreeSet;

use strict_ooxml_pdf::content::{Item, Vector};
use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_pdf::PdfPage;
use strict_ooxml_wml::model::values::TabLeader;

/// One leader: the line it belongs to, by its baseline and the x where the
/// piece it leads to starts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Leader {
    /// Points from the page's top.
    pub(crate) baseline: f64,
    /// Where the piece after the leader starts.
    pub(crate) right: f64,
    pub(crate) kind: TabLeader,
}

/// A path that may be part of a leader: one horizontal line, in points from
/// the page's top.
struct Candidate {
    index: usize,
    top: f64,
    left: f64,
    right: f64,
    /// `Some(dot)` for a dashed path: whether its dashes are dots.
    dotted: Option<bool>,
}

fn candidate(index: usize, vector: &Vector, page_height: f64) -> Option<Candidate> {
    if vector.stroke.is_none() || vector.fill.is_some() || vector.gradient.is_some() {
        return None;
    }
    let mut points = vector
        .subpaths
        .iter()
        .flat_map(|subpath| subpath.points.iter())
        .map(|&(x, y)| vector.ctm.apply(x, y));
    let first = points.next()?;
    let (mut left, mut right, mut low, mut high) = (first.0, first.0, first.1, first.1);
    for (x, y) in points {
        left = left.min(x);
        right = right.max(x);
        low = low.min(y);
        high = high.max(y);
    }
    if !(left.is_finite() && right.is_finite() && low.is_finite() && high.is_finite())
        || high - low > 0.6
    {
        return None;
    }
    let width = vector.line_width.max(0.1);
    let dotted = vector
        .dash
        .first()
        .filter(|_| vector.dash.iter().any(|value| *value > 0.0))
        .map(|on| *on <= width * 2.0);
    Some(Candidate {
        index,
        top: page_height - f64::midpoint(low, high),
        left,
        right,
        dotted,
    })
}

/// The page's leaders, and the indices of the paths that drew them.
pub(crate) fn find(page: &PdfPage, lines: &[GlyphLine]) -> (Vec<Leader>, BTreeSet<usize>) {
    let candidates: Vec<Candidate> = page
        .items()
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Vector(vector) => candidate(index, vector, page.geometry.height),
            _ => None,
        })
        .collect();
    let mut leaders = Vec::new();
    let mut used = BTreeSet::new();
    if candidates.is_empty() {
        return (leaders, used);
    }
    for after in lines {
        if after.text().trim().is_empty() {
            continue;
        }
        // The piece before it on its baseline: the nearest one to its left.
        let Some(before) = lines
            .iter()
            .filter(|line| {
                (line.baseline - after.baseline).abs() < 0.5
                    && line.x + line.width <= after.x + 0.5
                    && !line.text().trim().is_empty()
            })
            .max_by(|a, b| (a.x + a.width).total_cmp(&(b.x + b.width)))
        else {
            continue;
        };
        let start = before.x + before.width;
        let gap = after.x - start;
        if gap < after.size {
            continue;
        }
        let size = after.size.max(before.size);
        let found: Vec<&Candidate> = candidates
            .iter()
            .filter(|path| {
                path.top >= after.baseline - size * 0.4
                    && path.top <= after.baseline + size * 0.6
                    && path.left >= before.x
                    && path.left >= start - size
                    && path.right <= after.x + 2.0
                    && path.right - path.left >= gap * 0.25
            })
            .collect();
        let Some(dotted) = found.iter().find_map(|path| path.dotted) else {
            continue;
        };
        used.extend(found.iter().map(|path| path.index));
        leaders.push(Leader {
            baseline: after.baseline,
            right: after.x,
            kind: if dotted {
                TabLeader::Dot
            } else {
                TabLeader::Hyphen
            },
        });
    }
    (leaders, used)
}

/// The leader of the tab that reaches `line`, if one was drawn.
pub(crate) fn of(leaders: &[Leader], line: &GlyphLine) -> Option<TabLeader> {
    leaders
        .iter()
        .find(|leader| {
            (leader.baseline - line.baseline).abs() < 0.5 && (leader.right - line.x).abs() < 1.0
        })
        .map(|leader| leader.kind)
}
