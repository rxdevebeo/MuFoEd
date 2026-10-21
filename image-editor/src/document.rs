#[path = "layer_compositing.rs"]
mod layer_compositing;
pub use layer_compositing::{LayerBlendMode, LayerMask, MaskTarget};
#[path = "tone.rs"]
mod tone;
pub use tone::{Curves, Histogram, LevelChannel, Levels, ToneAdjustment, ToneCurve};
#[path = "retouch.rs"]
mod retouch;
pub use retouch::{RetouchMode, RetouchStroke};
#[path = "selection.rs"]
mod selection;
pub use selection::PixelClipboard;
#[path = "workbench_selection.rs"]
mod workbench_selection;
pub use workbench_selection::{SelectionOp, SelectionRefine};
#[path = "photo_geometry.rs"]
mod photo_geometry;
pub use photo_geometry::PhotoGeometry;
#[path = "color.rs"]
mod color;
pub use color::ColorCorrection;
#[path = "transform.rs"]
mod transform;
pub use transform::CanvasMode;
#[path = "manual.rs"]
mod manual;
#[path = "psd.rs"]
mod psd;
pub use manual::{BrushStyle, Shape};
use std::{collections::VecDeque, io::Read, path::Path};

use image::{ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};

const MAX_PIXELS: u64 = 16_777_216;
const MAX_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = 128 * 1024 * 1024;

/// Editing, decoding, or project validation failure.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// Invalid dimensions, parameters, or project data.
    #[error("{0}")]
    Invalid(String),
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Raster codec failure.
    #[error(transparent)]
    Image(#[from] image::ImageError),
    /// Project serialization failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn invalid(message: &str) -> EditError {
    EditError::Invalid(message.into())
}

fn byte_len(width: u32, height: u32) -> Result<usize, EditError> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(invalid("Canvas must contain 1..16777216 pixels"));
    }
    Ok(usize::try_from(pixels).map_err(|_| invalid("Canvas exceeds address space"))? * 4)
}

/// Validated, straight-alpha, sRGB RGBA8 raster.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RasterData")]
pub struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

#[derive(Deserialize)]
struct RasterData {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl TryFrom<RasterData> for Raster {
    type Error = EditError;
    fn try_from(data: RasterData) -> Result<Self, Self::Error> {
        Self::new(data.width, data.height, data.pixels)
    }
}

impl Raster {
    /// Construct an RGBA raster after validating dimensions and buffer length.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, EditError> {
        if pixels.len() != byte_len(width, height)? {
            return Err(invalid("RGBA buffer length mismatch"));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    /// Create a solid raster.
    pub fn solid(width: u32, height: u32, color: [u8; 4]) -> Result<Self, EditError> {
        let len = byte_len(width, height)?;
        Ok(Self {
            width,
            height,
            pixels: color.repeat(len / 4),
        })
    }
    /// Decode an image with allocation and dimension limits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EditError> {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_DOCUMENT_BYTES as u64);
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        reader.limits(limits);
        let image = reader.decode()?.to_rgba8();
        Self::new(image.width(), image.height(), image.into_raw())
    }
    /// Encode PNG, retaining alpha.
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), EditError> {
        self.image()
            .save_with_format(path, image::ImageFormat::Png)?;
        Ok(())
    }
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Row-major straight-alpha bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    fn image(&self) -> RgbaImage {
        // All construction and deserialization paths validate the exact buffer length.
        RgbaImage::from_raw(self.width, self.height, self.pixels.clone()).expect("validated raster")
    }
    fn from_image(image: RgbaImage) -> Self {
        Self {
            width: image.width(),
            height: image.height(),
            pixels: image.into_raw(),
        }
    }
    fn validate(&self) -> Result<(), EditError> {
        if self.pixels.len() != byte_len(self.width, self.height)? {
            return Err(invalid("Corrupt raster buffer"));
        }
        Ok(())
    }
}

/// A full-canvas layer; order in the document is bottom to top.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    #[serde(default)]
    blend_mode: LayerBlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mask: Option<LayerMask>,
    #[serde(default)]
    locked: bool,
    name: String,
    visible: bool,
    opacity: u8,
    raster: Raster,
    #[serde(default)]
    recipe: Option<crate::generation::Request>,
}
impl Layer {
    /// Whether pixel edits and destructive layer operations are blocked.
    pub fn locked(&self) -> bool {
        self.locked
    }
    /// Original inference parameters, when this layer came from a generation candidate.
    pub fn recipe(&self) -> Option<&crate::generation::Request> {
        self.recipe.as_ref()
    }
    /// User-facing name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Whether the layer contributes to export.
    pub fn visible(&self) -> bool {
        self.visible
    }
    /// Opacity from 0 to 255.
    pub fn opacity(&self) -> u8 {
        self.opacity
    }
    /// Layer pixels.
    pub fn raster(&self) -> &Raster {
        &self.raster
    }
}

