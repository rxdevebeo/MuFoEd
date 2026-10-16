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

// ----- pictures -------------------------------------------------------------
//
// What the editor does to a picture's pixels: it decodes the picture by
// drawing it at its own size (the rasterizer reads PNG, JPEG and GIF), works
// on the pixels and hands back PNG. The document only ever gets new bytes.

/// Base64 for a `data:` URI.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (index, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                let digit = usize::try_from((n >> shift) & 63).unwrap_or(0);
                out.push(char::from(ALPHABET.get(digit).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A picture's pixels at its own size, on transparency.
pub(crate) fn picture_pixmap(bytes: &[u8]) -> Option<resvg::tiny_skia::Pixmap> {
    let (_, mime, _) = strict_ooxml::edit::picture_format(bytes)?;
    let (width, height) = strict_ooxml::edit::picture_pixels(bytes)?;
    let (width, height) = (width.min(MAX_SIDE), height.min(MAX_SIDE));
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"          width=\"{width}\" height=\"{height}\"><image width=\"{width}\" height=\"{height}\"          preserveAspectRatio=\"none\" xlink:href=\"data:{mime};base64,{}\"/></svg>",
        base64(bytes)
    );
    let tree = usvg::Tree::from_str(&svg, options()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

/// A picture as PNG, at its own size: what a vision model takes.
#[cfg(feature = "ocr")]
pub(crate) fn picture_png(bytes: &[u8]) -> Option<Vec<u8>> {
    picture_pixmap(bytes)?.encode_png().ok()
}

/// A picture's pixels for the system clipboard: width, height and
/// premultiplied RGBA.
pub(crate) fn picture_pixels_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let pixmap = picture_pixmap(bytes)?;
    Some((
        pixmap.width() as usize,
        pixmap.height() as usize,
        pixmap.take(),
    ))
}

/// A picture turned a quarter clockwise (or, `clockwise` false, against),
/// as PNG.
pub(crate) fn picture_rotated(bytes: &[u8], clockwise: bool) -> Option<Vec<u8>> {
    let source = picture_pixmap(bytes)?;
    let (width, height) = (source.width(), source.height());
    let mut target = resvg::tiny_skia::Pixmap::new(height, width)?;
    #[allow(clippy::cast_precision_loss)]
    let transform = if clockwise {
        resvg::tiny_skia::Transform::from_row(0.0, 1.0, -1.0, 0.0, height as f32, 0.0)
    } else {
        resvg::tiny_skia::Transform::from_row(0.0, -1.0, 1.0, 0.0, 0.0, width as f32)
    };
    target.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &resvg::tiny_skia::PixmapPaint::default(),
        transform,
        None,
    );
    target.encode_png().ok()
}

/// A picture mirrored left to right (or, `vertical`, top to bottom), as PNG.
pub(crate) fn picture_flipped(bytes: &[u8], vertical: bool) -> Option<Vec<u8>> {
    let source = picture_pixmap(bytes)?;
    let (width, height) = (source.width(), source.height());
    let mut target = resvg::tiny_skia::Pixmap::new(width, height)?;
    #[allow(clippy::cast_precision_loss)]
    let transform = if vertical {
        resvg::tiny_skia::Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, height as f32)
    } else {
        resvg::tiny_skia::Transform::from_row(-1.0, 0.0, 0.0, 1.0, width as f32, 0.0)
    };
    target.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &resvg::tiny_skia::PixmapPaint::default(),
        transform,
        None,
    );
    target.encode_png().ok()
}

/// A picture with its brightness and contrast changed, each in `-1..=1`
/// (0 leaves it), as PNG: each channel goes through
/// `(c - 0.5) * contrast + 0.5 + brightness`, as Word's adjustments do.
pub(crate) fn picture_adjusted(bytes: &[u8], brightness: f32, contrast: f32) -> Option<Vec<u8>> {
    adjusted_pixmap(&picture_pixmap(bytes)?, brightness, contrast)
        .encode_png()
        .ok()
}

