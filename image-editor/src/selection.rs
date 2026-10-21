//! Mask-based pixel selections and straight-alpha clipboard fragments.
use super::{blend, invalid, Document, EditError, Raster};

/// Image or mask content cropped to the selection bounds, including soft coverage.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelClipboard {
    raster: Raster,
    origin: [u32; 2],
    mask: bool,
}
impl PixelClipboard {
    /// Import a validated raster, e.g. from the host's system clipboard.
    pub fn from_raster(raster: Raster) -> Self {
        Self {
            raster,
            origin: [0, 0],
            mask: false,
        }
    }
    /// Whether this fragment contains a grayscale selection/inpaint mask.
    pub fn is_mask(&self) -> bool {
        self.mask
    }
    /// Convert clipboard content for the global editing target. Image-to-mask
    /// uses grayscale luminance multiplied by alpha; mask-to-image is grayscale.
    #[must_use]
    pub fn for_mask(&self, mask: bool) -> Self {
        let mut copy = self.clone();
        if mask && !copy.mask {
            for p in copy.raster.pixels.chunks_exact_mut(4) {
                let value = mask_coverage(p.try_into().expect("RGBA"));
                p.copy_from_slice(&[value, value, value, 255]);
            }
        }
        copy.mask = mask;
        copy
    }
    /// Image pixels use straight alpha; masks use opaque grayscale coverage.
    pub fn raster(&self) -> &Raster {
        &self.raster
    }
    /// Original top-left canvas coordinates, for paste in place.
    pub fn origin(&self) -> [u32; 2] {
        self.origin
    }
}
impl Document {
    /// Apply the same pixel algorithms to mask coverage without touching layers.
    /// Structural document operations are rejected; errors leave the mask intact.
    pub fn edit_mask(
        &mut self,
        operation: impl FnOnce(&mut Document) -> Result<(), EditError>,
    ) -> Result<(), EditError> {
        let mut target = Document::from_raster(self.mask_raster())?;
        target.selection.clone_from(&self.selection);
        operation(&mut target)?;
        if target.width != self.width || target.height != self.height || target.layers.len() != 1 {
            return Err(invalid(
                "Операция маски не должна изменять размер холста или число слоёв",
            ));
        }
        self.mask = target.layers[0]
            .raster
            .pixels
            .chunks_exact(4)
            .map(|p| mask_coverage(p.try_into().expect("RGBA")))
            .collect();
        Ok(())
    }
    /// Replace the shared selection/inpaint mask with an even-odd polygon.
    /// Pixel centers determine coverage; points may extend outside the canvas.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn select_polygon(&mut self, points: &[[f32; 2]]) -> Result<(), EditError> {
        if !(3..=4096).contains(&points.len())
            || points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 32768.0)
        {
            return Err(invalid("Лассо: нужны 3..4096 конечных точек"));
        }
        let top = points
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .clamp(0.0, self.height as f32) as u32;
        let bottom = points
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .clamp(0.0, self.height as f32) as u32;
        let mut mask = vec![0; self.mask.len()];
        let mut intersections = Vec::with_capacity(points.len());
        for y in top..bottom {
            let scan = y as f32 + 0.5;
            intersections.clear();
            for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
                if (a[1] > scan) != (b[1] > scan) {
                    intersections.push(a[0] + (scan - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
            intersections.sort_by(f32::total_cmp);
            for pair in intersections.chunks_exact(2) {
                let left = (pair[0] - 0.5).ceil().clamp(0.0, self.width as f32) as usize;
                let right = (pair[1] - 0.5).ceil().clamp(0.0, self.width as f32) as usize;
                let row = y as usize * self.width as usize;
                mask[row + left..row + right].fill(255);
            }
        }
        self.mask = mask;
        Ok(())
    }
    /// Copy the active layer or visible composite through the shared mask.
    /// An empty mask means the whole canvas, as with Copy when nothing is selected.
    pub fn copy_selection(&self, merged: bool) -> Result<PixelClipboard, EditError> {
        self.copy_content(merged, false)
    }
    /// Copy only mask coverage, preserving soft edges and its canvas origin.
    pub fn copy_mask(&self) -> Result<PixelClipboard, EditError> {
        self.copy_content(false, true)
    }
    fn copy_content(&self, merged: bool, mask_only: bool) -> Result<PixelClipboard, EditError> {
        self.copy_content_mask(merged, mask_only.then_some(self.mask.as_slice()))
    }
    pub(super) fn copy_content_mask(
        &self,
        merged: bool,
        mask: Option<&[u8]>,
    ) -> Result<PixelClipboard, EditError> {
        let mask_only = mask.is_some();
        let mask = mask.unwrap_or(&self.mask);
        let coverage = if mask_only && self.selection.is_empty() {
            mask
        } else {
            &self.selection
        };
        let selected = coverage.iter().any(|v| *v > 0);
        if mask_only && !selected {
            return Err(invalid("Маска пуста"));
        }
        let mut bounds = [self.width, self.height, 0, 0];
        if selected {
            for (i, coverage) in coverage.iter().enumerate() {
                if *coverage > 0 {
                    let x = u32::try_from(i % self.width as usize).expect("bounded canvas");
                    let y = u32::try_from(i / self.width as usize).expect("bounded canvas");
                    bounds[0] = bounds[0].min(x);
                    bounds[1] = bounds[1].min(y);
                    bounds[2] = bounds[2].max(x + 1);
                    bounds[3] = bounds[3].max(y + 1);
                }
            }
        } else {
            bounds = [0, 0, self.width, self.height];
        }
        let composite = merged.then(|| self.flatten());
        let masked = (!merged && !mask_only && self.layers[self.active].mask.is_some())
            .then(|| self.layers[self.active].masked_raster());
        let source = composite
            .as_ref()
            .or(masked.as_ref())
            .unwrap_or(&self.layers[self.active].raster);
        let [left, top, right, bottom] = bounds;
        let mut raster = Raster::solid(right - left, bottom - top, [0; 4])?;
        for y in top..bottom {
            for x in left..right {
                let src = (y as usize * self.width as usize + x as usize) * 4;
                let dst = ((y - top) as usize * raster.width as usize + (x - left) as usize) * 4;
                if mask_only {
                    let coverage = if self.selection.is_empty() {
                        mask[src / 4]
                    } else {
                        u8::try_from(
                            (u16::from(mask[src / 4]) * u16::from(self.selection[src / 4]) + 127)
                                / 255,
                        )
                        .expect("coverage")
                    };
                    raster.pixels[dst..dst + 4]
                        .copy_from_slice(&[coverage, coverage, coverage, 255]);
                    continue;
                }
                raster.pixels[dst..dst + 4].copy_from_slice(&source.pixels[src..src + 4]);
                if selected {
                    let alpha = (u16::from(source.pixels[src + 3]) * u16::from(coverage[src / 4])
                        + 127)
                        / 255;
                    raster.pixels[dst + 3] = u8::try_from(alpha).expect("alpha <=255");
                }
                if raster.pixels[dst + 3] == 0 {
                    raster.pixels[dst..dst + 4].fill(0);
                }
            }
        }
        Ok(PixelClipboard {
            raster,
            origin: [left, top],
            mask: mask_only,
        })
    }
    /// Paste with source-over compositing at canvas coordinates. A new layer is
    /// full-canvas and unlocked; current-layer insertion respects its pixel lock.
    /// Existing selection is a source selection and does not clip the paste again.
    /// Mask fragments replace the shared mask; layer targets/locks do not apply.
    pub fn paste_pixels(
        &mut self,
        clip: &PixelClipboard,
        at: [i32; 2],
        new_layer: bool,
    ) -> Result<(), EditError> {
        if !clip.mask && !new_layer {
            self.require_unlocked()?;
        }
        let left = i64::from(at[0]).max(0);
        let top = i64::from(at[1]).max(0);
        let right = (i64::from(at[0]) + i64::from(clip.raster.width)).min(i64::from(self.width));
        let bottom = (i64::from(at[1]) + i64::from(clip.raster.height)).min(i64::from(self.height));
        if left >= right || top >= bottom {
            return Err(invalid("Фрагмент находится за пределами холста"));
        }
        if clip.mask {
            let mut mask = vec![0; self.mask.len()];
            for y in top..bottom {
                for x in left..right {
                    let src = (usize::try_from(y - i64::from(at[1])).expect("clipped")
                        * clip.raster.width as usize
                        + usize::try_from(x - i64::from(at[0])).expect("clipped"))
                        * 4;
                    let dst = usize::try_from(y).expect("clipped") * self.width as usize
                        + usize::try_from(x).expect("clipped");
                    mask[dst] = clip.raster.pixels[src];
                }
            }
            self.mask = mask;
            return Ok(());
        }
        let mut added = if new_layer {
            Some(Raster::solid(self.width, self.height, [0; 4])?)
        } else {
            None
        };
        let target = added
            .as_mut()
            .unwrap_or(&mut self.layers[self.active].raster);
        for y in top..bottom {
            for x in left..right {
                let src = (usize::try_from(y - i64::from(at[1])).expect("clipped")
                    * clip.raster.width as usize
                    + usize::try_from(x - i64::from(at[0])).expect("clipped"))
                    * 4;
                let dst = (usize::try_from(y).expect("clipped") * self.width as usize
                    + usize::try_from(x).expect("clipped"))
                    * 4;
                let color = clip.raster.pixels[src..src + 4].try_into().expect("RGBA");
                blend(&mut target.pixels[dst..dst + 4], color, 1.0);
            }
        }
        if let Some(raster) = added {
            self.add_layer("Вставленный фрагмент".into(), raster)?;
        }
        Ok(())
    }
}

// Integer Rec.709 luminance followed by straight-alpha coverage, rounded once.
pub(super) fn mask_coverage(p: [u8; 4]) -> u8 {
    let luminance =
        (54 * u32::from(p[0]) + 183 * u32::from(p[1]) + 19 * u32::from(p[2]) + 128) / 256;
    u8::try_from((luminance * u32::from(p[3]) + 127) / 255).expect("coverage <=255")
}
