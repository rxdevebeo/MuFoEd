//! Page-level wrap exclusions carried across paragraphs (`FIX_PLAN` §3.3, F15).

use crate::layout::floating::WrapSide;
use std::sync::Arc;

/// Page-space polygon and its text clearance.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Contour {
    pub points: Vec<(f64, f64)>,
    pub through: bool,
    /// Left, right, top, bottom clearance in pixels.
    pub distance: [f64; 4],
}

impl Contour {
    /// The polygon's occupied intervals over a whole line band. Through wrap
    /// keeps holes and concavities; tight wrap uses the outer envelope.
    pub(crate) fn intervals(&self, top: f64, bottom: f64) -> Vec<(f64, f64)> {
        let top = top - self.distance[3];
        let bottom = bottom + self.distance[2];
        let mut levels = vec![top, bottom];
        levels.extend(
            self.points
                .iter()
                .map(|point| point.1)
                .filter(|y| *y > top && *y < bottom),
        );
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        let mut intervals = Vec::new();
        for band in levels.windows(2) {
            let &[start, end] = band else {
                continue;
            };
            if end - start <= 1e-9 {
                continue;
            }
            let inset = ((end - start) * 1e-8).min(1e-7);
            let mut swept: Vec<(f64, f64)> = Vec::new();
            for y in [start + inset, start.midpoint(end), end - inset] {
                let mut xs = Vec::new();
                for (a, b) in self
                    .points
                    .iter()
                    .zip(self.points.iter().cycle().skip(1))
                    .take(self.points.len())
                {
                    if (a.1 <= y && y < b.1) || (b.1 <= y && y < a.1) {
                        xs.push(a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1));
                    }
                }
                xs.sort_by(f64::total_cmp);
                for (index, pair) in xs.chunks_exact(2).enumerate() {
                    let &[left, right] = pair else {
                        continue;
                    };
                    if let Some(span) = swept.get_mut(index) {
                        span.0 = span.0.min(left);
                        span.1 = span.1.max(right);
                    } else {
                        swept.push((left, right));
                    }
                }
            }
            // Each edge moves linearly between consecutive vertex levels. Use
            // its entire swept interval, including positions between samples.
            intervals.extend(swept.into_iter().map(|(left, right)| {
                (
                    left - self.distance[0] - 1e-6,
                    right + self.distance[1] + 1e-6,
                )
            }));
        }
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (left, right) in intervals {
            if let Some(previous) = merged.last_mut().filter(|previous| previous.1 >= left) {
                previous.1 = previous.1.max(right);
            } else {
                merged.push((left, right));
            }
        }
        if !self.through {
            if let (Some(first), Some(last)) = (merged.first(), merged.last()) {
                return vec![(first.0, last.1)];
            }
        }
        merged
    }
}

/// A wrap rectangle in absolute page coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PageExclusion {
    /// Left edge, already expanded by `distL`.
    pub left: f64,
    /// Right edge, already expanded by `distR`.
    pub right: f64,
    /// Top edge on the page, already expanded by `distT`.
    pub top: f64,
    /// Bottom edge on the page, already expanded by `distB`.
    pub bottom: f64,
    /// Side policy.
    pub side: WrapSide,
    pub contour: Option<Arc<Contour>>,
}

impl PageExclusion {
    /// The same exclusion moved by `(dx, dy)`: its object moved with the
    /// lines that host it (column balancing).
    #[must_use]
    pub(crate) fn shifted(&self, dx: f64, dy: f64) -> Self {
        Self {
            left: self.left + dx,
            right: self.right + dx,
            top: self.top + dy,
            bottom: self.bottom + dy,
            side: self.side,
            contour: self.contour.as_ref().map(|contour| {
                Arc::new(Contour {
                    points: contour
                        .points
                        .iter()
                        .map(|(x, y)| (x + dx, y + dy))
                        .collect(),
                    through: contour.through,
                    distance: contour.distance,
                })
            }),
        }
    }

    /// Converts page coordinates into paragraph-local ones for a host at `host_y`.
    ///
    /// `column_dx` is how far the paragraph's column is painted to the right
    /// of where its lines are laid out (the paginator lays every column out
    /// from the content left edge and moves the lines afterwards).
    #[must_use]
    pub(crate) fn into_paragraph_local(
        self,
        host_y: f64,
        column_dx: f64,
    ) -> crate::layout::floating::WrapExclusion {
        crate::layout::floating::WrapExclusion {
            left: self.left - column_dx,
            right: self.right - column_dx,
            top: self.top - host_y,
            bottom: self.bottom - host_y,
            side: self.side,
            contour: self.contour,
            page_y: host_y,
            page_dx: column_dx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Contour;

    #[test]
    fn diamond_band_does_not_use_the_bounding_rectangle() {
        let contour = Contour {
            points: vec![(50.0, 0.0), (100.0, 50.0), (50.0, 100.0), (0.0, 50.0)],
            through: false,
            distance: [0.0; 4],
        };
        let spans = contour.intervals(0.0, 10.0);
        assert_eq!(spans.len(), 1);
        assert!((spans[0].0 - 40.0).abs() < 1e-5);
        assert!((spans[0].1 - 60.0).abs() < 1e-5);
        assert!(contour.intervals(101.0, 110.0).is_empty());
    }

    #[test]
    fn through_keeps_a_concavity_where_tight_excludes_it() {
        let mut contour = Contour {
            points: vec![
                (0.0, 0.0),
                (100.0, 0.0),
                (100.0, 100.0),
                (70.0, 100.0),
                (70.0, 30.0),
                (30.0, 30.0),
                (30.0, 100.0),
                (0.0, 100.0),
            ],
            through: true,
            distance: [2.0, 3.0, 4.0, 5.0],
        };
        let spans = contour.intervals(50.0, 60.0);
        assert_eq!(spans.len(), 2);
        assert!((spans[0].1 - 33.0).abs() < 1e-5);
        assert!((spans[1].0 - 68.0).abs() < 1e-5);
        contour.through = false;
        assert_eq!(contour.intervals(50.0, 60.0).len(), 1);
    }

    #[test]
    fn narrow_diagonal_excludes_the_whole_swept_line_band() {
        let contour = Contour {
            points: vec![(0.0, 0.0), (1.0, 0.0), (101.0, 100.0), (100.0, 100.0)],
            through: true,
            distance: [0.0; 4],
        };
        let spans = contour.intervals(0.0, 100.0);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].0 <= 0.0 && spans[0].1 >= 101.0);
    }
}
