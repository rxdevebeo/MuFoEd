//! A page as one grayscale buffer in `[0, 1]`.

use std::path::Path;

/// Rec. 601 luma coefficients, the ones the SVG gate used before this was a
/// crate.
///
/// They are written out rather than pulled from a library because a *fidelity*
/// gate's number is only meaningful against the number it was first measured
/// with: changing the luminance weights moves every SSIM score in the project,
/// and `coverage/render-gates.toml` is calibrated to these.
const LUMA_RED: f64 = 0.2126;
const LUMA_GREEN: f64 = 0.7152;
const LUMA_BLUE: f64 = 0.0722;

/// A pixel at or below this gray level counts as "ink" (text).
///
/// Ink is a gray *level*, not an alpha: a page is a white sheet with marks on
/// it, so a light gray is a mark rendered at low coverage and a transparent
/// pixel is paper. Compositing on white is what makes the two agree.
pub const INK_LEVEL: f64 = 0.5;

/// A grayscale page, row-major, one `f64` per pixel in `[0, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    width: usize,
    height: usize,
    pixels: Vec<f64>,
}

impl Gray {
    /// Wraps a buffer, which must be `width * height` long.
    ///
    /// # Panics
    ///
    /// Panics if the buffer length does not match the dimensions, or if either
    /// is zero. Every measure here divides by width or height and an empty page
    /// has no ink to compare, so a zero-sized image is a caller error, not an
    /// input to be reported on.
    #[must_use]
    pub fn new(width: usize, height: usize, pixels: Vec<f64>) -> Self {
        assert!(width > 0 && height > 0, "a page has pixels");
        assert_eq!(pixels.len(), width * height, "buffer size mismatch");
        Self {
            width,
            height,
            pixels,
        }
    }