/// A picture small enough for a preview: at most `side` pixels each way.
pub(crate) fn picture_preview(bytes: &[u8], side: u32) -> Option<resvg::tiny_skia::Pixmap> {
    let full = picture_pixmap(bytes)?;
    let scale = (side as f32 / full.width().max(full.height()) as f32).min(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (width, height) = (
        ((full.width() as f32 * scale).round() as u32).max(1),
        ((full.height() as f32 * scale).round() as u32).max(1),
    );
    let mut small = resvg::tiny_skia::Pixmap::new(width, height)?;
    small.draw_pixmap(
        0,
        0,
        full.as_ref(),
        &resvg::tiny_skia::PixmapPaint {
            quality: resvg::tiny_skia::FilterQuality::Bicubic,
            ..resvg::tiny_skia::PixmapPaint::default()
        },
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        None,
    );
    Some(small)
}

/// The pixels with brightness and contrast changed (see
/// [`picture_adjusted`]).
pub(crate) fn adjusted_pixmap(
    source: &resvg::tiny_skia::Pixmap,
    brightness: f32,
    contrast: f32,
) -> resvg::tiny_skia::Pixmap {
    let mut pixmap = source.clone();
    let slope = if contrast >= 0.0 {
        1.0 / (1.0 - contrast.min(0.95))
    } else {
        1.0 + contrast.max(-1.0)
    };
    for pixel in pixmap.pixels_mut() {
        let color = pixel.demultiply();
        let alpha = color.alpha();
        let adjust = |channel: u8| -> u8 {
            let value = f32::from(channel) / 255.0;
            let value = (value - 0.5) * slope + 0.5 + brightness;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let out = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            out
        };
        if let Some(adjusted) = resvg::tiny_skia::ColorU8::from_rgba(
            adjust(color.red()),
            adjust(color.green()),
            adjust(color.blue()),
            alpha,
        )
        .premultiply()
        .into()
        {
            *pixel = adjusted;
        }
    }
    pixmap
}

/// A picture cut to the part inside the fractions `left`, `top`, `right`,
/// `bottom` taken off its sides, as PNG.
pub(crate) fn picture_cropped(bytes: &[u8], sides: [f32; 4]) -> Option<Vec<u8>> {
    let source = picture_pixmap(bytes)?;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (source.width() as f32, source.height() as f32);
    let [left, top, right, bottom] = sides.map(|side| side.clamp(0.0, 0.95));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let x = (left * width).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let y = (top * height).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let w = ((1.0 - left - right).max(0.02) * width).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let h = ((1.0 - top - bottom).max(0.02) * height).round() as u32;
    let rect = resvg::tiny_skia::IntRect::from_xywh(
        i32::try_from(x).ok()?,
        i32::try_from(y).ok()?,
        w.max(1),
        h.max(1),
    )?;
    source.clone_rect(rect)?.encode_png().ok()
}

/// The families of the installed fonts, sorted, each once.
pub(crate) fn installed_families() -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut names: Vec<String> = options()
            .fontdb
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .filter(|name| !name.starts_with('@') && !name.is_empty())
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        names
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

#[cfg(all(test, feature = "ocr"))]
mod live {
    /// A drawn formula read back by the local vision model and turned into
    /// an equation. Needs Ollama with `MUFOED_LIVE_MODEL` (default
    /// `qwen3-vl:8b-instruct`); run with `--ignored`.
    #[test]
    #[ignore = "needs a local Ollama vision model"]
    fn a_drawn_formula_comes_back_as_an_equation() {
        use strict_ooxml::TextRecovery;
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"420\" height=\"140\">\
            <rect width=\"420\" height=\"140\" fill=\"white\"/>\
            <text x=\"20\" y=\"80\" font-family=\"Tinos\" font-size=\"36\" font-style=\"italic\">x =</text>\
            <text x=\"110\" y=\"55\" font-family=\"Tinos\" font-size=\"32\" font-style=\"italic\">a + b</text>\
            <line x1=\"105\" y1=\"70\" x2=\"215\" y2=\"70\" stroke=\"black\" stroke-width=\"2\"/>\
            <text x=\"140\" y=\"110\" font-family=\"Tinos\" font-size=\"32\" font-style=\"italic\">c</text>\
            </svg>";
        let tree = resvg::usvg::Tree::from_str(svg, super::options()).expect("svg");
        let mut pixmap = resvg::tiny_skia::Pixmap::new(420, 140).expect("pixmap");
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        let png = pixmap.encode_png().expect("png");
        let mut config = strict_ooxml::OllamaConfig::from_env();
        config.model = std::env::var("MUFOED_LIVE_MODEL")
            .unwrap_or_else(|_| "qwen3-vl:8b-instruct".to_owned());
        let vision = strict_ooxml::OllamaVision::new(config);
        let answer = vision
            .recover_formula(&strict_ooxml::OcrImage::png(png))
            .expect("an answer")
            .expect("a formula");
        eprintln!("{}", answer.text);
        let expression = strict_ooxml::math_from_mathml(&answer.text).expect("MathML");
        let kinds: Vec<_> = expression
            .nodes
            .iter()
            .map(strict_ooxml::model::MathNode::element_name)
            .collect();
        eprintln!("{kinds:?}");
        assert!(kinds.contains(&"m:f"), "{kinds:?}");
    }
}
