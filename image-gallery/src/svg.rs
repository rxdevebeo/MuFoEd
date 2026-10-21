//! SVG and SVGZ through resvg.

// The canvas size is clamped to 1..=MAX_SIDE before the casts.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};

use crate::Error;

/// Longest side of a rendered SVG.
const MAX_SIDE: f32 = 16_384.0;

/// Parsing options with the system fonts, loaded once.
fn options() -> &'static usvg::Options<'static> {
    static OPTIONS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        database.load_system_fonts();
        usvg::Options {
            fontdb: Arc::new(database),
            ..usvg::Options::default()
        }
    })
}

/// Renders an SVG (or gzip-compressed SVGZ) document.
///
/// `scale` multiplies the document's own size; `fit`, when given, instead
/// scales the document so its longer side is that many pixels.
pub(crate) fn render(data: &[u8], scale: f32, fit: Option<u32>) -> Result<image::RgbaImage, Error> {
    let tree =
        usvg::Tree::from_data(data, options()).map_err(|error| Error::Codec(error.to_string()))?;
    let size = tree.size();
    let longest = size.width().max(size.height());
    let factor = match fit {
        Some(side) if longest > 0.0 => side as f32 / longest,
        _ if scale.is_finite() && scale > 0.0 => scale,
        _ => 1.0,
    };
    let factor = factor.min(MAX_SIDE / longest.max(1.0));
    let width = (size.width() * factor).ceil().clamp(1.0, MAX_SIDE) as u32;
    let height = (size.height() * factor).ceil().clamp(1.0, MAX_SIDE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| Error::Codec("SVG: canvas could not be allocated".into()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(factor, factor),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let colour = pixel.demultiply();
        rgba.extend_from_slice(&[colour.red(), colour.green(), colour.blue(), colour.alpha()]);
    }
    image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| Error::Codec("SVG: canvas size mismatch".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;

    #[test]
    fn renders_at_natural_size_and_fitted() {
        let image = render(SQUARE, 1.0, None).expect("renders");
        assert_eq!(image.dimensions(), (20, 10));
        assert_eq!(image.get_pixel(2, 2).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(15, 5).0[3], 0);
        let fitted = render(SQUARE, 1.0, Some(200)).expect("renders");
        assert_eq!(fitted.dimensions(), (200, 100));
        assert!(render(b"<svg", 1.0, None).is_err());
    }
}
