//! Writing pictures: format choice, per-format options, metadata, and an
//! atomic replace of the target file.

use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::codecs;
use image::{DynamicImage, ImageEncoder};
use serde::{Deserialize, Serialize};

use crate::{Error, Format};

/// The target format of a recipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// The source's own format where it can be written; otherwise PNG
    /// (JPEG for camera RAW previews).
    Keep,
    /// This format.
    To(Format),
}

impl OutputFormat {
    /// The format a file of `source` format is written in.
    pub fn resolve(self, source: Format) -> Format {
        match self {
            Self::To(format) if format.can_write() => format,
            Self::Keep if source.can_write() => source,
            Self::Keep if source == Format::Raw => Format::Jpeg,
            Self::To(_) | Self::Keep => Format::Png,
        }
    }
}

/// PNG compression effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PngLevel {
    /// Fast, larger files.
    Fast,
    /// Balanced.
    Default,
    /// Smallest files, slow.
    Best,
}

/// Per-format options and the metadata policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EncodeOptions {
    /// JPEG quality, 1 to 100.
    pub jpeg_quality: u8,
    /// AVIF quality, 1 to 100.
    pub avif_quality: u8,
    /// AVIF encoder speed, 1 (slow, small) to 10 (fast).
    pub avif_speed: u8,
    /// PNG compression effort.
    pub png_level: PngLevel,
    /// Write the EXIF chunk where the format carries one.
    pub keep_exif: bool,
    /// Write the ICC profile where the format carries one.
    pub keep_icc: bool,
    /// Background of transparent pixels for formats without alpha, RGB.
    pub background: [u8; 3],
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 90,
            avif_quality: 80,
            avif_speed: 6,
            png_level: PngLevel::Default,
            keep_exif: true,
            keep_icc: true,
            background: [255, 255, 255],
        }
    }
}

/// Largest side an ICO image may have.
const ICO_SIDE: u32 = 256;

/// Encodes `image` as `format`, embedding `icc` and `exif` as the options
/// allow and the format supports.
pub fn encode(
    image: &DynamicImage,
    format: Format,
    options: &EncodeOptions,
    icc: Option<&[u8]>,
    exif: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    if !format.can_write() {
        return Err(Error::Invalid(format!(
            "{} cannot be written",
            format.name()
        )));
    }
    let prepared = prepare(image, format, options);
    let mut out = Cursor::new(Vec::new());
    let icc = icc.filter(|_| options.keep_icc);
    let exif = exif.filter(|_| options.keep_exif);
    let metadata = Metadata { icc, exif };
    crate::error::contain(|| match format {
        Format::Jpeg => write_with(
            &prepared,
            &metadata,
            codecs::jpeg::JpegEncoder::new_with_quality(
                &mut out,
                options.jpeg_quality.clamp(1, 100),
            ),
        ),
        Format::Png => {
            let level = match options.png_level {
                PngLevel::Fast => codecs::png::CompressionType::Fast,
                PngLevel::Default => codecs::png::CompressionType::Default,
                PngLevel::Best => codecs::png::CompressionType::Best,
            };
            write_with(
                &prepared,
                &metadata,
                codecs::png::PngEncoder::new_with_quality(
                    &mut out,
                    level,
                    codecs::png::FilterType::Adaptive,
                ),
            )
        }
        Format::WebP => write_with(
            &prepared,
            &metadata,
            codecs::webp::WebPEncoder::new_lossless(&mut out),
        ),
        Format::Gif => write_with(&prepared, &metadata, codecs::gif::GifEncoder::new(&mut out)),
        Format::Bmp => write_with(&prepared, &metadata, codecs::bmp::BmpEncoder::new(&mut out)),
        Format::Ico => write_with(&prepared, &metadata, codecs::ico::IcoEncoder::new(&mut out)),
        Format::Tiff => write_with(
            &prepared,
            &metadata,
            codecs::tiff::TiffEncoder::new(&mut out),
        ),
        Format::Tga => write_with(&prepared, &metadata, codecs::tga::TgaEncoder::new(&mut out)),
        Format::Pnm => write_with(&prepared, &metadata, codecs::pnm::PnmEncoder::new(&mut out)),
        Format::Qoi => write_with(&prepared, &metadata, codecs::qoi::QoiEncoder::new(&mut out)),
        Format::Farbfeld => write_with(
            &prepared,
            &metadata,
            codecs::farbfeld::FarbfeldEncoder::new(&mut out),
        ),
        Format::Exr => write_with(
            &prepared,
            &metadata,
            codecs::openexr::OpenExrEncoder::new(&mut out),
        ),
        Format::Hdr => write_with(&prepared, &metadata, codecs::hdr::HdrEncoder::new(&mut out)),
        #[cfg(feature = "avif")]
        Format::Avif => write_with(
            &prepared,
            &metadata,
            codecs::avif::AvifEncoder::new_with_speed_quality(
                &mut out,
                options.avif_speed.clamp(1, 10),
                options.avif_quality.clamp(1, 100),
            ),
        ),
        other => Err(Error::Invalid(format!(
            "{} cannot be written",
            other.name()
        ))),
    })?;
    Ok(out.into_inner())
}

