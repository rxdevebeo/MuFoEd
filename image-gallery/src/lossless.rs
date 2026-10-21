//! Edits that leave the compressed pixels alone.
//!
//! - Quarter turns and mirrors of a JPEG are recorded in its EXIF orientation
//!   tag instead of re-encoding (which would lose quality each time).
//! - Metadata is removed from JPEG and PNG files by dropping segments and
//!   chunks; the orientation survives as a minimal EXIF chunk so the picture
//!   does not fall over.

use crate::exif::{self, Orientation};
use crate::ops::{Axis, Operation, QuarterTurn};

/// A JPEG marker segment: its marker byte and the byte range of the whole
/// segment (marker included).
struct Segment {
    marker: u8,
    start: usize,
    end: usize,
}

/// The header segments of a JPEG, up to (not including) the scan, and where
/// the scan begins. `None` for anything that is not a well-formed JPEG.
fn segments(data: &[u8]) -> Option<(Vec<Segment>, usize)> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    let mut out = Vec::new();
    loop {
        // Fill bytes may precede a marker.
        while data.get(at) == Some(&0xFF) && data.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if data.get(at) != Some(&0xFF) {
            return None;
        }
        let marker = *data.get(at + 1)?;
        if marker == 0xDA {
            return Some((out, at));
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.push(Segment {
                marker,
                start: at,
                end: at + 2,
            });
            at += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([*data.get(at + 2)?, *data.get(at + 3)?]));
        if length < 2 {
            return None;
        }
        let end = at + 2 + length;
        if end > data.len() {
            return None;
        }
        out.push(Segment {
            marker,
            start: at,
            end,
        });
        at = end;
    }
}

const EXIF_PREFIX: &[u8] = b"Exif\0\0";

fn is_exif(data: &[u8], segment: &Segment) -> bool {
    segment.marker == 0xE1 && data[segment.start + 4..segment.end].starts_with(EXIF_PREFIX)
}

/// An APP1 segment holding `chunk`, or `None` if it does not fit.
fn exif_segment(chunk: &[u8]) -> Option<Vec<u8>> {
    let length = u16::try_from(2 + EXIF_PREFIX.len() + chunk.len()).ok()?;
    let mut out = vec![0xFF, 0xE1];
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(EXIF_PREFIX);
    out.extend_from_slice(chunk);
    Some(out)
}

/// The orientation a list of quarter turns and mirrors amounts to, applied
/// after `start`. `None` if a step is not a reorientation.
pub(crate) fn compose(start: Orientation, operations: &[Operation]) -> Option<Orientation> {
    operations.iter().try_fold(start, |orientation, operation| {
        Some(match operation {
            Operation::Rotate(QuarterTurn::Right) => orientation.turn_right(),
            Operation::Rotate(QuarterTurn::Left) => orientation.turn_left(),
            Operation::Rotate(QuarterTurn::Half) => orientation.turn_right().turn_right(),
            Operation::Flip(Axis::Horizontal) => orientation.mirror_horizontally(),
            Operation::Flip(Axis::Vertical) => orientation.mirror_vertically(),
            _ => return None,
        })
    })
}

/// Records `operations` (quarter turns and mirrors only) in the JPEG's EXIF
/// orientation. `None` when the file cannot be edited this way: not a JPEG,
/// another kind of step, or an EXIF chunk without an orientation entry.
pub(crate) fn reorient_jpeg(data: &[u8], operations: &[Operation]) -> Option<Vec<u8>> {
    let (segments, _) = segments(data)?;
    let exif_segment_index = segments.iter().position(|segment| is_exif(data, segment));
    let current = exif_segment_index
        .and_then(|index| {
            let segment = &segments[index];
            exif::orientation(&data[segment.start + 4 + EXIF_PREFIX.len()..segment.end])
        })
        .unwrap_or(1);
    let target = compose(Orientation::from_exif(current)?, operations)?.to_exif();
    let mut out = data.to_vec();
    if let Some(index) = exif_segment_index {
        let segment = &segments[index];
        let chunk = &mut out[segment.start + 4 + EXIF_PREFIX.len()..segment.end];
        if !exif::set_orientation(chunk, target) {
            // An EXIF chunk without the tag would need its directory
            // rewritten; leave that to a re-encode.
            return None;
        }
    } else {
        // A new APP1 goes after APP0 (JFIF), where readers look for it.
        let at = segments
            .first()
            .filter(|segment| segment.marker == 0xE0)
            .map_or(2, |segment| segment.end);
        let segment = exif_segment(&exif::orientation_chunk(target))?;
        out = [&data[..at], &segment, &data[at..]].concat();
    }
    Some(out)
}

