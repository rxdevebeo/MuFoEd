//! Pixel operations used by the manual workbench; all coordinates are canvas-local.
use super::{blend, invalid, Document, EditError, Raster, Tool, MAX_DOCUMENT_BYTES};

/// Brush strength and edge softness. The shared mask is opt-in for manual drawing.
#[derive(Clone, Copy, Debug)]
pub struct BrushStyle {
    /// Per-segment opacity, from 0 to 1.
    pub opacity: f32,
    /// Solid inner radius, as a fraction of the brush radius, from 0 to 1.
    pub hardness: f32,
    /// Multiply paint coverage by the shared grayscale mask.
    pub respect_mask: bool,
}
impl Default for BrushStyle {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            hardness: 1.0,
            respect_mask: false,
        }
    }
}
impl BrushStyle {
    pub(super) fn validate(self) -> Result<(), EditError> {
        if ![self.opacity, self.hardness]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return Err(invalid(
                "Opacity and hardness must be finite values in 0..1",
            ));
        }
        Ok(())
    }
}

/// Raster shape drawn into the active layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A straight, round-capped line.
    Line,
    /// An axis-aligned rectangle.
    Rectangle,
    /// An ellipse inside the drag bounds.
    Ellipse,
}

impl Raster {
    /// Sample straight-alpha RGBA; outside coordinates return `None`.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some(self.pixels[i..i + 4].try_into().expect("RGBA"))
    }
    /// A bounded aspect-preserving thumbnail, retaining transparency.
    pub fn thumbnail(&self, side: u32) -> Result<Self, EditError> {
        if !(1..=256).contains(&side) {
            return Err(invalid("Thumbnail side must be in 1..256"));
        }
        Ok(Self::from_image(image::imageops::thumbnail(
            &self.image(),
            side,
            side,
        )))
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
impl Document {
    pub(super) fn require_unlocked(&self) -> Result<(), EditError> {
        if self.layers[self.active].locked {
            return Err(invalid(
                "Слой заблокирован. Снимите блокировку перед редактированием.",
            ));
        }
        Ok(())
    }
    /// Lock or unlock active-layer pixel edits; presentation properties stay editable.
    pub fn set_layer_locked(&mut self, locked: bool) {
        self.layers[self.active].locked = locked;
    }
    /// Duplicate immediately above the active layer; its copy starts unlocked.
    pub fn duplicate_layer(&mut self) -> Result<(), EditError> {
        if self.layers.len() >= 16
            || self.memory_bytes() + self.layers[self.active].memory_bytes() > MAX_DOCUMENT_BYTES
        {
            return Err(invalid("Document layer/memory limit reached"));
        }
        let mut layer = self.layers[self.active].clone();
        layer.name.push_str(" — копия");
        layer.locked = false;
        self.active += 1;
        self.layers.insert(self.active, layer);
        Ok(())
    }
    /// Move a layer to an exact stack position, retaining the selected layer's identity.
    pub fn reorder_layer(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        if from >= self.layers.len() || to >= self.layers.len() {
            return Err(invalid("Unknown layer"));
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        self.active = if self.active == from {
            to
        } else {
            let after_remove = self.active - usize::from(self.active > from);
            after_remove + usize::from(after_remove >= to)
        };
        Ok(())
    }
    /// Merge the active visible layer into the visible layer below, baking both opacities.
    /// Locks/hidden layers are rejected. RGBA8 baking can introduce rounding at lower layers.
    pub fn merge_down(&mut self) -> Result<(), EditError> {
        self.require_unlocked()?;
        if self.active == 0 {
            return Err(invalid("No layer below the active layer"));
        }
        let below = self.active - 1;
        if self.layers[below].locked
            || !self.layers[below].visible
            || !self.layers[self.active].visible
        {
            return Err(invalid("Merge requires two visible, unlocked layers"));
        }
        if below > 0
            && [below, self.active]
                .iter()
                .any(|&i| self.layers[i].blend_mode != super::LayerBlendMode::Normal)
        {
            return Err(invalid("Blend modes depend on lower layers; merge is only safe at the bottom of the stack or with Normal modes"));
        }
        let mut pixels = vec![0; self.layers[below].raster.pixels.len()];
        for index in [below, self.active] {
            self.composite_layer(&mut pixels, index, &self.layers[index].raster);
        }
        self.layers[below].raster.pixels = pixels;
        self.layers[below].opacity = 255;
        self.layers[below].mask = None;
        self.layers[below].blend_mode = super::LayerBlendMode::Normal;
        self.layers[below].recipe = None;
        self.layers.remove(self.active);
        self.active = below;
        self.sync_layer_version();
        Ok(())
    }
    /// Translate active-layer pixels without wraparound; uncovered pixels become transparent.
    pub fn translate_layer(&mut self, dx: i32, dy: i32) -> Result<(), EditError> {
        self.require_unlocked()?;
        if dx.unsigned_abs() > 32768 || dy.unsigned_abs() > 32768 {
            return Err(invalid("Invalid layer translation"));
        }
        let source = &self.layers[self.active].raster.pixels;
        let mut pixels = vec![0; source.len()];
        for y in 0..self.height.cast_signed() {
            for x in 0..self.width.cast_signed() {
                let (sx, sy) = (x - dx, y - dy);
                if sx >= 0
                    && sy >= 0
                    && sx < self.width.cast_signed()
                    && sy < self.height.cast_signed()
                {
                    let dst = (y as usize * self.width as usize + x as usize) * 4;
                    let src = (sy as usize * self.width as usize + sx as usize) * 4;
                    pixels[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
                }
            }
        }
        self.layers[self.active].raster.pixels = pixels;
        if let Some(mask) = &mut self.layers[self.active].mask {
            let mut moved = vec![mask.default_color; mask.pixels.len()];
            for y in 0..self.height.cast_signed() {
                for x in 0..self.width.cast_signed() {
                    let (sx, sy) = (x - dx, y - dy);
                    if sx >= 0
                        && sy >= 0
                        && sx < self.width.cast_signed()
                        && sy < self.height.cast_signed()
                    {
                        moved[y as usize * self.width as usize + x as usize] =
                            mask.pixels[sy as usize * self.width as usize + sx as usize];
                    }
                }
            }
            mask.pixels = moved;
        }
        Ok(())
    }
    pub(super) fn coverage(&self, index: usize, style: BrushStyle) -> f32 {
        style.opacity
            * self
                .selection
                .get(index)
                .map_or(1.0, |v| f32::from(*v) / 255.0)
            * if style.respect_mask {
                f32::from(self.mask[index]) / 255.0
            } else {
                1.0
            }
    }
    /// Draw one interpolated segment with softness and opacity.
    pub fn stroke_with_style(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        radius: f32,
        color: [u8; 4],
        tool: Tool,
        style: BrushStyle,
    ) -> Result<(), EditError> {
        points(from, to)?;
        style.validate()?;
        if !radius.is_finite() || !(0.5..=256.0).contains(&radius) {
            return Err(invalid("Invalid brush radius"));
        }
        if matches!(tool, Tool::Brush | Tool::Eraser) {
            self.require_unlocked()?;
        }
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
                let edge = if style.hardness >= 1.0 {
                    1.0
                } else {
                    ((1.0 - distance / radius) / (1.0 - style.hardness)).clamp(0.0, 1.0)
                };
                let index = y as usize * self.width as usize + x as usize;
                let amount = edge
                    * if matches!(tool, Tool::Mask | Tool::MaskEraser) {
                        style.opacity
                    } else {
                        self.coverage(index, style)
                    };
                match tool {
                    Tool::Mask | Tool::MaskEraser => {
                        let target = if tool == Tool::Mask { 255.0 } else { 0.0 };
                        self.mask[index] = (f32::from(self.mask[index]) * (1.0 - amount)
                            + target * amount)
                            .round() as u8;
                    }
                    Tool::Brush => blend(
                        &mut self.layers[self.active].raster.pixels[index * 4..index * 4 + 4],
                        color,
                        amount,
                    ),
                    Tool::Eraser => {
                        let alpha = &mut self.layers[self.active].raster.pixels[index * 4 + 3];
                        *alpha = (f32::from(*alpha) * (1.0 - amount * f32::from(color[3]) / 255.0))
                            .round() as u8;
                    }
                }
            }
        }
        Ok(())
    }
    /// Paint a line or bounded shape into the active layer; line width is in pixels.
    #[allow(clippy::too_many_arguments)] // Explicit geometry, shape and brush settings.
    pub fn draw_shape(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        shape: Shape,
        width: f32,
        filled: bool,
        color: [u8; 4],
        style: BrushStyle,
    ) -> Result<(), EditError> {
        self.require_unlocked()?;
        points(from, to)?;
        style.validate()?;
        if !width.is_finite() || !(1.0..=512.0).contains(&width) {
            return Err(invalid("Invalid shape stroke width"));
        }
        if shape == Shape::Line {
            return self.stroke_with_style(from, to, width / 2.0, color, Tool::Brush, style);
        }
        let min = [from[0].min(to[0]), from[1].min(to[1])];
        let max = [from[0].max(to[0]), from[1].max(to[1])];
        let rx = ((max[0] - min[0]) / 2.0).max(0.5);
        let ry = ((max[1] - min[1]) / 2.0).max(0.5);
        for y in min[1].floor().max(0.0) as u32..(max[1].ceil().max(0.0) as u32).min(self.height) {
            for x in min[0].floor().max(0.0) as u32..(max[0].ceil().max(0.0) as u32).min(self.width)
            {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let inside = if shape == Shape::Rectangle {
                    filled
                        || p[0] - min[0] <= width
                        || max[0] - p[0] <= width
                        || p[1] - min[1] <= width
                        || max[1] - p[1] <= width
                } else {
                    let dx = p[0] - min[0].midpoint(max[0]);
                    let dy = p[1] - min[1].midpoint(max[1]);
                    let outer = (dx / rx).powi(2) + (dy / ry).powi(2) <= 1.0;
                    let inner = rx > width
                        && ry > width
                        && (dx / (rx - width)).powi(2) + (dy / (ry - width)).powi(2) < 1.0;
                    outer && (filled || !inner)
                };
                if inside {
                    let index = y as usize * self.width as usize + x as usize;
                    let amount = self.coverage(index, style);
                    blend(
                        &mut self.layers[self.active].raster.pixels[index * 4..index * 4 + 4],
                        color,
                        amount,
                    );
                }
            }
        }
        Ok(())
    }
    /// Linear foreground-to-background gradient across the layer, optionally clipped by mask.
    pub fn gradient(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        foreground: [u8; 4],
        background: [u8; 4],
        style: BrushStyle,
    ) -> Result<(), EditError> {
        self.require_unlocked()?;
        points(from, to)?;
        style.validate()?;
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let length = dx * dx + dy * dy;
        if length < 0.001 {
            return Err(invalid("Drag to define a gradient"));
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let t = (((x as f32 + 0.5 - from[0]) * dx + (y as f32 + 0.5 - from[1]) * dy)
                    / length)
                    .clamp(0.0, 1.0);
                let mut color = [0; 4];
                let alpha = f32::from(foreground[3]) * (1.0 - t) + f32::from(background[3]) * t;
                for c in 0..3 {
                    color[c] = if alpha > 0.0 {
                        ((f32::from(foreground[c]) * f32::from(foreground[3]) * (1.0 - t)
                            + f32::from(background[c]) * f32::from(background[3]) * t)
                            / alpha)
                            .round() as u8
                    } else {
                        0
                    };
                }
                color[3] = alpha.round() as u8;
                let index = y as usize * self.width as usize + x as usize;
                let amount = self.coverage(index, style);
                blend(
                    &mut self.layers[self.active].raster.pixels[index * 4..index * 4 + 4],
                    color,
                    amount,
                );
            }
        }
        Ok(())
    }
    /// Contiguous four-connected bucket fill. Tolerance compares all four source channels.
    pub fn bucket_fill(
        &mut self,
        x: u32,
        y: u32,
        color: [u8; 4],
        tolerance: u8,
        style: BrushStyle,
    ) -> Result<(), EditError> {
        self.require_unlocked()?;
        style.validate()?;
        let target = self.layers[self.active]
            .raster
            .pixel(x, y)
            .ok_or_else(|| invalid("Fill point is outside the canvas"))?;
        let start = y as usize * self.width as usize + x as usize;
        let mut seen = vec![false; self.mask.len()];
        let mut queue = std::collections::VecDeque::from([start]);
        seen[start] = true;
        while let Some(index) = queue.pop_front() {
            let source = &self.layers[self.active].raster.pixels[index * 4..index * 4 + 4];
            if !source
                .iter()
                .zip(target)
                .all(|(a, b)| a.abs_diff(b) <= tolerance)
                || self.selection.get(index).is_some_and(|v| *v == 0)
                || (style.respect_mask && self.mask[index] == 0)
            {
                continue;
            }
            let amount = self.coverage(index, style);
            blend(
                &mut self.layers[self.active].raster.pixels[index * 4..index * 4 + 4],
                color,
                amount,
            );
            let x = index % self.width as usize;
            for next in [
                if x > 0 { Some(index - 1) } else { None },
                if x + 1 < self.width as usize {
                    Some(index + 1)
                } else {
                    None
                },
                index.checked_sub(self.width as usize),
                (index + (self.width as usize) < self.mask.len())
                    .then_some(index + self.width as usize),
            ]
            .into_iter()
            .flatten()
            {
                if !seen[next] {
                    seen[next] = true;
                    queue.push_back(next);
                }
            }
        }
        Ok(())
    }
    /// Replace the shared mask with a clipped rectangular selection.
    pub fn select_rectangle(&mut self, from: [f32; 2], to: [f32; 2]) -> Result<(), EditError> {
        points(from, to)?;
        let x0 = from[0].min(to[0]).floor().max(0.0) as u32;
        let y0 = from[1].min(to[1]).floor().max(0.0) as u32;
        let x1 = (from[0].max(to[0]).ceil().max(0.0) as u32).min(self.width);
        let y1 = (from[1].max(to[1]).ceil().max(0.0) as u32).min(self.height);
        self.mask.fill(0);
        for y in y0..y1 {
            for x in x0..x1 {
                self.mask[y as usize * self.width as usize + x as usize] = 255;
            }
        }
        Ok(())
    }
}
fn points(from: [f32; 2], to: [f32; 2]) -> Result<(), EditError> {
    if from
        .iter()
        .chain(to.iter())
        .all(|v| v.is_finite() && v.abs() <= 32768.0)
    {
        Ok(())
    } else {
        Err(invalid("Invalid drawing coordinates"))
    }
}
