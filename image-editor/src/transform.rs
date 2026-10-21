//! Canvas transforms share the same inverse map for every layer and the mask.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
use super::{byte_len, invalid, Document, EditError, PhotoGeometry, Raster, MAX_DOCUMENT_BYTES};

/// How source pixels are placed into the new canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasMode {
    /// Resample to the exact dimensions, independently along each axis.
    #[default]
    Scale,
    /// Preserve proportions and add transparent margins.
    Fit,
    /// Preserve proportions and crop excess pixels.
    Fill,
    /// Change bounds without resampling pixels; pad or crop at the anchor.
    Bounds,
}
impl Document {
    /// Resize all layers and the mask. Anchor components are 0 (start), 1 (center), or 2 (end).
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        mode: CanvasMode,
        anchor: [u8; 2],
    ) -> Result<(), EditError> {
        if anchor.iter().any(|&v| v > 2) {
            return Err(invalid("Invalid canvas anchor"));
        }
        let sx = f64::from(width) / f64::from(self.width);
        let sy = f64::from(height) / f64::from(self.height);
        let (sx, sy) = match mode {
            CanvasMode::Scale => (sx, sy),
            CanvasMode::Fit => (sx.min(sy), sx.min(sy)),
            CanvasMode::Fill => (sx.max(sy), sx.max(sy)),
            CanvasMode::Bounds => (1.0, 1.0),
        };
        let offset = |target: u32, source: u32, scale: f64, a: u8| {
            let value = (f64::from(target) - f64::from(source) * scale) * f64::from(a) / 2.0;
            if mode == CanvasMode::Bounds {
                value.floor()
            } else {
                value
            }
        };
        let dx = offset(width, self.width, sx, anchor[0]);
        let dy = offset(height, self.height, sy, anchor[1]);
        self.remap(width, height, true, |x, y| ((x - dx) / sx, (y - dy) / sy))
    }
    /// Rotate clockwise around the image center; optionally expand bounds to retain every corner.
    /// Colors use premultiplied-alpha bilinear sampling to avoid dark transparent fringes.
    pub fn rotate(&mut self, degrees: f64, expand: bool) -> Result<(), EditError> {
        if !degrees.is_finite() {
            return Err(invalid("Rotation angle must be finite"));
        }
        let degrees = degrees.rem_euclid(360.0);
        if degrees.abs() < 1e-10 {
            return Ok(());
        }
        let turns = (degrees / 90.0).round();
        if (degrees - turns * 90.0).abs() < 1e-10
            && (expand || self.width == self.height || turns as u32 == 2)
        {
            for _ in 0..turns as u32 {
                self.rotate90();
            }
            return Ok(());
        }
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let (nw, nh) = if expand {
            (
                (w * cos.abs() + h * sin.abs() - 1e-10).ceil() as u32,
                (w * sin.abs() + h * cos.abs() - 1e-10).ceil() as u32,
            )
        } else {
            (self.width, self.height)
        };
        self.remap(nw, nh, false, |x, y| {
            let x = x - f64::from(nw) / 2.0;
            let y = y - f64::from(nh) / 2.0;
            (cos * x + sin * y + w / 2.0, -sin * x + cos * y + h / 2.0)
        })
    }
    /// Rotate and scale the active layer around its nontransparent content center.
    /// Canvas dimensions, other layers and mask are unchanged; excess pixels are clipped.
    pub fn transform_layer(&mut self, degrees: f64, scale: [f64; 2]) -> Result<(), EditError> {
        self.transform_photo_layer(degrees, scale, PhotoGeometry::default())
    }
    /// Combine rotation, scaling, projective and radial correction in one resampling pass.
    pub fn transform_photo_layer(
        &mut self,
        degrees: f64,
        scale: [f64; 2],
        photo: PhotoGeometry,
    ) -> Result<(), EditError> {
        self.transform_photo_layer_about(degrees, scale, photo, self.active_layer_center())
    }
    /// Resample around an explicit pivot; used by the canvas transform preview.
    pub fn transform_photo_layer_about(
        &mut self,
        degrees: f64,
        scale: [f64; 2],
        photo: PhotoGeometry,
        center: [f64; 2],
    ) -> Result<(), EditError> {
        if center.iter().any(|v| !v.is_finite() || v.abs() > 32768.0) {
            return Err(invalid("Invalid transform pivot"));
        }
        self.require_unlocked()?;
        photo.validate()?;
        if !degrees.is_finite()
            || scale
                .iter()
                .any(|v| !v.is_finite() || !(0.01..=16.0).contains(v))
        {
            return Err(invalid(
                "Layer scale must be 1..1600% and angle must be finite",
            ));
        }
        if degrees.rem_euclid(360.0).abs() < 1e-10
            && scale.iter().all(|v| (*v - 1.0).abs() < f64::EPSILON)
            && photo == PhotoGeometry::default()
        {
            return Ok(());
        }
        let layer = &mut self.layers[self.active];
        let mut pixels = vec![0; byte_len(self.width, self.height)?];
        for y in 0..self.height {
            for x in 0..self.width {
                let [sx, sy] = photo.source_point(
                    [f64::from(x) + 0.5, f64::from(y) + 0.5],
                    [self.width, self.height],
                    degrees,
                    scale,
                    center,
                );
                let i = (y as usize * self.width as usize + x as usize) * 4;
                pixels[i..i + 4].copy_from_slice(&sample(&layer.raster, sx - 0.5, sy - 0.5));
            }
        }
        layer.raster = Raster::new(self.width, self.height, pixels)?;
        if let Some(mask) = &mut layer.mask {
            let mut pixels = vec![mask.default_color; mask.pixels.len()];
            for y in 0..self.height {
                for x in 0..self.width {
                    let [sx, sy] = photo.source_point(
                        [f64::from(x) + 0.5, f64::from(y) + 0.5],
                        [self.width, self.height],
                        degrees,
                        scale,
                        center,
                    );
                    pixels[y as usize * self.width as usize + x as usize] =
                        super::layer_compositing::sample_mask(
                            mask,
                            self.width,
                            self.height,
                            sx - 0.5,
                            sy - 0.5,
                        );
                }
            }
            mask.pixels = pixels;
        }
        Ok(())
    }
    pub(crate) fn active_layer_center(&self) -> [f64; 2] {
        let mut bounds = [self.width, self.height, 0, 0];
        for y in 0..self.height {
            for x in 0..self.width {
                if self.layers[self.active].raster.pixels
                    [(y as usize * self.width as usize + x as usize) * 4 + 3]
                    > 0
                {
                    bounds[0] = bounds[0].min(x);
                    bounds[1] = bounds[1].min(y);
                    bounds[2] = bounds[2].max(x + 1);
                    bounds[3] = bounds[3].max(y + 1);
                }
            }
        }
        if bounds[2] == 0 {
            return [f64::from(self.width) / 2.0, f64::from(self.height) / 2.0];
        }
        [
            f64::from(bounds[0] + bounds[2]) / 2.0,
            f64::from(bounds[1] + bounds[3]) / 2.0,
        ]
    }
    /// Import a centered layer, shrinking oversized sources to fit without upscaling smaller ones.
    pub fn import_layer(&mut self, name: String, source: &Raster) -> Result<(), EditError> {
        source.validate()?;
        let scale = (f64::from(self.width) / f64::from(source.width))
            .min(f64::from(self.height) / f64::from(source.height))
            .min(1.0);
        let width = (f64::from(source.width) * scale).round().max(1.0) as u32;
        let height = (f64::from(source.height) * scale).round().max(1.0) as u32;
        let image = if [width, height] == [source.width, source.height] {
            source.image()
        } else {
            image::imageops::resize(
                &source.image(),
                width,
                height,
                image::imageops::FilterType::Lanczos3,
            )
        };
        let mut target = image::RgbaImage::new(self.width, self.height);
        image::imageops::overlay(
            &mut target,
            &image,
            i64::from((self.width - width) / 2),
            i64::from((self.height - height) / 2),
        );
        self.add_layer(name, Raster::from_image(target))
    }
    /// Delete every layer and reset the mask, retaining canvas dimensions.
    /// A fresh visible, unlocked, opaque layer with transparent pixels is created.
    pub fn clear_canvas(&mut self) -> Result<(), EditError> {
        *self = Self::new(self.width, self.height)?;
        Ok(())
    }
    /// Delete other layers, preserving the active layer and mask.
    /// Locked layers are protected from deletion.
    pub fn retain_active_layer(&mut self) -> Result<(), EditError> {
        if self
            .layers
            .iter()
            .enumerate()
            .any(|(i, l)| l.locked && i != self.active)
        {
            return Err(invalid("Unlock other layers before deleting them"));
        }
        self.layers = vec![self.layers[self.active].clone()];
        self.active = 0;
        self.sync_layer_version();
        Ok(())
    }
    fn remap(
        &mut self,
        width: u32,
        height: u32,
        clamp_edges: bool,
        map: impl Fn(f64, f64) -> (f64, f64),
    ) -> Result<(), EditError> {
        let len = byte_len(width, height)?;
        if self.resized_memory_bytes(len) > MAX_DOCUMENT_BYTES {
            return Err(invalid("Transformed document exceeds memory budget"));
        }
        let mut layers = self.layers.clone();
        let mut mask = vec![0; len / 4];
        let mut selection = if self.selection.is_empty() {
            Vec::new()
        } else {
            vec![0; len / 4]
        };
        for layer in &mut layers {
            let mut pixels = vec![0; len];
            for y in 0..height {
                for x in 0..width {
                    let (sx, sy) = map(f64::from(x) + 0.5, f64::from(y) + 0.5);
                    let (sx, sy) = if clamp_edges {
                        if sx < 0.0
                            || sy < 0.0
                            || sx >= f64::from(self.width)
                            || sy >= f64::from(self.height)
                        {
                            continue;
                        }
                        (
                            (sx - 0.5).clamp(0.0, f64::from(self.width - 1)),
                            (sy - 0.5).clamp(0.0, f64::from(self.height - 1)),
                        )
                    } else {
                        (sx - 0.5, sy - 0.5)
                    };
                    let dst = (y as usize * width as usize + x as usize) * 4;
                    pixels[dst..dst + 4].copy_from_slice(&sample(&layer.raster, sx, sy));
                }
            }
            layer.raster = Raster::new(width, height, pixels)?;
            if let Some(mask) = &mut layer.mask {
                let mut pixels = vec![mask.default_color; len / 4];
                for y in 0..height {
                    for x in 0..width {
                        let (sx, sy) = map(f64::from(x) + 0.5, f64::from(y) + 0.5);
                        let (sx, sy) = if clamp_edges
                            && sx >= 0.0
                            && sy >= 0.0
                            && sx < f64::from(self.width)
                            && sy < f64::from(self.height)
                        {
                            (
                                (sx - 0.5).clamp(0.0, f64::from(self.width - 1)),
                                (sy - 0.5).clamp(0.0, f64::from(self.height - 1)),
                            )
                        } else {
                            (sx - 0.5, sy - 0.5)
                        };
                        pixels[y as usize * width as usize + x as usize] =
                            super::layer_compositing::sample_mask(
                                mask,
                                self.width,
                                self.height,
                                sx,
                                sy,
                            );
                    }
                }
                mask.pixels = pixels;
            }
        }
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = map(f64::from(x) + 0.5, f64::from(y) + 0.5);
                if sx >= 0.0
                    && sy >= 0.0
                    && sx < f64::from(self.width)
                    && sy < f64::from(self.height)
                {
                    mask[y as usize * width as usize + x as usize] =
                        self.mask[sy.floor() as usize * self.width as usize + sx.floor() as usize];
                    if !selection.is_empty() {
                        selection[y as usize * width as usize + x as usize] = self.selection
                            [sy.floor() as usize * self.width as usize + sx.floor() as usize];
                    }
                }
            }
        }
        self.layers = layers;
        self.mask = mask;
        self.selection = selection;
        self.normalize_selection();
        self.width = width;
        self.height = height;
        Ok(())
    }
}
pub(super) fn sample(raster: &Raster, x: f64, y: f64) -> [u8; 4] {
    if x.fract().abs() < f64::EPSILON
        && y.fract().abs() < f64::EPSILON
        && x >= 0.0
        && y >= 0.0
        && x < f64::from(raster.width)
        && y < f64::from(raster.height)
    {
        let i = (y as usize * raster.width as usize + x as usize) * 4;
        return raster.pixels[i..i + 4].try_into().expect("RGBA pixel");
    }
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let mut sum = [0.0; 4];
    for (dx, dy, weight) in [
        (0.0, 0.0, (1.0 - fx) * (1.0 - fy)),
        (1.0, 0.0, fx * (1.0 - fy)),
        (0.0, 1.0, (1.0 - fx) * fy),
        (1.0, 1.0, fx * fy),
    ] {
        let (px, py) = (ix + dx, iy + dy);
        if px >= 0.0 && py >= 0.0 && px < f64::from(raster.width) && py < f64::from(raster.height) {
            let i = (py as usize * raster.width as usize + px as usize) * 4;
            let alpha = f64::from(raster.pixels[i + 3]) / 255.0;
            for (c, value) in sum.iter_mut().take(3).enumerate() {
                *value += f64::from(raster.pixels[i + c]) * alpha * weight;
            }
            sum[3] += alpha * weight;
        }
    }
    if sum[3] <= 0.0 {
        return [0; 4];
    }
    [
        (sum[0] / sum[3]).round() as u8,
        (sum[1] / sum[3]).round() as u8,
        (sum[2] / sum[3]).round() as u8,
        (sum[3] * 255.0).round() as u8,
    ]
}
