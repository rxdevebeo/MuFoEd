//! PNG preview: the SVG export rasterized with `resvg` and the system fonts.

use std::sync::Arc;

use crate::model::ChemDocument;

/// Largest bitmap side, so a large scale cannot ask for gigabytes.
const MAX_SIDE: u32 = 8192;

impl ChemDocument {
    /// Renders the document to a PNG at `scale` device pixels per SVG pixel.
    ///
    /// Text uses the fonts the system provides; the appearance matches the SVG
    /// export rather than the on-screen widget.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic for a non-positive scale, invalid SVG or a
    /// rasterization failure.
    pub fn to_png(&self, scale: f32) -> Result<Vec<u8>, String> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err("PNG scale must be a positive number".into());
        }
        let svg = self.to_svg()?;
        let mut database = resvg::usvg::fontdb::Database::new();
        database.load_system_fonts();
        let options = resvg::usvg::Options {
            fontdb: Arc::new(database),
            ..resvg::usvg::Options::default()
        };
        let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
        let size = tree.size();
        let limit = |side: f32| {
            let pixels = (side * scale).ceil();
            if pixels.is_finite() && pixels >= 1.0 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    (pixels.min(f64::from(MAX_SIDE) as f32)) as u32
                }
            } else {
                1
            }
        };
        let width = limit(size.width());
        let height = limit(size.height());
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| "PNG bitmap could not be allocated".to_owned())?;
        pixmap.fill(resvg::tiny_skia::Color::WHITE);
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap.encode_png().map_err(|e| e.to_string())
    }
}