    /// A page of one uniform level, in `[0, 1]`.
    ///
    /// A blank page and a black page are the two "no ink" candidates a gate has
    /// to be able to reject, and both are made here rather than spelled out at
    /// each call site.
    #[must_use]
    pub fn filled(width: usize, height: usize, level: f64) -> Self {
        Self::new(width, height, vec![level; width * height])
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// The pixels, row-major.
    #[must_use]
    pub fn pixels(&self) -> &[f64] {
        &self.pixels
    }

    /// The pixel at `(column, row)`.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are outside the page.
    #[must_use]
    pub fn at(&self, column: usize, row: usize) -> f64 {
        self.pixels[row * self.width + column]
    }

    /// This page with every row moved down by `dy` rows, white filling the top.
    ///
    /// # Panics
    ///
    /// Panics on a shift that is not an `isize`.
    #[must_use]
    pub fn shifted_down(&self, dy: isize) -> Self {
        let mut out = vec![1.0; self.pixels.len()];
        for row in 0..self.height {
            let source = row as isize - dy;
            if source >= 0 && (source as usize) < self.height {
                out[row * self.width..(row + 1) * self.width].copy_from_slice(
                    &self.pixels[source as usize * self.width..(source as usize + 1) * self.width],
                );
            } else {
                // A shift that moves content off the page leaves paper there,
                // which is what the exposed edge of a shifted page looks like.
                out[row * self.width..(row + 1) * self.width].fill(1.0);
            }
        }
        Self::new(self.width, self.height, out)
    }

    /// This page with every column moved right by `dx` columns, white filling
    /// the left.
    ///
    /// # Panics
    ///
    /// Panics on a shift that is not an `isize`.
    #[must_use]
    pub fn shifted_right(&self, dx: isize) -> Self {
        let mut out = vec![1.0; self.pixels.len()];
        for row in 0..self.height {
            for column in 0..self.width {
                let source = column as isize - dx;
                if source >= 0 && (source as usize) < self.width {
                    out[row * self.width + column] =
                        self.pixels[row * self.width + source as usize];
                }
            }
        }
        Self::new(self.width, self.height, out)
    }

    /// This page stretched about its middle row by `by` rows: the top half moves
    /// up, the bottom half down.
    ///
    /// This is the deformation the extent check exists for: the two halves
    /// cancel in the ink centroid while both ink edges move, so a page a block
    /// too tall passes the centroid and fails the extent.
    #[must_use]
    pub fn stretched_vertically(&self, by: isize) -> Self {
        let middle = (self.height / 2) as isize;
        let mut out = vec![1.0; self.pixels.len()];
        for row in 0..self.height {
            // Above the middle the content comes from *below* the row, so it
            // moves up; below the middle it comes from above, so it moves down.
            let source = if (row as isize) < middle {
                row as isize + by
            } else {
                row as isize - by
            };
            if source >= 0 && (source as usize) < self.height {
                out[row * self.width..(row + 1) * self.width].copy_from_slice(
                    &self.pixels[source as usize * self.width..(source as usize + 1) * self.width],
                );
            } else {
                out[row * self.width..(row + 1) * self.width].fill(1.0);
            }
        }
        Self::new(self.width, self.height, out)
    }
}

/// Rec. 601 luma of one 8-bit RGB triple, in `[0, 1]`.
#[must_use]
pub fn luma8(red: u8, green: u8, blue: u8) -> f64 {
    (LUMA_RED * f64::from(red) + LUMA_GREEN * f64::from(green) + LUMA_BLUE * f64::from(blue))
        / 255.0
}

/// Converts an RGBA buffer to a page, compositing on white.
///
/// Alpha is composited rather than kept, and the reason is the same as in the
/// rasterizers: half of every image pipeline reads an absent channel as black,
/// so a page whose paper is transparent comes back a black rectangle. A page
/// has no transparency worth preserving.
///
/// # Panics
///
/// Panics if the buffer is not `width * height * 4` bytes.
#[must_use]
pub fn from_rgba8(rgba: &[u8], width: usize, height: usize) -> Gray {
    assert_eq!(rgba.len(), width * height * 4, "RGBA buffer size mismatch");
    let pixels = (0..width * height)
        .map(|index| {
            let alpha = u16::from(rgba[index * 4 + 3]);
            let composite = |channel: u8| (u16::from(channel) + (255 - alpha)).min(255) as u8;
            luma8(
                composite(rgba[index * 4]),
                composite(rgba[index * 4 + 1]),
                composite(rgba[index * 4 + 2]),
            )
        })
        .collect();
    Gray::new(width, height, pixels)
}

/// Decodes an 8-bit PNG file into a page.
///
/// Palette, gray-scale and 16-bit files are expanded to 8-bit RGB(A) by the
/// decoder, so a reference committed by a different tool is still read as the
/// luminance it represents. What is refused is a file that is not an 8-bit PNG
/// at all: a reference the gate cannot decode is a gate that cannot run, and
/// the error has to name the file.
///
/// # Errors
///
/// Returns the reason the file could not be read, decoded, or interpreted.
pub fn load_png_gray(path: impl AsRef<Path>) -> Result<Gray, String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    decode_png_gray(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// Decodes an 8-bit PNG from memory.
///
/// This is how a gate reads the *candidate* side as well as the reference: the
/// rasterizer hands back PNG bytes, and running them through this decoder is
/// what makes "the same pixels" a fact rather than an assumption.
///
/// # Errors
///
/// Returns the reason the bytes could not be decoded, or a description of the
/// image if it is not 8-bit PNG.
pub fn decode_png_gray(bytes: &[u8]) -> Result<Gray, String> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes));
    reader.set_transformations(png::Transformations::EXPAND);
    let mut reader = reader.read_info().map_err(|error| error.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "the PNG has no decodable size".to_owned())?;
    let mut buffer = vec![0u8; size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "a reference must be 8-bit, this one is {:?}",
            info.bit_depth
        ));
    }
    let (width, height) = (info.width, info.height);
    let count = (width * height) as usize;
    let pixels = (0..count)
        .map(|index| {
            let (red, green, blue) = match info.color_type {
                png::ColorType::Rgb => (
                    buffer[index * 3],
                    buffer[index * 3 + 1],
                    buffer[index * 3 + 2],
                ),
                png::ColorType::Rgba => (
                    buffer[index * 4],
                    buffer[index * 4 + 1],
                    buffer[index * 4 + 2],
                ),
                png::ColorType::Grayscale => {
                    let value = buffer[index];
                    (value, value, value)
                }
                png::ColorType::GrayscaleAlpha => {
                    let value = buffer[index * 2];
                    (value, value, value)
                }
                png::ColorType::Indexed => {
                    return Err("the palette expansion left an indexed PNG behind".to_owned())
                }
            };
            Ok(luma8(red, green, blue))
        })
        .collect::<Result<Vec<f64>, String>>()?;
    Ok(Gray::new(width as usize, height as usize, pixels))
}

