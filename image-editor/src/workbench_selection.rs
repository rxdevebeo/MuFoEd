//! Manual selection is independent of the inpaint mask and belongs to Undo/PSD.
use super::{invalid, Document, EditError, MAX_DOCUMENT_BYTES};
use std::collections::VecDeque;

/// How a new geometric selection combines with the existing area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionOp {
    /// Replace the current selection.
    #[default]
    Replace,
    /// Union with the existing area.
    Add,
    /// Subtract the new area.
    Subtract,
    /// Keep the overlapping area.
    Intersect,
}
/// Coverage refinements, expressed in canvas pixels.
#[derive(Clone, Copy, Debug)]
pub enum SelectionRefine {
    /// Complement coverage.
    Invert,
    /// Gaussian feathering with sigma in 0.1..64 pixels.
    Feather(f32),
    /// Expand coverage by a square radius up to128 pixels.
    Grow(u32),
    /// Contract coverage by a square radius up to128 pixels.
    Shrink(u32),
}
impl Document {
    pub(super) fn normalize_selection(&mut self) {
        if !self.selection.iter().any(|v| *v != 0) {
            self.selection.clear();
        }
    }
    /// Empty means no selection; painting then covers the whole target.
    pub fn selection(&self) -> &[u8] {
        &self.selection
    }
    /// Replace coverage, checking dimensions and memory before mutation.
    pub fn set_selection(&mut self, mut area: Vec<u8>) -> Result<(), EditError> {
        if !area.is_empty() && area.len() != self.mask.len() {
            return Err(invalid("Selection dimensions differ from canvas"));
        }
        if !area.iter().any(|v| *v != 0) {
            area.clear();
        }
        if self.memory_bytes() - self.selection.len() + area.len() > MAX_DOCUMENT_BYTES {
            return Err(invalid("Selection exceeds document memory budget"));
        }
        self.selection = area;
        Ok(())
    }
    /// Deselect without altering the inpaint mask.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }
    /// Combine coverage; soft subtraction preserves fractional edges.
    pub fn combine_selection(&mut self, area: Vec<u8>, op: SelectionOp) -> Result<(), EditError> {
        if area.len() != self.mask.len() {
            return Err(invalid("Selection dimensions differ from canvas"));
        }
        let area = area
            .into_iter()
            .enumerate()
            .map(|(i, b)| {
                let a = self.selection.get(i).copied().unwrap_or(0);
                match op {
                    SelectionOp::Replace => b,
                    SelectionOp::Add => a.max(b),
                    SelectionOp::Subtract => {
                        u8::try_from((u16::from(a) * (255 - u16::from(b)) + 127) / 255)
                            .expect("coverage")
                    }
                    SelectionOp::Intersect => a.min(b),
                }
            })
            .collect();
        self.set_selection(area)
    }
    /// Select pixels matching a sample. Contiguous mode uses four-neighbor flood
    /// fill; global mode includes disconnected islands. Premultiplied RGBA distance
    /// ignores hidden RGB in fully transparent pixels. The inpaint mask is untouched.
    pub fn select_by_color(
        &mut self,
        sample: &super::Raster,
        at: [u32; 2],
        tolerance: u8,
        contiguous: bool,
        op: SelectionOp,
    ) -> Result<(), EditError> {
        if sample.width != self.width
            || sample.height != self.height
            || at[0] >= self.width
            || at[1] >= self.height
        {
            return Err(invalid("Color selection sample outside canvas"));
        }
        let canonical = |p: &[u8]| {
            [
                u8::try_from(u16::from(p[0]) * u16::from(p[3]) / 255)
                    .expect("premultiplied channel"),
                u8::try_from(u16::from(p[1]) * u16::from(p[3]) / 255)
                    .expect("premultiplied channel"),
                u8::try_from(u16::from(p[2]) * u16::from(p[3]) / 255)
                    .expect("premultiplied channel"),
                p[3],
            ]
        };
        let start = at[1] as usize * self.width as usize + at[0] as usize;
        let reference = canonical(&sample.pixels[start * 4..start * 4 + 4]);
        let matches = |i: usize| {
            canonical(&sample.pixels[i * 4..i * 4 + 4])
                .iter()
                .zip(reference)
                .all(|(a, b)| a.abs_diff(b) <= tolerance)
        };
        let mut area = vec![0; self.mask.len()];
        if contiguous {
            // One byte visitation map; each index is pushed at most once.
            let mut seen = vec![false; area.len()];
            let mut stack = vec![u32::try_from(start).expect("bounded canvas")];
            seen[start] = true;
            while let Some(i) = stack.pop() {
                let i = i as usize;
                if !matches(i) {
                    continue;
                }
                area[i] = 255;
                let w = self.width as usize;
                for next in [
                    (!i.is_multiple_of(w)).then(|| i - 1),
                    (i % w + 1 < w).then_some(i + 1),
                    (i >= w).then(|| i - w),
                    (i + w < area.len()).then_some(i + w),
                ]
                .into_iter()
                .flatten()
                {
                    if !seen[next] {
                        seen[next] = true;
                        stack.push(u32::try_from(next).expect("bounded canvas"));
                    }
                }
            }
        } else {
            for (i, v) in area.iter_mut().enumerate() {
                if matches(i) {
                    *v = 255;
                }
            }
        }
        self.combine_selection(area, op)
    }
    /// Rectangular manual selection; the inpaint mask is preserved.
    pub fn select_area_rectangle(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        op: SelectionOp,
    ) -> Result<(), EditError> {
        let mut shape = Self::new(self.width, self.height)?;
        shape.select_rectangle(from, to)?;
        self.combine_selection(shape.mask, op)
    }
    /// Polygon manual selection; the inpaint mask is preserved.
    pub fn select_area_polygon(
        &mut self,
        points: &[[f32; 2]],
        op: SelectionOp,
    ) -> Result<(), EditError> {
        let mut shape = Self::new(self.width, self.height)?;
        shape.select_polygon(points)?;
        self.combine_selection(shape.mask, op)
    }
    /// Select the active layer's alpha, independently of layer visibility.
    pub fn select_layer_alpha(&mut self) -> Result<(), EditError> {
        self.set_selection(
            self.layers[self.active]
                .raster
                .pixels
                .chunks_exact(4)
                .map(|p| p[3])
                .collect(),
        )
    }
    /// Copy either area to the other explicitly, without linking their lifetimes.
    pub fn selection_to_mask(&mut self) -> Result<(), EditError> {
        if self.selection.is_empty() {
            return Err(invalid("Сначала выделите область"));
        }
        self.mask.clone_from(&self.selection);
        Ok(())
    }
    /// Replace manual selection with mask coverage.
    pub fn mask_to_selection(&mut self) -> Result<(), EditError> {
        self.set_selection(self.mask.clone())
    }
    /// Refine the manual selection without touching layers or the inpaint mask.
    pub fn refine_selection(&mut self, refine: SelectionRefine) -> Result<(), EditError> {
        if matches!(refine, SelectionRefine::Invert) {
            let area = if self.selection.is_empty() {
                vec![255; self.mask.len()]
            } else {
                self.selection.iter().map(|v| 255 - v).collect()
            };
            return self.set_selection(area);
        }
        if self.selection.is_empty() {
            return Err(invalid("Сначала выделите область"));
        }
        let area = match refine {
            SelectionRefine::Feather(sigma) => {
                if !sigma.is_finite() || !(0.1..=64.0).contains(&sigma) {
                    return Err(invalid("Feather sigma must be 0.1..64"));
                }
                let img =
                    image::GrayImage::from_raw(self.width, self.height, self.selection.clone())
                        .expect("selection");
                image::imageops::blur(&img, sigma).into_raw()
            }
            SelectionRefine::Grow(radius) | SelectionRefine::Shrink(radius) => {
                if radius > 128 {
                    return Err(invalid("Selection radius must be <=128"));
                }
                morphology(
                    &self.selection,
                    self.width as usize,
                    self.height as usize,
                    radius as usize,
                    matches!(refine, SelectionRefine::Grow(_)),
                )
            }
            SelectionRefine::Invert => unreachable!(),
        };
        self.set_selection(area)
    }
    /// Clear selected coverage on the active target; no selection means all.
    pub fn delete_area(&mut self, mask: bool) -> Result<(), EditError> {
        if !mask {
            self.require_unlocked()?;
        }
        for i in 0..self.mask.len() {
            let amount = u16::from(self.selection.get(i).copied().unwrap_or(255));
            let value = if mask {
                &mut self.mask[i]
            } else {
                &mut self.layers[self.active].raster.pixels[i * 4 + 3]
            };
            *value =
                u8::try_from((u16::from(*value) * (255 - amount) + 127) / 255).expect("coverage");
        }
        Ok(())
    }
    /// Mirror active-layer pixels; canvas and other layers stay unchanged.
    pub fn flip_layer(&mut self, horizontal: bool) -> Result<(), EditError> {
        self.require_unlocked()?;
        let raster = &mut self.layers[self.active].raster;
        *raster = super::Raster::from_image(if horizontal {
            image::imageops::flip_horizontal(&raster.image())
        } else {
            image::imageops::flip_vertical(&raster.image())
        });
        Ok(())
    }
}
// Two separable monotonic-deque filters: linear in pixel count, not radius.
fn morphology(src: &[u8], w: usize, h: usize, r: usize, grow: bool) -> Vec<u8> {
    fn line(values: impl Iterator<Item = u8>, n: usize, r: usize, grow: bool) -> Vec<u8> {
        let padded = std::iter::repeat_n(0, r)
            .chain(values)
            .chain(std::iter::repeat_n(0, r));
        let mut queue: VecDeque<(usize, u8)> = VecDeque::new();
        let mut out = Vec::with_capacity(n);
        for (i, v) in padded.enumerate() {
            while queue
                .back()
                .is_some_and(|(_, last)| if grow { *last <= v } else { *last >= v })
            {
                queue.pop_back();
            }
            queue.push_back((i, v));
            while queue.front().is_some_and(|(j, _)| *j + 2 * r < i) {
                queue.pop_front();
            }
            if i >= 2 * r {
                out.push(queue.front().expect("window").1);
            }
        }
        out
    }
    let mut temp = Vec::with_capacity(src.len());
    for row in src.chunks_exact(w) {
        temp.extend(line(row.iter().copied(), w, r, grow));
    }
    let mut out = vec![0; src.len()];
    for x in 0..w {
        for (y, v) in line((0..h).map(|y| temp[y * w + x]), h, r, grow)
            .into_iter()
            .enumerate()
        {
            out[y * w + x] = v;
        }
    }
    out
}

