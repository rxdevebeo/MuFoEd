//! Media parts as PDF image XObjects.
//!
//! Two encodings, chosen by what the bytes already are:
//!
//! - **JPEG** passes through untouched under `DCTDecode`. A JPEG decoder is a
//!   large dependency for no gain: the PDF filter *is* the codec, and a
//!   re-encoded image would differ from the one the document carries. The
//!   colour space comes from the SOF component count (AUD-81); an Adobe APP14
//!   marker with `transform = 2` (YCCK) asks for an inverted `/Decode`.
//! - **PNG** is decoded to raw 8-bit samples plus, when the file has an alpha
//!   channel, an `/SMask`. 16-bit samples keep the high byte, 1/2/4-bit and
//!   palette rows expand to 8-bit grey or RGB (AUD-81). PDF has no PNG filter,
//!   so the samples have to be laid out by hand — which is lossless, so
//!   SC-8's `SSIM = 1.0` holds for it.
//!
//! Anything else (GIF, BMP, TIFF, EMF, WMF, SVG) is a recorded loss with a
//! placeholder drawn in its place. A wrong-but-plausible image is worse than a
//! marked gap.

use std::sync::Arc;

/// How an image part is encoded in the PDF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Encoded {
    /// JPEG bytes, stored as they are.
    Jpeg {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The original bytes.
        data: Arc<Vec<u8>>,
        /// PDF colour space name: `DeviceGray`, `DeviceRGB`, or `DeviceCMYK`.
        color_space: JpegColorSpace,
        /// Whether `/Decode [1 0 1 0 1 0 1 0]` must invert CMYK (Adobe YCCK).
        invert_cmyk: bool,
    },
    /// Raw samples, deflated by the PDF writer.
    Raw {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Interleaved components, 8 bits each.
        samples: Vec<u8>,
        /// Components per pixel (1 for grey, 3 for RGB).
        components: u8,
        /// The alpha channel, when the image had one.
        alpha: Option<Vec<u8>>,
    },
}

/// Colour space a JPEG SOF declares, by component count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegColorSpace {
    /// One component.
    Gray,
    /// Three components.
    Rgb,
    /// Four components.
    Cmyk,
}

impl JpegColorSpace {
    /// The PDF name written on the image XObject.
    #[must_use]
    pub fn pdf_name(self) -> &'static [u8] {
        match self {
            Self::Gray => b"DeviceGray",
            Self::Rgb => b"DeviceRGB",
            Self::Cmyk => b"DeviceCMYK",
        }
    }
}

/// Why a media part could not be embedded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageReject {
    /// A short machine-readable reason.
    pub kind: RejectKind,
    /// The media part, for the report.
    pub part: String,
}

/// The reason a media part was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectKind {
    /// The bytes are not an image format this backend can embed.
    UnsupportedFormat,
    /// The header parsed but the samples could not be read.
    Damaged,
    /// The part is larger than the budget allows.
    TooLarge,
}

impl ImageReject {
    /// A human-readable reason.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self.kind {
            RejectKind::UnsupportedFormat => "image format cannot be embedded in a PDF",
            RejectKind::Damaged => "image data is damaged or truncated",
            RejectKind::TooLarge => "image exceeds the embedding budget",
        }
    }
}

/// Largest image this backend will decode, in bytes.
///
/// A 40 MP photograph is 120 MB of raw samples; the limit keeps a hostile or
/// absurd part from turning a render into an allocation failure. It is the same
/// spirit as `ResourceLimits::max_single_uncompressed` on the reading side.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// Encodes a media part for embedding, with the default uncompressed budget.
///
/// # Errors
///
/// Returns a [`ImageReject`] for a format or a size this backend refuses.
pub fn encode(part: &str, bytes: &[u8]) -> Result<Encoded, ImageReject> {
    encode_with_limit(part, bytes, MAX_IMAGE_BYTES as u64)
}