/// The top-left `width × height` window of a page, for a candidate that came
/// back a pixel wider than the reference it is compared with.
///
/// A rasterizer that rounds *up* gives a page one pixel larger than the grid it
/// was asked for; cropping from the bottom and right throws away the outermost
/// strip of paper, which is inside the extent tolerance anyway. Cropping is
/// refused rather than padded when the candidate is genuinely smaller, because
/// silently scaling a short candidate up would compare two different pages.
#[must_use]
pub fn crop(page: &Gray, width: usize, height: usize) -> Option<Gray> {
    if page.width() < width || page.height() < height {
        return None;
    }
    let pixels = (0..height)
        .flat_map(|row| (0..width).map(move |column| page.at(column, row)))
        .collect();
    Some(Gray::new(width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::{crop, decode_png_gray, from_rgba8, load_png_gray, luma8, Gray, INK_LEVEL};

    #[test]
    fn luma_is_rec_601() {
        assert!((luma8(255, 255, 255) - 1.0).abs() < 1e-12);
        assert!(luma8(0, 0, 0).abs() < 1e-12);
        assert!((luma8(255, 0, 0) - 0.2126).abs() < 1e-12);
        // Green carries most of the luminance, which is why a page's text
        // weight is decided by it.
        assert!(luma8(0, 255, 0) > luma8(255, 0, 0));
    }

    #[test]
    fn transparent_pixels_become_paper() {
        // A fully transparent white pixel is paper, not black: compositing on
        // white is what stops a transparent page reading as a black rectangle.
        let rgba = [255, 255, 255, 0];
        assert!((from_rgba8(&rgba, 1, 1).at(0, 0) - 1.0).abs() < 1e-12);
        // Half-transparent black over white lands halfway between the two, which
        // is exactly what a glyph edge looks like and why ink is a gray *level*
        // rather than an alpha.
        let half = [0, 0, 0, 128];
        let level = from_rgba8(&half, 1, 1).at(0, 0);
        assert!(
            (level - f64::from(127) / 255.0).abs() < 1e-9,
            "half-black over white is half-gray, got {level}"
        );
        assert!(
            (level - INK_LEVEL).abs() < 0.01,
            "and it is neither paper nor ink: it is a mark at half coverage"
        );
    }

    #[test]
    fn a_shifted_page_leaves_paper_where_content_left() {
        let mut pixels = vec![1.0; 4 * 4];
        pixels[3 * 4] = 0.0;
        let page = Gray::new(4, 4, pixels);
        let moved = page.shifted_down(1);
        assert!((moved.at(0, 0) - 1.0).abs() < 1e-12, "the top row is paper");
        assert!(moved.at(0, 3) > INK_LEVEL, "the ink fell off the page");
    }

    #[test]
    fn a_stretched_page_keeps_its_ink() {
        // Two rows of ink either side of the middle row, so the stretch moves
        // the top one up and the bottom one down and both land inside the page.
        let mut pixels = vec![1.0; 4 * 12];
        pixels[2 * 4] = 0.0;
        pixels[9 * 4] = 0.0;
        let page = Gray::new(4, 12, pixels);
        let stretched = page.stretched_vertically(2);
        // Rows 2 and 9 are seven apart; after a 2 px stretch they are eleven
        // apart, which is what "a block that grew a line" looks like.
        assert!(stretched.at(0, 0) < INK_LEVEL, "the top ink moved up");
        assert!(stretched.at(0, 11) < INK_LEVEL, "the bottom ink moved down");
        assert!(
            (stretched.at(0, 2) - 1.0).abs() < 1e-12,
            "the row the top ink came from is now paper"
        );
    }

    #[test]
    fn a_missing_file_says_which_one() {
        let error = load_png_gray("no/such/reference.png").expect_err("must fail");
        assert!(error.contains("reference.png"), "{error}");
    }

    #[test]
    fn a_crop_drops_the_outer_strip_and_refuses_to_grow() {
        let page = Gray::filled(6, 8, 1.0);
        let cropped = crop(&page, 4, 4).expect("a smaller window exists");
        assert_eq!((cropped.width(), cropped.height()), (4, 4));
        assert!(
            crop(&page, 8, 8).is_none(),
            "a short candidate is not padded"
        );
    }

    #[test]
    fn a_png_round_trips_through_the_decoder() {
        // The candidate side of both gates arrives as PNG bytes, so the decoder
        // has to read what a rasterizer wrote, not only what a producer committed.
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer
                .write_image_data(&[0, 0, 0, 255, 255, 255])
                .expect("pixels");
        }
        let page = decode_png_gray(&bytes).expect("decodes");
        assert!((page.at(0, 0) - 0.0).abs() < 1e-12, "black is black");
        assert!((page.at(1, 0) - 1.0).abs() < 1e-12, "white is white");
    }
}
