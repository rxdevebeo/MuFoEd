//! Bounded adapters for palette, JPEG and MMR layers.
use crate::{bounded, decompress, DjvuError, DjvuLimits};
// Accept owned codec errors directly as map_err callbacks.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn error(message: impl ToString) -> DjvuError {
    DjvuError::Image(message.to_string())
}

pub(super) struct Palette {
    colors: Vec<[u8; 3]>,
    indices: Vec<usize>,
    indexed: bool,
}
impl Palette {
    pub(super) fn parse(data: &[u8], limits: DjvuLimits) -> Result<Self, DjvuError> {
        if data.len() < 3 || data[0] & 0x7f != 0 {
            return Err(error("invalid FGbz version/header"));
        }
        let count = usize::from(u16::from_be_bytes([data[1], data[2]]));
        if count == 0 {
            return Err(error("empty FGbz palette"));
        }
        let end = 3 + count * 3;
        let bytes = data
            .get(3..end)
            .ok_or_else(|| error("truncated FGbz colors"))?;
        let colors = bytes.chunks_exact(3).map(|c| [c[2], c[1], c[0]]).collect();
        let indices = if data[0] & 0x80 != 0 {
            let header = data
                .get(end..end + 3)
                .ok_or_else(|| error("truncated FGbz index count"))?;
            let n = (usize::from(header[0]) << 16)
                | (usize::from(header[1]) << 8)
                | usize::from(header[2]);
            bounded(n, limits.max_sjbz_symbols.min(65536), "FGbz indices")?;
            let raw = decompress(&data[end + 3..], n * 2, limits)?;
            if raw.len() != n * 2 {
                return Err(error("FGbz index table length mismatch"));
            }
            let values = raw
                .chunks_exact(2)
                .map(|c| usize::from(u16::from_be_bytes([c[0], c[1]])))
                .collect::<Vec<_>>();
            if values.iter().any(|&i| i >= count) {
                return Err(error("FGbz index outside palette"));
            }
            values
        } else {
            if data.len() != end || count != 1 {
                return Err(error("FGbz needs per-blit indices for multiple colors"));
            }
            Vec::new()
        };
        Ok(Self {
            colors,
            indices,
            indexed: data[0] & 0x80 != 0,
        })
    }
    pub(super) fn validate(&self, blits: usize) -> Result<(), DjvuError> {
        if self.indexed && self.indices.len() != blits {
            return Err(error("FGbz table does not match JB2 blit count"));
        }
        Ok(())
    }
    pub(super) fn color(&self, blit: usize) -> [u8; 3] {
        self.colors[if self.indices.is_empty() {
            0
        } else {
            self.indices[blit]
        }]
    }
}

pub(super) fn jpeg(
    data: &[u8],
    width: u32,
    height: u32,
    limits: DjvuLimits,
) -> Result<djvu_pixmap::Pixmap, DjvuError> {
    use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
    let options = DecoderOptions::default()
        .set_max_width(width as usize)
        .set_max_height(height as usize)
        .set_strict_mode(true)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(data), options);
    decoder.decode_headers().map_err(error)?;
    let info = decoder.info().ok_or_else(|| error("JPEG has no frame"))?;
    let w = u32::from(info.width);
    let h = u32::from(info.height);
    bounded(
        w as usize * h as usize,
        limits.max_ink_pixels,
        "JPEG pixels",
    )?;
    if w == 0 || h == 0 || !(1..=12).any(|r| width.div_ceil(r) == w && height.div_ceil(r) == h) {
        return Err(error("JPEG dimensions do not match page reduction 1..12"));
    }
    let rgb = decoder.decode().map_err(error)?;
    if rgb.len() != w as usize * h as usize * 3 {
        return Err(error("JPEG did not produce RGB pixels"));
    }
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for pixel in rgb.chunks_exact(3) {
        rgba.extend_from_slice(pixel);
        rgba.push(255);
    }
    Ok(djvu_pixmap::Pixmap {
        width: w,
        height: h,
        data: rgba,
    })
}

pub(super) fn mmr(
    data: &[u8],
    width: u32,
    height: u32,
    limits: DjvuLimits,
) -> Result<djvu_bitmap::Bitmap, DjvuError> {
    if data.len() < 8 || &data[..3] != b"MMR" || data[3] & !3 != 0 {
        return Err(error("invalid Smmr header"));
    }
    if data[3] & 2 != 0 {
        return Err(error("striped Smmr is not yet supported"));
    }
    let w = u32::from(u16::from_be_bytes([data[4], data[5]]));
    let h = u32::from(u16::from_be_bytes([data[6], data[7]]));
    bounded(
        w as usize * h as usize,
        limits.max_ink_pixels,
        "Smmr pixels",
    )?;
    if (w, h) != (width, height) {
        return Err(error("Smmr dimensions differ from INFO"));
    }
    struct Output {
        bitmap: djvu_bitmap::Bitmap,
        x: u32,
        y: u32,
        invert: bool,
        bad: bool,
    }
    impl hayro_ccitt::Decoder for Output {
        fn push_pixels(&mut self, white: bool, count: u32) {
            if self.y >= self.bitmap.height || count > self.bitmap.width.saturating_sub(self.x) {
                self.bad = true;
                return;
            }
            if !white ^ self.invert {
                for x in self.x..self.x + count {
                    self.bitmap.set_black(x, self.y);
                }
            }
            self.x += count;
        }
        fn next_line(&mut self) {
            if self.x != self.bitmap.width {
                self.bad = true;
            }
            self.x = 0;
            self.y = self.y.saturating_add(1);
        }
    }
    let settings = hayro_ccitt::DecodeSettings {
        columns: w,
        rows: h,
        end_of_block: false,
        end_of_line: false,
        rows_are_byte_aligned: false,
        encoding: hayro_ccitt::EncodingMode::Group4,
        invert_black: false,
    };
    let mut output = Output {
        bitmap: djvu_bitmap::Bitmap::new(w, h),
        x: 0,
        y: 0,
        invert: data[3] & 1 != 0,
        bad: false,
    };
    let mut context = hayro_ccitt::DecoderContext::new(settings);
    hayro_ccitt::decode(&data[8..], &mut output, &mut context)
        .map_err(|_| error("invalid/truncated Smmr G4 stream"))?;
    if output.bad || output.y != h {
        return Err(error("incomplete Smmr rows"));
    }
    Ok(output.bitmap)
}
