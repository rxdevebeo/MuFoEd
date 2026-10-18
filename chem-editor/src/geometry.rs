//! Geometry helpers shared by the editor and the renderers: standard bond
//! length, regular ring templates, angle snapping and a free-direction search
//! for placing new substituents.

use crate::model::{Point, Structure};

/// Default length of a new bond, in logical pixels.
pub const BOND_LENGTH: f32 = 44.0;
/// Shortest bond the editor will create.
pub const MIN_BOND_LENGTH: f32 = 14.0;
/// Angle snapping increment, in degrees.
pub const SNAP_DEGREES: f32 = 15.0;

/// A point at `distance` from `origin`, along `angle` (radians, y down).
#[must_use]
pub fn position_at(origin: Point, angle: f32, distance: f32) -> Point {
    Point::new(
        origin.x + angle.cos() * distance,
        origin.y + angle.sin() * distance,
    )
}

/// Snaps an angle to the nearest [`SNAP_DEGREES`] multiple.
#[must_use]
pub fn snap_angle(angle: f32) -> f32 {
    let step = SNAP_DEGREES.to_radians();
    (angle / step).round() * step
}

/// Vertices of a regular `sides`-gon centred on `center`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn ring_vertices(center: Point, sides: usize, radius: f32, rotation: f32) -> Vec<Point> {
    let sides = sides.max(3);
    (0..sides)
        .map(|index| {
            let angle = rotation + std::f32::consts::TAU * (index as f32) / (sides as f32);
            position_at(center, angle, radius)
        })
        .collect()
}

/// Vertices of a ring template whose first vertex sits at `anchor`.
///
/// The ring is drawn so that `anchor` becomes the first vertex and the rest
/// follow clockwise, which keeps a substituent attached to the clicked atom.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn ring_from_anchor(anchor: Point, sides: usize, radius: f32, rotation: f32) -> Vec<Point> {
    let sides = sides.max(3);
    let step = std::f32::consts::TAU / (sides as f32);
    (0..sides)
        .map(|index| {
            let angle = rotation + step * (index as f32);
            position_at(anchor, angle, radius)
        })
        .collect()
}

/// Chooses an angle from `parent` that keeps the most distance from the bonds
/// already present, so new substituents do not overlap existing ones.
#[must_use]
pub fn free_direction(structure: &Structure, parent: Point, parent_id: u64) -> f32 {
    let existing: Vec<f32> = structure
        .bonds_at(parent_id)
        .filter_map(|bond| {
            let other = if bond.a == parent_id { bond.b } else { bond.a };
            let atom = structure.atom(other)?;
            let delta = atom.position.sub(parent);
            (delta.length() > 0.01).then(|| delta.y.atan2(delta.x))
        })
        .collect();
    if existing.is_empty() {
        return 0.0;
    }
    let mut best = 0.0_f32;
    let mut best_score = f32::NEG_INFINITY;
    for step in 0_u16..24 {
        let angle = std::f32::consts::TAU * f32::from(step) / 24.0;
        let score = existing
            .iter()
            .map(|other| angle_distance(angle, *other))
            .fold(f32::INFINITY, f32::min);
        if score > best_score {
            best_score = score;
            best = angle;
        }
    }
    snap_angle(best)
}

fn angle_distance(a: f32, b: f32) -> f32 {
    let raw = (a - b).abs() % std::f32::consts::TAU;
    raw.min(std::f32::consts::TAU - raw)
}

/// A unit vector perpendicular to `from -> to`, or `(0, 0)` for a degenerate
/// bond.
#[must_use]
pub fn perpendicular(from: Point, to: Point) -> Point {
    let delta = to.sub(from);
    let length = delta.length();
    if length < 0.001 {
        Point::new(0.0, 0.0)
    } else {
        Point::new(-delta.y / length, delta.x / length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_have_the_requested_number_of_vertices() {
        let ring = ring_vertices(Point::new(0.0, 0.0), 6, 40.0, 0.0);
        assert_eq!(ring.len(), 6);
        for vertex in &ring {
            assert!((vertex.length() - 40.0).abs() < 0.01);
        }
    }

    #[test]
    fn snapping_rounds_to_the_step() {
        let snapped = snap_angle(0.05);
        assert!(snapped.abs() < 1e-4);
        let quarter = snap_angle(std::f32::consts::FRAC_PI_2);
        assert!((quarter - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
    }
}
