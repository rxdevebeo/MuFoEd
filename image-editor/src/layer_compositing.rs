//! Non-destructive raster masks and separable blend modes on straight sRGB8.
use super::{blend, invalid, Document, EditError, Layer, Raster, MAX_DOCUMENT_BYTES};
use serde::{Deserialize, Serialize};
/// Supported separable layer blends, evaluated before source-over compositing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerBlendMode {
    /// Ordinary source-over.
    #[default]
    Normal,
    /// Multiply source and backdrop.
    Multiply,
    /// Screen source and backdrop.
    Screen,
    /// Multiply dark backdrop components and screen light ones.
    Overlay,
}
impl LayerBlendMode {
    /// Stable Photoshop blend-mode key.
    pub fn psd_key(self) -> &'static [u8; 4] {
        match self {
            Self::Normal => b"norm",
            Self::Multiply => b"mul ",
            Self::Screen => b"scrn",
            Self::Overlay => b"over",
        }
    }
}
/// Independent full-canvas grayscale layer coverage; pixels remain unmodified.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerMask {
    #[serde(
        serialize_with = "super::workbench_selection::serialize_area",
        deserialize_with = "super::workbench_selection::deserialize_area"
    )]
    pub(super) pixels: Vec<u8>,
    pub(super) enabled: bool,
    pub(super) default_color: u8,
}
impl LayerMask {
    /// Coverage, black hides and white reveals the layer.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    /// Coverage outside stored bounds during linked transforms (0 or 255).
    pub fn default_color(&self) -> u8 {
        self.default_color
    }
    /// Whether coverage contributes to rendering.
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}
/// Destination selected within the single global Mask editing mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskTarget {
    /// Shared generation/inpaint mask.
    #[default]
    Inpaint,
    /// Raster mask belonging to this layer index.
    Layer(usize),
}
impl Layer {
    /// Layer blending mode, independent of paint-tool opacity.
    pub fn blend_mode(&self) -> LayerBlendMode {
        self.blend_mode
    }
    /// Optional non-destructive full-canvas layer mask.
    pub fn mask(&self) -> Option<&LayerMask> {
        self.mask.as_ref()
    }
    /// Pixels with enabled mask coverage baked into alpha, without layer opacity.
    pub fn masked_raster(&self) -> Raster {
        let mut raster = self.raster.clone();
        if let Some(mask) = self.mask.as_ref().filter(|m| m.enabled) {
            for (p, &coverage) in raster.pixels.chunks_exact_mut(4).zip(&mask.pixels) {
                p[3] = u8::try_from((u16::from(p[3]) * u16::from(coverage) + 127) / 255)
                    .expect("alpha");
            }
        }
        raster
    }
    pub(super) fn memory_bytes(&self) -> usize {
        self.raster.pixels.len() + self.mask.as_ref().map_or(0, |m| m.pixels.len())
    }
}
impl Document {
    pub(super) fn sync_layer_version(&mut self) {
        self.version = if self.has_layer_features() { 2 } else { 1 };
    }
    /// Whether standard masks or advanced blend modes are present.
    pub fn has_layer_features(&self) -> bool {
        self.layers
            .iter()
            .any(|l| l.mask.is_some() || l.blend_mode != LayerBlendMode::Normal)
    }
    pub(super) fn resized_memory_bytes(&self, len: usize) -> usize {
        len * self.layers.len()
            + len / 4
                * (1 + usize::from(!self.selection.is_empty())
                    + self.layers.iter().filter(|l| l.mask.is_some()).count())
    }
    /// Set the active layer's blend mode; locked layers reject changes.
    pub fn set_blend_mode(&mut self, mode: LayerBlendMode) -> Result<(), EditError> {
        self.require_unlocked()?;
        self.layers[self.active].blend_mode = mode;
        self.sync_layer_version();
        Ok(())
    }
    /// Create/replace active-layer coverage, preserving raster and inpaint pixels.
    pub fn set_layer_mask(&mut self, pixels: Vec<u8>) -> Result<(), EditError> {
        self.require_unlocked()?;
        if pixels.len() != self.mask.len() {
            return Err(invalid("Layer mask dimensions differ from canvas"));
        }
        let previous = self.layers[self.active]
            .mask
            .as_ref()
            .map_or(0, |m| m.pixels.len());
        if self.memory_bytes() - previous + pixels.len() > MAX_DOCUMENT_BYTES {
            return Err(invalid("Layer mask exceeds document memory budget"));
        }
        let enabled = self.layers[self.active]
            .mask
            .as_ref()
            .is_none_or(|m| m.enabled);
        self.layers[self.active].mask = Some(LayerMask {
            pixels,
            enabled,
            default_color: self.layers[self.active]
                .mask
                .as_ref()
                .map_or(0, |m| m.default_color),
        });
        self.sync_layer_version();
        Ok(())
    }
    /// White reveal-all or black hide-all mask for the active layer.
    pub fn add_layer_mask(&mut self, reveal: bool) -> Result<(), EditError> {
        self.set_layer_mask(vec![if reveal { 255 } else { 0 }; self.mask.len()])?;
        self.layers[self.active]
            .mask
            .as_mut()
            .expect("created mask")
            .default_color = if reveal { 255 } else { 0 };
        Ok(())
    }
    /// Enable/disable the active mask without destroying its coverage.
    pub fn set_layer_mask_enabled(&mut self, enabled: bool) -> Result<(), EditError> {
        self.require_unlocked()?;
        self.layers[self.active]
            .mask
            .as_mut()
            .ok_or_else(|| invalid("Layer has no mask"))?
            .enabled = enabled;
        Ok(())
    }
    /// Remove the active layer's mask, retaining original image pixels.
    pub fn remove_layer_mask(&mut self) -> Result<(), EditError> {
        self.require_unlocked()?;
        self.layers[self.active].mask = None;
        self.sync_layer_version();
        Ok(())
    }
    /// Bake enabled layer coverage into alpha, then remove the mask. A disabled
    /// mask is removed without changing alpha, preserving the visible result.
    pub fn apply_layer_mask(&mut self) -> Result<(), EditError> {
        self.require_unlocked()?;
        let layer = &mut self.layers[self.active];
        if layer.mask.is_none() {
            return Err(invalid("Layer has no mask"));
        }
        layer.raster = layer.masked_raster();
        layer.mask = None;
        self.sync_layer_version();
        Ok(())
    }
    /// Invert layer coverage; this never touches the independent inpaint mask.
    pub fn invert_layer_mask(&mut self) -> Result<(), EditError> {
        self.require_unlocked()?;
        let mask = self.layers[self.active]
            .mask
            .as_mut()
            .ok_or_else(|| invalid("Layer has no mask"))?;
        for v in &mut mask.pixels {
            *v = 255 - *v;
        }
        mask.default_color = 255 - mask.default_color;
        Ok(())
    }
    /// Read either mask without depending on the UI's global edit mode.
    pub fn target_mask(&self, target: MaskTarget) -> Result<&[u8], EditError> {
        match target {
            MaskTarget::Inpaint => Ok(&self.mask),
            MaskTarget::Layer(index) => self
                .layers
                .get(index)
                .and_then(|l| l.mask.as_ref())
                .map(|m| m.pixels.as_slice())
                .ok_or_else(|| invalid("Layer mask no longer exists")),
        }
    }
    /// Grayscale representation suitable for shared pixel tools and histogram.
    pub fn target_mask_raster(&self, target: MaskTarget) -> Result<Raster, EditError> {
        Raster::new(
            self.width,
            self.height,
            self.target_mask(target)?
                .iter()
                .flat_map(|&v| [v, v, v, 255])
                .collect(),
        )
    }
    /// Run the same raster tools against either mask. Structural mutations and
    /// errors are rejected before publication; locked layer masks are protected.
    pub fn edit_target_mask(
        &mut self,
        target: MaskTarget,
        operation: impl FnOnce(&mut Document) -> Result<(), EditError>,
    ) -> Result<(), EditError> {
        if target == MaskTarget::Inpaint {
            return self.edit_mask(operation);
        }
        let MaskTarget::Layer(index) = target else {
            unreachable!()
        };
        let layer = self
            .layers
            .get(index)
            .ok_or_else(|| invalid("Unknown layer"))?;
        if layer.locked {
            return Err(invalid("Слой заблокирован"));
        }
        let mut proxy = Self::from_raster(self.target_mask_raster(target)?)?;
        proxy.selection.clone_from(&self.selection);
        operation(&mut proxy)?;
        if proxy.width != self.width || proxy.height != self.height || proxy.layers.len() != 1 {
            return Err(invalid("Mask operation changed document structure"));
        }
        let pixels = proxy.layers[0]
            .raster
            .pixels
            .chunks_exact(4)
            .map(|p| super::selection::mask_coverage(p.try_into().expect("RGBA")))
            .collect();
        self.layers[index]
            .mask
            .as_mut()
            .expect("existing mask")
            .pixels = pixels;
        Ok(())
    }
    // Private adapter for legacy clipboard/mask commands only: these callbacks
    // cannot change the layer stack. The independent AI plane is always restored.
    pub(crate) fn with_mask_target<T>(
        &mut self,
        target: MaskTarget,
        operation: impl FnOnce(&mut Document) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        let MaskTarget::Layer(index) = target else {
            return operation(self);
        };
        self.target_mask(target)?;
        if self.layers[index].locked {
            return Err(invalid("Слой заблокирован"));
        }
        std::mem::swap(
            &mut self.mask,
            &mut self.layers[index].mask.as_mut().expect("mask").pixels,
        );
        let result = operation(self);
        std::mem::swap(
            &mut self.mask,
            &mut self.layers[index].mask.as_mut().expect("mask").pixels,
        );
        result
    }
    /// Copy the selected mask, including when its layer is locked.
    pub fn copy_target_mask(&self, target: MaskTarget) -> Result<super::PixelClipboard, EditError> {
        self.copy_content_mask(false, Some(self.target_mask(target)?))
    }
    pub(super) fn composite_layer(&self, pixels: &mut [u8], index: usize, raster: &Raster) {
        let layer = &self.layers[index];
        for (i, (dst, src)) in pixels
            .chunks_exact_mut(4)
            .zip(raster.pixels.chunks_exact(4))
            .enumerate()
        {
            let mask = layer
                .mask
                .as_ref()
                .filter(|m| m.enabled)
                .map_or(1.0, |m| f32::from(m.pixels[i]) / 255.0);
            blend_layer(
                dst,
                src.try_into().expect("RGBA"),
                f32::from(layer.opacity) / 255.0 * mask,
                layer.blend_mode,
            );
        }
    }
}
// W3C compositing-1 section 10: alpha-aware blending followed by source-over.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn blend_layer(dst: &mut [u8], src: [u8; 4], opacity: f32, mode: LayerBlendMode) {
    if mode == LayerBlendMode::Normal {
        blend(dst, src, opacity);
        return;
    }
    let sa = f32::from(src[3]) / 255.0 * opacity;
    let da = f32::from(dst[3]) / 255.0;
    let alpha = sa + da * (1.0 - sa);
    if alpha == 0.0 {
        dst.fill(0);
        return;
    }
    for c in 0..3 {
        let source = f32::from(src[c]) / 255.0;
        let back = f32::from(dst[c]) / 255.0;
        let mixed = match mode {
            LayerBlendMode::Multiply => source * back,
            LayerBlendMode::Screen => 1.0 - (1.0 - source) * (1.0 - back),
            LayerBlendMode::Overlay => {
                if back <= 0.5 {
                    2.0 * source * back
                } else {
                    1.0 - 2.0 * (1.0 - source) * (1.0 - back)
                }
            }
            LayerBlendMode::Normal => source,
        };
        dst[c] = ((sa * (1.0 - da) * source + sa * da * mixed + (1.0 - sa) * da * back) / alpha
            * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    dst[3] = (alpha * 255.0).round() as u8;
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub(super) fn sample_mask(mask: &LayerMask, width: u32, height: u32, x: f64, y: f64) -> u8 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let mut sum = 0.0;
    for (dx, dy, w) in [
        (0.0, 0.0, (1.0 - fx) * (1.0 - fy)),
        (1.0, 0.0, fx * (1.0 - fy)),
        (0.0, 1.0, (1.0 - fx) * fy),
        (1.0, 1.0, fx * fy),
    ] {
        let (px, py) = (ix + dx, iy + dy);
        let v = if px >= 0.0 && py >= 0.0 && px < f64::from(width) && py < f64::from(height) {
            mask.pixels[py as usize * width as usize + px as usize]
        } else {
            mask.default_color
        };
        sum += f64::from(v) * w;
    }
    sum.round().clamp(0.0, 255.0) as u8
}
