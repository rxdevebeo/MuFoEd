//! Manual projective, shear and radial lens correction using inverse sampling.
use super::{invalid, EditError};
/// Manual photo geometry, separate from a calibrated camera or lens profile.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhotoGeometry {
    /// Horizontal perspective coefficient, -0.4..0.4.
    pub horizontal: f64,
    /// Vertical perspective coefficient, -0.4..0.4.
    pub vertical: f64,
    /// Horizontal shear, -0.5..0.5.
    pub shear: f64,
    /// First radial lens coefficient, -0.5..0.5.
    pub radial: f64,
    /// Second radial lens coefficient, -0.25..0.25.
    pub radial2: f64,
}
impl PhotoGeometry {
    pub(crate) fn validate(self) -> Result<(), EditError> {
        for (v, max) in [
            (self.horizontal, 0.4),
            (self.vertical, 0.4),
            (self.shear, 0.5),
            (self.radial, 0.5),
            (self.radial2, 0.25),
        ] {
            if !v.is_finite() || v.abs() > max {
                return Err(invalid("Invalid photo geometry coefficient"));
            }
        }
        Ok(())
    }
    // Pixel-edge coordinates. Shared by the raster commit and GPU preview mesh.
    pub(crate) fn source_point(
        self,
        point: [f64; 2],
        dimensions: [u32; 2],
        angle: f64,
        scale: [f64; 2],
        center: [f64; 2],
    ) -> [f64; 2] {
        let [w, h] = dimensions.map(f64::from);
        let nx = (point[0] - w / 2.0) / (w / 2.0);
        let ny = (point[1] - h / 2.0) / (h / 2.0);
        let denominator = 1.0 + self.horizontal * nx + self.vertical * ny;
        let x = (nx - self.shear * ny) / denominator * w / 2.0;
        let y = ny / denominator * h / 2.0;
        let radius = w.max(h) / 2.0;
        let r2 = (x * x + y * y) / (radius * radius);
        let radial = 1.0 + self.radial * r2 + self.radial2 * r2 * r2;
        let dx = x * radial + w / 2.0 - center[0];
        let dy = y * radial + h / 2.0 - center[1];
        let (sin, cos) = angle.rem_euclid(360.0).to_radians().sin_cos();
        [
            (cos * dx + sin * dy) / scale[0] + center[0],
            (-sin * dx + cos * dy) / scale[1] + center[1],
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inverse_projective_and_radial_coordinates_match_analytic_points() {
        let p = PhotoGeometry {
            horizontal: 0.2,
            ..Default::default()
        }
        .source_point([150.0, 50.0], [200, 100], 0.0, [1.0; 2], [100.0, 50.0]);
        assert!((p[0] - (100.0 + 50.0 / 1.1)).abs() < 1e-10);
        let p = PhotoGeometry {
            radial: 0.2,
            ..Default::default()
        }
        .source_point([150.0, 50.0], [200, 100], 0.0, [1.0; 2], [100.0, 50.0]);
        assert!((p[0] - 152.5).abs() < 1e-10);
        let p = PhotoGeometry {
            shear: 0.2,
            ..Default::default()
        }
        .source_point([100.0, 75.0], [200, 100], 0.0, [1.0; 2], [100.0, 50.0]);
        assert!((p[0] - 90.0).abs() < 1e-10);
    }
}
