//! Rectangular clipping (G1, `PDF-VECTOR-GRAPHICS-2026-10-19.md`).
//!
//! Nearly every clip a design tool writes is an axis-aligned rectangle
//! (`x y w h re W n`): a picture cropped to its frame, a shape cut to a card.
//! Before this module no clip was applied at all, and a header illustration
//! spilled out of its frame over the cards below it. A rectangle is applied
//! here, exactly: a path is cut to it, a picture is cropped to it. A clip of any
//! other shape is still not applied, and says so.
//!
//! Everything is in device space, PDF's y up, the space the page's boxes are
//! measured in; a path is taken there through its CTM and brought back.

use crate::content::{Matrix, SubPath};

/// `[x0, y0, x1, y1]`, `x0 <= x1` and `y0 <= y1` unless empty.
pub(crate) type Rect = [f64; 4];

const EPSILON: f64 = 0.01;

/// The device-space rectangle a clip path is, when it is one: a single closed
/// subpath of four corners whose edges are all horizontal or vertical.
pub(crate) fn device_rect(subpaths: &[SubPath], ctm: Matrix) -> Option<Rect> {
    let mut drawn = subpaths.iter().filter(|subpath| !subpath.points.is_empty());
    let subpath = drawn.next()?;
    if drawn.next().is_some() {
        return None;
    }
    let mut corners: Vec<(f64, f64)> = Vec::with_capacity(5);
    for &(x, y) in &subpath.points {
        let point = ctm.apply(x, y);
        if !(point.0.is_finite() && point.1.is_finite()) {
            return None;
        }
        let repeat = corners.last().is_some_and(|last| {
            (last.0 - point.0).abs() < EPSILON && (last.1 - point.1).abs() < EPSILON
        });
        if !repeat {
            corners.push(point);
        }
    }
    if let (Some(first), Some(last)) = (corners.first().copied(), corners.last().copied()) {
        if corners.len() > 1
            && (first.0 - last.0).abs() < EPSILON
            && (first.1 - last.1).abs() < EPSILON
        {
            corners.pop();
        }
    }
    if corners.len() != 4 {
        return None;
    }
    let edges_straight = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .all(|(a, b)| (a.0 - b.0).abs() < EPSILON || (a.1 - b.1).abs() < EPSILON);
    if !edges_straight {
        return None;
    }
    bounds(corners.iter().copied())
}

/// The common part of two rectangles; empty when they do not meet.
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn is_empty(rect: Rect) -> bool {
    rect[2] - rect[0] < EPSILON || rect[3] - rect[1] < EPSILON
}

fn bounds(points: impl Iterator<Item = (f64, f64)>) -> Option<Rect> {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in points {
        out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
    }
    out.iter().all(|value| value.is_finite()).then_some(out)
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner[0] >= outer[0] - EPSILON
        && inner[1] >= outer[1] - EPSILON
        && inner[2] <= outer[2] + EPSILON
        && inner[3] <= outer[3] + EPSILON
}

fn disjoint(a: Rect, b: Rect) -> bool {
    a[2] < b[0] - EPSILON || b[2] < a[0] - EPSILON || a[3] < b[1] - EPSILON || b[3] < a[1] - EPSILON
}

fn invert(m: Matrix) -> Option<Matrix> {
    let det = m.a * m.d - m.b * m.c;
    if !(det.is_finite() && det.abs() > 1e-12) {
        return None;
    }
    Some(Matrix {
        a: m.d / det,
        b: -m.b / det,
        c: -m.c / det,
        d: m.a / det,
        e: (m.c * m.f - m.d * m.e) / det,
        f: (m.b * m.e - m.a * m.f) / det,
    })
}

/// What a rectangular clip does to a path.
#[derive(Debug, PartialEq)]
pub(crate) enum Clipped {
    /// The path is inside the clip, or cannot be cut exactly: drawn as it is.
    Unchanged,
    /// The path is wholly outside the clip: nothing of it shows.
    Hidden,
    /// The part of the path inside the clip, in the path's own space.
    Cut(Vec<SubPath>),
}

