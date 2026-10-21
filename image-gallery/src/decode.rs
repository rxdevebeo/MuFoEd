//! Reading any supported picture into an [`image::DynamicImage`].
//!
//! Besides the pixels, [`load`] returns what conversion must carry over: the
//! ICC profile and the EXIF chunk. With [`LoadOptions::auto_orient`] the
//! EXIF orientation is applied to the pixels and reset to "upright" in the
//! returned chunk, so a written copy is not turned twice.

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageDecoder};

use crate::exif::{self, ExifSummary, Orientation};
use crate::formats::{sniff, Format};
use crate::Error;

/// How to read a picture.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadOptions {
    /// Apply the EXIF orientation to the pixels.
    pub auto_orient: bool,
    /// Refuse pictures with more pixels than this.
    pub max_pixels: u64,
    /// Scale of vector documents relative to their own size.
    pub vector_scale: f32,
    /// Render vector documents so their longer side is this long instead.
    pub vector_fit: Option<u32>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            auto_orient: true,
            max_pixels: 268_435_456,
            vector_scale: 1.0,
            vector_fit: None,
        }
    }
}

/// A decoded picture and the metadata worth keeping.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// The pixels, upright when [`LoadOptions::auto_orient`] was set.
    pub image: DynamicImage,
    /// The format the file was read as.
    pub format: Format,
    /// The embedded ICC profile.
    pub icc: Option<Vec<u8>>,
    /// The EXIF chunk (raw TIFF structure), orientation reset if applied.
    pub exif: Option<Vec<u8>>,
    /// The camera fields, for the properties panel and name templates.
    pub summary: Option<ExifSummary>,
    /// The orientation the file declared (EXIF value, 1 when none).
    pub orientation: u8,
}

/// Largest file this crate reads into memory.
const MAX_FILE: u64 = 2 << 30;

/// Reads the file at `path`.
pub fn load(path: &Path, options: &LoadOptions) -> Result<Decoded, Error> {
    let size = std::fs::metadata(path)
        .map_err(|error| Error::io(path, error))?
        .len();
    if size > MAX_FILE {
        return Err(Error::Codec("the file is too large".into()));
    }
    let data = std::fs::read(path).map_err(|error| Error::io(path, error))?;
    load_bytes(&data, Format::from_path(path), options)
}

/// Reads a picture held in memory; `hint` is the format the name suggests.
pub fn load_bytes(
    data: &[u8],
    hint: Option<Format>,
    options: &LoadOptions,
) -> Result<Decoded, Error> {
    crate::error::contain(|| decode(data, hint, options))
}

/// The format the content is in, `hint` breaking ties.
pub fn detect(data: &[u8], hint: Option<Format>) -> Option<Format> {
    let head = &data[..data.len().min(4096)];
    // RAW files are TIFF structures; only the name tells them apart.
    if hint == Some(Format::Raw) {
        return hint;
    }
    sniff(head)
        .or_else(|| {
            image::guess_format(head)
                .ok()
                .and_then(Format::from_image_format)
        })
        .or(hint)
}

fn decode(data: &[u8], hint: Option<Format>, options: &LoadOptions) -> Result<Decoded, Error> {
    let format = detect(data, hint).ok_or(Error::UnknownFormat)?;
    let mut decoded = match format {
        Format::Raw => decode_raw(data, options)?,
        Format::Psd => {
            let psd = crate::psd::decode(data, options.max_pixels)?;
            plain(psd.image, format, psd.icc, psd.exif)
        }
        #[cfg(feature = "svg")]
        Format::Svg => {
            let image = crate::svg::render(data, options.vector_scale, options.vector_fit)?;
            plain(DynamicImage::ImageRgba8(image), format, None, None)
        }
        #[cfg(feature = "jxl")]
        Format::Jxl => {
            let decoder = jxl_oxide::integration::JxlDecoder::new(Cursor::new(data))?;
            from_decoder(decoder, format, options)?
        }
        Format::Avif | Format::Heic => return Err(Error::Unsupported(format.name())),
        other => {
            let image_format = other
                .image_format()
                .ok_or(Error::Unsupported(other.name()))?;
            let mut reader = image::ImageReader::with_format(Cursor::new(data), image_format);
            reader.limits(limits(options));
            from_decoder(reader.into_decoder()?, format, options)?
        }
    };
    orient(&mut decoded, options);
    Ok(decoded)
}

