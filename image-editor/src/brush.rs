//! Gesture-level brush opacity: union coverage is composited once over the source.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
use super::{blend, invalid, BrushStyle, Document, EditError, Raster, Tool};

/// Immutable gesture source and maximum coverage reached at each pixel.
/// Segment subdivision, repeated stationary events and crossing a stroke cannot
/// exceed its opacity. Start another stroke to build up additional paint.
pub struct BrushStroke {
    source: Raster,
    coverage: Vec<f32>,
    active: usize,
    radius: f32,
    color: [u8; 4],
    tool: Tool,
    style: BrushStyle,
}
impl Document {
    /// Capture one brush/eraser gesture. Mask tools use the same API via `edit_target_mask`.
    pub fn begin_brush_stroke(
        &self,
        radius: f32,
        color: [u8; 4],
        tool: Tool,
        style: BrushStyle,
    ) -> Result<BrushStroke, EditError> {
        self.require_unlocked()?;
        style.validate()?;
        if !radius.is_finite()
            || !(0.5..=256.0).contains(&radius)
            || !matches!(tool, Tool::Brush | Tool::Eraser)
        {
            return Err(invalid("Invalid brush gesture"));
        }
        Ok(BrushStroke {
            source: self.layers[self.active].raster.clone(),
            coverage: vec![0.0; self.mask.len()],
            active: self.active,
            radius,
            color,
            tool,
            style,
        })
    }
    /// Extend a stroke from its original source, honoring soft selection and opacity.
    pub fn brush_segment(
        &mut self,
        stroke: &mut BrushStroke,
        from: [f32; 2],
        to: [f32; 2],
    ) -> Result<(), EditError> {
        self.require_unlocked()?;
        if self.active != stroke.active
            || self.width != stroke.source.width
            || self.height != stroke.source.height
            || from
                .iter()
                .chain(to.iter())
                .any(|v| !v.is_finite() || v.abs() > 32768.0)
        {
            return Err(invalid("Brush gesture target or coordinates changed"));
        }
        let r = stroke.radius;
        let x0 = (from[0].min(to[0]) - r).floor().max(0.0) as u32;
        let y0 = (from[1].min(to[1]) - r).floor().max(0.0) as u32;
        let x1 = ((from[0].max(to[0]) + r).ceil().max(0.0) as u32).min(self.width);
        let y1 = ((from[1].max(to[1]) + r).ceil().max(0.0) as u32).min(self.height);
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
                if distance > r {
                    continue;
                }
                let edge = if stroke.style.hardness >= 1.0 {
                    1.0
                } else {
                    ((1.0 - distance / r) / (1.0 - stroke.style.hardness)).clamp(0.0, 1.0)
                };
                let i = y as usize * self.width as usize + x as usize;
                let amount = edge * self.coverage(i, stroke.style);
                if amount <= stroke.coverage[i] {
                    continue;
                }
                stroke.coverage[i] = amount;
                let pixel = &mut self.layers[self.active].raster.pixels[i * 4..i * 4 + 4];
                pixel.copy_from_slice(&stroke.source.pixels[i * 4..i * 4 + 4]);
                if stroke.tool == Tool::Eraser {
                    pixel[3] = (f32::from(pixel[3])
                        * (1.0 - amount * f32::from(stroke.color[3]) / 255.0))
                        .round() as u8;
                } else {
                    blend(pixel, stroke.color, amount);
                }
            }
        }
        Ok(())
    }
}