/// Cuts a path to `rect`. A fill is cut as polygons (each subpath closed, as
/// filling closes it), a stroke as lines; a path both filled and stroked that
/// crosses the clip is left whole, because a cut fill would draw the clip's
/// edge as stroke.
pub(crate) fn clip_path(
    subpaths: &[SubPath],
    ctm: Matrix,
    rect: Rect,
    fill: bool,
    stroke: bool,
) -> Clipped {
    if is_empty(rect) {
        return Clipped::Hidden;
    }
    let device: Vec<(Vec<(f64, f64)>, bool)> = subpaths
        .iter()
        .map(|subpath| {
            (
                subpath
                    .points
                    .iter()
                    .map(|&(x, y)| ctm.apply(x, y))
                    .collect(),
                subpath.closed,
            )
        })
        .collect();
    let Some(extent) = bounds(device.iter().flat_map(|(points, _)| points.iter().copied())) else {
        return Clipped::Unchanged;
    };
    if contains(rect, extent) {
        return Clipped::Unchanged;
    }
    // A stroke's width reaches past its centre line; a path whose box misses
    // the clip by less than that is not certainly hidden, so it is cut instead.
    if disjoint(rect, extent) {
        return Clipped::Hidden;
    }
    if fill == stroke {
        return Clipped::Unchanged;
    }
    let Some(back) = invert(ctm) else {
        return Clipped::Unchanged;
    };
    let mut cut: Vec<SubPath> = Vec::new();
    for (points, closed) in &device {
        if fill {
            let polygon = clip_polygon(points, rect);
            if polygon.len() >= 3 {
                cut.push(SubPath {
                    points: polygon.into_iter().map(|(x, y)| back.apply(x, y)).collect(),
                    closed: true,
                });
            }
        } else {
            for line in clip_polyline(points, *closed, rect) {
                cut.push(SubPath {
                    points: line.into_iter().map(|(x, y)| back.apply(x, y)).collect(),
                    closed: false,
                });
            }
        }
    }
    if cut.is_empty() {
        Clipped::Hidden
    } else {
        Clipped::Cut(cut)
    }
}

/// Sutherland–Hodgman against the four edges of `rect`.
fn clip_polygon(points: &[(f64, f64)], rect: Rect) -> Vec<(f64, f64)> {
    let mut polygon = points.to_vec();
    // (inside test, intersection with the edge) for left, right, bottom, top.
    let edges: [(usize, f64, bool); 4] = [
        (0, rect[0], true),
        (0, rect[2], false),
        (1, rect[1], true),
        (1, rect[3], false),
    ];
    for (axis, bound, lower) in edges {
        let inside = |point: (f64, f64)| {
            let value = if axis == 0 { point.0 } else { point.1 };
            if lower {
                value >= bound
            } else {
                value <= bound
            }
        };
        let cross = |a: (f64, f64), b: (f64, f64)| {
            let (va, vb) = if axis == 0 { (a.0, b.0) } else { (a.1, b.1) };
            let t = if (vb - va).abs() < f64::EPSILON {
                0.0
            } else {
                (bound - va) / (vb - va)
            };
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let Some(&last) = polygon.last() else {
            return polygon;
        };
        let mut out = Vec::with_capacity(polygon.len().saturating_add(4));
        let mut previous = last;
        for &point in &polygon {
            match (inside(previous), inside(point)) {
                (true, true) => out.push(point),
                (true, false) => out.push(cross(previous, point)),
                (false, true) => {
                    out.push(cross(previous, point));
                    out.push(point);
                }
                (false, false) => {}
            }
            previous = point;
        }
        polygon = out;
    }
    polygon
}

/// Liang–Barsky per segment; a line that leaves and re-enters the clip is
/// split into pieces.
fn clip_polyline(points: &[(f64, f64)], closed: bool, rect: Rect) -> Vec<Vec<(f64, f64)>> {
    let mut segments: Vec<((f64, f64), (f64, f64))> = points
        .iter()
        .copied()
        .zip(points.iter().copied().skip(1))
        .collect();
    if closed {
        if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
            segments.push((last, first));
        }
    }
    let mut lines: Vec<Vec<(f64, f64)>> = Vec::new();
    for (a, b) in segments {
        let Some((from, to)) = clip_segment(a, b, rect) else {
            continue;
        };
        let joins = lines
            .last()
            .and_then(|line| line.last())
            .is_some_and(|end| {
                (end.0 - from.0).abs() < EPSILON && (end.1 - from.1).abs() < EPSILON
            });
        match lines.last_mut() {
            Some(line) if joins => line.push(to),
            _ => lines.push(vec![from, to]),
        }
    }
    lines
}

fn clip_segment(a: (f64, f64), b: (f64, f64), rect: Rect) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let mut low = 0.0_f64;
    let mut high = 1.0_f64;
    for (p, q) in [
        (-dx, a.0 - rect[0]),
        (dx, rect[2] - a.0),
        (-dy, a.1 - rect[1]),
        (dy, rect[3] - a.1),
    ] {
        if p.abs() < f64::EPSILON {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                low = low.max(t);
            } else {
                high = high.min(t);
            }
        }
    }
    (low <= high).then_some((
        (a.0 + dx * low, a.1 + dy * low),
        (a.0 + dx * high, a.1 + dy * high),
    ))
}

