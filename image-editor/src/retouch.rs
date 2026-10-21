//! Snapshot-based clone and color-adaptive healing, in canvas coordinates.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use super::{blend, invalid, BrushStyle, Document, EditError, Raster};

/// Texture transfer mode for manual retouching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetouchMode {
    /// Copy source pixels with straight-alpha source-over compositing.
    Clone,
    /// Retain source texture while matching the destination's local mean color.
    /// This is color-adaptive cloning, not a Poisson solver.
    Heal,
}
/// Immutable sample captured once per gesture. Overlapping strokes cannot feed back
/// from freshly painted pixels. A new gesture starts at the chosen source again.
pub struct RetouchStroke {
    source: Raster,
    offset: [f32; 2],
    radius: f32,
    style: BrushStyle,
    correction: [f32; 3],
}
impl Document {
    /// Capture the active layer and source/destination anchors before painting.
    pub fn begin_retouch(
        &self,
        source: [f32; 2],
        destination: [f32; 2],
        radius: f32,
        mode: RetouchMode,
        style: BrushStyle,
    ) -> Result<RetouchStroke, EditError> {
        self.require_unlocked()?;
        style.validate()?;
        if !radius.is_finite()
            || !(0.5..=256.0).contains(&radius)
            || !source
                .iter()
                .chain(destination.iter())
                .all(|v| v.is_finite())
            || source[0] < 0.0
            || source[1] < 0.0
            || source[0] >= self.width as f32
            || source[1] >= self.height as f32
            || destination.iter().any(|v| v.abs() > 32768.0)
        {
            return Err(invalid("Invalid retouch anchors or radius"));
        }
        let raster = &self.layers[self.active].raster;
        let mut correction = [0.0; 3];
        if mode == RetouchMode::Heal {
            let a = local_mean(raster, source, radius);
            let b = local_mean(raster, destination, radius);
            if let (Some(a), Some(b)) = (a, b) {
                for c in 0..3 {
                    correction[c] = b[c] - a[c];
                }
            }
        }
        Ok(RetouchStroke {
            source: raster.clone(),
            offset: [source[0] - destination[0], source[1] - destination[1]],
            radius,
            style,
            correction,
        })
    }
    /// Paint an interpolated segment from an immutable gesture sample. Selection,
    /// optional inpaint clipping, hardness, opacity and locked layers are respected.
    /// Source coordinates outside the canvas contribute no paint.
    pub fn retouch_segment(
        &mut self,
        stroke: &RetouchStroke,
        from: [f32; 2],
        to: [f32; 2],
    ) -> Result<(), EditError> {
        self.require_unlocked()?;
        if self.width != stroke.source.width
            || self.height != stroke.source.height
            || !from
                .iter()
                .chain(to.iter())
                .all(|v| v.is_finite() && v.abs() <= 32768.0)
        {
            return Err(invalid("Retouch coordinates or dimensions differ"));
        }
        let radius = stroke.radius;
        let x0 = (from[0].min(to[0]) - radius).floor().max(0.0) as u32;
        let y0 = (from[1].min(to[1]) - radius).floor().max(0.0) as u32;
        let x1 = ((from[0].max(to[0]) + radius).ceil().max(0.0) as u32).min(self.width);
        let y1 = ((from[1].max(to[1]) + radius).ceil().max(0.0) as u32).min(self.height);
        let delta = [to[0] - from[0], to[1] - from[1]];
        let length = delta[0] * delta[0] + delta[1] * delta[1];
        for y in y0..y1 {
            for x in x0..x1 {
                let p = [x as f32 + 0.5 - from[0], y as f32 + 0.5 - from[1]];
                let t = if length == 0.0 {
                    0.0
                } else {
                    ((p[0] * delta[0] + p[1] * delta[1]) / length).clamp(0.0, 1.0)
                };
                let distance = (p[0] - t * delta[0]).hypot(p[1] - t * delta[1]);
                if distance > radius {
                    continue;
                }
                let sx = x as f32 + stroke.offset[0];
                let sy = y as f32 + stroke.offset[1];
                if sx < 0.0 || sy < 0.0 || sx >= self.width as f32 || sy >= self.height as f32 {
                    continue;
                }
                let mut color =
                    super::transform::sample(&stroke.source, f64::from(sx), f64::from(sy));
                if color[3] == 0 {
                    continue;
                }
                for (c, correction) in color.iter_mut().take(3).zip(stroke.correction) {
                    *c = (f32::from(*c) + correction).round().clamp(0.0, 255.0) as u8;
                }
                let edge = if stroke.style.hardness >= 1.0 {
                    1.0
                } else {
                    ((1.0 - distance / radius) / (1.0 - stroke.style.hardness)).clamp(0.0, 1.0)
                };
                let i = y as usize * self.width as usize + x as usize;
                let amount = edge * self.coverage(i, stroke.style);
                blend(
                    &mut self.layers[self.active].raster.pixels[i * 4..i * 4 + 4],
                    color,
                    amount,
                );
            }
        }
        Ok(())
    }
}
// Two bounded patch scans once per gesture, rather than a neighborhood per pixel.
fn local_mean(raster: &Raster, at: [f32; 2], radius: f32) -> Option<[f32; 3]> {
    let mut sum = [0.0; 3];
    let mut alpha = 0.0;
    let x0 = (at[0] - radius).floor().max(0.0) as u32;
    let y0 = (at[1] - radius).floor().max(0.0) as u32;
    let x1 = ((at[0] + radius).ceil().max(0.0) as u32).min(raster.width);
    let y1 = ((at[1] + radius).ceil().max(0.0) as u32).min(raster.height);
    for y in y0..y1 {
        for x in x0..x1 {
            if (x as f32 + 0.5 - at[0]).hypot(y as f32 + 0.5 - at[1]) > radius {
                continue;
            }
            let p = raster.pixel(x, y).expect("bounded patch");
            let a = f32::from(p[3]) / 255.0;
            alpha += a;
            for c in 0..3 {
                sum[c] += f32::from(p[c]) * a;
            }
        }
    }
    (alpha > 0.0).then(|| sum.map(|v| v / alpha))
}
