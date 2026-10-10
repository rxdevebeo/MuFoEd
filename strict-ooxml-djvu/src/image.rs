//! Native layered DjVu composite, refused when a contributing codec is unsupported.
mod codecs;
use crate::{bounded, DjvuError, DjvuLimits, DjvuPage, Rect};
/// A complete native page raster and whether it contains non-white pixels.
#[derive(Debug, Clone)]
pub struct PageImage {
    /// Losslessly encoded composite PNG.
    pub bytes: Vec<u8>,
    /// At least one output pixel differs from white.
    pub has_ink: bool,
}
/// The decoded layers of one page, before compositing and before rotation.
struct Layers {
    width: u32,
    height: u32,
    mask: Option<djvu_bitmap::Bitmap>,
    blit_map: Option<Vec<i32>>,
    palette: Option<codecs::Palette>,
    bg: Option<djvu_pixmap::Pixmap>,
    fg: Option<djvu_pixmap::Pixmap>,
}
impl DjvuPage {
    /// Decode a page to PNG at native resolution, applying INFO rotation.
    /// A partial layer composite is never offered to OCR as a complete image.
    pub fn image_png(&self, limits: DjvuLimits) -> Result<Vec<u8>, DjvuError> {
        Ok(self.image(limits)?.bytes)
    }
    /// Decode once and retain the ink observation used by OCR policy.
    pub fn image(&self, limits: DjvuLimits) -> Result<PageImage, DjvuError> {
        let (w, h) = self.dimensions(limits)?;
        let layers = self.decode_layers(limits)?;
        if layers.mask.is_none() && layers.bg.is_none() {
            return Err(DjvuError::Image("page has no decodable image".into()));
        }
        let rgb = compose(&layers)?;
        let rotation = self.info.rotation;
        let (ow, oh) = if matches!(rotation, 90 | 270) {
            (h, w)
        } else {
            (w, h)
        };
        let rotated = rotate(&rgb, w, h, rotation);
        Ok(PageImage {
            has_ink: rotated.iter().any(|&sample| sample != 255),
            bytes: encode_png(&rotated, ow, oh)?,
        })
    }
    /// The bounding boxes of the 8-connected ink components of the page mask.
    ///
    /// Coordinates are points from the top-left, **unrotated**, in the same space
    /// as the native text zones (SC-D14). A page with no mask chunk yields none.
    pub fn ink_boxes(&self, limits: DjvuLimits) -> Result<Vec<Rect>, DjvuError> {
        let _ = self.dimensions(limits)?;
        let Some(mask) = decode_mask(&self.chunks, self.info, limits)? else {
            return Ok(Vec::new());
        };
        Ok(components(&mask, self.info.dpi, limits))
    }
    /// Bounding boxes of the page's colour/photographic background, in points,
    /// unrotated.
    ///
    /// A page whose background is a picture (IW44/JPEG/legacy plane) reports
    /// where that background is not white; a purely bitonal page has no
    /// background plane and yields none. This is the signal the bitonal mask
    /// cannot give: text and pen strokes live in the mask, a photograph lives in
    /// the background plane, and only the latter marks a figure.
    pub fn background_regions(&self, limits: DjvuLimits) -> Result<Vec<Rect>, DjvuError> {
        let (w, h) = self.dimensions(limits)?;
        let layers = self.decode_layers(limits)?;
        let Some(bg) = layers.bg.as_ref() else {
            return Ok(Vec::new());
        };
        Ok(background_boxes(bg, w, h, self.info.dpi, limits))
    }
    /// A PNG of one unrotated region (in points), for a model.
    ///
    /// INFO rotation is not applied: the region is cut from the same space as the
    /// text zones, so a caller can pass a zone or an ink box unchanged.
    pub fn region_png(&self, limits: DjvuLimits, region: Rect) -> Result<Vec<u8>, DjvuError> {
        let (w, h) = self.dimensions(limits)?;
        let layers = self.decode_layers(limits)?;
        if layers.mask.is_none() && layers.bg.is_none() {
            return Err(DjvuError::Image("page has no decodable image".into()));
        }
        let scale = f64::from(self.info.dpi) / 72.0;
        let x0 = floor_px(region.x, scale, w);
        let y0 = floor_px(region.y, scale, h);
        let x1 = ceil_px(region.x + region.width, scale, w);
        let y1 = ceil_px(region.y + region.height, scale, h);
        if x1 <= x0 || y1 <= y0 {
            return Err(DjvuError::Image("the region is empty".into()));
        }
        let rgb = compose(&layers)?;
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut crop = Vec::with_capacity((cw * ch * 3) as usize);
        for y in y0..y1 {
            let start = ((y * w + x0) * 3) as usize;
            let end = ((y * w + x1) * 3) as usize;
            crop.extend_from_slice(&rgb[start..end]);
        }
        encode_png(&crop, cw, ch)
    }
    /// Unrotated page dimensions in pixels, at the page's own pixel budget.
    fn dimensions(&self, limits: DjvuLimits) -> Result<(u32, u32), DjvuError> {
        let w = u32::from(self.info.width);
        let h = u32::from(self.info.height);
        bounded(
            w as usize * h as usize,
            limits.max_ink_pixels,
            "max_ink_pixels",
        )?;
        Ok((w, h))
    }
    /// Decode every image-bearing chunk; conflicts are refused, not guessed.
    fn decode_layers(&self, limits: DjvuLimits) -> Result<Layers, DjvuError> {
        let w = u32::from(self.info.width);
        let h = u32::from(self.info.height);
        let fail = |e: &dyn std::fmt::Display| DjvuError::Image(e.to_string());
        let mut dict = None;
        let mut mask = None;
        let mut bg = None;
        let mut fg = None;
        let mut bg_jpeg = None;
        let mut fg_jpeg = None;
        let mut palette = None;
        let mut blit_map = None;
        let mut blit_count = 0;
        let indexed = self.chunks.iter().any(|c| c.id == *b"FGbz");
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
                    let bm = if indexed {
                        let (bitmap, map, count) = djvu_jb2::decode_indexed_with_limits(
                            c.data(),
                            dict.as_ref(),
                            limits.max_ink_pixels,
                            limits.max_sjbz_symbols,
                        )
                        .map_err(|e| fail(&e))?;
                        blit_map = Some(map);
                        blit_count = count;
                        bitmap
                    } else {
                        djvu_jb2::decode_with_limits(
                            c.data(),
                            dict.as_ref(),
                            limits.max_ink_pixels,
                            limits.max_sjbz_symbols,
                        )
                        .map_err(|e| fail(&e))?
                    };
                    if bm.width != w || bm.height != h {
                        return Err(DjvuError::Image("JB2 dimensions differ from INFO".into()));
                    }
                    mask = Some(bm);
                }
                b"BG44" | b"FG44" | b"PM44" | b"BM44" => {
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
                    let plane = if c.id == *b"FG44" { &mut fg } else { &mut bg };
                    plane
                        .get_or_insert_with(djvu_iw44::Iw44Image::new)
                        .decode_chunk(c.data())
                        .map_err(|e| fail(&e))?;
                }
                b"Smmr" => {
                    if mask.is_some() {
                        return Err(codecs::error("multiple masks"));
                    }
                    mask = Some(codecs::mmr(c.data(), w, h, limits)?);
                }
                b"FGbz" => {
                    if palette.is_some() {
                        return Err(codecs::error("multiple FGbz palettes"));
                    }
                    palette = Some(codecs::Palette::parse(c.data(), limits)?);
                }
                b"BGjp" | b"FGjp" => {
                    let plane = if c.id == *b"BGjp" {
                        &mut bg_jpeg
                    } else {
                        &mut fg_jpeg
                    };
                    if plane.is_some() {
                        return Err(codecs::error("multiple JPEG planes"));
                    }
                    *plane = Some(codecs::jpeg(c.data(), w, h, limits)?);
                }
                _ => {}
            }
        }
        if mask.is_none() && (fg.is_some() || fg_jpeg.is_some() || palette.is_some()) {
            return Err(DjvuError::Image("foreground has no mask".into()));
        }
        if (bg.is_some() && bg_jpeg.is_some())
            || usize::from(fg.is_some())
                + usize::from(fg_jpeg.is_some())
                + usize::from(palette.is_some())
                > 1
        {
            return Err(codecs::error("conflicting color planes"));
        }
        if let Some(palette) = &palette {
            if blit_map.is_none() {
                return Err(codecs::error("FGbz needs a JB2 mask"));
            }
            palette.validate(blit_count)?;
        }
        let bg = bg
            .map(|p| p.to_rgb())
            .transpose()
            .map_err(|e| fail(&e))?
            .or(bg_jpeg);
        let fg = fg
            .map(|p| p.to_rgb())
            .transpose()
            .map_err(|e| fail(&e))?
            .or(fg_jpeg);
        Ok(Layers {
            width: w,
            height: h,
            mask,
            blit_map,
            palette,
            bg,
            fg,
        })
    }
}
/// The unrotated RGB composite of decoded layers, top-left row order.
fn compose(layers: &Layers) -> Result<Vec<u8>, DjvuError> {
    let (w, h) = (layers.width, layers.height);
    let mut rgb = vec![255; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            // Both codec output buffers use top-left row order.
            let ink = layers.mask.as_ref().is_some_and(|m| m.get(x, y));
            let plane = if ink {
                layers.fg.as_ref()
            } else {
                layers.bg.as_ref()
            };
            let mut color = plane.map_or(if ink { (0, 0, 0) } else { (255, 255, 255) }, |p| {
                p.get_rgb(x * p.width / w, y * p.height / h)
            });
            if ink {
                if let (Some(palette), Some(map)) = (&layers.palette, &layers.blit_map) {
                    let blit = usize::try_from(map[y as usize * w as usize + x as usize])
                        .map_err(|_| codecs::error("JB2 ink has no blit index"))?;
                    let [r, g, b] = palette.color(blit);
                    color = (r, g, b);
                }
            }
            let at = ((y * w + x) * 3) as usize;
            rgb[at..at + 3].copy_from_slice(&[color.0, color.1, color.2]);
        }
    }
    Ok(rgb)
}
/// Rotates an unrotated RGB buffer by an INFO flag's degrees.
fn rotate(rgb: &[u8], w: u32, h: u32, rotation: u16) -> Vec<u8> {
    if rotation == 0 {
        return rgb.to_vec();
    }
    let (ow, oh) = if matches!(rotation, 90 | 270) {
        (h, w)
    } else {
        (w, h)
    };
    let mut out = vec![255; (ow * oh * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let (ox, oy) = match rotation {
                90 => (y, w - 1 - x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (h - 1 - y, x),
                _ => (x, y),
            };
            let src = ((y * w + x) * 3) as usize;
            let dst = ((oy * ow + ox) * 3) as usize;
            out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
        }
    }
    out
}
/// One point coordinate to a clamped pixel edge, rounding toward zero.
fn floor_px(value: f64, scale: f64, limit: u32) -> u32 {
    edge(value * scale, limit, f64::floor)
}
/// One point coordinate to a clamped pixel edge, rounding away from zero.
fn ceil_px(value: f64, scale: f64, limit: u32) -> u32 {
    edge(value * scale, limit, f64::ceil)
}
fn edge(scaled: f64, limit: u32, round: fn(f64) -> f64) -> u32 {
    if scaled.is_finite() {
        round(scaled).clamp(0.0, f64::from(limit)) as u32
    } else {
        0
    }
}
fn encode_png(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>, DjvuError> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, w, h);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| codecs::error(&e))?;
        writer
            .write_image_data(rgb)
            .map_err(|e| codecs::error(&e))?;
    }
    Ok(png)
}
/// The boolean page mask, decoded from `Djbz`/`Sjbz`/`Smmr`, if the page has one.
fn decode_mask(
    chunks: &[crate::Chunk],
    info: crate::PageInfo,
    limits: DjvuLimits,
) -> Result<Option<djvu_bitmap::Bitmap>, DjvuError> {
    let w = u32::from(info.width);
    let h = u32::from(info.height);
    let fail = |e: &dyn std::fmt::Display| DjvuError::Image(e.to_string());
    let indexed = chunks.iter().any(|c| c.id == *b"FGbz");
    let mut dict = None;
    let mut mask = None;
    for c in chunks {
        match &c.id {
            b"Djbz" => {
                if dict.is_some() || mask.is_some() {
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
                    return Err(codecs::error("multiple masks"));
                }
                let bm = if indexed {
                    djvu_jb2::decode_indexed_with_limits(
                        c.data(),
                        dict.as_ref(),
                        limits.max_ink_pixels,
                        limits.max_sjbz_symbols,
                    )
                    .map_err(|e| fail(&e))?
                    .0
                } else {
                    djvu_jb2::decode_with_limits(
                        c.data(),
                        dict.as_ref(),
                        limits.max_ink_pixels,
                        limits.max_sjbz_symbols,
                    )
                    .map_err(|e| fail(&e))?
                };
                if bm.width != w || bm.height != h {
                    return Err(codecs::error("JB2 dimensions differ from INFO"));
                }
                mask = Some(bm);
            }
            b"Smmr" => {
                if mask.is_some() {
                    return Err(codecs::error("multiple masks"));
                }
                mask = Some(codecs::mmr(c.data(), w, h, limits)?);
            }
            _ => {}
        }
    }
    Ok(mask)
}
/// Bounding boxes of the 8-connected black components, in points, capped.
fn components(mask: &djvu_bitmap::Bitmap, dpi: u16, limits: DjvuLimits) -> Vec<Rect> {
    let w = mask.width as usize;
    let h = mask.height as usize;
    let scale = 72.0 / f64::from(dpi);
    let cap = limits.max_zones.max(1);
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let mut seen = vec![false; w * h];
    let mut boxes = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if seen[y * w + x] || !mask.get(x as u32, y as u32) {
                continue;
            }
            let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (x, x, y, y);
            stack.clear();
            stack.push((x, y));
            seen[y * w + x] = true;
            while let Some((cx, cy)) = stack.pop() {
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = cx as i32 + dx;
                        let ny = cy as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let (nx, ny) = (nx as usize, ny as usize);
                        if seen[ny * w + nx] || !mask.get(nx as u32, ny as u32) {
                            continue;
                        }
                        seen[ny * w + nx] = true;
                        stack.push((nx, ny));
                        lo_x = lo_x.min(nx);
                        hi_x = hi_x.max(nx);
                        lo_y = lo_y.min(ny);
                        hi_y = hi_y.max(ny);
                    }
                }
            }
            boxes.push(Rect {
                x: lo_x as f64 * scale,
                y: lo_y as f64 * scale,
                width: (hi_x - lo_x + 1) as f64 * scale,
                height: (hi_y - lo_y + 1) as f64 * scale,
            });
            if boxes.len() >= cap {
                return boxes;
            }
        }
    }
    boxes
}
/// Bounding boxes of the 8-connected non-white regions of a background plane,
/// in page points (the plane may be a reduced image, so it is upsampled).
fn background_boxes(
    bg: &djvu_pixmap::Pixmap,
    page_w: u32,
    page_h: u32,
    dpi: u16,
    limits: DjvuLimits,
) -> Vec<Rect> {
    let bw = bg.width as usize;
    let bh = bg.height as usize;
    if bw == 0 || bh == 0 {
        return Vec::new();
    }
    let scale_x = f64::from(page_w) / f64::from(bg.width) * 72.0 / f64::from(dpi);
    let scale_y = f64::from(page_h) / f64::from(bg.height) * 72.0 / f64::from(dpi);
    let cap = limits.max_zones.max(1);
    let mut seen = vec![false; bw * bh];
    let mut boxes = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for y in 0..bh {
        for x in 0..bw {
            if seen[y * bw + x] || background_white(bg, x, y) {
                continue;
            }
            let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (x, x, y, y);
            let mut size = 0usize;
            stack.clear();
            stack.push((x, y));
            seen[y * bw + x] = true;
            while let Some((cx, cy)) = stack.pop() {
                size += 1;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = cx as i32 + dx;
                        let ny = cy as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= bw as i32 || ny >= bh as i32 {
                            continue;
                        }
                        let (nx, ny) = (nx as usize, ny as usize);
                        if seen[ny * bw + nx] || background_white(bg, nx, ny) {
                            continue;
                        }
                        seen[ny * bw + nx] = true;
                        stack.push((nx, ny));
                        lo_x = lo_x.min(nx);
                        hi_x = hi_x.max(nx);
                        lo_y = lo_y.min(ny);
                        hi_y = hi_y.max(ny);
                    }
                }
            }
            // A speck is not a picture.
            if size < 4 {
                continue;
            }
            boxes.push(Rect {
                x: lo_x as f64 * scale_x,
                y: lo_y as f64 * scale_y,
                width: (hi_x - lo_x + 1) as f64 * scale_x,
                height: (hi_y - lo_y + 1) as f64 * scale_y,
            });
            if boxes.len() >= cap {
                return boxes;
            }
        }
    }
    boxes
}
/// A background pixel is white when every channel is at least 240.
fn background_white(bg: &djvu_pixmap::Pixmap, x: usize, y: usize) -> bool {
    let (r, g, b) = bg.get_rgb(x as u32, y as u32);
    r >= 240 && g >= 240 && b >= 240
}
