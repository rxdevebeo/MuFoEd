//! The formats the gallery recognizes, reads and writes.

use serde::{Deserialize, Serialize};

/// A picture format, by file extension and by content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Portable Network Graphics, including APNG (first frame).
    Png,
    /// JPEG / JFIF.
    Jpeg,
    /// GIF (first frame).
    Gif,
    /// WebP; written lossless.
    WebP,
    /// Windows bitmap.
    Bmp,
    /// Windows icon.
    Ico,
    /// TIFF.
    Tiff,
    /// Truevision TGA.
    Tga,
    /// `DirectDraw` surface (read only).
    Dds,
    /// Radiance HDR.
    Hdr,
    /// `OpenEXR`.
    Exr,
    /// Netpbm: PBM, PGM, PPM, PAM.
    Pnm,
    /// Quite OK Image.
    Qoi,
    /// Farbfeld.
    Farbfeld,
    /// AV1 Image File Format (written only: reading needs a native decoder).
    Avif,
    /// JPEG XL (read only).
    Jxl,
    /// SVG and SVGZ (read only).
    Svg,
    /// Photoshop PSD and PSB, flattened composite (read only).
    Psd,
    /// Camera RAW: the embedded full-size preview (read only).
    Raw,
    /// HEIF / HEIC (recognized, not readable: no pure-Rust decoder).
    Heic,
    /// A 3D model (glTF, FBX, COLLADA, OBJ, 3MF, STL, PLY, OFF), read as a
    /// rendered preview.
    Model,
}

/// Camera RAW extensions. Their embedded previews are what the gallery shows.
const RAW_EXTENSIONS: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "raf", "pef",
    "ptx", "erf", "3fr", "fff", "iiq", "mos", "mrw", "x3f", "srw", "dcr", "kdc", "rwl", "mef",
];

impl Format {
    /// Every format, in the order menus list them.
    pub const ALL: [Self; 21] = [
        Self::Png,
        Self::Jpeg,
        Self::WebP,
        Self::Gif,
        Self::Tiff,
        Self::Bmp,
        Self::Ico,
        Self::Tga,
        Self::Pnm,
        Self::Qoi,
        Self::Farbfeld,
        Self::Exr,
        Self::Hdr,
        Self::Dds,
        Self::Avif,
        Self::Jxl,
        Self::Svg,
        Self::Psd,
        Self::Raw,
        Self::Heic,
        Self::Model,
    ];

    /// The format's display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
            Self::Tiff => "TIFF",
            Self::Tga => "TGA",
            Self::Dds => "DDS",
            Self::Hdr => "HDR",
            Self::Exr => "OpenEXR",
            Self::Pnm => "PNM",
            Self::Qoi => "QOI",
            Self::Farbfeld => "Farbfeld",
            Self::Avif => "AVIF",
            Self::Jxl => "JPEG XL",
            Self::Svg => "SVG",
            Self::Psd => "PSD",
            Self::Raw => "RAW",
            Self::Heic => "HEIF",
            Self::Model => "3D",
        }
    }

    /// File extensions, lower case, the preferred one first.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png", "apng"],
            Self::Jpeg => &["jpg", "jpeg", "jpe", "jfif", "jif"],
            Self::Gif => &["gif"],
            Self::WebP => &["webp"],
            Self::Bmp => &["bmp", "dib"],
            Self::Ico => &["ico"],
            Self::Tiff => &["tif", "tiff"],
            Self::Tga => &["tga", "icb", "vda", "vst"],
            Self::Dds => &["dds"],
            Self::Hdr => &["hdr", "rgbe"],
            Self::Exr => &["exr"],
            Self::Pnm => &["ppm", "pgm", "pbm", "pnm", "pam"],
            Self::Qoi => &["qoi"],
            Self::Farbfeld => &["ff", "farbfeld"],
            Self::Avif => &["avif"],
            Self::Jxl => &["jxl"],
            Self::Svg => &["svg", "svgz"],
            Self::Psd => &["psd", "psb"],
            Self::Raw => RAW_EXTENSIONS,
            Self::Heic => &["heic", "heif", "hif"],
            Self::Model => crate::model3d::EXTENSIONS,
        }
    }

    /// The preferred extension for files this crate writes.
    pub fn extension(self) -> &'static str {
        self.extensions()[0]
    }

    /// Finds the format of an extension (with or without the dot, any case).
    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|format| format.extensions().contains(&extension.as_str()))
    }

    /// Finds the format of a path by its extension.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(std::ffi::OsStr::to_str)
            .and_then(Self::from_extension)
    }

    /// Whether this build can read the format.
    pub fn can_read(self) -> bool {
        match self {
            Self::Avif | Self::Heic => false,
            Self::Jxl => cfg!(feature = "jxl"),
            Self::Svg => cfg!(feature = "svg"),
            _ => true,
        }
    }

    /// Whether this build can write the format.
    pub fn can_write(self) -> bool {
        match self {
            Self::Dds
            | Self::Jxl
            | Self::Svg
            | Self::Psd
            | Self::Raw
            | Self::Heic
            | Self::Model => false,
            Self::Avif => cfg!(feature = "avif"),
            _ => true,
        }
    }

    /// The formats this build writes, in menu order.
    pub fn writable() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|format| format.can_write())
    }

    /// Whether a file of this format can carry an EXIF chunk this crate writes.
    pub fn carries_exif(self) -> bool {
        matches!(self, Self::Jpeg | Self::Png | Self::WebP | Self::Avif)
    }

    /// Whether the format stores transparency.
    pub fn has_alpha(self) -> bool {
        !matches!(self, Self::Jpeg | Self::Pnm | Self::Hdr)
    }

    /// The matching format of the `image` crate.
    pub(crate) fn image_format(self) -> Option<image::ImageFormat> {
        Some(match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Gif => image::ImageFormat::Gif,
            Self::WebP => image::ImageFormat::WebP,
            Self::Bmp => image::ImageFormat::Bmp,
            Self::Ico => image::ImageFormat::Ico,
            Self::Tiff => image::ImageFormat::Tiff,
            Self::Tga => image::ImageFormat::Tga,
            Self::Dds => image::ImageFormat::Dds,
            Self::Hdr => image::ImageFormat::Hdr,
            Self::Exr => image::ImageFormat::OpenExr,
            Self::Pnm => image::ImageFormat::Pnm,
            Self::Qoi => image::ImageFormat::Qoi,
            Self::Farbfeld => image::ImageFormat::Farbfeld,
            Self::Avif => image::ImageFormat::Avif,
            Self::Jxl | Self::Svg | Self::Psd | Self::Raw | Self::Heic | Self::Model => {
                return None
            }
        })
    }

    /// The format of an `image` crate format, if this crate lists it.
    pub(crate) fn from_image_format(format: image::ImageFormat) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.image_format() == Some(format))
    }
}

