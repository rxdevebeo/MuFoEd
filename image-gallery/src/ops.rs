//! The operations of a processing recipe.
//!
//! Each [`Operation`] maps a picture to a new one. Operations keep the bit
//! depth class of their input (8-bit, 16-bit or float) and add colour or
//! transparency channels only when they create them (sepia, a transparent
//! canvas).

// Pixel geometry: sizes are positive and bounded by the picture, floats are
// rounded and clamped before every cast. Colour formulas keep their
// one-letter channel names.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::similar_names
)]

use image::imageops::FilterType;
use image::{
    ColorType, DynamicImage, GenericImageView, ImageBuffer, Rgba, Rgba32FImage, RgbaImage,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::Error;

/// A quarter or half turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarterTurn {
    /// 90° clockwise.
    Right,
    /// 90° counter-clockwise.
    Left,
    /// 180°.
    Half,
}

/// A mirror axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Left and right swap.
    Horizontal,
    /// Top and bottom swap.
    Vertical,
}

/// A resampling filter, fastest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    /// Nearest neighbour: pixel art, no smoothing.
    Nearest,
    /// Bilinear.
    Bilinear,
    /// Catmull-Rom bicubic.
    Bicubic,
    /// Gaussian.
    Gaussian,
    /// Lanczos with a window of 3: the sharpest.
    Lanczos,
}

impl Filter {
    /// Every filter, for menus.
    pub const ALL: [Self; 5] = [
        Self::Nearest,
        Self::Bilinear,
        Self::Bicubic,
        Self::Gaussian,
        Self::Lanczos,
    ];

    fn to_image(self) -> FilterType {
        match self {
            Self::Nearest => FilterType::Nearest,
            Self::Bilinear => FilterType::Triangle,
            Self::Bicubic => FilterType::CatmullRom,
            Self::Gaussian => FilterType::Gaussian,
            Self::Lanczos => FilterType::Lanczos3,
        }
    }
}

/// How a resize chooses the new size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeMode {
    /// Exactly this size; the aspect ratio may change.
    Exact {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// The largest size that fits in the box, keeping the aspect ratio.
    Fit {
        /// Box width.
        width: u32,
        /// Box height.
        height: u32,
    },
    /// Covers the box, keeping the aspect ratio, then crops the overflow
    /// around the centre.
    Fill {
        /// Box width.
        width: u32,
        /// Box height.
        height: u32,
    },
    /// This width, height to scale.
    Width {
        /// Width in pixels.
        width: u32,
    },
    /// This height, width to scale.
    Height {
        /// Height in pixels.
        height: u32,
    },
    /// The longer side becomes this long.
    LongSide {
        /// Side in pixels.
        size: u32,
    },
    /// The shorter side becomes this long.
    ShortSide {
        /// Side in pixels.
        size: u32,
    },
    /// Both sides scaled by a percentage.
    Percent {
        /// Percent of the current size.
        percent: f32,
    },
}

/// Where a cropped or padded picture sits along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// Left or top.
    Start,
    /// Centre.
    Center,
    /// Right or bottom.
    End,
}

impl Align {
    /// The offset of a span of `inner` inside `outer` (either may be larger).
    fn offset(self, outer: u32, inner: u32) -> i64 {
        let spare = i64::from(outer) - i64::from(inner);
        match self {
            Self::Start => 0,
            Self::Center => spare / 2,
            Self::End => spare,
        }
    }
}

/// A position within the picture: horizontal and vertical alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// Horizontal alignment.
    pub x: Align,
    /// Vertical alignment.
    pub y: Align,
}

impl Anchor {
    /// The centre.
    pub const CENTER: Self = Self {
        x: Align::Center,
        y: Align::Center,
    };
}

/// What part of the picture a crop keeps.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CropMode {
    /// A rectangle in pixels, clipped to the picture.
    Rect {
        /// Left edge.
        x: u32,
        /// Top edge.
        y: u32,
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// The largest area of this aspect ratio, placed by the anchor.
    Aspect {
        /// Ratio width.
        width: u32,
        /// Ratio height.
        height: u32,
        /// Which part is kept.
        anchor: Anchor,
    },
    /// Cuts these many pixels from each edge.
    Margins {
        /// From the left.
        left: u32,
        /// From the top.
        top: u32,
        /// From the right.
        right: u32,
        /// From the bottom.
        bottom: u32,
    },
    /// Removes uniform borders of the corner pixel's colour.
    Trim {
        /// Largest channel difference still counted as border (0 to 255).
        tolerance: u8,
    },
}

