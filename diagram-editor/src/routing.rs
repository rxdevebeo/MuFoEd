//! Bounded rectilinear visibility-grid search, no library or external service.
#![allow(clippy::many_single_char_names, clippy::too_many_lines)]
use crate::{Connection, Diagram, Node, Point, Port};
use std::{cmp::Ordering, collections::BinaryHeap};
type Bounds = (Point, Point);
fn inside(p: Point, (a, b): Bounds) -> bool {
    p.x > a.x + 0.001 && p.x < b.x - 0.001 && p.y > a.y + 0.001 && p.y < b.y - 0.001
}
fn blocked(a: Point, b: Point, bounds: Bounds) -> bool {
    let (lo, hi) = bounds;
    if (a.x - b.x).abs() < 0.001 {
        a.x > lo.x + 0.001
            && a.x < hi.x - 0.001
            && a.y.max(b.y) > lo.y + 0.001
            && a.y.min(b.y) < hi.y - 0.001
    } else if (a.y - b.y).abs() < 0.001 {
        a.y > lo.y + 0.001
            && a.y < hi.y - 0.001
            && a.x.max(b.x) > lo.x + 0.001
            && a.x.min(b.x) < hi.x - 0.001
    } else {
        // Slanted endpoint lead: slab intersection, useful for rotated nodes.
        let mut enter = 0.0_f32;
        let mut leave = 1.0_f32;
        for (start, delta, min, max) in [(a.x, b.x - a.x, lo.x, hi.x), (a.y, b.y - a.y, lo.y, hi.y)]
        {
            if delta.abs() < 0.001 {
                if start <= min || start >= max {
                    return false;
                }
            } else {
                let t1 = (min - start) / delta;
                let t2 = (max - start) / delta;
                enter = enter.max(t1.min(t2));
                leave = leave.min(t1.max(t2));
            }
        }
        enter < leave - 0.001
    }
}
fn lead(n: &Node, port: Port, clearance: f32) -> (Point, Point) {
    let p = n.port(port);
    let v = match port {
        Port::Left => Point::new(-1.0, 0.0),
        Port::Right => Point::new(1.0, 0.0),
        Port::Top => Point::new(0.0, -1.0),
        Port::Bottom => Point::new(0.0, 1.0),
    };
    let v = crate::model::rotate(v, Point::new(0.0, 0.0), n.style.rotation);
    let (a, b) = n.visual_bounds();
    let mut distance = 24.0_f32;
    for (p, d, min, max) in [(p.x, v.x, a.x, b.x), (p.y, v.y, a.y, b.y)] {
        if d > 0.001 {
            distance = distance.max((max + clearance - p) / d);
        } else if d < -0.001 {
            distance = distance.max((min - clearance - p) / d);
        }
    }
    (p, Point::new(p.x + v.x * distance, p.y + v.y * distance))
}
#[derive(Clone, Copy)]
struct Entry {
    score: f32,
    cost: f32,
    state: usize,
}
impl PartialEq for Entry {
    fn eq(&self, r: &Self) -> bool {
        self.score.to_bits() == r.score.to_bits() && self.state == r.state
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, r: &Self) -> Option<Ordering> {
        Some(self.cmp(r))
    }
}
impl Ord for Entry {
    fn cmp(&self, r: &Self) -> Ordering {
        r.score
            .total_cmp(&self.score)
            .then_with(|| r.state.cmp(&self.state))
    }
}
pub(crate) fn avoid(d: &Diagram, c: &Connection) -> Option<Vec<Point>> {
    let source = d.node(c.from)?;
    let target = d.node(c.to)?;
    let margin = 8.0 + c.width / 2.0;
    let (start, a) = lead(source, c.from_port, margin + 4.0);
    let (end, b) = lead(target, c.to_port, margin + 4.0);
    let bounds: Vec<_> = d
        .nodes
        .iter()
        .map(|n| {
            let (a, b) = n.visual_bounds();
            (
                Point::new(a.x - margin, a.y - margin),
                Point::new(b.x + margin, b.y + margin),
            )
        })
        .collect();
    // No false success if an endpoint lead crosses a third shape.
    for (n, r) in d.nodes.iter().zip(&bounds) {
        if n.id != source.id && blocked(start, a, *r) || n.id != target.id && blocked(end, b, *r) {
            return None;
        }
    }
    if bounds.iter().any(|r| inside(a, *r) || inside(b, *r)) {
        return None;
    }
    let mut path = vec![start, a];
    // Fast path keeps large unobstructed scenes cheap.
    for corner in [Point::new(b.x, a.y), Point::new(a.x, b.y)] {
        if !bounds
            .iter()
            .any(|r| blocked(a, corner, *r) || blocked(corner, b, *r))
        {
            path.extend([corner, b, end]);
            return Some(simplify(path));
        }
    }
    // At most 66*66 grid points, two directional states each. No unbounded work
    // in an interactive frame. Failure is explicit through route_status().
    if bounds.len() > 32 {
        return None;
    }
    let mut xs = vec![a.x, b.x];
    let mut ys = vec![a.y, b.y];
    for (lo, hi) in &bounds {
        xs.extend([lo.x, hi.x]);
        ys.extend([lo.y, hi.y]);
    }
    xs.sort_by(f32::total_cmp);
    ys.sort_by(f32::total_cmp);
    xs.dedup();
    ys.dedup();
    let width = xs.len();
    let height = ys.len();
    let position = |index: usize| Point::new(xs[index % width], ys[index / width]);
    let index = |p: Point| {
        Some(
            ys.binary_search_by(|v| v.total_cmp(&p.y)).ok()? * width
                + xs.binary_search_by(|v| v.total_cmp(&p.x)).ok()?,
        )
    };
    let start = index(a)?;
    let end_index = index(b)?;
    let mut costs = vec![f32::INFINITY; width * height * 2];
    let mut previous = vec![usize::MAX; costs.len()];
    let mut heap = BinaryHeap::new();
    for direction in 0..2 {
        let state = start * 2 + direction;
        costs[state] = 0.0;
        heap.push(Entry {
            score: 0.0,
            cost: 0.0,
            state,
        });
    }
    let mut finish = None;
    while let Some(Entry { cost, state, .. }) = heap.pop() {
        if cost > costs[state] {
            continue;
        }
        let at = state / 2;
        if at == end_index {
            finish = Some(state);
            break;
        }
        let x = at % width;
        let y = at / width;
        let p = position(at);
        for neighbor in [
            x.checked_sub(1).map(|x| y * width + x),
            (x + 1 < width).then_some(y * width + x + 1),
            y.checked_sub(1).map(|y| y * width + x),
            (y + 1 < height).then_some((y + 1) * width + x),
        ]
        .into_iter()
        .flatten()
        {
            let q = position(neighbor);
            if bounds.iter().any(|r| blocked(p, q, *r)) {
                continue;
            }
            let direction = usize::from((p.y - q.y).abs() > 0.001);
            let next = neighbor * 2 + direction;
            let cost = cost
                + (p.x - q.x).abs()
                + (p.y - q.y).abs()
                + if state % 2 == direction { 0.0 } else { 12.0 };
            if cost < costs[next] {
                costs[next] = cost;
                previous[next] = state;
                heap.push(Entry {
                    score: cost + (q.x - b.x).abs() + (q.y - b.y).abs(),
                    cost,
                    state: next,
                });
            }
        }
    }
    let mut state = finish?;
    let mut middle = vec![];
    loop {
        middle.push(position(state / 2));
        if previous[state] == usize::MAX {
            break;
        }
        state = previous[state];
    }
    middle.reverse();
    path.extend(middle);
    path.push(end);
    Some(simplify(path))
}
fn simplify(path: Vec<Point>) -> Vec<Point> {
    let mut result: Vec<Point> = vec![];
    for p in path {
        if result.last() == Some(&p) {
            continue;
        }
        while result.len() >= 2 {
            let a = result[result.len() - 2];
            let b = result[result.len() - 1];
            let cross = (b.x - a.x) * (p.y - b.y) - (b.y - a.y) * (p.x - b.x);
            let dot = (b.x - a.x) * (p.x - b.x) + (b.y - a.y) * (p.y - b.y);
            if cross.abs() < 0.001 && dot >= 0.0 {
                result.pop();
            } else {
                break;
            }
        }
        result.push(p);
    }
    result
}