/// Encodes a media part for embedding.
///
/// `max_uncompressed` is the largest decoded sample buffer this call may
/// allocate — typically [`strict_ooxml_core::limits::ResourceLimits::max_single_uncompressed`]
/// from the render options (AUD-14). A PNG whose IHDR claims more than that is
/// refused before the buffer is allocated.
///
/// # Errors
///
/// Returns a [`ImageReject`] for a format or a size this backend refuses.
pub fn encode_with_limit(
    part: &str,
    bytes: &[u8],
    max_uncompressed: u64,
) -> Result<Encoded, ImageReject> {
    if bytes.is_empty() {
        return Err(reject(RejectKind::Damaged, part));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(reject(RejectKind::TooLarge, part));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg(bytes).ok_or_else(|| reject(RejectKind::Damaged, part));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png(bytes, max_uncompressed).map_err(|kind| reject(kind, part));
    }
    Err(reject(RejectKind::UnsupportedFormat, part))
}

fn reject(kind: RejectKind, part: &str) -> ImageReject {
    ImageReject {
        kind,
        part: part.to_owned(),
    }
}

/// Reads a JPEG's dimensions, component count and Adobe APP14 transform.
///
/// Only the header is parsed; the entropy-coded data is never touched, so a
/// truncated JPEG still embeds (and a reader shows as much of it as it can)
/// rather than being dropped.
fn jpeg(bytes: &[u8]) -> Option<Encoded> {
    let mut index = 2;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut components = 0u8;
    let mut adobe_transform: Option<u8> = None;
    let mut saw_sof = false;
    while index + 3 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        if marker == 0xD9 || marker == 0xDA {
            // EOI / SOS — the frame header is behind us.
            break;
        }
        if index + 4 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]);
        if length < 2 {
            return None;
        }
        let segment_end = index.checked_add(2)?.checked_add(usize::from(length))?;
        if segment_end > bytes.len() {
            return None;
        }
        let data = &bytes[index + 4..segment_end];
        // Adobe APP14: "Adobe\0" + version + flags + ColorTransform.
        if marker == 0xEE && data.starts_with(b"Adobe\0") && data.len() >= 12 {
            adobe_transform = Some(data[11]);
        }
        // SOF0..SOF15, excluding the DHT/JPG/DAC markers that share the range.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if data.len() < 6 {
                return None;
            }
            height = u32::from(u16::from_be_bytes([data[1], data[2]]));
            width = u32::from(u16::from_be_bytes([data[3], data[4]]));
            components = data[5];
            saw_sof = true;
        }
        index = segment_end;
    }
    if !saw_sof || width == 0 || height == 0 {
        return None;
    }
    let color_space = match components {
        1 => JpegColorSpace::Gray,
        3 => JpegColorSpace::Rgb,
        4 => JpegColorSpace::Cmyk,
        _ => return None,
    };
    // AUD-81: Adobe APP14 with ColorTransform = 2 (YCCK) means the CMYK samples
    // are inverted and need `/Decode [1 0 1 0 1 0 1 0]`.
    let invert_cmyk = color_space == JpegColorSpace::Cmyk && adobe_transform == Some(2);
    Some(Encoded::Jpeg {
        width,
        height,
        data: Arc::new(bytes.to_vec()),
        color_space,
        invert_cmyk,
    })
}