/// Raster painting tools. Mask white means regenerate; black means preserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Paint foreground RGBA onto the active layer.
    Brush,
    /// Remove alpha from the active layer.
    Eraser,
    /// Paint white into the shared inpaint mask.
    Mask,
    /// Restore black in the inpaint mask.
    MaskEraser,
}

/// Ready-made photo operations from image/imageproc.
#[derive(Clone, Copy, Debug)]
pub enum Adjustment {
    /// Add an offset to RGB channels, in -255..255.
    Brightness(i32),
    /// Contrast adjustment, in -100..100.
    Contrast(f32),
    /// Gaussian blur, sigma in 0.1..20.
    Blur(f32),
    /// Convert RGB to luminance, retaining alpha.
    Grayscale,
    /// Median noise reduction with a 3x3 kernel.
    Median,
}

/// Self-contained versioned project with layers and an inpaint mask.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DocumentData")]
pub struct Document {
    version: u32,
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    mask: Vec<u8>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "workbench_selection::serialize_area"
    )]
    selection: Vec<u8>,
}
#[derive(Deserialize)]
struct DocumentData {
    version: u32,
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    mask: Vec<u8>,
    #[serde(default, deserialize_with = "workbench_selection::deserialize_area")]
    selection: Vec<u8>,
}
impl TryFrom<DocumentData> for Document {
    type Error = EditError;
    fn try_from(data: DocumentData) -> Result<Self, Self::Error> {
        let mut doc = Self {
            version: data.version,
            width: data.width,
            height: data.height,
            layers: data.layers,
            active: data.active,
            mask: data.mask,
            selection: data.selection,
        };
        doc.validate()?;
        doc.normalize_selection();
        doc.sync_layer_version();
        Ok(doc)
    }
}
impl Document {
    /// Create a document at the generated size, retaining its reproducible recipe.
    pub fn from_generated(
        raster: Raster,
        request: crate::generation::Request,
    ) -> Result<Self, EditError> {
        let mut doc = Self::from_raster(raster)?;
        doc.layers[0].recipe = Some(request);
        Ok(doc)
    }
    /// Create a transparent project with one layer.
    pub fn new(width: u32, height: u32) -> Result<Self, EditError> {
        let raster = Raster::solid(width, height, [0; 4])?;
        Ok(Self {
            version: 1,
            width,
            height,
            layers: vec![Layer {
                name: "Layer 1".into(),
                visible: true,
                opacity: 255,
                raster,
                recipe: None,
                locked: false,
                blend_mode: LayerBlendMode::Normal,
                mask: None,
            }],
            active: 0,
            mask: vec![0; byte_len(width, height)? / 4],
            selection: Vec::new(),
        })
    }
    /// Create a project from an imported raster.
    pub fn from_raster(raster: Raster) -> Result<Self, EditError> {
        raster.validate()?;
        let mut doc = Self::new(raster.width, raster.height)?;
        doc.layers[0].raster = raster;
        doc.layers[0].name = "Imported image".into();
        Ok(doc)
    }
    /// Canvas width.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Canvas height.
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Layers, ordered bottom to top.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
    /// Active layer index.
    pub fn active(&self) -> usize {
        self.active
    }
    /// Select a valid layer.
    pub fn select(&mut self, index: usize) -> Result<(), EditError> {
        if index >= self.layers.len() {
            return Err(invalid("Unknown layer"));
        }
        self.active = index;
        Ok(())
    }
    /// Set active layer presentation properties.
    pub fn set_layer(&mut self, name: String, visible: bool, opacity: u8) {
        let layer = &mut self.layers[self.active];
        layer.name = name;
        layer.visible = visible;
        layer.opacity = opacity;
    }
    /// Add a same-size raster layer, preserving existing content.
    pub fn add_layer(&mut self, name: String, raster: Raster) -> Result<(), EditError> {
        raster.validate()?;
        if raster.width != self.width || raster.height != self.height {
            return Err(invalid("Layer must match canvas dimensions"));
        }
        if self.layers.len() >= 16 || self.memory_bytes() + raster.pixels.len() > MAX_DOCUMENT_BYTES
        {
            return Err(invalid("Document layer/memory limit reached"));
        }
        self.layers.push(Layer {
            name,
            visible: true,
            opacity: 255,
            raster,
            recipe: None,
            locked: false,
            blend_mode: LayerBlendMode::Normal,
            mask: None,
        });
        self.active = self.layers.len() - 1;
        Ok(())
    }
    /// Accept a generation candidate with its reproducible recipe stored in the project.
    pub fn add_generated_layer(
        &mut self,
        name: String,
        raster: Raster,
        request: crate::generation::Request,
    ) -> Result<(), EditError> {
        self.add_layer(name, raster)?;
        self.layers[self.active].recipe = Some(request);
        Ok(())
    }
    /// Remove the active layer, retaining at least one layer.
    pub fn remove_layer(&mut self) -> Result<(), EditError> {
        self.require_unlocked()?;
        if self.layers.len() == 1 {
            return Err(invalid("Cannot remove the last layer"));
        }
        self.layers.remove(self.active);
        self.active = self.active.min(self.layers.len() - 1);
        self.sync_layer_version();
        Ok(())
    }
    /// Move the active layer one step up or down.
    pub fn move_layer(&mut self, up: bool) {
        let next = if up {
            (self.active + 1).min(self.layers.len() - 1)
        } else {
            self.active.saturating_sub(1)
        };
        self.layers.swap(self.active, next);
        self.active = next;
    }
    /// Composite visible layers with straight-alpha source-over blending.
    pub fn flatten(&self) -> Raster {
        self.flatten_preview(None)
    }
    // Shared preview/export compositor; replacements have the source dimensions.
    pub(crate) fn flatten_preview(&self, active: Option<&Raster>) -> Raster {
        let mut pixels = vec![0; self.width as usize * self.height as usize * 4];
        for (index, layer) in self.layers.iter().enumerate().filter(|(_, l)| l.visible) {
            let raster = if index == self.active {
                active.unwrap_or(&layer.raster)
            } else {
                &layer.raster
            };
            self.composite_layer(&mut pixels, index, raster);
        }
        Raster {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
    /// Shared mask, one byte per pixel (white = regenerate).
    pub fn mask(&self) -> &[u8] {
        &self.mask
    }
    /// Export an opaque grayscale mask PNG, matching Plakat's mask convention.
    pub fn mask_raster(&self) -> Raster {
        Raster {
            width: self.width,
            height: self.height,
            pixels: self.mask.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
        }
    }
    /// Reset the inpaint mask.
    pub fn clear_mask(&mut self) {
        self.mask.fill(0);
    }
    /// Replace the shared grayscale mask without changing layer pixels.
    pub fn set_mask(&mut self, mask: Vec<u8>) -> Result<(), EditError> {
        if mask.len() != self.mask.len() {
            return Err(invalid("Mask must match canvas dimensions"));
        }
        self.mask = mask;
        Ok(())
    }
    /// Draw an interpolated brush segment; positions are image coordinates.
    // Coordinates are finite and bounded above; rounded color channels are in 0..255.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn stroke(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        radius: f32,
        color: [u8; 4],
        tool: Tool,
    ) -> Result<(), EditError> {
        self.stroke_with_style(from, to, radius, color, tool, BrushStyle::default())
    }
    /// Apply a photo operation to the active layer, keeping alpha unchanged.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Rounded luminance is 0..255.
    pub fn adjust(&mut self, adjustment: Adjustment) -> Result<(), EditError> {
        self.require_unlocked()?;
        let original = self.layers[self.active].raster.image();
        let mut result = match adjustment {
            Adjustment::Brightness(value) if (-255..=255).contains(&value) => {
                image::imageops::brighten(&original, value)
            }
            Adjustment::Contrast(value)
                if value.is_finite() && (-100.0..=100.0).contains(&value) =>
            {
                image::imageops::contrast(&original, value)
            }
            Adjustment::Blur(value) if value.is_finite() && (0.1..=20.0).contains(&value) => {
                image::imageops::blur(&original, value)
            }
            Adjustment::Grayscale => {
                let mut out = original.clone();
                for pixel in out.pixels_mut() {
                    let gray = (0.2126 * f32::from(pixel[0])
                        + 0.7152 * f32::from(pixel[1])
                        + 0.0722 * f32::from(pixel[2]))
                    .round() as u8;
                    pixel.0 = [gray, gray, gray, pixel[3]];
                }
                out
            }
            Adjustment::Median => imageproc::filter::median_filter(&original, 1, 1),
            _ => return Err(invalid("Invalid adjustment value")),
        };
        for (out, source) in result.pixels_mut().zip(original.pixels()) {
            out[3] = source[3];
        }
        self.layers[self.active].raster = Raster::from_image(result);
        Ok(())
    }
    /// Crop all layers and mask to a rectangle inside the current canvas.
    pub fn crop(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<(), EditError> {
        byte_len(width, height)?;
        if x.checked_add(width).is_none_or(|v| v > self.width)
            || y.checked_add(height).is_none_or(|v| v > self.height)
        {
            return Err(invalid("Crop rectangle is outside the canvas"));
        }
        for layer in &mut self.layers {
            if let Some(mask) = &mut layer.mask {
                let image =
                    image::GrayImage::from_raw(self.width, self.height, mask.pixels.clone())
                        .expect("mask");
                mask.pixels = image::imageops::crop_imm(&image, x, y, width, height)
                    .to_image()
                    .into_raw();
            }
            layer.raster = Raster::from_image(
                image::imageops::crop_imm(&layer.raster.image(), x, y, width, height).to_image(),
            );
        }
        let mut mask = Vec::with_capacity(width as usize * height as usize);
        for row in y..y + height {
            let start = row as usize * self.width as usize + x as usize;
            mask.extend_from_slice(&self.mask[start..start + width as usize]);
        }
        if !self.selection.is_empty() {
            let img = image::GrayImage::from_raw(self.width, self.height, self.selection.clone())
                .expect("selection");
            self.selection = image::imageops::crop_imm(&img, x, y, width, height)
                .to_image()
                .into_raw();
        }
        self.width = width;
        self.height = height;
        self.mask = mask;
        self.normalize_selection();
        Ok(())
    }
    /// Resize every layer with Lanczos3; mask uses nearest-neighbor sampling.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), EditError> {
        let len = byte_len(width, height)?;
        if self.resized_memory_bytes(len) > MAX_DOCUMENT_BYTES {
            return Err(invalid("Resized document exceeds memory budget"));
        }
        for layer in &mut self.layers {
            if let Some(mask) = &mut layer.mask {
                let image =
                    image::GrayImage::from_raw(self.width, self.height, mask.pixels.clone())
                        .expect("mask");
                mask.pixels = image::imageops::resize(
                    &image,
                    width,
                    height,
                    image::imageops::FilterType::Nearest,
                )
                .into_raw();
            }
            layer.raster = Raster::from_image(image::imageops::resize(
                &layer.raster.image(),
                width,
                height,
                image::imageops::FilterType::Lanczos3,
            ));
        }
        let mask = image::GrayImage::from_raw(self.width, self.height, self.mask.clone())
            .expect("validated mask");
        self.mask =
            image::imageops::resize(&mask, width, height, image::imageops::FilterType::Nearest)
                .into_raw();
        if !self.selection.is_empty() {
            let img = image::GrayImage::from_raw(self.width, self.height, self.selection.clone())
                .expect("selection");
            self.selection =
                image::imageops::resize(&img, width, height, image::imageops::FilterType::Nearest)
                    .into_raw();
        }
        self.width = width;
        self.height = height;
        self.normalize_selection();
        Ok(())
    }
    /// Rotate layers and mask clockwise by 90 degrees.
    pub fn rotate90(&mut self) {
        for layer in &mut self.layers {
            if let Some(mask) = &mut layer.mask {
                let image =
                    image::GrayImage::from_raw(self.width, self.height, mask.pixels.clone())
                        .expect("mask");
                mask.pixels = image::imageops::rotate90(&image).into_raw();
            }
            layer.raster = Raster::from_image(image::imageops::rotate90(&layer.raster.image()));
        }
        let mask = image::GrayImage::from_raw(self.width, self.height, self.mask.clone())
            .expect("validated mask");
        self.mask = image::imageops::rotate90(&mask).into_raw();
        if !self.selection.is_empty() {
            let img = image::GrayImage::from_raw(self.width, self.height, self.selection.clone())
                .expect("selection");
            self.selection = image::imageops::rotate90(&img).into_raw();
        }
        std::mem::swap(&mut self.width, &mut self.height);
        self.normalize_selection();
    }
    /// Encode a self-contained project; no paths to external layer assets.
    pub fn to_json(&self) -> Result<Vec<u8>, EditError> {
        Ok(serde_json::to_vec(self)?)
    }
    /// Parse and validate a version-1 project before any editing or rendering.
    pub fn from_json(bytes: &[u8]) -> Result<Self, EditError> {
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(invalid("Project file exceeds size limit"));
        }
        let doc: Self = serde_json::from_slice(bytes)?;
        Ok(doc)
    }
    fn validate(&self) -> Result<(), EditError> {
        let len = byte_len(self.width, self.height)?;
        if !(1..=2).contains(&self.version)
            || (self.has_layer_features() && self.version != 2)
            || self.layers.is_empty()
            || self.layers.len() > 16
            || self.active >= self.layers.len()
            || self.mask.len() != len / 4
            || (!self.selection.is_empty() && self.selection.len() != len / 4)
            || self.memory_bytes() > MAX_DOCUMENT_BYTES
        {
            return Err(invalid("Invalid project version, layers, or mask"));
        }
        for layer in &self.layers {
            layer.raster.validate()?;
            if layer
                .mask
                .as_ref()
                .is_some_and(|m| m.pixels.len() != len / 4 || ![0, 255].contains(&m.default_color))
            {
                return Err(invalid("Layer mask dimensions differ from canvas"));
            }
            if layer.raster.width != self.width || layer.raster.height != self.height {
                return Err(invalid("Layer dimensions differ from canvas"));
            }
        }
        Ok(())
    }
    /// Read a project with a bounded input size.
    pub fn open_project(path: impl AsRef<Path>) -> Result<Self, EditError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_PROJECT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.starts_with(b"8BPS") {
            Self::from_psd(&bytes)
        } else {
            Self::from_json(&bytes)
        }
    }
    /// Save the complete project through a temporary file in the destination directory.
    pub fn save_project(&self, path: impl AsRef<Path>) -> Result<(), EditError> {
        let path = path.as_ref();
        let bytes = match path.extension().and_then(std::ffi::OsStr::to_str) {
            Some(ext) if ext.eq_ignore_ascii_case("psd") => self.to_psd()?,
            Some(ext) if ext.eq_ignore_ascii_case("json") => self.to_json()?,
            _ => return Err(invalid("Save the project as .psd (or legacy .json)")),
        };
        if bytes.len() as u64 > MAX_PROJECT_BYTES {
            return Err(invalid("Serialized project exceeds size limit"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        std::io::Write::write_all(&mut temp, &bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
    fn memory_bytes(&self) -> usize {
        self.mask.len()
            + self.selection.len()
            + self.layers.iter().map(Layer::memory_bytes).sum::<usize>()
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Normalized RGBA channels remain in 0..255.
fn blend(dst: &mut [u8], src: [u8; 4], opacity: f32) {
    let sa = f32::from(src[3]) / 255.0 * opacity;
    let da = f32::from(dst[3]) / 255.0;
    let alpha = sa + da * (1.0 - sa);
    if alpha == 0.0 {
        dst.fill(0);
        return;
    }
    for i in 0..3 {
        dst[i] =
            ((f32::from(src[i]) * sa + f32::from(dst[i]) * da * (1.0 - sa)) / alpha).round() as u8;
    }
    dst[3] = (alpha * 255.0).round() as u8;
}

/// Bounded snapshot undo/redo. One checkpoint per user gesture or operation.
#[derive(Default)]
pub struct History {
    undo: VecDeque<Document>,
    redo: VecDeque<Document>,
}
impl History {
    /// Record the state before a mutation, invalidating redo.
    pub fn checkpoint(&mut self, doc: &Document) {
        self.redo.clear();
        self.undo.push_back(doc.clone());
        Self::trim(&mut self.undo);
    }
    /// Restore the previous state, if any.
    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        self.redo.push_back(std::mem::replace(doc, previous));
        Self::trim(&mut self.redo);
        true
    }
    /// Restore the next state, if any.
    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(next) = self.redo.pop_back() else {
            return false;
        };
        self.undo.push_back(std::mem::replace(doc, next));
        Self::trim(&mut self.undo);
        true
    }
    fn trim(queue: &mut VecDeque<Document>) {
        while queue.len() > 1
            && (queue.len() > 32
                || queue.iter().map(Document::memory_bytes).sum::<usize>() > 64 * 1024 * 1024)
        {
            queue.pop_front();
        }
    }
}
