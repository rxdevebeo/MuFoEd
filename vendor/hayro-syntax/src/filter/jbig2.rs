use crate::bit_reader::BitWriter;
use crate::object::Dict;
use crate::object::Stream;
use crate::object::dict::keys::JBIG2_GLOBALS;
use crate::object::stream::{FilterResult, ImageColorSpace, ImageData, ImageDecodeParams};
use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;
use core::iter;

/// Decode JBIG2 data from a PDF stream.
///
/// The `params` dictionary may contain a `JBIG2Globals` entry pointing to
/// a stream with shared symbol dictionaries.
pub(crate) fn decode(
    data: &[u8],
    params: &Dict<'_>,
    image_params: &ImageDecodeParams,
) -> Option<FilterResult<'static>> {
    let globals = params
        .get::<Stream<'_>>(JBIG2_GLOBALS)
        .and_then(|g| g.decoded().ok());

    let image = hayro_jbig2::Image::new_embedded(data, globals.as_deref()).ok()?;

    // PrintCraft / MuFoEd patch: LaurenzV/hayro#1259 — a crafted JBIG2 header can
    // claim absurd width×height so `vec![0; row_bytes * height]` (or the luma8
    // decode buffer) OOMs the process before ImageXObject / draw_image guards run.
    // Same ceiling as hayro::MAX_IMAGE_PIXELS / ImageXObject::new.
    if !pixel_budget_ok(image.width(), image.height()) {
        return None;
    }

    // Whenever possible (if we don't have an indexed color space), we convert
    // the data as 8-bit instead of 1-bit, so that it can be easier converted
    // into an RGBA8 image.

    // We need to invert the color because JBIG2 uses black = 1 and
    // white = 0, but PDF uses the opposite.
    let (decoded, bpc) = if image_params.is_indexed {
        let row_bytes = (image.width() as usize).div_ceil(8);
        let mut packed = vec![0_u8; row_bytes * image.height() as usize];

        struct BitWriterDecoder<'a> {
            writer: BitWriter<'a>,
        }

        impl hayro_jbig2::Decoder for BitWriterDecoder<'_> {
            fn push_pixel(&mut self, black: bool) {
                let _ = self.writer.write(u32::from(!black));
            }

            fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
                let byte_value = if black { 0x00 } else { 0xFF };
                let _ = self.writer.fill_bytes(byte_value, chunk_count as usize);
            }

            fn next_line(&mut self) {
                // Images need to be padded to the byte boundary after each row.
                self.writer.align();
            }
        }

        let writer = BitWriter::new(&mut packed, 1)?;
        let mut decoder = BitWriterDecoder { writer };
        image.decode(&mut decoder).ok()?;

        (packed, 1)
    } else {
        struct Luma8Decoder {
            output: Vec<u8>,
        }

        impl hayro_jbig2::Decoder for Luma8Decoder {
            fn push_pixel(&mut self, black: bool) {
                self.output.push(if black { 0x00 } else { 0xFF });
            }

            fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
                let byte = if black { 0x00 } else { 0xFF };
                self.output
                    .extend(iter::repeat_n(byte, chunk_count as usize * 8));
            }

            fn next_line(&mut self) {}
        }

        let mut decoder = Luma8Decoder { output: Vec::new() };
        image.decode(&mut decoder).ok()?;

        (decoder.output, 8)
    };

    Some(FilterResult {
        data: Cow::Owned(decoded),
        image_data: Some(ImageData {
            alpha: None,
            color_space: Some(ImageColorSpace::Gray),
            bits_per_component: bpc,
            width: image.width(),
            height: image.height(),
        }),
    })
}

/// Same ceiling as `hayro::MAX_IMAGE_PIXELS` (2^28 ≈ 268 MP).
fn pixel_budget_ok(width: u32, height: u32) -> bool {
    const MAX_IMAGE_PIXELS: u64 = 1 << 28;
    u64::from(width).saturating_mul(u64::from(height)) <= MAX_IMAGE_PIXELS
}

#[cfg(test)]
mod tests {
    use super::pixel_budget_ok;

    #[test]
    fn pixel_budget_accepts_realistic_page_images() {
        assert!(pixel_budget_ok(4000, 6000));
        assert!(pixel_budget_ok(1 << 14, 1 << 14)); // 268_435_456 exactly
    }

    #[test]
    fn pixel_budget_rejects_hayro_1259_scale_claims() {
        // Upstream issue: allocation of ~66 GB from a crafted JBIG2 header.
        assert!(!pixel_budget_ok(u32::MAX, 2));
        assert!(!pixel_budget_ok(1 << 15, 1 << 15));
    }
}
