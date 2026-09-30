//! The 2x3 affine matrix PDF's `cm` operator takes, and the placement transforms
//! the layout implies.
//!
//! SVG and PDF disagree about three things at once: the origin, the direction
//! of y, and whether a shape's local coordinates are scaled to its box. Getting
//! that wrong produces a mirrored or displaced drawing that still *looks* like a
//! drawing, so the composition lives in one tested place rather than in the
//! middle of a content stream.

/// A PDF transformation matrix `[a b c d e f]`, in the order `cm` expects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    /// x scale (or cos component).
    pub a: f32,
    /// y shear.
    pub b: f32,
    /// x shear.
    pub c: f32,
    /// y scale (or sin component).
    pub d: f32,
    /// x translation.
    pub e: f32,
    /// y translation.
    pub f: f32,
}

impl Matrix {
    /// The identity matrix.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A translation.
    #[must_use]
    pub const fn translate(x: f32, y: f32) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    /// A scale.
    #[must_use]
    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    /// A rotation by `degrees`, counter-clockwise in PDF's y-up space.
    #[must_use]
    pub fn rotate(degrees: f32) -> Self {
        let radians = f64::from(degrees) * std::f64::consts::PI / 180.0;
        let (sin, cos) = (radians.sin() as f32, radians.cos() as f32);
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Composes two matrices in the order a `cm` chain emits them.
    ///
    /// PDF transforms row vectors (`x' = x · M`), so `cm A` followed by `cm B`
    /// composes to `A · B` and sends a point through `A` first. Naming that
    /// order directly — `chain(first, second)` — is deliberate: the alternative,
    /// a `then` method on the matrix, reads as the opposite of what it does.
    #[must_use]
    pub fn chain(first: Self, second: Self) -> Self {
        Self {
            a: first.a * second.a + first.b * second.c,
            b: first.a * second.b + first.b * second.d,
            c: first.c * second.a + first.d * second.c,
            d: first.c * second.b + first.d * second.d,
            e: first.e * second.a + first.f * second.c + second.e,
            f: first.e * second.b + first.f * second.d + second.f,
        }
    }

    /// The matrix as `cm` takes it.
    #[must_use]
    pub const fn to_array(self) -> [f32; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }
}

#[cfg(test)]
mod tests {
    use super::Matrix;

    #[test]
    fn chain_applies_its_arguments_in_order() {
        // `chain(translate(10), scale(2))` emits `cm translate` then `cm scale`,
        // so the point (1, 1) moves to (11, 11) and is then doubled: (22, 22).
        let matrix = Matrix::chain(Matrix::translate(10.0, 10.0), Matrix::scale(2.0, 2.0));
        let x = matrix.a * 1.0 + matrix.c * 1.0 + matrix.e;
        let y = matrix.b * 1.0 + matrix.d * 1.0 + matrix.f;
        assert!((x - 22.0).abs() < 1e-5, "{x}");
        assert!((y - 22.0).abs() < 1e-5, "{y}");

        // The other order is a different matrix, which is why the order at a call
        // site is part of the code rather than an accident.
        let other = Matrix::chain(Matrix::scale(2.0, 2.0), Matrix::translate(10.0, 10.0));
        let x = other.a * 1.0 + other.c * 1.0 + other.e;
        assert!((x - 12.0).abs() < 1e-5, "{x}");
    }

    #[test]
    fn a_flip_mirrors() {
        let matrix = Matrix::scale(-1.0, 1.0);
        let x = matrix.a * 4.0 + matrix.e;
        assert!((x + 4.0).abs() < 1e-6, "{x}");
    }

    #[test]
    fn a_quarter_turn_maps_x_onto_y() {
        let matrix = Matrix::rotate(90.0);
        let x = matrix.a * 1.0 + matrix.c * 0.0 + matrix.e;
        let y = matrix.b * 1.0 + matrix.d * 0.0 + matrix.f;
        assert!(x.abs() < 1e-5, "{x}");
        assert!((y - 1.0).abs() < 1e-5, "{y}");
    }

    #[test]
    fn identity_is_neutral() {
        let matrix = Matrix::chain(Matrix::translate(3.0, 4.0), Matrix::IDENTITY);
        assert!((matrix.e - 3.0).abs() < 1e-6);
        assert!((matrix.f - 4.0).abs() < 1e-6);
        assert!((matrix.a - 1.0).abs() < 1e-6, "{}", matrix.a);
    }
}
