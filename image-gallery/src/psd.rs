//! The flattened composite of Photoshop PSD and PSB files.
//!
//! A PSD file stores, after its layers, the merged picture Photoshop shows
//! ("maximize compatibility"); that is what the gallery reads. Bitmap,
//! grayscale, indexed, RGB, CMYK, multichannel, duotone and Lab documents of
//! 1, 8, 16 and 32 bits per channel are converted to RGBA.

// Pixel arithmetic: every float is clamped to 0..=1 before it is scaled to the
// target range, so the casts below cannot truncate or lose the sign.
// Colour formulas keep their textbook one-letter names.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names
)]

use image::{DynamicImage, ImageBuffer, Rgba};

use crate::Error;

/// The decoded composite and the metadata of the image resources.
pub(crate) struct Psd {
    pub(crate) image: DynamicImage,
    pub(crate) icc: Option<Vec<u8>>,
    pub(crate) exif: Option<Vec<u8>>,
}

/// Image resource IDs.
const RESOURCE_ICC: u16 = 1039;
const RESOURCE_EXIF: u16 = 1058;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bitmap,
    Gray,
    Indexed,
    Rgb,
    Cmyk,
    Multichannel,
    Duotone,
    Lab,
}

impl Mode {
    fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            0 => Self::Bitmap,
            1 => Self::Gray,
            2 => Self::Indexed,
            3 => Self::Rgb,
            4 => Self::Cmyk,
            7 => Self::Multichannel,
            8 => Self::Duotone,
            9 => Self::Lab,
            _ => return None,
        })
    }

    /// Channels of colour, before any transparency.
    fn colour_channels(self) -> usize {
        match self {
            Self::Bitmap | Self::Gray | Self::Indexed | Self::Multichannel | Self::Duotone => 1,
            Self::Rgb | Self::Lab => 3,
            Self::Cmyk => 4,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&end| end <= self.data.len());
        let end = end.ok_or_else(|| Error::Codec("PSD: unexpected end of file".into()))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let high = u64::from(self.u32()?);
        Ok(high << 32 | u64::from(self.u32()?))
    }

    /// A length field: 32 bits in PSD, 64 bits in PSB where the format says so.
    fn length(&mut self, wide: bool) -> Result<usize, Error> {
        let length = if wide {
            self.u64()?
        } else {
            u64::from(self.u32()?)
        };
        usize::try_from(length).map_err(|_| invalid("section too long"))
    }

    fn section(&mut self, wide: bool) -> Result<&'a [u8], Error> {
        let length = self.length(wide)?;
        self.take(length)
    }
}

fn invalid(what: &str) -> Error {
    Error::Codec(format!("PSD: {what}"))
}

/// Decodes the composite, refusing pictures of more than `max_pixels`.
pub(crate) fn decode(data: &[u8], max_pixels: u64) -> Result<Psd, Error> {
    let mut reader = Reader { data, at: 0 };
    if reader.take(4)? != b"8BPS" {
        return Err(invalid("not a Photoshop file"));
    }
    let wide = match reader.u16()? {
        1 => false,
        2 => true,
        _ => return Err(invalid("unknown version")),
    };
    reader.take(6)?;
    let channels = usize::from(reader.u16()?);
    let height = reader.u32()?;
    let width = reader.u32()?;
    let depth = reader.u16()?;
    let mode = Mode::from_code(reader.u16()?).ok_or_else(|| invalid("unknown colour mode"))?;
    if width == 0 || height == 0 || channels == 0 || channels > 56 {
        return Err(invalid("bad header"));
    }
    if u64::from(width) * u64::from(height) > max_pixels {
        return Err(Error::TooLarge { width, height });
    }
    if !matches!(depth, 1 | 8 | 16 | 32) || (depth == 1) != (mode == Mode::Bitmap) {
        return Err(invalid("unsupported bit depth"));
    }
    let palette = reader.section(false)?;
    let resources = reader.section(false)?;
    let layers = reader.section(wide)?;
    let merged_alpha = merged_alpha(layers, wide);
    let compression = reader.u16()?;
    let rest = &data[reader.at..];

    let (icc, exif) = read_resources(resources);
    let colour = mode.colour_channels();
    if channels < colour {
        return Err(invalid("too few channels for the colour mode"));
    }
    let alpha = merged_alpha && channels > colour;
    let used = colour + usize::from(alpha);
    let geometry = Geometry {
        width: width as usize,
        height: height as usize,
        row: match depth {
            1 => (width as usize).div_ceil(8),
            _ => width as usize * usize::from(depth / 8),
        },
        channels,
    };
    let planes = read_planes(rest, compression, &geometry, used, wide, depth)?;
    let samples = Samples {
        planes: &planes,
        geometry: &geometry,
        depth,
    };
    let image = compose(&samples, mode, palette, alpha);
    Ok(Psd { image, icc, exif })
}

