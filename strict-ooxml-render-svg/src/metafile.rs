//! Rasterization helpers for media a viewer cannot show as stored: legacy
//! Windows metafile previews and TIFF pictures.

/// Rasterizes a WMF or EMF image part to PNG at the requested output size;
/// a TIFF part is decoded to PNG at its own size (WQ-017).
///
/// The raster is rendered at 2x the laid-out size to keep small strokes crisp;
/// `max_uncompressed_bytes` bounds its allocation. Returns `None` when the
/// input is not a supported metafile or cannot be rasterized, so callers can
/// retain their existing placeholder behavior.
#[must_use]
pub fn rasterize_metafile_preview(
    bytes: &[u8],
    width_px: f64,
    height_px: f64,
    max_uncompressed_bytes: u64,
) -> Option<Vec<u8>> {
    const DEFAULT_PIXEL_BUDGET: u64 = 16_000_000;
    let max_pixels = u32::try_from((max_uncompressed_bytes / 4).clamp(1, DEFAULT_PIXEL_BUDGET))
        .unwrap_or(u32::MAX);
    if is_tiff(bytes) {
        return tiff_to_png(bytes, u64::from(max_pixels));
    }
    let raster = emfsdk::render::decode_metafile_as_raster_with_options(
        bytes,
        None,
        emfsdk::render::RenderOptions {
            target_width_px: Some(raster_dimension(width_px)),
            target_height_px: Some(raster_dimension(height_px)),
            max_pixels: Some(max_pixels),
        },
    )
    .ok()??;
    (raster.content_type == "image/png").then_some(raster.data)
}

fn raster_dimension(pixels: f64) -> u32 {
    (pixels * 2.0).ceil().clamp(1.0, f64::from(u32::MAX)) as u32
}

/// Whether `bytes` start with a TIFF header (little- or big-endian).
fn is_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*")
}

/// Decodes the first image of a TIFF to an 8-bit PNG, or `None` when its
/// layout is not one Word writes (palette, float, more than 16 bits) or it
/// exceeds `max_pixels`.
fn tiff_to_png(bytes: &[u8], max_pixels: u64) -> Option<Vec<u8>> {
    use tiff::decoder::{Decoder, DecodingResult, Limits};
    use tiff::ColorType;

    let mut limits = Limits::default();
    limits.decoding_buffer_size = usize::try_from(max_pixels.saturating_mul(8)).ok()?;
    let mut decoder = Decoder::new(std::io::Cursor::new(bytes))
        .ok()?
        .with_limits(limits);
    let (width, height) = decoder.dimensions().ok()?;
    if u64::from(width) * u64::from(height) > max_pixels || width == 0 || height == 0 {
        return None;
    }
    let colortype = decoder.colortype().ok()?;
    let samples: Vec<u8> = match decoder.read_image().ok()? {
        DecodingResult::U8(samples) => samples,
        // 16-bit samples keep their high byte.
        DecodingResult::U16(samples) => samples.iter().map(|value| (value >> 8) as u8).collect(),
        _ => return None,
    };
    let (channels, png_color) = match colortype {
        ColorType::Gray(8 | 16) => (1, png::ColorType::Grayscale),
        ColorType::GrayA(8 | 16) => (2, png::ColorType::GrayscaleAlpha),
        ColorType::RGB(8 | 16) => (3, png::ColorType::Rgb),
        ColorType::RGBA(8 | 16) => (4, png::ColorType::Rgba),
        ColorType::CMYK(8) => (4, png::ColorType::Rgb),
        _ => return None,
    };
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if samples.len() < pixels * channels {
        return None;
    }
    let data: Vec<u8> = if matches!(colortype, ColorType::CMYK(_)) {
        samples
            .chunks_exact(4)
            .take(pixels)
            .filter_map(|pixel| {
                let &[c, m, y, k] = pixel else {
                    return None;
                };
                let [c, m, y, k] = [c, m, y, k].map(u16::from);
                let ink = |value: u16| ((255 - value) * (255 - k) / 255) as u8;
                Some([ink(c), ink(m), ink(y)])
            })
            .flatten()
            .collect()
    } else {
        samples.get(..pixels * channels)?.to_vec()
    };
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png_color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&data).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::rasterize_metafile_preview;

    fn tiff(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut encoder = tiff::encoder::TiffEncoder::new(&mut out).expect("encoder");
        encoder
            .write_image::<tiff::encoder::colortype::RGB8>(width, height, rgb)
            .expect("image");
        out.into_inner()
    }

    #[test]
    fn a_tiff_picture_becomes_a_png() {
        let bytes = tiff(2, 1, &[255, 0, 0, 0, 0, 255]);
        let png = rasterize_metafile_preview(&bytes, 10.0, 10.0, 1 << 20).expect("png");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().expect("png header");
        assert_eq!((reader.info().width, reader.info().height), (2, 1));
    }

    #[test]
    fn an_oversized_tiff_is_refused() {
        let bytes = tiff(4, 4, &[0; 48]);
        assert!(rasterize_metafile_preview(&bytes, 10.0, 10.0, 16).is_none());
    }
}
