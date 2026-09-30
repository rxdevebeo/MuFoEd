//! Unit conversion and colour parsing between the layout's pixels and PDF's
//! points.
//!
//! The layout works in CSS pixels at `RenderOptions::scale` (96 px per inch by
//! default) and measures y **downwards from the top** of the page. PDF measures
//! in points (1/72 inch) with y **upwards from the bottom**. Every coordinate
//! crossing the boundary goes through this module, so the flip lives in exactly
//! one place.

/// PDF points per inch.
pub const POINTS_PER_INCH: f64 = 72.0;

/// Converts a pixel distance to points, given the layout's scale.
#[must_use]
pub fn px_to_pt(px: f64, scale: f64) -> f64 {
    if scale <= 0.0 {
        return px * POINTS_PER_INCH / 96.0;
    }
    px * POINTS_PER_INCH / scale
}

/// Converts a pixel *position* on a page to a PDF y coordinate.
///
/// The page is `page_height_pt` tall and the pixel y is measured from its top,
/// so the conversion is a reflection about the middle.
#[must_use]
pub fn px_y_to_pt(y_px: f64, page_height_pt: f64, scale: f64) -> f64 {
    page_height_pt - px_to_pt(y_px, scale)
}

/// Parses `#rrggbb`, `rrggbb` or `#rgb` into PDF's 0..=1 RGB triple.
///
/// An unparseable colour is black rather than an error: a colour that cannot be
/// read must not lose the glyph or the rule it was drawn with, and the layout
/// only ever emits values in these forms.
#[must_use]
pub fn rgb(color: &str) -> (f32, f32, f32) {
    let hex = color.trim().trim_start_matches('#');
    let expand = |value: &str| -> Option<(f32, f32, f32)> {
        let digits: Vec<u8> = value
            .bytes()
            .map(|b| (b as char).to_digit(16).map_or(255, |d| d as u8))
            .collect();
        match digits.len() {
            3 => Some((
                f32::from(digits[0] * 17) / 255.0,
                f32::from(digits[1] * 17) / 255.0,
                f32::from(digits[2] * 17) / 255.0,
            )),
            6 => Some((
                f32::from(digits[0] * 16 + digits[1]) / 255.0,
                f32::from(digits[2] * 16 + digits[3]) / 255.0,
                f32::from(digits[4] * 16 + digits[5]) / 255.0,
            )),
            _ => None,
        }
    };
    expand(hex).unwrap_or((0.0, 0.0, 0.0))
}

/// Returns `true` when a colour is neither `none` nor empty.
#[must_use]
pub fn is_paint(color: &str) -> bool {
    let trimmed = color.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("none")
}

/// Parses an SVG dash array (`"4 3"`, `"4,3"`, `"none"`) into PDF's array form.
#[must_use]
pub fn dash_array(value: &str) -> Vec<f32> {
    if value.trim().eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    value
        .split([' ', ','])
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
        .collect()
}

/// Deflates `data` for a `/FlateDecode` stream, or returns it unchanged when
/// deflating would make it bigger.
///
/// `pdf-writer` records the filter but does not apply it, so the caller must
/// hand it bytes that really are deflated — a stream that claims `FlateDecode`
/// and carries plain bytes is unreadable, and no error says so.
///
/// The choice depends only on the two lengths, so it stays reproducible.
#[must_use]
pub fn flate(data: &[u8]) -> (Vec<u8>, bool) {
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
    if compressed.len() < data.len() {
        (compressed, true)
    } else {
        (data.to_vec(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::{dash_array, is_paint, px_to_pt, px_y_to_pt, rgb};

    #[test]
    fn points_follow_the_scale() {
        assert!((px_to_pt(96.0, 96.0) - 72.0).abs() < 1e-9);
        assert!((px_to_pt(96.0, 144.0) - 48.0).abs() < 1e-9);
        // A nonsensical scale falls back to the CSS default rather than
        // producing an infinite coordinate.
        assert!((px_to_pt(96.0, 0.0) - 72.0).abs() < 1e-9);
    }

    #[test]
    fn y_is_reflected_about_the_page_middle() {
        // A baseline 96 px below the top of a 96 px/inch page is 72 - 72 = 0.
        assert!((px_y_to_pt(96.0, 72.0, 96.0) - 0.0).abs() < 1e-9);
        assert!((px_y_to_pt(0.0, 792.0, 96.0) - 792.0).abs() < 1e-9);
    }

    #[test]
    fn colours_parse_in_both_lengths() {
        assert_eq!(rgb("#ff8000"), (1.0, 128.0 / 255.0, 0.0));
        assert_eq!(rgb("000000"), (0.0, 0.0, 0.0));
        assert_eq!(rgb("#fff"), (1.0, 1.0, 1.0));
        // An unreadable colour is black, not a panic and not a missing glyph.
        assert_eq!(rgb("not-a-colour"), (0.0, 0.0, 0.0));
    }

    #[test]
    fn none_is_not_paint() {
        assert!(!is_paint("none"));
        assert!(!is_paint(""));
        assert!(is_paint("#000000"));
    }

    #[test]
    fn dash_arrays_parse() {
        assert_eq!(dash_array("4 3"), vec![4.0, 3.0]);
        assert_eq!(dash_array("4,3"), vec![4.0, 3.0]);
        assert!(dash_array("none").is_empty());
        // A keyword is not a number and is dropped; the number beside it is
        // kept, because losing the whole array would silently turn a dashed
        // rule into a solid one.
        assert_eq!(dash_array("dash 3"), vec![3.0]);
    }
}
