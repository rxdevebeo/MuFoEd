//! Camera RAW files through their embedded previews.
//!
//! Demosaicing sensor data needs a RAW developer; every camera, however,
//! stores a JPEG rendition of the shot, usually at full size, for its own
//! screen. The gallery shows and converts that rendition:
//!
//! - TIFF-based files (DNG, CR2, NEF, ARW, ORF, RW2, PEF, ...): the JPEG
//!   strips and `JPEGInterchangeFormat` blocks of every directory;
//! - Fujifilm RAF: the JPEG the header points at;
//! - anything else (CR3, CRW, X3F, ...): the largest JPEG found in the file.

use std::io::Cursor;

use crate::tiff::{find, tag, Tiff};
use crate::Error;

/// The chosen preview: the JPEG bytes and the orientation the RAW container
/// declares for it (EXIF value), if any.
pub(crate) struct Preview<'a> {
    pub(crate) jpeg: &'a [u8],
    pub(crate) orientation: Option<u8>,
}

/// Most JPEG starts the fallback scan inspects.
const MAX_SCANNED: usize = 64;

/// Finds the largest decodable JPEG preview of a RAW file.
pub(crate) fn preview(data: &[u8]) -> Result<Preview<'_>, Error> {
    if data.starts_with(b"FUJIFILMCCD-RAW") {
        if let Some(jpeg) = raf_preview(data) {
            return Ok(Preview {
                jpeg,
                orientation: None,
            });
        }
    }
    if let Some(tiff) = Tiff::new(data) {
        let orientation = tiff
            .first_ifd()
            .and_then(|ifd| tiff.entries(ifd))
            .and_then(|entries| find(&entries, tag::ORIENTATION).and_then(|e| tiff.uint(e, 0)))
            .and_then(|value| u8::try_from(value).ok());
        let mut candidates = tiff_candidates(&tiff);
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.len()));
        if let Some(jpeg) = largest(candidates.into_iter()) {
            return Ok(Preview { jpeg, orientation });
        }
    }
    let starts = data
        .windows(3)
        .enumerate()
        .filter(|(_, window)| window == &[0xFF, 0xD8, 0xFF])
        .map(|(at, _)| &data[at..])
        .take(MAX_SCANNED);
    largest(starts)
        .map(|jpeg| Preview {
            jpeg,
            orientation: None,
        })
        .ok_or_else(|| Error::Codec("RAW: no embedded preview found".into()))
}

/// The JPEG of a Fujifilm RAF header: offset and length at bytes 84 and 88.
fn raf_preview(data: &[u8]) -> Option<&[u8]> {
    let word = |at: usize| -> Option<usize> {
        let bytes: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_be_bytes(bytes)).ok()
    };
    let (offset, length) = (word(84)?, word(88)?);
    data.get(offset..offset.checked_add(length)?)
        .filter(|jpeg| jpeg.starts_with(&[0xFF, 0xD8]))
}

/// Every JPEG block a TIFF-based RAW declares.
fn tiff_candidates<'a>(tiff: &Tiff<'a>) -> Vec<&'a [u8]> {
    let data = tiff.data();
    let mut out = Vec::new();
    let block = |offset: u32, length: u32| -> Option<&'a [u8]> {
        let offset = usize::try_from(offset).ok()?;
        let length = usize::try_from(length).ok()?;
        data.get(offset..offset.checked_add(length)?)
            .filter(|jpeg| jpeg.starts_with(&[0xFF, 0xD8]))
    };
    for ifd in tiff.all_ifds() {
        let Some(entries) = tiff.entries(ifd) else {
            continue;
        };
        let value = |id| find(&entries, id).and_then(|entry| tiff.uint(entry, 0));
        if let (Some(offset), Some(length)) = (value(tag::JPEG_OFFSET), value(tag::JPEG_LENGTH)) {
            out.extend(block(offset, length));
        }
        // JPEG-compressed strips that are not sensor data (CFA / linear raw).
        let compression = value(tag::COMPRESSION);
        let photometric = value(tag::PHOTOMETRIC);
        let sensor = matches!(photometric, Some(32803 | 34892));
        if matches!(compression, Some(6 | 7)) && !sensor {
            if let (Some(offset), Some(length)) =
                (value(tag::STRIP_OFFSETS), value(tag::STRIP_BYTE_COUNTS))
            {
                out.extend(block(offset, length));
            }
        }
    }
    out
}