/// How a canvas change chooses the new size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasMode {
    /// Exactly this size: smaller cuts, larger pads.
    Size {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// Pads to this aspect ratio.
    Aspect {
        /// Ratio width.
        width: u32,
        /// Ratio height.
        height: u32,
    },
    /// A border of this many pixels on every side.
    Border {
        /// Border width.
        size: u32,
    },
}

/// One step of a recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// A quarter or half turn.
    Rotate(QuarterTurn),
    /// A mirror.
    Flip(Axis),
    /// A turn by any angle; the canvas grows to hold the corners.
    Straighten {
        /// Clockwise degrees.
        degrees: f32,
        /// The colour of the uncovered corners, RGBA.
        fill: [u8; 4],
    },
    /// A change of size.
    Resize {
        /// How the size is chosen.
        mode: ResizeMode,
        /// The resampling filter.
        filter: Filter,
        /// Whether pictures smaller than the target may grow.
        enlarge: bool,
    },
    /// A crop.
    Crop(CropMode),
    /// A larger or smaller canvas.
    Canvas {
        /// How the size is chosen.
        mode: CanvasMode,
        /// The colour of the new area, RGBA.
        colour: [u8; 4],
        /// Where the picture sits on the canvas.
        anchor: Anchor,
    },
    /// Brightness, -100 to 100.
    Brightness(f32),
    /// Contrast, -100 to 100.
    Contrast(f32),
    /// Saturation, -100 (grey) to 100.
    Saturation(f32),
    /// Gamma, 0.1 to 10; above 1 brightens the midtones.
    Gamma(f32),
    /// Hue rotation in degrees.
    Hue(f32),
    /// Shades of grey.
    Grayscale,
    /// Sepia toning.
    Sepia,
    /// A negative.
    Invert,
    /// Stretches every channel to the full range.
    AutoContrast,
    /// Gaussian blur of this radius (sigma, pixels).
    Blur(f32),
    /// Unsharp mask.
    Sharpen {
        /// Radius (sigma, pixels).
        sigma: f32,
        /// Smallest difference that is sharpened.
        threshold: i32,
    },
    /// Composes transparency onto a solid colour, RGB.
    Flatten([u8; 3]),
    /// A tool the host registered in [`Tools`], such as a background remover
    /// backed by a model.
    Tool {
        /// The registered ID.
        id: String,
    },
}

/// A host-supplied picture tool.
pub type ToolFn = dyn Fn(DynamicImage) -> Result<DynamicImage, Error> + Send + Sync;

/// The tools a host registered: model-backed or otherwise external steps
/// that recipes name by ID.
#[derive(Clone, Default)]
pub struct Tools {
    entries: Vec<(String, String, std::sync::Arc<ToolFn>)>,
}

impl std::fmt::Debug for Tools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|(id, _, _)| id))
            .finish()
    }
}

impl Tools {
    /// Registers (or replaces) the tool `id`, shown as `label`.
    pub fn register(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        tool: impl Fn(DynamicImage) -> Result<DynamicImage, Error> + Send + Sync + 'static,
    ) {
        let id = id.into();
        self.entries.retain(|(known, _, _)| *known != id);
        self.entries
            .push((id, label.into(), std::sync::Arc::new(tool)));
    }

    /// The registered IDs and labels, in registration order.
    pub fn list(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(id, label, _)| (id.as_str(), label.as_str()))
    }

    /// The label of `id`, if registered.
    pub fn label(&self, id: &str) -> Option<&str> {
        self.list()
            .find(|(known, _)| *known == id)
            .map(|(_, label)| label)
    }

    fn run(&self, id: &str, image: DynamicImage) -> Result<DynamicImage, Error> {
        let (_, _, tool) = self
            .entries
            .iter()
            .find(|(known, _, _)| known == id)
            .ok_or_else(|| Error::Invalid(format!("the tool \"{id}\" is not available")))?;
        tool(image)
    }
}

impl Operation {
    /// One operation of each kind with sensible parameters, for the "add"
    /// menu.
    pub fn catalogue() -> Vec<Self> {
        vec![
            Self::Resize {
                mode: ResizeMode::LongSide { size: 1920 },
                filter: Filter::Lanczos,
                enlarge: false,
            },
            Self::Crop(CropMode::Aspect {
                width: 4,
                height: 3,
                anchor: Anchor::CENTER,
            }),
            Self::Crop(CropMode::Trim { tolerance: 8 }),
            Self::Rotate(QuarterTurn::Right),
            Self::Flip(Axis::Horizontal),
            Self::Straighten {
                degrees: 1.0,
                fill: [255, 255, 255, 255],
            },
            Self::Canvas {
                mode: CanvasMode::Border { size: 20 },
                colour: [255, 255, 255, 255],
                anchor: Anchor::CENTER,
            },
            Self::Brightness(10.0),
            Self::Contrast(10.0),
            Self::Saturation(20.0),
            Self::Gamma(1.2),
            Self::Hue(30.0),
            Self::AutoContrast,
            Self::Grayscale,
            Self::Sepia,
            Self::Invert,
            Self::Blur(2.0),
            Self::Sharpen {
                sigma: 1.0,
                threshold: 2,
            },
            Self::Flatten([255, 255, 255]),
        ]
    }