// Hex keeps selection metadata at two bytes per pixel rather than the four
// needed by JSON's [255,255,...]. Legacy array metadata remains readable.
pub(super) fn serialize_area<S: serde::Serializer>(
    area: &[u8],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if area.len() > usize::try_from(super::MAX_PIXELS).expect("pixel limit") {
        return Err(serde::ser::Error::custom("Selection exceeds pixel limit"));
    }
    let mut text = String::with_capacity(area.len() * 2);
    for value in area {
        text.push(char::from(HEX[usize::from(value >> 4)]));
        text.push(char::from(HEX[usize::from(value & 15)]));
    }
    serializer.serialize_str(&text)
}
pub(super) fn deserialize_area<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<u8>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Hex(String),
        Legacy(Vec<u8>),
    }
    let max = usize::try_from(super::MAX_PIXELS).expect("pixel limit");
    match <Stored as serde::Deserialize>::deserialize(deserializer)? {
        Stored::Legacy(area) => {
            if area.len() > max {
                return Err(serde::de::Error::custom("Selection exceeds pixel limit"));
            }
            Ok(area)
        }
        Stored::Hex(text) => {
            if text.len() % 2 != 0 || text.len() / 2 > max {
                return Err(serde::de::Error::custom(
                    "Invalid selection encoding length",
                ));
            }
            let digit = |v: u8| -> Result<u8, D::Error> {
                match v {
                    b'0'..=b'9' => Ok(v - b'0'),
                    b'a'..=b'f' => Ok(v - b'a' + 10),
                    b'A'..=b'F' => Ok(v - b'A' + 10),
                    _ => Err(serde::de::Error::custom("Invalid selection encoding")),
                }
            };
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| Ok(digit(pair[0])? * 16 + digit(pair[1])?))
                .collect()
        }
    }
}