/// The metadata to embed.
struct Metadata<'a> {
    icc: Option<&'a [u8]>,
    exif: Option<&'a [u8]>,
}

/// Embeds the metadata the encoder supports (the others decline, which is
/// not an error) and encodes.
fn write_with(
    image: &DynamicImage,
    metadata: &Metadata<'_>,
    mut encoder: impl ImageEncoder,
) -> Result<(), Error> {
    if let Some(icc) = metadata.icc {
        let _ = encoder.set_icc_profile(icc.to_vec());
    }
    if let Some(exif) = metadata.exif {
        let _ = encoder.set_exif_metadata(exif.to_vec());
    }
    Ok(image.write_with_encoder(encoder)?)
}

/// Converts a picture to a colour type the format's encoder accepts.
fn prepare(image: &DynamicImage, format: Format, options: &EncodeOptions) -> DynamicImage {
    use image::ColorType as C;
    let mut image = if format.has_alpha() || !image.color().has_alpha() {
        image.clone()
    } else {
        crate::ops::Operation::Flatten(options.background)
            .apply(image.clone())
            .unwrap_or_else(|_| DynamicImage::ImageRgb8(image.to_rgb8()))
    };
    if format == Format::Ico && (image.width() > ICO_SIDE || image.height() > ICO_SIDE) {
        image = image.resize(ICO_SIDE, ICO_SIDE, image::imageops::FilterType::Lanczos3);
    }
    let colour = image.color();
    let grey = !colour.has_color();
    let alpha = colour.has_alpha();
    let wide = matches!(
        colour,
        C::L16 | C::La16 | C::Rgb16 | C::Rgba16 | C::Rgb32F | C::Rgba32F
    );
    match format {
        Format::Jpeg | Format::Pnm => {
            if grey {
                DynamicImage::ImageLuma8(image.to_luma8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            }
        }
        Format::Png | Format::Tiff => match colour {
            C::Rgb32F => DynamicImage::ImageRgb16(image.to_rgb16()),
            C::Rgba32F => DynamicImage::ImageRgba16(image.to_rgba16()),
            _ => image,
        },
        Format::Gif | Format::Ico => DynamicImage::ImageRgba8(image.to_rgba8()),
        Format::Farbfeld => DynamicImage::ImageRgba16(image.to_rgba16()),
        Format::Exr => {
            if alpha {
                DynamicImage::ImageRgba32F(image.to_rgba32f())
            } else {
                DynamicImage::ImageRgb32F(image.to_rgb32f())
            }
        }
        Format::Hdr => DynamicImage::ImageRgb32F(image.to_rgb32f()),
        Format::Bmp | Format::Tga if grey && !alpha && !wide => {
            DynamicImage::ImageLuma8(image.to_luma8())
        }
        _ => {
            if alpha {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            }
        }
    }
}

/// Counter that keeps temporary names of concurrent writers apart.
static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `target` through a temporary file in the same folder,
/// so a failure never leaves a half-written picture in place.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), Error> {
    let folder = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let folder = folder.unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary: PathBuf = folder.join(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let result = std::fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, target));
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(Error::io(target, error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{load_bytes, LoadOptions};
    use image::{GenericImageView, Rgba, RgbaImage};

    fn picture() -> DynamicImage {
        let mut image = RgbaImage::from_pixel(300, 200, Rgba([10, 200, 30, 255]));
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        DynamicImage::ImageRgba8(image)
    }

    #[test]
    fn every_writable_format_round_trips() {
        let options = EncodeOptions::default();
        for format in Format::writable() {
            let bytes = encode(&picture(), format, &options, None, None)
                .unwrap_or_else(|error| panic!("{format:?}: {error}"));
            if !format.can_read() {
                continue;
            }
            let decoded = load_bytes(&bytes, Some(format), &LoadOptions::default())
                .unwrap_or_else(|error| panic!("{format:?}: {error}"));
            assert_eq!(decoded.format, format, "{format:?}");
            let expected = if format == Format::Ico {
                (256, 171)
            } else {
                (300, 200)
            };
            let size = decoded.image.dimensions();
            assert!(
                size == expected || (format == Format::Ico && size.0 == 256),
                "{format:?}: {size:?}"
            );
        }
    }

    #[test]
    fn metadata_follows_the_options() {
        let exif = crate::exif::orientation_chunk(1);
        let icc = b"not really a profile".to_vec();
        for format in [Format::Jpeg, Format::Png, Format::WebP] {
            let kept = encode(
                &picture(),
                format,
                &EncodeOptions::default(),
                Some(&icc),
                Some(&exif),
            )
            .expect("encodes");
            let decoded = load_bytes(&kept, None, &LoadOptions::default()).expect("decodes");
            assert_eq!(decoded.exif.as_deref(), Some(exif.as_slice()), "{format:?}");
            let stripped = EncodeOptions {
                keep_exif: false,
                ..EncodeOptions::default()
            };
            let bytes =
                encode(&picture(), format, &stripped, Some(&icc), Some(&exif)).expect("encodes");
            let decoded = load_bytes(&bytes, None, &LoadOptions::default()).expect("decodes");
            assert!(decoded.exif.is_none(), "{format:?}");
        }
    }

    #[test]
    fn jpeg_flattens_on_the_background() {
        let options = EncodeOptions {
            background: [255, 0, 0],
            ..EncodeOptions::default()
        };
        let bytes = encode(&picture(), Format::Jpeg, &options, None, None).expect("encodes");
        let decoded = load_bytes(&bytes, None, &LoadOptions::default()).expect("decodes");
        let corner = decoded.image.to_rgb8().get_pixel(0, 0).0;
        assert!(corner[0] > 150 && corner[1] < 120, "{corner:?}");
    }

    #[test]
    fn keep_resolves_unwritable_sources() {
        assert_eq!(OutputFormat::Keep.resolve(Format::Jpeg), Format::Jpeg);
        assert_eq!(OutputFormat::Keep.resolve(Format::Psd), Format::Png);
        assert_eq!(OutputFormat::Keep.resolve(Format::Raw), Format::Jpeg);
        assert_eq!(
            OutputFormat::To(Format::Svg).resolve(Format::Png),
            Format::Png
        );
    }

    #[test]
    fn atomic_write_replaces() {
        let folder =
            std::env::temp_dir().join(format!("image-gallery-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let target = folder.join("a.bin");
        write_atomic(&target, b"one").expect("writes");
        write_atomic(&target, b"two").expect("replaces");
        assert_eq!(std::fs::read(&target).expect("reads"), b"two");
        let leftovers = std::fs::read_dir(&folder).expect("lists").count();
        assert_eq!(leftovers, 1);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