struct Geometry {
    width: usize,
    height: usize,
    /// Bytes per row of one channel.
    row: usize,
    /// Channels stored in the file.
    channels: usize,
}

/// Whether the layer section says the merged picture has transparency: a
/// negative layer count means the first extra channel is the merged alpha.
fn merged_alpha(layers: &[u8], wide: bool) -> bool {
    let mut reader = Reader {
        data: layers,
        at: 0,
    };
    let Ok(info_length) = reader.length(wide) else {
        return false;
    };
    if info_length < 2 {
        return false;
    }
    reader.u16().is_ok_and(|count| count.cast_signed() < 0)
}

fn read_resources(resources: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let mut reader = Reader {
        data: resources,
        at: 0,
    };
    let (mut icc, mut exif) = (None, None);
    while reader.data.len() - reader.at >= 12 {
        match resource(&mut reader) {
            Ok((RESOURCE_ICC, body)) => icc = Some(body.to_vec()),
            Ok((RESOURCE_EXIF, body)) => exif = Some(body.to_vec()),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    (icc, exif)
}

/// One image resource block: its ID and body.
fn resource<'a>(reader: &mut Reader<'a>) -> Result<(u16, &'a [u8]), Error> {
    reader.take(4)?;
    let id = reader.u16()?;
    let name = usize::from(reader.take(1)?[0]);
    // The Pascal name and its length byte are padded to an even size.
    reader.take(name + usize::from(name.is_multiple_of(2)))?;
    let size = reader.u32()? as usize;
    let body = reader.take(size)?;
    if size % 2 == 1 && reader.at < reader.data.len() {
        reader.take(1)?;
    }
    Ok((id, body))
}

/// Reads the first `used` channel planes, decompressed.
fn read_planes(
    data: &[u8],
    compression: u16,
    geometry: &Geometry,
    used: usize,
    wide: bool,
    depth: u16,
) -> Result<Vec<Vec<u8>>, Error> {
    let plane = geometry
        .row
        .checked_mul(geometry.height)
        .ok_or_else(|| invalid("picture too large"))?;
    match compression {
        0 => (0..used)
            .map(|channel| {
                let start = channel * plane;
                data.get(start..start + plane)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| invalid("image data truncated"))
            })
            .collect(),
        1 => {
            let rows = geometry.channels * geometry.height;
            let entry = if wide { 4 } else { 2 };
            let table = data
                .get(..rows * entry)
                .ok_or_else(|| invalid("row table truncated"))?;
            let mut at = rows * entry;
            let mut planes = Vec::with_capacity(used);
            for channel in 0..used {
                let mut out = Vec::with_capacity(plane);
                for row in 0..geometry.height {
                    let index = (channel * geometry.height + row) * entry;
                    let length = if wide {
                        u32::from_be_bytes(table[index..index + 4].try_into().unwrap_or([0; 4]))
                            as usize
                    } else {
                        usize::from(u16::from_be_bytes([table[index], table[index + 1]]))
                    };
                    let packed = data
                        .get(at..at + length)
                        .ok_or_else(|| invalid("compressed row truncated"))?;
                    at += length;
                    unpack_bits(packed, geometry.row, &mut out);
                }
                planes.push(out);
            }
            Ok(planes)
        }
        2 | 3 => {
            let total = plane * geometry.channels;
            let inflated = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, total)
                .map_err(|_| invalid("bad ZIP data"))?;
            if inflated.len() < plane * used {
                return Err(invalid("ZIP data truncated"));
            }
            let mut planes: Vec<Vec<u8>> = inflated
                .chunks(plane)
                .take(used)
                .map(<[u8]>::to_vec)
                .collect();
            if compression == 3 {
                for plane in &mut planes {
                    undo_prediction(plane, geometry.row, depth)?;
                }
            }
            Ok(planes)
        }
        _ => Err(invalid("unknown compression")),
    }
}

/// Appends one `PackBits` row of `row` bytes; a short row is padded with zeros.
fn unpack_bits(mut packed: &[u8], row: usize, out: &mut Vec<u8>) {
    let start = out.len();
    while out.len() - start < row {
        let Some((&header, rest)) = packed.split_first() else {
            break;
        };
        packed = rest;
        let header = header.cast_signed();
        if header >= 0 {
            let count = usize::from(header.cast_unsigned()) + 1;
            let count = count.min(packed.len());
            out.extend_from_slice(&packed[..count]);
            packed = &packed[count..];
        } else if header != -128 {
            let count = usize::from((-header).cast_unsigned()) + 1;
            let Some((&value, rest)) = packed.split_first() else {
                break;
            };
            packed = rest;
            out.extend(std::iter::repeat_n(value, count));
        }
    }
    out.resize(start + row, 0);
}

