//! Levels/curves share one byte LUT path for preview and committed pixels.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use super::{invalid, Document, EditError, Raster};
/// Input range, gamma and output range for one channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelChannel {
    /// Input black/white, strictly increasing.
    pub input: [u8; 2],
    /// Midtone gamma, 0.1..10; greater values brighten midtones.
    pub gamma: f32,
    /// Output black/white, increasing or equal.
    pub output: [u8; 2],
}
impl Default for LevelChannel {
    fn default() -> Self {
        Self {
            input: [0, 255],
            gamma: 1.0,
            output: [0, 255],
        }
    }
}
/// Master RGB, red, green, blue levels, composed in that order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Levels {
    /// Master, R, G and B parameters.
    pub channels: [LevelChannel; 4],
}
/// Smooth curve knots, with unique ascending x coordinates and endpoints x=0/255.
#[derive(Clone, Debug, PartialEq)]
pub struct ToneCurve {
    /// Input/output byte pairs; endpoints may move vertically.
    pub points: Vec<[u8; 2]>,
}
impl Default for ToneCurve {
    fn default() -> Self {
        Self {
            points: vec![[0, 0], [255, 255]],
        }
    }
}
/// Master RGB, red, green, blue curves. Each channel accepts 2..32 knots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curves {
    /// Master, R, G and B curves.
    pub channels: [ToneCurve; 4],
}
/// A destructive raster correction. Parameters are validated before pixel mutation.
#[derive(Clone, Debug, PartialEq)]
pub enum ToneAdjustment {
    /// Black/white range and gamma.
    Levels(Levels),
    /// Shape-preserving cubic curves, without overshoot between adjacent knots.
    Curves(Curves),
}
impl ToneAdjustment {
    /// Compose master then individual channels into exact RGB byte lookup tables.
    pub fn lookup_tables(&self) -> Result<[[u8; 256]; 3], EditError> {
        let mut tables = [[0; 256]; 4];
        for (i, table) in tables.iter_mut().enumerate() {
            match self {
                Self::Levels(levels) => {
                    let c = levels.channels[i];
                    if c.input[0] >= c.input[1]
                        || c.output[0] > c.output[1]
                        || !c.gamma.is_finite()
                        || !(0.1..=10.0).contains(&c.gamma)
                    {
                        return Err(invalid("Levels: input black must precede white, output must increase, gamma 0.1..10"));
                    }
                    for (x, y) in table.iter_mut().enumerate() {
                        let v = ((x as f32 - f32::from(c.input[0]))
                            / f32::from(c.input[1] - c.input[0]))
                        .clamp(0.0, 1.0)
                        .powf(1.0 / c.gamma);
                        *y = (f32::from(c.output[0]) + v * f32::from(c.output[1] - c.output[0]))
                            .round() as u8;
                    }
                }
                Self::Curves(curves) => {
                    *table = curves.channels[i].lookup()?;
                }
            }
        }
        Ok(std::array::from_fn(|c| {
            std::array::from_fn(|x| tables[c + 1][tables[0][x] as usize])
        }))
    }
    /// Apply to straight RGB with exact alpha preservation. Empty coverage means all
    /// pixels; otherwise its length must equal pixel count. Hidden RGB is preserved.
    pub fn apply(&self, source: &Raster, selection: &[u8]) -> Result<Raster, EditError> {
        let tables = self.lookup_tables()?;
        if !selection.is_empty() && selection.len() != source.pixels.len() / 4 {
            return Err(invalid("Tone selection dimensions differ"));
        }
        let mut result = source.clone();
        for (i, p) in result.pixels.chunks_exact_mut(4).enumerate() {
            if p[3] == 0 {
                continue;
            }
            let coverage = u32::from(selection.get(i).copied().unwrap_or(255));
            for c in 0..3 {
                let from = u32::from(p[c]);
                let to = u32::from(tables[c][p[c] as usize]);
                p[c] = u8::try_from((from * (255 - coverage) + to * coverage + 127) / 255)
                    .expect("bounded RGB");
            }
        }
        Ok(result)
    }
}
impl ToneCurve {
    /// Monotone Hermite interpolation within every segment, including nonmonotone
    /// whole curves. Limiting tangents prevents halos from interpolation overshoot.
    pub fn lookup(&self) -> Result<[u8; 256], EditError> {
        let knots = &self.points;
        let count = knots.len();
        if !(2..=32).contains(&count)
            || knots[0][0] != 0
            || knots[count - 1][0] != 255
            || knots.windows(2).any(|w| w[0][0] >= w[1][0])
        {
            return Err(invalid(
                "Curves require 2..32 ordered knots with endpoints at 0 and 255",
            ));
        }
        let slopes: Vec<f32> = knots
            .windows(2)
            .map(|w| (f32::from(w[1][1]) - f32::from(w[0][1])) / f32::from(w[1][0] - w[0][0]))
            .collect();
        let mut tangent = vec![0.0; count];
        tangent[0] = slopes[0];
        tangent[count - 1] = slopes[count - 2];
        for segment in 1..count - 1 {
            if slopes[segment - 1] * slopes[segment] > 0.0 {
                tangent[segment] = (slopes[segment - 1] + slopes[segment]) * 0.5;
            }
        }
        for (segment, &slope) in slopes.iter().enumerate() {
            if slope == 0.0 {
                tangent[segment] = 0.0;
                tangent[segment + 1] = 0.0;
                continue;
            }
            let norm = (tangent[segment] / slope).hypot(tangent[segment + 1] / slope);
            if norm > 3.0 {
                tangent[segment] *= 3.0 / norm;
                tangent[segment + 1] *= 3.0 / norm;
            }
        }
        Ok(std::array::from_fn(|x| {
            let segment = knots
                .partition_point(|v| usize::from(v[0]) <= x)
                .saturating_sub(1)
                .min(count - 2);
            let width = f32::from(knots[segment + 1][0] - knots[segment][0]);
            let fraction = (x as f32 - f32::from(knots[segment][0])) / width;
            let (left, right) = (
                f32::from(knots[segment][1]),
                f32::from(knots[segment + 1][1]),
            );
            let value = (2.0 * fraction * fraction * fraction - 3.0 * fraction * fraction + 1.0)
                * left
                + (fraction * fraction * fraction - 2.0 * fraction * fraction + fraction)
                    * width
                    * tangent[segment]
                + (-2.0 * fraction * fraction * fraction + 3.0 * fraction * fraction) * right
                + (fraction * fraction * fraction - fraction * fraction)
                    * width
                    * tangent[segment + 1];
            value.clamp(left.min(right), left.max(right)).round() as u8
        }))
    }
}
/// Source histogram: luminance, red, green, blue. Counts use alpha×selection
/// weights (an opaque selected pixel weighs 65025); invisible RGB contributes zero.
#[derive(Clone, Debug)]
pub struct Histogram {
    /// Weighted bins for luminance, R, G and B.
    pub bins: [[u64; 256]; 4],
}
impl Histogram {
    /// Measure the same selected source used by the correction.
    pub fn from_raster(source: &Raster, selection: &[u8]) -> Result<Self, EditError> {
        if !selection.is_empty() && selection.len() != source.pixels.len() / 4 {
            return Err(invalid("Histogram selection dimensions differ"));
        }
        let mut bins = [[0; 256]; 4];
        for (i, p) in source.pixels.chunks_exact(4).enumerate() {
            let weight = u64::from(p[3]) * u64::from(selection.get(i).copied().unwrap_or(255));
            let luma =
                (2126 * u32::from(p[0]) + 7152 * u32::from(p[1]) + 722 * u32::from(p[2]) + 5000)
                    / 10000;
            bins[0][luma as usize] += weight;
            for c in 0..3 {
                bins[c + 1][p[c] as usize] += weight;
            }
        }
        Ok(Self { bins })
    }
}
impl Document {
    /// Preview correction as a complete visible composite, identical to PNG export
    /// after applying it. No document pixels or presentation properties are changed.
    pub fn preview_tone(&self, adjustment: &ToneAdjustment) -> Result<Raster, EditError> {
        let corrected = adjustment.apply(&self.layers[self.active].raster, &self.selection)?;
        Ok(self.flatten_preview(Some(&corrected)))
    }
    /// Correct only the unlocked active layer, honoring soft manual selection.
    pub fn correct_tone(&mut self, adjustment: &ToneAdjustment) -> Result<(), EditError> {
        self.require_unlocked()?;
        let result = adjustment.apply(&self.layers[self.active].raster, &self.selection)?;
        self.layers[self.active].raster = result;
        Ok(())
    }
}
