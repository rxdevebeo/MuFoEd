//! Native JB2/IW44 composite, refused when any contributing codec is unsupported.
use crate::{bounded, DjvuError, DjvuLimits, DjvuPage};
/// A complete native page raster and whether it contains non-white pixels.
#[derive(Debug, Clone)]
pub struct PageImage {
    /// Losslessly encoded composite PNG.
    pub bytes: Vec<u8>,
    /// At least one output pixel differs from white.
    pub has_ink: bool,
}
impl DjvuPage {
    /// Decode a page to PNG at native resolution, applying INFO rotation.
    /// A partial layer composite is never offered to OCR as a complete image.
    pub fn image_png(&self, limits: DjvuLimits) -> Result<Vec<u8>, DjvuError> {
        Ok(self.image(limits)?.bytes)
    }
    /// Decode once and retain the ink observation used by OCR policy.
    pub fn image(&self, limits: DjvuLimits) -> Result<PageImage, DjvuError> {
        let w = u32::from(self.info.width);
        let h = u32::from(self.info.height);
        bounded(
            w as usize * h as usize,
            limits.max_ink_pixels,
            "max_ink_pixels",
        )?;
        let fail = |e: &dyn std::fmt::Display| DjvuError::Image(e.to_string());
        let mut dict = None;
        let mut mask = None;
        let mut bg = None;
        let mut fg = None;
        for c in &self.chunks {
            match &c.id {
                b"Djbz" => {
                    if mask.is_some() || dict.is_some() {
                        return Err(DjvuError::Image(
                            "dictionary must precede mask and occur once".into(),
                        ));
                    }
                    dict = Some(
                        djvu_jb2::decode_dict_with_limits(c.data(), None, limits.max_sjbz_symbols)
                            .map_err(|e| fail(&e))?,
                    );
                }
                b"Sjbz" => {
                    if mask.is_some() {
                        return Err(DjvuError::Image("multiple masks".into()));
                    }
                    let bm = djvu_jb2::decode_with_limits(
                        c.data(),
                        dict.as_ref(),
                        limits.max_ink_pixels,
                        limits.max_sjbz_symbols,
                    )
                    .map_err(|e| fail(&e))?;
                    if bm.width != w || bm.height != h {
                        return Err(DjvuError::Image("JB2 dimensions differ from INFO".into()));
                    }
                    mask = Some(bm);
                }
                b"BG44" | b"FG44" => {
                    if c.data().first() == Some(&0) {
                        let header = c
                            .data()
                            .get(..9)
                            .ok_or_else(|| DjvuError::Image("short IW44 header".into()))?;
                        let iw = usize::from(u16::from_be_bytes([header[4], header[5]]));
                        let ih = usize::from(u16::from_be_bytes([header[6], header[7]]));
                        bounded(iw * ih, limits.max_ink_pixels, "max_ink_pixels (IW44)")?;
                        if !(1..=12u32).any(|red| {
                            w.div_ceil(red) as usize == iw && h.div_ceil(red) as usize == ih
                        }) {
                            return Err(DjvuError::Image(
                                "IW44 dimensions do not match a page reduction 1..12".into(),
                            ));
                        }
                    }
                    let plane = if c.id == *b"BG44" { &mut bg } else { &mut fg };
                    plane
                        .get_or_insert_with(djvu_iw44::Iw44Image::new)
                        .decode_chunk(c.data())
                        .map_err(|e| fail(&e))?;
                }
                b"Smmr" | b"FGbz" | b"BGjp" | b"FGjp" => {
                    return Err(DjvuError::Image(format!(
                        "unsupported layer {}",
                        String::from_utf8_lossy(&c.id)
                    )))
                }
                _ => {}
            }
        }
        if mask.is_none() && bg.is_none() {
            return Err(DjvuError::Image("page has no decodable image".into()));
        }
        if mask.is_none() && fg.is_some() {
            return Err(DjvuError::Image("foreground has no mask".into()));
        }
        let bg = bg.map(|p| p.to_rgb()).transpose().map_err(|e| fail(&e))?;
        let fg = fg.map(|p| p.to_rgb()).transpose().map_err(|e| fail(&e))?;
        let rotation = self.info.rotation;
        let (ow, oh) = if matches!(rotation, 90 | 270) {
            (h, w)
        } else {
            (w, h)
        };
        let mut rgb = vec![255; ow as usize * oh as usize * 3];
        for y in 0..h {
            for x in 0..w {
                // Both codec output buffers use top-left row order.
                let ink = mask.as_ref().is_some_and(|m| m.get(x, y));
                let plane = if ink { fg.as_ref() } else { bg.as_ref() };
                let color = plane.map_or(if ink { (0, 0, 0) } else { (255, 255, 255) }, |p| {
                    p.get_rgb(x * p.width / w, y * p.height / h)
                });
                let (ox, oy) = match rotation {
                    90 => (y, w - 1 - x),
                    180 => (w - 1 - x, h - 1 - y),
                    270 => (h - 1 - y, x),
                    _ => (x, y),
                };
                let at = (oy as usize * ow as usize + ox as usize) * 3;
                rgb[at..at + 3].copy_from_slice(&[color.0, color.1, color.2]);
            }
        }
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, ow, oh);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| fail(&e))?;
            writer.write_image_data(&rgb).map_err(|e| fail(&e))?;
        }
        Ok(PageImage {
            bytes: png,
            has_ink: rgb.iter().any(|&sample| sample != 255),
        })
    }
}