/// Recognizes a format from the first bytes of a file.
///
/// Only the formats the `image` crate does not sniff itself, or sniffs
/// ambiguously, are checked here; [`None`] means "ask the extension".
pub fn sniff(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"8BPS") {
        return Some(Format::Psd);
    }
    if head.starts_with(&[0xFF, 0x0A]) || head.starts_with(b"\0\0\0\x0CJXL \r\n\x87\n") {
        return Some(Format::Jxl);
    }
    if head.starts_with(b"glTF")
        || head.starts_with(b"Kaydara FBX Binary")
        || head.starts_with(b"ply\n")
        || head.starts_with(b"ply\r\n")
    {
        return Some(Format::Model);
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        return Some(Format::Raw);
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        if brand == b"avif" || brand == b"avis" {
            return Some(Format::Avif);
        }
        if HEIF_BRANDS.iter().any(|known| brand == known.as_slice()) {
            return Some(Format::Heic);
        }
        if brand == b"crx " {
            return Some(Format::Raw);
        }
    }
    let text = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let text = text.trim_ascii_start();
    if text.starts_with(b"<svg") || (text.starts_with(b"<?xml") && contains(head, b"<svg")) {
        return Some(Format::Svg);
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        // Gzip: SVGZ is the only compressed picture format listed.
        return Some(Format::Svg);
    }
    None
}

/// ISO-BMFF major brands of HEIF pictures.
const HEIF_BRANDS: [&[u8; 4]; 7] = [
    b"heic", b"heix", b"hevc", b"heim", b"heis", b"mif1", b"msf1",
];

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_back() {
        for format in Format::ALL {
            for extension in format.extensions() {
                assert_eq!(
                    Format::from_extension(extension),
                    Some(format),
                    "{extension}"
                );
            }
        }
        assert_eq!(Format::from_extension(".JPG"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("txt"), None);
    }

    #[test]
    fn sniffs_the_formats_image_does_not() {
        assert_eq!(sniff(b"8BPS\0\x01"), Some(Format::Psd));
        assert_eq!(sniff(&[0xFF, 0x0A, 0, 0]), Some(Format::Jxl));
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0\0\0"), Some(Format::Heic));
        assert_eq!(sniff(b"\0\0\0\x18ftypavif\0\0\0\0"), Some(Format::Avif));
        assert_eq!(sniff(b"  <svg xmlns="), Some(Format::Svg));
        assert_eq!(sniff(b"<?xml version=\"1.0\"?>\n<svg"), Some(Format::Svg));
        assert_eq!(sniff(b"\x89PNG"), None);
    }

    #[test]
    fn writable_formats_are_readable_or_documented() {
        for format in Format::writable() {
            assert!(format.image_format().is_some(), "{format:?}");
        }
    }
}
