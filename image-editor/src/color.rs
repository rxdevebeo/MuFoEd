//! Color corrections on straight RGB; alpha and other layers are preserved.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
use super::{invalid, Document, EditError, Raster};

/// Combined photo correction applied once to the source, with neutral defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorCorrection {
    /// Exposure in stops, -5..5.
    pub exposure: f32,
    /// Additive brightness, -1..1.
    pub brightness: f32,
    /// Contrast multiplier around mid-gray, 0..3.
    pub contrast: f32,
    /// Saturation multiplier, 0..3.
    pub saturation: f32,
    /// Hue rotation in degrees, -180..180.
    pub hue: f32,
    /// Warm/cool red-blue balance, -1..1.
    pub temperature: f32,
    /// Gamma, 0.1..5.
    pub gamma: f32,
}
impl Default for ColorCorrection {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0.0,
            temperature: 0.0,
            gamma: 1.0,
        }
    }
}
impl ColorCorrection {
    /// Correct a standalone raster, including previews. Invalid parameters fail before mutation.
    pub fn apply(&self, source: &Raster) -> Result<Raster, EditError> {
        for (v, min, max) in [
            (self.exposure, -5.0, 5.0),
            (self.brightness, -1.0, 1.0),
            (self.contrast, 0.0, 3.0),
            (self.saturation, 0.0, 3.0),
            (self.hue, -180.0, 180.0),
            (self.temperature, -1.0, 1.0),
            (self.gamma, 0.1, 5.0),
        ] {
            if !v.is_finite() || !(min..=max).contains(&v) {
                return Err(invalid("Invalid color correction"));
            }
        }
        if *self == Self::default() {
            return Ok(source.clone());
        }
        let mut image = source.image();
        if self.hue.abs() > f32::EPSILON {
            image = image::imageops::huerotate(&image, self.hue.round() as i32);
        }
        let exposure = self.exposure.exp2();
        for p in image.pixels_mut() {
            let mut rgb = [
                f32::from(p[0]) / 255.0,
                f32::from(p[1]) / 255.0,
                f32::from(p[2]) / 255.0,
            ];
            if self.exposure.abs() > f32::EPSILON {
                for c in &mut rgb {
                    let linear = if *c <= 0.04045 {
                        *c / 12.92
                    } else {
                        ((*c + 0.055) / 1.055).powf(2.4)
                    };
                    let linear = (linear * exposure).clamp(0.0, 1.0);
                    *c = if linear <= 0.003_130_8 {
                        linear * 12.92
                    } else {
                        1.055 * linear.powf(1.0 / 2.4) - 0.055
                    };
                }
            }
            let luminance = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            for (i, c) in rgb.iter_mut().enumerate() {
                *c = luminance + (*c - luminance) * self.saturation;
                *c += match i {
                    0 => self.temperature * 0.15,
                    2 => -self.temperature * 0.15,
                    _ => 0.0,
                };
                *c = ((*c - 0.5) * self.contrast + 0.5 + self.brightness).clamp(0.0, 1.0);
                p[i] = (c.powf(1.0 / self.gamma) * 255.0).round() as u8;
            }
        }
        // Hue routines and future operations must never alter transparency.
        for (p, s) in image.pixels_mut().zip(source.pixels.chunks_exact(4)) {
            p[3] = s[3];
        }
        Ok(Raster::from_image(image))
    }
}
impl Document {
    /// Apply combined color correction to the unlocked active layer.
    pub fn correct_color(&mut self, correction: ColorCorrection) -> Result<(), EditError> {
        self.require_unlocked()?;
        let layer = &mut self.layers[self.active];
        let mut result = correction.apply(&layer.raster)?;
        result.restrict_rgb_adjustment(&layer.raster, &self.selection);
        layer.raster = result;
        Ok(())
    }
}