    /// Whether the operation only re-orders pixels by quarter turns and
    /// mirrors, which JPEG can record losslessly in its EXIF orientation.
    pub fn is_reorientation(&self) -> bool {
        matches!(self, Self::Rotate(_) | Self::Flip(_))
    }

    /// Applies the operation; a [`Operation::Tool`] fails, having no
    /// registry to look in (see [`Operation::apply_with`]).
    pub fn apply(&self, image: DynamicImage) -> Result<DynamicImage, Error> {
        self.apply_with(image, &Tools::default())
    }

    /// Applies the operation, looking tools up in `tools`.
    pub fn apply_with(&self, image: DynamicImage, tools: &Tools) -> Result<DynamicImage, Error> {
        if let Self::Tool { id } = self {
            return tools.run(id, image);
        }
        Ok(match self.clone() {
            Self::Rotate(QuarterTurn::Right) => image.rotate90(),
            Self::Rotate(QuarterTurn::Left) => image.rotate270(),
            Self::Rotate(QuarterTurn::Half) => image.rotate180(),
            Self::Flip(Axis::Horizontal) => image.fliph(),
            Self::Flip(Axis::Vertical) => image.flipv(),
            Self::Straighten { degrees, fill } => straighten(&image, degrees, fill),
            Self::Resize {
                mode,
                filter,
                enlarge,
            } => resize(image, mode, filter, enlarge)?,
            Self::Crop(mode) => crop(&image, mode)?,
            Self::Canvas {
                mode,
                colour,
                anchor,
            } => canvas(&image, mode, colour, anchor)?,
            Self::Brightness(amount) => {
                let shift = amount.clamp(-100.0, 100.0) / 100.0;
                map_colour(&image, false, |v| v + shift)
            }
            Self::Contrast(amount) => {
                let factor = 1.0 + amount.clamp(-100.0, 100.0) / 100.0;
                map_colour(&image, false, |v| (v - 0.5) * factor + 0.5)
            }
            Self::Gamma(gamma) => {
                let exponent = 1.0 / gamma.clamp(0.1, 10.0);
                map_colour(&image, false, |v| v.max(0.0).powf(exponent))
            }
            Self::Saturation(amount) => {
                let factor = 1.0 + amount.clamp(-100.0, 100.0) / 100.0;
                map_pixels(&image, false, false, |[r, g, b, a]| {
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    [
                        luma + (r - luma) * factor,
                        luma + (g - luma) * factor,
                        luma + (b - luma) * factor,
                        a,
                    ]
                })
            }
            Self::Hue(degrees) => image.huerotate(degrees.round() as i32),
            Self::Grayscale => image.grayscale(),
            Self::Sepia => map_pixels(&image, true, false, |[r, g, b, a]| {
                [
                    0.393 * r + 0.769 * g + 0.189 * b,
                    0.349 * r + 0.686 * g + 0.168 * b,
                    0.272 * r + 0.534 * g + 0.131 * b,
                    a,
                ]
            }),
            Self::Invert => {
                let mut image = image;
                image.invert();
                image
            }
            Self::AutoContrast => auto_contrast(&image),
            Self::Blur(sigma) => {
                if sigma > 0.0 {
                    image.blur(sigma.min(250.0))
                } else {
                    image
                }
            }
            Self::Sharpen { sigma, threshold } => {
                image.unsharpen(sigma.clamp(0.1, 100.0), threshold)
            }
            Self::Flatten(colour) => flatten(&image, colour),
            Self::Tool { .. } => image,
        })
    }
}

/// Applies a list of operations in order.
pub fn apply_all(
    image: DynamicImage,
    operations: &[Operation],
    tools: &Tools,
) -> Result<DynamicImage, Error> {
    operations
        .iter()
        .try_fold(image, |image, operation| operation.apply_with(image, tools))
}

/// The bit depth class of a colour type.
fn depth(colour: ColorType) -> u8 {
    match colour {
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16 => 16,
        ColorType::Rgb32F | ColorType::Rgba32F => 32,
        _ => 8,
    }
}

