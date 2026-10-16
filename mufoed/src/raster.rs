//! Turning a page's SVG into pixels.
//!
//! The renderer names its bundled faces in `font-family`, so the font database
//! holds exactly those programs (`face_source`), the same bytes that produced
//! the layout's advances. Installed fonts come after them, for scripts the
//! bundle does not draw.

use std::sync::{Arc, OnceLock};

use resvg::usvg;
use strict_ooxml_render_svg::font::builtin::{bundled_families, face_source};

/// The largest page bitmap side, so a huge zoom cannot ask for gigabytes.
const MAX_SIDE: u32 = 8192;

/// A rasterized page.
pub(crate) struct Bitmap {
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// Premultiplied RGBA, as tiny-skia leaves it.
    pub(crate) rgba: Vec<u8>,
}

fn options() -> &'static usvg::Options<'static> {
    static OPTIONS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        let mut seen: Vec<*const u8> = Vec::new();
        for family in bundled_families() {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let Some(source) = face_source(family, bold, italic) else {
                    continue;
                };
                let pointer = source.data.as_ptr();
                if seen.contains(&pointer) {
                    continue;
                }
                seen.push(pointer);
                let data: &'static [u8] = source.data;
                database.load_font_source(usvg::fontdb::Source::Binary(Arc::new(data)));
            }
        }
        database.load_system_fonts();
        database.set_serif_family("Tinos");
        database.set_sans_serif_family("Arimo");
        database.set_monospace_family("Cousine");
        let mut options = usvg::Options {
            fontdb: Arc::new(database),
            ..usvg::Options::default()
        };
        "Arimo".clone_into(&mut options.font_family);
        options
    })
}

/// Warms the font database up on a worker so the first page does not wait.
pub(crate) fn prepare() {
    let _ = options();
}

/// Rasterizes `svg` at `scale` device pixels per SVG pixel.
pub(crate) fn rasterize(svg: &str, scale: f32) -> Result<Bitmap, String> {
    let tree = usvg::Tree::from_str(svg, options()).map_err(|error| error.to_string())?;
    let size = tree.size();
    let limit = |side: f32| {
        let pixels = (side * scale).ceil();
        if pixels.is_finite() && pixels >= 1.0 {
            // Bounded above by MAX_SIDE, so the cast cannot truncate.
            (pixels.min(MAX_SIDE as f32)) as u32
        } else {
            1
        }
    };
    let width = limit(size.width());
    let height = limit(size.height());
    let effective = (width as f32 / size.width()).min(height as f32 / size.height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "page bitmap could not be allocated".to_owned())?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(effective, effective),
        &mut pixmap.as_mut(),
    );
    Ok(Bitmap {
        width: width as usize,
        height: height as usize,
        rgba: pixmap.take(),
    })
}

/// An installed face's program and its index in the file, found as the
/// rasterizer finds it.
pub(crate) fn installed_face(family: &str, bold: bool, italic: bool) -> Option<(Vec<u8>, u32)> {
    let database = &options().fontdb;
    let query = usvg::fontdb::Query {
        families: &[usvg::fontdb::Family::Name(family)],
        weight: if bold {
            usvg::fontdb::Weight::BOLD
        } else {
            usvg::fontdb::Weight::NORMAL
        },
        stretch: usvg::fontdb::Stretch::Normal,
        style: if italic {
            usvg::fontdb::Style::Italic
        } else {
            usvg::fontdb::Style::Normal
        },
    };
    let id = database.query(&query)?;
    database.with_face_data(id, |data, index| (data.to_vec(), index))
}