/// Removes EXIF, XMP, IPTC and comments from a JPEG, keeping the ICC
/// profile, JFIF/Adobe colour information and the orientation.
pub(crate) fn strip_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    let (segments, scan) = segments(data)?;
    let orientation = segments
        .iter()
        .find(|segment| is_exif(data, segment))
        .and_then(|segment| {
            exif::orientation(&data[segment.start + 4 + EXIF_PREFIX.len()..segment.end])
        })
        .unwrap_or(1);
    let mut out = vec![0xFF, 0xD8];
    let mut inserted = orientation == 1;
    for segment in &segments {
        // APP1 (EXIF, XMP), APP12 (Ducky), APP13 (IPTC/Photoshop), COM.
        let drop = matches!(segment.marker, 0xE1 | 0xEC | 0xED | 0xFE);
        if !inserted && segment.marker != 0xE0 {
            out.extend(exif_segment(&exif::orientation_chunk(orientation))?);
            inserted = true;
        }
        if !drop {
            out.extend_from_slice(&data[segment.start..segment.end]);
        }
    }
    if !inserted {
        out.extend(exif_segment(&exif::orientation_chunk(orientation))?);
    }
    out.extend_from_slice(&data[scan..]);
    Some(out)
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Removes text, time and EXIF chunks from a PNG, keeping the ICC profile and
/// the orientation.
pub(crate) fn strip_png(data: &[u8]) -> Option<Vec<u8>> {
    if !data.starts_with(PNG_SIGNATURE) {
        return None;
    }
    let mut out = PNG_SIGNATURE.to_vec();
    let mut at = PNG_SIGNATURE.len();
    let mut orientation = 1;
    while at < data.len() {
        let length =
            usize::try_from(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?)).ok()?;
        let kind: [u8; 4] = data.get(at + 4..at + 8)?.try_into().ok()?;
        let end = at.checked_add(12)?.checked_add(length)?;
        let chunk = data.get(at..end)?;
        match &kind {
            b"eXIf" => {
                orientation = exif::orientation(&chunk[8..8 + length]).unwrap_or(1);
            }
            b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" => {}
            b"IDAT" if orientation != 1 => {
                out.extend(png_chunk(*b"eXIf", &exif::orientation_chunk(orientation)));
                orientation = 1;
                out.extend_from_slice(chunk);
            }
            _ => out.extend_from_slice(chunk),
        }
        at = end;
        if &kind == b"IEND" {
            break;
        }
    }
    Some(out)
}

fn png_chunk(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len()).unwrap_or(0);
    let mut out = length.to_be_bytes().to_vec();
    out.extend_from_slice(&kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{load_bytes, LoadOptions};
    use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
    use std::io::Cursor;

    fn jpeg(with_exif: Option<u8>) -> Vec<u8> {
        let mut picture = RgbImage::from_pixel(16, 8, Rgb([20, 20, 200]));
        picture.put_pixel(0, 0, Rgb([250, 0, 0]));
        let mut out = Vec::new();
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut out), 95);
        if let Some(value) = with_exif {
            image::ImageEncoder::set_exif_metadata(&mut encoder, exif::orientation_chunk(value))
                .expect("JPEG takes EXIF");
        }
        DynamicImage::ImageRgb8(picture)
            .write_with_encoder(encoder)
            .expect("encodes");
        out
    }

    fn orientation_of(data: &[u8]) -> u8 {
        load_bytes(data, None, &LoadOptions::default())
            .expect("decodes")
            .orientation
    }

    #[test]
    fn turns_are_recorded_without_touching_the_scan() {
        let original = jpeg(None);
        let turned =
            reorient_jpeg(&original, &[Operation::Rotate(QuarterTurn::Right)]).expect("edits");
        assert_eq!(orientation_of(&turned), 6);
        let decoded = load_bytes(&turned, None, &LoadOptions::default()).expect("decodes");
        assert_eq!(decoded.image.dimensions(), (8, 16));
        // The compressed data is the same bytes.
        let (_, scan) = segments(&original).expect("jpeg");
        assert!(turned.ends_with(&original[scan..]));

        let back = reorient_jpeg(&turned, &[Operation::Rotate(QuarterTurn::Left)]).expect("edits");
        assert_eq!(orientation_of(&back), 1);
        assert_eq!(back.len(), turned.len(), "patched in place");

        let with_exif = jpeg(Some(3));
        let mirrored =
            reorient_jpeg(&with_exif, &[Operation::Flip(Axis::Vertical)]).expect("edits");
        assert_eq!(orientation_of(&mirrored), 2);
        assert!(reorient_jpeg(&with_exif, &[Operation::Invert]).is_none());
        assert!(reorient_jpeg(b"not a jpeg", &[]).is_none());
    }

    #[test]
    fn stripping_keeps_the_orientation() {
        let tagged = jpeg(Some(8));
        let stripped = strip_jpeg(&tagged).expect("strips");
        assert_eq!(orientation_of(&stripped), 8);
        let plain = strip_jpeg(&jpeg(None)).expect("strips");
        assert_eq!(orientation_of(&plain), 1);
        assert!(load_bytes(&plain, None, &LoadOptions::default()).is_ok());
    }

    #[test]
    fn png_text_chunks_go() {
        let mut out = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(Cursor::new(&mut out));
        image::ImageEncoder::set_exif_metadata(&mut encoder, exif::orientation_chunk(6))
            .expect("EXIF");
        DynamicImage::ImageRgb8(RgbImage::new(4, 2))
            .write_with_encoder(encoder)
            .expect("encodes");
        // Add a text chunk after the header.
        let header_end = PNG_SIGNATURE.len() + 25;
        let text = png_chunk(*b"tEXt", b"Author\0Someone");
        let out = [&out[..header_end], &text, &out[header_end..]].concat();
        let stripped = strip_png(&out).expect("strips");
        assert!(!stripped.windows(4).any(|window| window == b"tEXt"));
        let decoded = load_bytes(&stripped, None, &LoadOptions::default()).expect("decodes");
        assert_eq!(decoded.orientation, 6);
        assert_eq!(decoded.image.dimensions(), (2, 4));
    }

    #[test]
    fn crc_matches_the_png_reference() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }
}