/// Reads width, height and a budget channel count from a PNG IHDR.
///
/// Done by hand so a hostile header is refused **before** `png::Decoder` is
/// asked for an output buffer (AUD-14). The signature and the first chunk are
/// the only bytes touched. Channel count is the **post-expand** upper bound
/// (palette → RGB/A, 16-bit → 8-bit), because that is what we store.
fn png_ihdr_budget(bytes: &[u8]) -> Option<(u32, u32, u64)> {
    // signature (8) + length (4) + type (4) + IHDR data (13)
    if bytes.len() < 8 + 4 + 4 + 13 {
        return None;
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let length = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if length != 13 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let color_type = bytes[25];
    // After `normalize_to_color8` every sample is one byte; palette may grow to
    // RGBA when a tRNS chunk is present, so budget 4 channels for type 3.
    let channels: u64 = match color_type {
        0 => 1, // grey (+ expand of 1/2/4-bit)
        4 => 2, // grey+alpha
        2 => 3, // RGB
        3 | 6 => 4, // indexed → RGB/A, or native RGBA
        _ => return None,
    };
    Some((width, height, channels))
}

/// Decodes a PNG into raw 8-bit samples and an optional alpha channel.
///
/// The IHDR size is checked **before** the output buffer is allocated (AUD-14):
/// a 67-byte file that claims 65535×65535 must not spend gigabytes on a `vec!`.
/// 16-bit, sub-byte and palette rows are normalised to 8-bit grey/RGB (AUD-81).
fn png(bytes: &[u8], max_uncompressed: u64) -> Result<Encoded, RejectKind> {
    let (width, height, channels) = png_ihdr_budget(bytes).ok_or(RejectKind::Damaged)?;
    if width == 0 || height == 0 {
        return Err(RejectKind::Damaged);
    }
    let needed = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(channels));
    let Some(needed) = needed else {
        return Err(RejectKind::TooLarge);
    };
    if needed > max_uncompressed {
        return Err(RejectKind::TooLarge);
    }

    // The decoder needs a `Seek`, which a byte slice is not.
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // AUD-81: 16 → high byte, 1/2/4 → 8-bit, palette → RGB (or RGBA with tRNS).
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|_| RejectKind::Damaged)?;
    let buffer_size = reader.output_buffer_size().unwrap_or(0);
    if buffer_size as u64 > max_uncompressed {
        return Err(RejectKind::TooLarge);
    }
    let mut buffer = vec![0; buffer_size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|_| RejectKind::Damaged)?;
    let samples = &buffer[..info.buffer_size()];
    // A PDF image XObject carries colour samples and, separately, a soft mask.
    // The decoded PNG interleaves alpha with the colour channels, so an RGBA
    // image has to be *split* here: leaving the fourth channel in would write
    // four components into a `/DeviceRGB` stream, which reads as a shift for
    // every pixel after the first.
    let (components, colour, alpha) = match info.color_type {
        png::ColorType::Grayscale => (1u8, samples.to_vec(), None),
        png::ColorType::GrayscaleAlpha => (1, take(samples, 2, &[0]), Some(take(samples, 2, &[1]))),
        png::ColorType::Rgb => (3u8, samples.to_vec(), None),
        png::ColorType::Rgba => (
            3,
            take(samples, 4, &[0, 1, 2]),
            Some(take(samples, 4, &[3])),
        ),
        // After `normalize_to_color8` palette rows are expanded; an Indexed
        // result here would mean the transform was ignored.
        png::ColorType::Indexed => return Err(RejectKind::Damaged),
    };
    Ok(Encoded::Raw {
        width: info.width,
        height: info.height,
        samples: colour,
        components,
        alpha,
    })
}