/// Undoes the horizontal delta prediction of ZIP-with-prediction data.
fn undo_prediction(plane: &mut [u8], row: usize, depth: u16) -> Result<(), Error> {
    for line in plane.chunks_mut(row) {
        match depth {
            8 => {
                let mut previous = 0u8;
                for byte in line.iter_mut() {
                    *byte = byte.wrapping_add(previous);
                    previous = *byte;
                }
            }
            16 => {
                let mut previous = 0u16;
                for pair in line.chunks_exact_mut(2) {
                    let value = u16::from_be_bytes([pair[0], pair[1]]).wrapping_add(previous);
                    pair.copy_from_slice(&value.to_be_bytes());
                    previous = value;
                }
            }
            _ => return Err(invalid("ZIP prediction of this bit depth is not supported")),
        }
    }
    Ok(())
}

struct Samples<'a> {
    planes: &'a [Vec<u8>],
    geometry: &'a Geometry,
    depth: u16,
}

impl Samples<'_> {
    /// A sample in 0..=1.
    fn get(&self, channel: usize, x: usize, y: usize) -> f32 {
        let plane = &self.planes[channel];
        let row = y * self.geometry.row;
        match self.depth {
            1 => {
                let byte = plane.get(row + x / 8).copied().unwrap_or(0);
                // Bitmap: a set bit is black.
                if byte & (0x80 >> (x % 8)) == 0 {
                    1.0
                } else {
                    0.0
                }
            }
            8 => f32::from(plane.get(row + x).copied().unwrap_or(0)) / 255.0,
            16 => {
                let at = row + 2 * x;
                let value = plane
                    .get(at..at + 2)
                    .map_or(0, |pair| u16::from_be_bytes([pair[0], pair[1]]));
                f32::from(value) / 65535.0
            }
            _ => {
                let at = row + 4 * x;
                let value = plane.get(at..at + 4).map_or(0.0, |bytes| {
                    f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                });
                // 32-bit documents are linear light.
                encode_srgb(value)
            }
        }
    }
}

