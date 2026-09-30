//! Media parts as PDF image XObjects.
//!
//! Two encodings, chosen by what the bytes already are:
//!
//! - **JPEG** passes through untouched under `DCTDecode`. A JPEG decoder is a
//!   large dependency for no gain: the PDF filter *is* the codec, and a
//!   re-encoded image would differ from the one the document carries.
//! - **PNG** is decoded to raw samples plus, when the file has an alpha channel,
//!   an `/SMask`. PDF has no PNG filter, so the samples have to be laid out by
//!   hand — which is lossless, so SC-8's `SSIM = 1.0` holds for it.
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

/// Encodes a media part for embedding.
///
/// # Errors
///
/// Returns a [`ImageReject`] for a format or a size this backend refuses.
pub fn encode(part: &str, bytes: &[u8]) -> Result<Encoded, ImageReject> {
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
        return png(bytes).ok_or_else(|| reject(RejectKind::Damaged, part));
    }
    Err(reject(RejectKind::UnsupportedFormat, part))
}

fn reject(kind: RejectKind, part: &str) -> ImageReject {
    ImageReject {
        kind,
        part: part.to_owned(),
    }
}

/// Reads a JPEG's dimensions from its SOF marker.
///
/// Only the header is parsed; the entropy-coded data is never touched, so a
/// truncated JPEG still embeds (and a reader shows as much of it as it can)
/// rather than being dropped.
fn jpeg(bytes: &[u8]) -> Option<Encoded> {
    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        // SOF0..SOF15, excluding the DHT/JPG/DAC markers that share the range.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]);
            if width == 0 || height == 0 {
                return None;
            }
            return Some(Encoded::Jpeg {
                width: u32::from(width),
                height: u32::from(height),
                data: Arc::new(bytes.to_vec()),
            });
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]);
        if length < 2 {
            return None;
        }
        index += 2 + usize::from(length);
    }
    None
}

/// Decodes a PNG into raw samples and an optional alpha channel.
fn png(bytes: &[u8]) -> Option<Encoded> {
    // The decoder needs a `Seek`, which a byte slice is not.
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer).ok()?;
    let width = info.width;
    let height = info.height;
    if width == 0 || height == 0 {
        return None;
    }
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
        // A palette image would need a colour-space and a 16-bit one a
        // downsample: decisions this backend does not make silently. Naming the
        // variant means a future `ColorType` becomes a compile error here rather
        // than a mis-decode in a document.
        png::ColorType::Indexed => return None,
    };
    Some(Encoded::Raw {
        width,
        height,
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
    use super::{encode, Encoded, RejectKind, MAX_IMAGE_BYTES};

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
            } => {
                assert_eq!((width, height), (32, 64));
                assert_eq!(data.as_slice(), jpeg.as_slice());
            }
            Encoded::Raw { .. } => panic!("expected a JPEG passthrough"),
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
        assert_eq!(
            encode("/word/media/x.bin", b"").expect_err("empty").kind,
            RejectKind::Damaged
        );
    }

    #[test]
    fn the_budget_is_enforced_before_decoding() {
        let huge = vec![0u8; MAX_IMAGE_BYTES + 1];
        assert_eq!(
            encode("/word/media/big.png", &huge)
                .expect_err("budget")
                .kind,
            RejectKind::TooLarge
        );
    }
}