/// The candidate whose JPEG header declares the most pixels and decodes.
fn largest<'a>(candidates: impl Iterator<Item = &'a [u8]>) -> Option<&'a [u8]> {
    let mut sized: Vec<(u64, &[u8])> = candidates
        .filter_map(|jpeg| {
            let reader =
                image::ImageReader::with_format(Cursor::new(jpeg), image::ImageFormat::Jpeg);
            let (width, height) = crate::error::contain(|| Ok(reader.into_dimensions()?)).ok()?;
            Some((u64::from(width) * u64::from(height), jpeg))
        })
        .filter(|(pixels, _)| *pixels > 0)
        .collect();
    sized.sort_by_key(|(pixels, _)| std::cmp::Reverse(*pixels));
    sized.first().map(|(_, jpeg)| *jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::build;

    pub(crate) fn tiny_jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let picture = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
        image::DynamicImage::ImageRgb8(picture)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .expect("encodes");
        out
    }

    #[test]
    fn picks_the_largest_tiff_preview() {
        let small = tiny_jpeg(8, 8);
        let big = tiny_jpeg(32, 16);
        // Two directories' worth of previews in one: JPEGInterchangeFormat for
        // the small one, a JPEG strip for the big one.
        let header = build(&[
            (tag::ORIENTATION, 3, 1, vec![0, 8]),
            (tag::COMPRESSION, 3, 1, vec![0, 6]),
            (tag::PHOTOMETRIC, 3, 1, vec![0, 6]),
            (tag::JPEG_OFFSET, 4, 1, vec![0; 4]),
            (tag::JPEG_LENGTH, 4, 1, vec![0; 4]),
            (tag::STRIP_OFFSETS, 4, 1, vec![0; 4]),
            (tag::STRIP_BYTE_COUNTS, 4, 1, vec![0; 4]),
        ]);
        let small_at = u32::try_from(header.len()).expect("small");
        let big_at = small_at + u32::try_from(small.len()).expect("small");
        let file = [
            build(&[
                (tag::ORIENTATION, 3, 1, vec![0, 8]),
                (tag::COMPRESSION, 3, 1, vec![0, 6]),
                (tag::PHOTOMETRIC, 3, 1, vec![0, 6]),
                (tag::JPEG_OFFSET, 4, 1, small_at.to_be_bytes().to_vec()),
                (
                    tag::JPEG_LENGTH,
                    4,
                    1,
                    (big_at - small_at).to_be_bytes().to_vec(),
                ),
                (tag::STRIP_OFFSETS, 4, 1, big_at.to_be_bytes().to_vec()),
                (
                    tag::STRIP_BYTE_COUNTS,
                    4,
                    1,
                    u32::try_from(big.len())
                        .expect("small")
                        .to_be_bytes()
                        .to_vec(),
                ),
            ]),
            small,
            big.clone(),
        ]
        .concat();
        let preview = preview(&file).expect("preview");
        assert_eq!(preview.jpeg, big.as_slice());
        assert_eq!(preview.orientation, Some(8));
    }

    #[test]
    fn falls_back_to_scanning_and_reads_raf() {
        let jpeg = tiny_jpeg(16, 16);
        let mut unknown = b"\0\0\0\x18ftypcrx \0\0\0\0junk".to_vec();
        unknown.extend_from_slice(&jpeg);
        assert_eq!(preview(&unknown).expect("scan").jpeg, jpeg.as_slice());

        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        raf.resize(100, 0);
        raf[84..88].copy_from_slice(&100u32.to_be_bytes());
        raf[88..92].copy_from_slice(&u32::try_from(jpeg.len()).expect("small").to_be_bytes());
        raf.extend_from_slice(&jpeg);
        assert_eq!(preview(&raf).expect("raf").jpeg, jpeg.as_slice());

        assert!(preview(b"nothing to see").is_err());
    }
}