/// Picks the channels `offsets` out of every `stride`-byte pixel.
fn take(samples: &[u8], stride: usize, offsets: &[usize]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() / stride * offsets.len());
    for pixel in samples.chunks_exact(stride) {
        for offset in offsets {
            if let Some(sample) = pixel.get(*offset) {
                out.push(*sample);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{encode, Encoded, JpegColorSpace, RejectKind, MAX_IMAGE_BYTES};

    /// A 2x2 RGB PNG. The fixture is *encoded* rather than hand-assembled: what
    /// this test is about is decoding, and a hand-written zlib stream would test
    /// the fixture instead.
    fn rgb_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            // Top row red, green; bottom row blue, white.
            writer
                .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
                .expect("data");
        }
        out
    }

    /// A 2x2 PNG with an alpha channel, for the soft-mask path.
    fn rgba_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 0, 0, 255, 64])
                .expect("data");
        }
        out
    }

    /// A 2×2 16-bit RGB PNG whose high bytes are a solid colour.
    fn rgb16_png(high: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Sixteen);
            let mut writer = encoder.write_header().expect("header");
            let mut data = Vec::with_capacity(2 * 2 * 3 * 2);
            for _ in 0..4 {
                for channel in high {
                    data.push(channel);
                    data.push(0x33); // low byte discarded by STRIP_16
                }
            }
            writer.write_image_data(&data).expect("data");
        }
        out
    }

    /// A 2×2 palette PNG with one red entry.
    fn indexed_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_palette(vec![200, 40, 50]);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[0, 0, 0, 0]).expect("data");
        }
        out
    }

    /// A 4×1 greyscale PNG at 2 bits per sample (values 0,1,2,3 → expanded).
    fn gray2_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 4, 1);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Two);
            let mut writer = encoder.write_header().expect("header");
            // One byte packs four 2-bit samples: 00 01 10 11 → 0x1B
            writer.write_image_data(&[0x1B]).expect("data");
        }
        out
    }

    #[test]
    fn a_png_becomes_raw_samples() {
        let encoded = encode("/word/media/image1.png", &rgb_png()).expect("encoded");
        match encoded {
            Encoded::Raw {
                width,
                height,
                samples,
                components,
                alpha,
            } => {
                assert_eq!((width, height), (2, 2));
                assert_eq!(components, 3);
                assert!(alpha.is_none());
                assert_eq!(samples.len(), 12, "2x2 RGB");
                // The first pixel is opaque red, the second opaque green.
                assert_eq!(&samples[0..3], &[255, 0, 0]);
                assert_eq!(&samples[3..6], &[0, 255, 0]);
            }
            Encoded::Jpeg { .. } => panic!("a PNG must not become a JPEG passthrough"),
        }
    }

    #[test]
    fn a_png_with_alpha_yields_a_soft_mask() {
        let encoded = encode("/word/media/image5.png", &rgba_png()).expect("encoded");
        match encoded {
            Encoded::Raw {
                samples,
                components,
                alpha,
                ..
            } => {
                assert_eq!(components, 3);
                let alpha = alpha.expect("an alpha channel becomes a soft mask");
                assert_eq!(alpha.len(), 4, "one alpha sample per pixel");
                assert_eq!(&alpha[..4], &[255, 128, 0, 64]);
                // The colour samples are the RGB channels, alpha removed.
                assert_eq!(samples.len(), 12);
                assert_eq!(&samples[0..3], &[255, 0, 0]);
            }
            Encoded::Jpeg { .. } => panic!("a PNG must not become a JPEG passthrough"),
        }
    }

    #[test]
    fn a_16bit_png_keeps_the_high_byte() {
        let encoded = encode("/word/media/h16.png", &rgb16_png([0xAB, 0xCD, 0xEF])).expect("ok");
        match encoded {
            Encoded::Raw {
                samples,
                components,
                ..
            } => {
                assert_eq!(components, 3);
                assert_eq!(&samples[0..3], &[0xAB, 0xCD, 0xEF]);
            }
            Encoded::Jpeg { .. } => panic!("png"),
        }
    }

    #[test]
    fn a_palette_png_expands_to_rgb() {
        let encoded = encode("/word/media/pal.png", &indexed_png()).expect("ok");
        match encoded {
            Encoded::Raw {
                samples,
                components,
                alpha,
                ..
            } => {
                assert_eq!(components, 3);
                assert!(alpha.is_none());
                assert_eq!(&samples[0..3], &[200, 40, 50]);
            }
            Encoded::Jpeg { .. } => panic!("png"),
        }
    }

    #[test]
    fn a_2bit_grey_png_expands_to_eight_bit() {
        let encoded = encode("/word/media/g2.png", &gray2_png()).expect("ok");
        match encoded {
            Encoded::Raw {
                samples,
                components,
                ..
            } => {
                assert_eq!(components, 1);
                assert_eq!(samples.len(), 4);
                // libpng-style expand: value * 255 / (2^bits - 1)
                assert_eq!(samples[0], 0);
                assert_eq!(samples[1], 85);
                assert_eq!(samples[2], 170);
                assert_eq!(samples[3], 255);
            }
            Encoded::Jpeg { .. } => panic!("png"),
        }
    }

    #[test]
    fn a_jpeg_passes_through_byte_for_byte() {
        // A minimal SOF0 header is enough: nothing but the marker is read.
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&64u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&32u16.to_be_bytes()); // width
        jpeg.extend_from_slice(&[0x03, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        match encode("/word/media/image2.jpg", &jpeg).expect("encoded") {
            Encoded::Jpeg {
                width,
                height,
                data,
                color_space,
                invert_cmyk,
            } => {
                assert_eq!((width, height), (32, 64));
                assert_eq!(data.as_slice(), jpeg.as_slice());
                assert_eq!(color_space, JpegColorSpace::Rgb);
                assert!(!invert_cmyk);
            }
            Encoded::Raw { .. } => panic!("expected a JPEG passthrough"),
        }
    }

    #[test]
    fn jpeg_component_count_selects_the_colour_space() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cases = [
            ("jpeg-gray-128.jpg", JpegColorSpace::Gray, false),
            ("jpeg-rgb-red.jpg", JpegColorSpace::Rgb, false),
            ("jpeg-cmyk-adobe.jpg", JpegColorSpace::Cmyk, true),
        ];
        for (name, space, invert) in cases {
            let bytes = std::fs::read(fixtures.join(name)).expect(name);
            match encode(&format!("/word/media/{name}"), &bytes).expect(name) {
                Encoded::Jpeg {
                    color_space,
                    invert_cmyk,
                    width,
                    height,
                    ..
                } => {
                    assert_eq!(color_space, space, "{name}");
                    assert_eq!(invert_cmyk, invert, "{name}");
                    assert_eq!((width, height), (8, 8), "{name}");
                }
                Encoded::Raw { .. } => panic!("{name}: expected JPEG"),
            }
        }
    }

    #[test]
    fn unsupported_and_damaged_input_is_a_recorded_reject() {
        let gif = b"GIF89a\0\0\0\0";
        let reject = encode("/word/media/image3.gif", gif).expect_err("gif");
        assert_eq!(reject.kind, RejectKind::UnsupportedFormat);
        assert!(reject.reason().contains("format"));
        assert!(reject.part.ends_with("image3.gif"));

        let truncated = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert_eq!(
            encode("/word/media/image4.png", &truncated)
                .expect_err("png")
                .kind,
            RejectKind::Damaged
        );
    }

    #[test]
    fn the_budget_is_enforced_before_decoding() {
        let mut huge = vec![0u8; MAX_IMAGE_BYTES + 1];
        huge[0] = 0xFF;
        huge[1] = 0xD8;
        assert_eq!(
            encode("/word/media/huge.jpg", &huge)
                .expect_err("size")
                .kind,
            RejectKind::TooLarge
        );
    }

    #[test]
    fn an_ihdr_that_claims_gigabytes_is_refused_before_allocation() {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &byte in bytes {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&65_535u32.to_be_bytes());
        ihdr.extend_from_slice(&65_535u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut typed = b"IHDR".to_vec();
        typed.extend_from_slice(&ihdr);
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(&typed);
        out.extend_from_slice(&crc32(&typed).to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(b"IEND");
        out.extend_from_slice(&crc32(b"IEND").to_be_bytes());
        let reject = encode("/word/media/bomb.png", &out).expect_err("bomb");
        assert_eq!(reject.kind, RejectKind::TooLarge);
    }
}