fn limits(options: &LoadOptions) -> image::Limits {
    let mut limits = image::Limits::default();
    // Room for the decoded picture at 16 bytes per pixel and the codec's own
    // buffers.
    limits.max_alloc = Some(options.max_pixels.saturating_mul(16));
    limits
}

fn plain(
    image: DynamicImage,
    format: Format,
    icc: Option<Vec<u8>>,
    exif: Option<Vec<u8>>,
) -> Decoded {
    let orientation = exif.as_deref().and_then(exif::orientation).unwrap_or(1);
    let summary = exif.as_deref().and_then(ExifSummary::read);
    Decoded {
        image,
        format,
        icc,
        exif,
        summary,
        orientation,
    }
}

fn from_decoder(
    mut decoder: impl ImageDecoder,
    format: Format,
    options: &LoadOptions,
) -> Result<Decoded, Error> {
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > options.max_pixels {
        return Err(Error::TooLarge { width, height });
    }
    let icc = decoder.icc_profile().ok().flatten();
    let exif = decoder.exif_metadata().ok().flatten();
    let image = DynamicImage::from_decoder(decoder)?;
    Ok(plain(image, format, icc, exif))
}

fn decode_raw(data: &[u8], options: &LoadOptions) -> Result<Decoded, Error> {
    let preview = crate::raw::preview(data)?;
    let mut reader =
        image::ImageReader::with_format(Cursor::new(preview.jpeg), image::ImageFormat::Jpeg);
    reader.limits(limits(options));
    let mut decoded = from_decoder(reader.into_decoder()?, Format::Raw, options)?;
    // The RAW container describes the shot better than its preview does.
    if let Some(summary) = ExifSummary::read(data) {
        decoded.summary = Some(summary);
    }
    if decoded.exif.is_none() {
        if let Some(value) = preview.orientation.filter(|value| (1..=8).contains(value)) {
            decoded.orientation = value;
            decoded.exif = Some(exif::orientation_chunk(value));
        }
    }
    Ok(decoded)
}

/// Applies the declared orientation to the pixels and resets the tag.
fn orient(decoded: &mut Decoded, options: &LoadOptions) {
    if !options.auto_orient || decoded.orientation == 1 {
        return;
    }
    if let Some(orientation) = Orientation::from_exif(decoded.orientation) {
        decoded.image.apply_orientation(orientation.to_image());
        if let Some(chunk) = decoded.exif.as_mut() {
            exif::set_orientation(chunk, 1);
        }
    }
}

/// What can be learnt about a picture without decoding its pixels (where
/// the format allows).
#[derive(Clone, Debug, Default)]
pub struct Probe {
    /// The format the content is in.
    pub format: Option<Format>,
    /// Width and height as shown, the EXIF orientation applied.
    pub size: Option<(u32, u32)>,
    /// The camera fields.
    pub summary: Option<ExifSummary>,
}

/// Probes the file at `path`.
pub fn probe(path: &Path) -> Result<Probe, Error> {
    let data = std::fs::read(path).map_err(|error| Error::io(path, error))?;
    probe_bytes(&data, Format::from_path(path))
}