/// Converts an RGBA working buffer back to the source's class of colour
/// type: its depth, and grey or colour, opaque or not.
fn finish(
    source: ColorType,
    work: DynamicImage,
    adds_colour: bool,
    adds_alpha: bool,
) -> DynamicImage {
    let colour = source.has_color() || adds_colour;
    let alpha = source.has_alpha() || adds_alpha;
    let target = match (depth(source), colour, alpha) {
        (32, _, true) => ColorType::Rgba32F,
        (32, _, false) => ColorType::Rgb32F,
        (16, true, true) => ColorType::Rgba16,
        (16, true, false) => ColorType::Rgb16,
        (16, false, true) => ColorType::La16,
        (16, false, false) => ColorType::L16,
        (_, true, true) => ColorType::Rgba8,
        (_, true, false) => ColorType::Rgb8,
        (_, false, true) => ColorType::La8,
        (_, false, false) => ColorType::L8,
    };
    if work.color() == target {
        return work;
    }
    match target {
        ColorType::Rgba32F => DynamicImage::ImageRgba32F(work.to_rgba32f()),
        ColorType::Rgb32F => DynamicImage::ImageRgb32F(work.to_rgb32f()),
        ColorType::Rgba16 => DynamicImage::ImageRgba16(work.to_rgba16()),
        ColorType::Rgb16 => DynamicImage::ImageRgb16(work.to_rgb16()),
        ColorType::La16 => DynamicImage::ImageLumaA16(work.to_luma_alpha16()),
        ColorType::L16 => DynamicImage::ImageLuma16(work.to_luma16()),
        ColorType::Rgba8 => DynamicImage::ImageRgba8(work.to_rgba8()),
        ColorType::Rgb8 => DynamicImage::ImageRgb8(work.to_rgb8()),
        ColorType::La8 => DynamicImage::ImageLumaA8(work.to_luma_alpha8()),
        _ => DynamicImage::ImageLuma8(work.to_luma8()),
    }
}