/// A picture cut to a rectangular clip: the box that shows and how much of
/// the picture is cut from each side.
#[derive(Debug, PartialEq)]
pub(crate) enum ImageCut {
    /// The picture is inside the clip, or turned, mirrored or skewed (whose
    /// crop is not a box cut): placed as it is.
    Whole,
    /// Nothing of the picture shows.
    Hidden,
    /// `(x, y, width, height)` of the part that shows, and the fractions of
    /// the picture cut off `[left, top, right, bottom]`.
    Cropped((f64, f64, f64, f64), [f64; 4]),
}

/// Crops a picture whose unit square `ctm` places to `rect`.
pub(crate) fn image_cut(ctm: Matrix, rect: Rect) -> ImageCut {
    let upright = ctm.b.abs() < 1e-9 && ctm.c.abs() < 1e-9 && ctm.a > 0.0 && ctm.d > 0.0;
    let box_ = [ctm.e, ctm.f, ctm.e + ctm.a, ctm.f + ctm.d];
    if !box_.iter().all(|value| value.is_finite()) {
        return ImageCut::Whole;
    }
    if is_empty(rect) || disjoint(rect, box_) {
        return ImageCut::Hidden;
    }
    if !upright || contains(rect, box_) {
        return ImageCut::Whole;
    }
    let shown = intersect(box_, rect);
    if is_empty(shown) {
        return ImageCut::Hidden;
    }
    let (width, height) = (ctm.a, ctm.d);
    let crop = [
        (shown[0] - box_[0]) / width,
        (box_[3] - shown[3]) / height,
        (box_[2] - shown[2]) / width,
        (shown[1] - box_[1]) / height,
    ];
    ImageCut::Cropped(
        (shown[0], shown[1], shown[2] - shown[0], shown[3] - shown[1]),
        crop.map(|fraction| fraction.clamp(0.0, 1.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Vec<SubPath> {
        vec![SubPath {
            points: vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)],
            closed: true,
        }]
    }

    #[test]
    fn a_re_clip_is_a_rectangle_and_a_triangle_is_not() {
        let scaled = Matrix {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 2.0,
            e: 10.0,
            f: 20.0,
        };
        assert_eq!(
            device_rect(&rect_path(0.0, 0.0, 5.0, 5.0), scaled),
            Some([10.0, 20.0, 20.0, 30.0])
        );
        let triangle = vec![SubPath {
            points: vec![(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)],
            closed: true,
        }];
        assert_eq!(device_rect(&triangle, Matrix::IDENTITY), None);
    }

    #[test]
    fn a_fill_is_cut_to_the_clip_and_one_outside_is_hidden() {
        let clip = [0.0, 0.0, 10.0, 10.0];
        let Clipped::Cut(cut) = clip_path(
            &rect_path(5.0, 5.0, 10.0, 10.0),
            Matrix::IDENTITY,
            clip,
            true,
            false,
        ) else {
            panic!("expected a cut");
        };
        let extent = bounds(cut.iter().flat_map(|s| s.points.iter().copied())).unwrap();
        assert_eq!(extent, [5.0, 5.0, 10.0, 10.0]);
        assert_eq!(
            clip_path(
                &rect_path(20.0, 20.0, 5.0, 5.0),
                Matrix::IDENTITY,
                clip,
                true,
                false
            ),
            Clipped::Hidden
        );
        assert_eq!(
            clip_path(
                &rect_path(1.0, 1.0, 5.0, 5.0),
                Matrix::IDENTITY,
                clip,
                true,
                false
            ),
            Clipped::Unchanged
        );
    }

    #[test]
    fn a_stroke_is_cut_into_the_pieces_inside() {
        let line = vec![SubPath {
            points: vec![(-5.0, 5.0), (15.0, 5.0)],
            closed: false,
        }];
        let Clipped::Cut(cut) =
            clip_path(&line, Matrix::IDENTITY, [0.0, 0.0, 10.0, 10.0], false, true)
        else {
            panic!("expected a cut");
        };
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0].points, vec![(0.0, 5.0), (10.0, 5.0)]);
    }

    #[test]
    fn a_picture_is_cropped_to_the_part_that_shows() {
        // A 100 x 200 picture at (0, 0); the clip shows its upper half.
        let ctm = Matrix {
            a: 100.0,
            b: 0.0,
            c: 0.0,
            d: 200.0,
            e: 0.0,
            f: 0.0,
        };
        let ImageCut::Cropped(shown, crop) = image_cut(ctm, [0.0, 100.0, 100.0, 300.0]) else {
            panic!("expected a crop");
        };
        assert_eq!(shown, (0.0, 100.0, 100.0, 100.0));
        assert_eq!(crop, [0.0, 0.0, 0.0, 0.5]);
        assert_eq!(
            image_cut(ctm, [500.0, 500.0, 600.0, 600.0]),
            ImageCut::Hidden
        );
        assert_eq!(image_cut(ctm, [-1.0, -1.0, 101.0, 201.0]), ImageCut::Whole);
    }
}