fn compose(samples: &Samples<'_>, mode: Mode, palette: &[u8], alpha: bool) -> DynamicImage {
    let width = samples.geometry.width;
    let height = samples.geometry.height;
    let alpha_channel = mode.colour_channels();
    let pixel = |x: usize, y: usize| -> [f32; 4] {
        let s = |channel| samples.get(channel, x, y);
        let [r, g, b] = match mode {
            Mode::Bitmap | Mode::Gray | Mode::Multichannel | Mode::Duotone => {
                let v = s(0);
                [v, v, v]
            }
            Mode::Indexed => {
                let index = (s(0) * 255.0).round() as usize;
                if palette.len() >= 768 {
                    [
                        f32::from(palette[index]) / 255.0,
                        f32::from(palette[256 + index]) / 255.0,
                        f32::from(palette[512 + index]) / 255.0,
                    ]
                } else {
                    let v = s(0);
                    [v, v, v]
                }
            }
            Mode::Rgb => [s(0), s(1), s(2)],
            // Photoshop stores CMYK inverted: 1 is no ink.
            Mode::Cmyk => {
                let k = s(3);
                [s(0) * k, s(1) * k, s(2) * k]
            }
            Mode::Lab => lab_to_srgb(s(0) * 100.0, s(1) * 255.0 - 128.0, s(2) * 255.0 - 128.0),
        };
        let a = if alpha { s(alpha_channel) } else { 1.0 };
        [r, g, b, a]
    };
    if samples.depth == 16 {
        let mut out = ImageBuffer::<Rgba<u16>, Vec<u16>>::new(width as u32, height as u32);
        for (x, y, target) in out.enumerate_pixels_mut() {
            let p = pixel(x as usize, y as usize);
            *target = Rgba(p.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16));
        }
        DynamicImage::ImageRgba16(out)
    } else {
        let mut out = image::RgbaImage::new(width as u32, height as u32);
        for (x, y, target) in out.enumerate_pixels_mut() {
            let p = pixel(x as usize, y as usize);
            *target = Rgba(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        DynamicImage::ImageRgba8(out)
    }
}

/// Linear light to the sRGB transfer curve, clamped to 0..=1.
pub(crate) fn encode_srgb(linear: f32) -> f32 {
    let v = if linear.is_finite() {
        linear.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// CIE L*a*b* (D50, as Photoshop uses) to sRGB.
fn lab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inverse = |t: f32| {
        let cube = t * t * t;
        if cube > 0.008_856 {
            cube
        } else {
            (116.0 * t - 16.0) / 903.3
        }
    };
    let (x, y, z) = (0.9642 * inverse(fx), inverse(fy), 0.8249 * inverse(fz));
    // Bradford-adapted XYZ (D50) to linear sRGB.
    let r = 3.133_856 * x - 1.616_867 * y - 0.490_615 * z;
    let g = -0.978_768 * x + 1.916_142 * y + 0.033_454 * z;
    let blue = 0.071_945 * x - 0.228_991 * y + 1.405_243 * z;
    [encode_srgb(r), encode_srgb(g), encode_srgb(blue)]
}

/// Builds a PSD file for tests: version 1, raw or RLE planes as given.
#[cfg(test)]
pub(crate) fn build(
    mode: u16,
    depth: u16,
    width: u32,
    height: u32,
    planes: &[Vec<u8>],
    layer_count: i16,
) -> Vec<u8> {
    let mut out = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    out.extend_from_slice(&u16::try_from(planes.len()).expect("few").to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&depth.to_be_bytes());
    out.extend_from_slice(&mode.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    // One resource: a fake ICC profile.
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&RESOURCE_ICC.to_be_bytes());
    resource.extend_from_slice(&[0, 0]);
    resource.extend_from_slice(&4u32.to_be_bytes());
    resource.extend_from_slice(b"ICC!");
    out.extend_from_slice(&u32::try_from(resource.len()).expect("small").to_be_bytes());
    out.extend_from_slice(&resource);
    // Layer section: info length 2, then the layer count.
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&layer_count.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for plane in planes {
        out.extend_from_slice(plane);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn reads_raw_rgb_with_merged_alpha() {
        let planes = vec![vec![255, 0], vec![0, 255], vec![0, 0], vec![255, 128]];
        let psd = decode(&build(3, 8, 2, 1, &planes, -1), 1 << 20).expect("decodes");
        assert_eq!(psd.icc.as_deref(), Some(&b"ICC!"[..]));
        let image = psd.image.to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 255, 0, 128]);
        // Without the negative layer count the fourth channel is not alpha.
        let opaque = decode(&build(3, 8, 2, 1, &planes, 0), 1 << 20).expect("decodes");
        assert_eq!(opaque.image.to_rgba8().get_pixel(1, 0).0[3], 255);
    }

    #[test]
    fn reads_rle_grayscale_and_cmyk() {
        let mut out = Vec::new();
        unpack_bits(&[0xFD, 7, 1, 9], 5, &mut out);
        assert_eq!(out, [7, 7, 7, 7, 9]);
        // CMYK, all channels "no ink" except K at half: mid grey.
        let planes = vec![vec![255], vec![255], vec![255], vec![128]];
        let psd = decode(&build(4, 8, 1, 1, &planes, 0), 16).expect("decodes");
        assert_eq!(psd.image.to_rgba8().get_pixel(0, 0).0, [128, 128, 128, 255]);
        assert_eq!(psd.image.dimensions(), (1, 1));
    }

    #[test]
    fn sixteen_bit_and_bitmap() {
        let planes = vec![vec![0xFF, 0xFF, 0, 0]];
        let psd = decode(&build(1, 16, 2, 1, &planes, 0), 16).expect("decodes");
        assert!(matches!(psd.image, DynamicImage::ImageRgba16(_)));
        assert_eq!(psd.image.to_rgba8().get_pixel(0, 0).0, [255, 255, 255, 255]);
        let bitmap = decode(&build(0, 1, 2, 1, &[vec![0b0100_0000]], 0), 16).expect("decodes");
        let bitmap = bitmap.image.to_rgba8();
        assert_eq!(bitmap.get_pixel(0, 0).0[0], 255);
        assert_eq!(bitmap.get_pixel(1, 0).0[0], 0);
    }

    #[test]
    fn hostile_headers_fail_cleanly() {
        let planes = vec![vec![0; 4]];
        let file = build(1, 8, 2, 2, &planes, 0);
        assert!(matches!(decode(&file, 3), Err(Error::TooLarge { .. })));
        for cut in 0..file.len() {
            let _ = decode(&file[..cut], 1 << 20);
        }
        let mut bad = file;
        bad[4] = 9;
        assert!(decode(&bad, 1 << 20).is_err());
    }
}