/// Runs `f` over every pixel as RGBA in 0..=1, in parallel, at 8-bit
/// precision for 8-bit sources and float precision otherwise.
fn map_pixels(
    image: &DynamicImage,
    adds_colour: bool,
    adds_alpha: bool,
    f: impl Fn([f32; 4]) -> [f32; 4] + Sync,
) -> DynamicImage {
    let source = image.color();
    let work = if depth(source) == 8 {
        let mut buffer = image.to_rgba8();
        buffer.par_chunks_mut(4).for_each(|pixel| {
            let out = f([
                f32::from(pixel[0]) / 255.0,
                f32::from(pixel[1]) / 255.0,
                f32::from(pixel[2]) / 255.0,
                f32::from(pixel[3]) / 255.0,
            ]);
            for (target, value) in pixel.iter_mut().zip(out) {
                *target = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        DynamicImage::ImageRgba8(buffer)
    } else {
        let mut buffer = image.to_rgba32f();
        buffer.par_chunks_mut(4).for_each(|pixel| {
            let out = f([pixel[0], pixel[1], pixel[2], pixel[3]]);
            pixel.copy_from_slice(&out.map(|value| value.clamp(0.0, 1.0)));
        });
        DynamicImage::ImageRgba32F(buffer)
    };
    finish(source, work, adds_colour, adds_alpha)
}

/// Runs `f` over the colour channels, leaving alpha alone.
fn map_colour(
    image: &DynamicImage,
    adds_colour: bool,
    f: impl Fn(f32) -> f32 + Sync,
) -> DynamicImage {
    map_pixels(image, adds_colour, false, |[r, g, b, a]| {
        [f(r), f(g), f(b), a]
    })
}

fn resize(
    image: DynamicImage,
    mode: ResizeMode,
    filter: Filter,
    enlarge: bool,
) -> Result<DynamicImage, Error> {
    let (width, height) = image.dimensions();
    let (w, h) = (f64::from(width), f64::from(height));
    let zero = || Error::Invalid("resize: the target size is zero".into());
    let scale = match mode {
        ResizeMode::Exact {
            width: target_w,
            height: target_h,
        } => {
            if target_w == 0 || target_h == 0 {
                return Err(zero());
            }
            if !enlarge && (target_w > width || target_h > height) {
                return Ok(image);
            }
            return Ok(image.resize_exact(target_w, target_h, filter.to_image()));
        }
        ResizeMode::Fill {
            width: target_w,
            height: target_h,
        } => {
            if target_w == 0 || target_h == 0 {
                return Err(zero());
            }
            if !enlarge && (target_w > width || target_h > height) {
                // Too small to fill: keep the size, crop to the box's shape.
                return crop(
                    &image,
                    CropMode::Aspect {
                        width: target_w,
                        height: target_h,
                        anchor: Anchor::CENTER,
                    },
                );
            }
            return Ok(image.resize_to_fill(target_w, target_h, filter.to_image()));
        }
        ResizeMode::Fit {
            width: target_w,
            height: target_h,
        } => (f64::from(target_w) / w).min(f64::from(target_h) / h),
        ResizeMode::Width { width: target } => f64::from(target) / w,
        ResizeMode::Height { height: target } => f64::from(target) / h,
        ResizeMode::LongSide { size } => f64::from(size) / w.max(h),
        ResizeMode::ShortSide { size } => f64::from(size) / w.min(h),
        ResizeMode::Percent { percent } => f64::from(percent) / 100.0,
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err(zero());
    }
    if (!enlarge && scale > 1.0) || (scale - 1.0).abs() < 1e-9 {
        return Ok(image);
    }
    let new_w = (w * scale).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    let new_h = (h * scale).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    if u64::from(new_w) * u64::from(new_h) > 1 << 30 {
        return Err(Error::Invalid(
            "resize: the result would be too large".into(),
        ));
    }
    Ok(image.resize_exact(new_w, new_h, filter.to_image()))
}

fn crop(image: &DynamicImage, mode: CropMode) -> Result<DynamicImage, Error> {
    let (width, height) = image.dimensions();
    let (x, y, w, h) = match mode {
        CropMode::Rect {
            x,
            y,
            width: w,
            height: h,
        } => {
            let x = x.min(width);
            let y = y.min(height);
            (x, y, w.min(width - x), h.min(height - y))
        }
        CropMode::Aspect {
            width: ratio_w,
            height: ratio_h,
            anchor,
        } => {
            if ratio_w == 0 || ratio_h == 0 {
                return Err(Error::Invalid("crop: the aspect ratio is zero".into()));
            }
            let by_width = u64::from(width) * u64::from(ratio_h) / u64::from(ratio_w);
            let (w, h) = if by_width <= u64::from(height) {
                (width, by_width as u32)
            } else {
                let by_height = u64::from(height) * u64::from(ratio_w) / u64::from(ratio_h);
                (by_height as u32, height)
            };
            let x = anchor.x.offset(width, w) as u32;
            let y = anchor.y.offset(height, h) as u32;
            (x, y, w, h)
        }
        CropMode::Margins {
            left,
            top,
            right,
            bottom,
        } => {
            let w = width.saturating_sub(left.saturating_add(right));
            let h = height.saturating_sub(top.saturating_add(bottom));
            (left.min(width), top.min(height), w, h)
        }
        CropMode::Trim { tolerance } => trim_bounds(image, tolerance),
    };
    if w == 0 || h == 0 {
        return Err(Error::Invalid("crop: nothing would remain".into()));
    }
    Ok(image.crop_imm(x, y, w, h))
}

/// The bounds left when uniform borders of the top-left colour are removed.
fn trim_bounds(image: &DynamicImage, tolerance: u8) -> (u32, u32, u32, u32) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let border = *rgba.get_pixel(0, 0);
    let close = |x: u32, y: u32| {
        rgba.get_pixel(x, y)
            .0
            .iter()
            .zip(border.0)
            .all(|(&a, b)| a.abs_diff(b) <= tolerance)
    };
    let row_is_border = |y: u32| (0..width).all(|x| close(x, y));
    let column_is_border = |x: u32, top: u32, bottom: u32| (top..bottom).all(|y| close(x, y));
    let top = (0..height).find(|&y| !row_is_border(y)).unwrap_or(height);
    if top == height {
        // All border: keep the picture as it is.
        return (0, 0, width, height);
    }
    let bottom = (top..height)
        .rev()
        .find(|&y| !row_is_border(y))
        .map_or(height, |y| y + 1);
    let left = (0..width)
        .find(|&x| !column_is_border(x, top, bottom))
        .unwrap_or(0);
    let right = (left..width)
        .rev()
        .find(|&x| !column_is_border(x, top, bottom))
        .map_or(width, |x| x + 1);
    (left, top, right - left, bottom - top)
}

fn canvas(
    image: &DynamicImage,
    mode: CanvasMode,
    colour: [u8; 4],
    anchor: Anchor,
) -> Result<DynamicImage, Error> {
    let (width, height) = image.dimensions();
    let (new_w, new_h) = match mode {
        CanvasMode::Size { width, height } => (width, height),
        CanvasMode::Aspect {
            width: ratio_w,
            height: ratio_h,
        } => {
            if ratio_w == 0 || ratio_h == 0 {
                return Err(Error::Invalid("canvas: the aspect ratio is zero".into()));
            }
            let by_width = (u64::from(width) * u64::from(ratio_h)).div_ceil(u64::from(ratio_w));
            if by_width >= u64::from(height) {
                (width, u32::try_from(by_width).unwrap_or(u32::MAX))
            } else {
                let by_height =
                    (u64::from(height) * u64::from(ratio_w)).div_ceil(u64::from(ratio_h));
                (u32::try_from(by_height).unwrap_or(u32::MAX), height)
            }
        }
        CanvasMode::Border { size } => (
            width.saturating_add(size.saturating_mul(2)),
            height.saturating_add(size.saturating_mul(2)),
        ),
    };
    if new_w == 0 || new_h == 0 {
        return Err(Error::Invalid("canvas: the size is zero".into()));
    }
    if u64::from(new_w) * u64::from(new_h) > 1 << 30 {
        return Err(Error::Invalid(
            "canvas: the result would be too large".into(),
        ));
    }
    let (x, y) = match mode {
        CanvasMode::Border { size } => (i64::from(size), i64::from(size)),
        _ => (
            anchor.x.offset(new_w, width),
            anchor.y.offset(new_h, height),
        ),
    };
    let source = image.color();
    let adds_alpha = colour[3] < 255;
    let adds_colour = colour[0] != colour[1] || colour[1] != colour[2];
    let work = if depth(source) == 8 {
        let mut target = RgbaImage::from_pixel(new_w, new_h, Rgba(colour));
        image::imageops::replace(&mut target, &image.to_rgba8(), x, y);
        DynamicImage::ImageRgba8(target)
    } else {
        let fill = Rgba(colour.map(|channel| f32::from(channel) / 255.0));
        let mut target = Rgba32FImage::from_pixel(new_w, new_h, fill);
        image::imageops::replace(&mut target, &image.to_rgba32f(), x, y);
        DynamicImage::ImageRgba32F(target)
    };
    Ok(finish(source, work, adds_colour, adds_alpha))
}

/// Turns by any angle with bilinear sampling, growing the canvas.
fn straighten(image: &DynamicImage, degrees: f32, fill: [u8; 4]) -> DynamicImage {
    let degrees = degrees.rem_euclid(360.0);
    if degrees.abs() < 1e-4 {
        return image.clone();
    }
    let source = image.to_rgba32f();
    let (width, height) = source.dimensions();
    let (sin, cos) = f64::from(degrees).to_radians().sin_cos();
    let (w, h) = (f64::from(width), f64::from(height));
    let new_w = (w * cos.abs() + h * sin.abs()).round().max(1.0) as u32;
    let new_h = (w * sin.abs() + h * cos.abs()).round().max(1.0) as u32;
    let fill = fill.map(|channel| f32::from(channel) / 255.0);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (ncx, ncy) = (f64::from(new_w) / 2.0, f64::from(new_h) / 2.0);
    let mut out: Rgba32FImage = ImageBuffer::new(new_w, new_h);
    let row_len = new_w as usize * 4;
    out.par_chunks_mut(row_len)
        .enumerate()
        .for_each(|(row, line)| {
            let dy = row as f64 + 0.5 - ncy;
            for (column, pixel) in line.chunks_mut(4).enumerate() {
                let dx = column as f64 + 0.5 - ncx;
                // Inverse of a clockwise turn (y grows downwards).
                let sx = dx * cos + dy * sin + cx - 0.5;
                let sy = -dx * sin + dy * cos + cy - 0.5;
                pixel.copy_from_slice(&bilinear(&source, sx, sy).unwrap_or(fill));
            }
        });
    let adds_alpha = fill[3] < 1.0;
    finish(
        image.color(),
        DynamicImage::ImageRgba32F(out),
        false,
        adds_alpha,
    )
}

fn bilinear(source: &Rgba32FImage, x: f64, y: f64) -> Option<[f32; 4]> {
    let (width, height) = source.dimensions();
    if x < -0.5 || y < -0.5 || x > f64::from(width) - 0.5 || y > f64::from(height) - 0.5 {
        return None;
    }
    let x = x.clamp(0.0, f64::from(width - 1));
    let y = y.clamp(0.0, f64::from(height - 1));
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (fx, fy) = ((x - f64::from(x0)) as f32, (y - f64::from(y0)) as f32);
    let p = |px, py| source.get_pixel(px, py).0;
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut out = [0.0; 4];
    for (index, value) in out.iter_mut().enumerate() {
        let top = a[index] + (b[index] - a[index]) * fx;
        let bottom = c[index] + (d[index] - c[index]) * fx;
        *value = top + (bottom - top) * fy;
    }
    Some(out)
}

/// Stretches each colour channel so that 0.5 % of the pixels clip at each end.
fn auto_contrast(image: &DynamicImage) -> DynamicImage {
    let rgba = image.to_rgba8();
    let mut histograms = [[0u64; 256]; 3];
    for pixel in rgba.pixels() {
        for (histogram, &value) in histograms.iter_mut().zip(&pixel.0[..3]) {
            histogram[usize::from(value)] += 1;
        }
    }
    let total: u64 = histograms[0].iter().sum();
    let clip = total / 200;
    let bounds = histograms.map(|histogram| {
        let mut sum = 0;
        let low = histogram
            .iter()
            .position(|&count| {
                sum += count;
                sum > clip
            })
            .unwrap_or(0);
        sum = 0;
        let high = 255
            - histogram
                .iter()
                .rev()
                .position(|&count| {
                    sum += count;
                    sum > clip
                })
                .unwrap_or(0);
        (low as f32 / 255.0, high.max(low + 1) as f32 / 255.0)
    });
    map_pixels(image, false, false, |[r, g, b, a]| {
        let stretch = |value: f32, (low, high): (f32, f32)| (value - low) / (high - low);
        [
            stretch(r, bounds[0]),
            stretch(g, bounds[1]),
            stretch(b, bounds[2]),
            a,
        ]
    })
}

fn flatten(image: &DynamicImage, colour: [u8; 3]) -> DynamicImage {
    if !image.color().has_alpha() {
        return image.clone();
    }
    let background = colour.map(|channel| f32::from(channel) / 255.0);
    let source = image.color();
    let flat = map_pixels(image, false, false, |[r, g, b, a]| {
        [
            r * a + background[0] * (1.0 - a),
            g * a + background[1] * (1.0 - a),
            b * a + background[2] * (1.0 - a),
            1.0,
        ]
    });
    // Drop the now opaque alpha channel; a coloured background on a grey
    // picture makes it a colour picture.
    let colourful = source.has_color() || colour[0] != colour[1] || colour[1] != colour[2];
    match (depth(source), colourful) {
        (32, _) => DynamicImage::ImageRgb32F(flat.to_rgb32f()),
        (16, true) => DynamicImage::ImageRgb16(flat.to_rgb16()),
        (16, false) => DynamicImage::ImageLuma16(flat.to_luma16()),
        (_, true) => DynamicImage::ImageRgb8(flat.to_rgb8()),
        (_, false) => DynamicImage::ImageLuma8(flat.to_luma8()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([
                (x * 255 / width.max(1)) as u8,
                (y * 255 / height.max(1)) as u8,
                128,
                255,
            ]);
        }
        DynamicImage::ImageRgba8(image)
    }

    #[test]
    fn resize_modes() {
        let image = picture(400, 200);
        let size = |mode, enlarge| {
            Operation::Resize {
                mode,
                filter: Filter::Bilinear,
                enlarge,
            }
            .apply(image.clone())
            .expect("resizes")
            .dimensions()
        };
        assert_eq!(
            size(
                ResizeMode::Fit {
                    width: 100,
                    height: 100
                },
                false
            ),
            (100, 50)
        );
        assert_eq!(
            size(
                ResizeMode::Fill {
                    width: 100,
                    height: 100
                },
                false
            ),
            (100, 100)
        );
        assert_eq!(
            size(
                ResizeMode::Exact {
                    width: 10,
                    height: 30
                },
                false
            ),
            (10, 30)
        );
        assert_eq!(size(ResizeMode::Width { width: 200 }, false), (200, 100));
        assert_eq!(size(ResizeMode::Height { height: 50 }, false), (100, 50));
        assert_eq!(size(ResizeMode::LongSide { size: 40 }, false), (40, 20));
        assert_eq!(size(ResizeMode::ShortSide { size: 40 }, false), (80, 40));
        assert_eq!(
            size(ResizeMode::Percent { percent: 25.0 }, false),
            (100, 50)
        );
        // No growth unless asked for.
        assert_eq!(size(ResizeMode::LongSide { size: 800 }, false), (400, 200));
        assert_eq!(size(ResizeMode::LongSide { size: 800 }, true), (800, 400));
        assert_eq!(
            size(
                ResizeMode::Fill {
                    width: 300,
                    height: 300
                },
                false
            ),
            (200, 200)
        );
        assert!(Operation::Resize {
            mode: ResizeMode::Width { width: 0 },
            filter: Filter::Nearest,
            enlarge: true
        }
        .apply(image)
        .is_err());
    }

    #[test]
    fn crops() {
        let image = picture(400, 200);
        let size = |mode| crop(&image, mode).expect("crops").dimensions();
        assert_eq!(
            size(CropMode::Aspect {
                width: 1,
                height: 1,
                anchor: Anchor::CENTER
            }),
            (200, 200)
        );
        assert_eq!(
            size(CropMode::Margins {
                left: 10,
                top: 20,
                right: 30,
                bottom: 40
            }),
            (360, 140)
        );
        assert_eq!(
            size(CropMode::Rect {
                x: 350,
                y: 0,
                width: 100,
                height: 10
            }),
            (50, 10)
        );
        assert!(crop(
            &image,
            CropMode::Margins {
                left: 400,
                top: 0,
                right: 0,
                bottom: 0
            }
        )
        .is_err());
    }

    #[test]
    fn trims_uniform_borders() {
        let mut image = RgbaImage::from_pixel(20, 10, Rgba([255, 255, 255, 255]));
        for x in 5..9 {
            for y in 2..7 {
                image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let image = DynamicImage::ImageRgba8(image);
        let trimmed = crop(&image, CropMode::Trim { tolerance: 0 }).expect("trims");
        assert_eq!(trimmed.dimensions(), (4, 5));
        assert_eq!(trimmed.to_rgba8().get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn canvas_and_border_keep_the_depth() {
        let image = DynamicImage::ImageRgb16(picture(10, 10).to_rgb16());
        let bordered = canvas(
            &image,
            CanvasMode::Border { size: 5 },
            [255, 255, 255, 255],
            Anchor::CENTER,
        )
        .expect("pads");
        assert_eq!(bordered.dimensions(), (20, 20));
        assert_eq!(bordered.color(), ColorType::Rgb16);
        let square = canvas(
            &picture(40, 10),
            CanvasMode::Aspect {
                width: 1,
                height: 1,
            },
            [0, 0, 0, 0],
            Anchor::CENTER,
        )
        .expect("pads");
        assert_eq!(square.dimensions(), (40, 40));
        assert_eq!(square.color(), ColorType::Rgba8);
        assert_eq!(square.to_rgba8().get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn colour_operations_keep_grey_and_opacity() {
        let grey = DynamicImage::ImageLuma8(picture(8, 8).to_luma8());
        for operation in [
            Operation::Brightness(20.0),
            Operation::Contrast(-20.0),
            Operation::Gamma(2.0),
            Operation::AutoContrast,
            Operation::Saturation(50.0),
        ] {
            assert_eq!(
                operation.apply(grey.clone()).expect("applies").color(),
                ColorType::L8,
                "{operation:?}"
            );
        }
        assert_eq!(
            Operation::Sepia.apply(grey.clone()).expect("tones").color(),
            ColorType::Rgb8
        );
        let dark = Operation::Brightness(-100.0).apply(grey).expect("darkens");
        assert!(dark.to_luma8().pixels().all(|pixel| pixel.0[0] == 0));
    }

    #[test]
    fn straighten_grows_the_canvas() {
        let image = picture(100, 50);
        let turned = straighten(&image, 90.0, [0, 0, 0, 255]);
        assert_eq!(turned.dimensions(), (50, 100));
        let tilted = straighten(&image, 10.0, [0, 0, 0, 0]);
        assert!(tilted.width() > 100 && tilted.height() > 50);
        assert!(tilted.color().has_alpha());
        assert_eq!(straighten(&image, 360.0, [0; 4]).dimensions(), (100, 50));
    }

    #[test]
    fn flatten_removes_alpha() {
        let mut image = RgbaImage::new(2, 1);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
        let flat = flatten(&DynamicImage::ImageRgba8(image), [255, 0, 0]);
        assert_eq!(flat.color(), ColorType::Rgb8);
        assert_eq!(flat.to_rgb8().get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(flat.to_rgb8().get_pixel(1, 0).0, [0, 0, 0]);
    }

    #[test]
    fn recipes_round_trip_through_json() {
        let operations = Operation::catalogue();
        let json = serde_json::to_string(&operations).expect("serializes");
        let back: Vec<Operation> = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, operations);
        let image = apply_all(picture(30, 20), &operations, &Tools::default()).expect("all apply");
        assert!(image.width() > 0);
    }

    #[test]
    fn host_tools_run_by_id() {
        let mut tools = Tools::default();
        tools.register("halve", "Halve", |image: DynamicImage| {
            Ok(image.resize_exact(image.width() / 2, image.height() / 2, FilterType::Nearest))
        });
        assert_eq!(tools.label("halve"), Some("Halve"));
        let step = Operation::Tool { id: "halve".into() };
        let image = step.apply_with(picture(10, 10), &tools).expect("runs");
        assert_eq!(image.dimensions(), (5, 5));
        assert!(step.apply(picture(10, 10)).is_err());
    }
}
