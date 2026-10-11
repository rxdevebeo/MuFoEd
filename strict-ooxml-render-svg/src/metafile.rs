//! Rasterization helpers for legacy Windows metafile previews.

/// Rasterizes a WMF or EMF image part to PNG at the requested output size.
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