/// Probes a picture held in memory.
pub fn probe_bytes(data: &[u8], hint: Option<Format>) -> Result<Probe, Error> {
    crate::error::contain(|| {
        let format = detect(data, hint).ok_or(Error::UnknownFormat)?;
        let headers =
            |bytes: &[u8], image_format| -> Result<((u32, u32), Option<Vec<u8>>), Error> {
                let reader = image::ImageReader::with_format(Cursor::new(bytes), image_format);
                let mut decoder = reader.into_decoder()?;
                let size = decoder.dimensions();
                Ok((size, decoder.exif_metadata().ok().flatten()))
            };
        let (size, exif, summary) = match format {
            Format::Raw => {
                let preview = crate::raw::preview(data)?;
                let (size, exif) = headers(preview.jpeg, image::ImageFormat::Jpeg)?;
                let orientation = exif
                    .as_deref()
                    .and_then(exif::orientation)
                    .or(preview.orientation);
                let exif = exif.or_else(|| orientation.map(exif::orientation_chunk));
                (size, exif, ExifSummary::read(data))
            }
            other => {
                if let Some(image_format) = other.image_format().filter(|_| other != Format::Avif) {
                    let (size, exif) = headers(data, image_format)?;
                    let summary = exif.as_deref().and_then(ExifSummary::read);
                    (size, exif, summary)
                } else {
                    let decoded = decode(data, hint, &LoadOptions::default())?;
                    return Ok(Probe {
                        format: Some(format),
                        size: Some((decoded.image.width(), decoded.image.height())),
                        summary: decoded.summary,
                    });
                }
            }
        };
        let turned = exif
            .as_deref()
            .and_then(exif::orientation)
            .is_some_and(|value| value >= 5);
        Ok(Probe {
            format: Some(format),
            size: Some(if turned { (size.1, size.0) } else { size }),
            summary,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba, RgbaImage};

    fn png_with_exif(orientation: u8) -> Vec<u8> {
        let mut picture = RgbaImage::new(3, 2);
        picture.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let mut out = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(Cursor::new(&mut out));
        image::ImageEncoder::set_exif_metadata(&mut encoder, exif::orientation_chunk(orientation))
            .expect("PNG takes EXIF");
        DynamicImage::ImageRgba8(picture)
            .write_with_encoder(encoder)
            .expect("encodes");
        out
    }

    #[test]
    fn applies_and_resets_the_orientation() {
        let data = png_with_exif(6);
        let decoded = load_bytes(&data, None, &LoadOptions::default()).expect("decodes");
        assert_eq!(decoded.format, Format::Png);
        assert_eq!(decoded.orientation, 6);
        assert_eq!(decoded.image.dimensions(), (2, 3));
        // Turned right: the top-left red pixel is now top-right.
        assert_eq!(decoded.image.get_pixel(1, 0).0, [255, 0, 0, 255]);
        let chunk = decoded.exif.expect("kept");
        assert_eq!(exif::orientation(&chunk), Some(1));

        let raw = LoadOptions {
            auto_orient: false,
            ..LoadOptions::default()
        };
        let decoded = load_bytes(&data, None, &raw).expect("decodes");
        assert_eq!(decoded.image.dimensions(), (3, 2));
        assert_eq!(decoded.exif.as_deref().and_then(exif::orientation), Some(6));
    }

    #[test]
    fn content_wins_over_the_extension() {
        let data = png_with_exif(1);
        let decoded =
            load_bytes(&data, Some(Format::Jpeg), &LoadOptions::default()).expect("decodes");
        assert_eq!(decoded.format, Format::Png);
        assert!(matches!(
            load_bytes(b"not a picture", None, &LoadOptions::default()),
            Err(Error::UnknownFormat)
        ));
        assert!(matches!(
            load_bytes(b"\0\0\0\x18ftypheic\0\0\0\0", None, &LoadOptions::default()),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn refuses_oversized_pictures() {
        let data = png_with_exif(1);
        let tight = LoadOptions {
            max_pixels: 5,
            ..LoadOptions::default()
        };
        assert!(matches!(
            load_bytes(&data, None, &tight),
            Err(Error::TooLarge { .. })
        ));
    }

    #[test]
    fn reads_psd_through_the_same_entry() {
        let planes = vec![vec![10], vec![20], vec![30]];
        let data = crate::psd::build(3, 8, 1, 1, &planes, 0);
        let decoded = load_bytes(&data, None, &LoadOptions::default()).expect("decodes");
        assert_eq!(decoded.format, Format::Psd);
        assert_eq!(
            decoded.image.to_rgba8().get_pixel(0, 0).0,
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn damaged_files_fail_without_panicking() {
        let picture = DynamicImage::ImageRgba8(RgbaImage::from_pixel(9, 7, Rgba([1, 2, 3, 4])));
        let mut samples = vec![crate::psd::build(
            3,
            8,
            2,
            2,
            &[vec![1; 4], vec![2; 4], vec![3; 4]],
            0,
        )];
        for format in [
            Format::Png,
            Format::Jpeg,
            Format::Gif,
            Format::WebP,
            Format::Tiff,
            Format::Bmp,
            Format::Qoi,
        ] {
            let options = crate::encode::EncodeOptions::default();
            samples.push(
                crate::encode::encode(&picture, format, &options, None, None).expect("encodes"),
            );
        }
        // Damaged headers may claim huge sizes; keep the test's memory small.
        let small = LoadOptions {
            max_pixels: 1 << 20,
            ..LoadOptions::default()
        };
        for sample in &samples {
            for cut in (0..sample.len()).step_by(7) {
                let _ = load_bytes(&sample[..cut], None, &small);
                let _ = probe_bytes(&sample[..cut], None);
            }
            let mut flipped = sample.clone();
            for index in (16..flipped.len()).step_by(11) {
                flipped[index] ^= 0x5A;
            }
            let _ = load_bytes(&flipped, None, &small);
            assert!(load_bytes(sample, None, &LoadOptions::default()).is_ok());
        }
    }
}
