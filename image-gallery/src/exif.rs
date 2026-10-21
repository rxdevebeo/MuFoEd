//! EXIF: what the properties panel shows, and the orientation tag.
//!
//! An EXIF chunk here is the raw TIFF structure, without the `Exif\0\0`
//! prefix of a JPEG APP1 segment, as the `image` crate hands it out.

use serde::{Deserialize, Serialize};

use crate::tiff::{find, tag, Tiff};

/// The camera fields of an EXIF chunk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExifSummary {
    /// Camera maker.
    pub make: Option<String>,
    /// Camera model.
    pub model: Option<String>,
    /// Lens model.
    pub lens: Option<String>,
    /// When the picture was taken (or last changed, failing that).
    pub taken: Option<Stamp>,
    /// Exposure time, such as `1/125 s`.
    pub exposure: Option<String>,
    /// Aperture, such as `f/2.8`.
    pub aperture: Option<String>,
    /// ISO speed.
    pub iso: Option<u32>,
    /// Focal length, such as `35 mm`.
    pub focal_length: Option<String>,
    /// Software that wrote the file.
    pub software: Option<String>,
    /// The orientation tag, 1 to 8 (1 when absent).
    pub orientation: u8,
    /// Whether the chunk holds GPS coordinates.
    pub has_gps: bool,
}

impl ExifSummary {
    /// Reads the fields of an EXIF chunk, or of a TIFF-based RAW file.
    pub fn read(chunk: &[u8]) -> Option<Self> {
        let tiff = Tiff::new(chunk)?;
        let ifd0 = tiff.entries(tiff.first_ifd()?)?;
        let text =
            |entries: &[crate::tiff::Entry], id: u16| find(entries, id).and_then(|e| tiff.ascii(e));
        let mut summary = Self {
            make: text(&ifd0, tag::MAKE),
            model: text(&ifd0, tag::MODEL),
            software: text(&ifd0, tag::SOFTWARE),
            taken: text(&ifd0, tag::DATE_TIME).and_then(|text| Stamp::parse(&text)),
            orientation: find(&ifd0, tag::ORIENTATION)
                .and_then(|entry| tiff.uint(entry, 0))
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| (1..=8).contains(value))
                .unwrap_or(1),
            has_gps: find(&ifd0, tag::GPS_IFD).is_some(),
            ..Self::default()
        };
        let exif = find(&ifd0, tag::EXIF_IFD)
            .and_then(|entry| tiff.uint(entry, 0))
            .and_then(|at| tiff.entries(at as usize));
        if let Some(exif) = exif {
            if let Some(taken) = text(&exif, tag::DATE_TIME_ORIGINAL).and_then(|t| Stamp::parse(&t))
            {
                summary.taken = Some(taken);
            }
            summary.lens = text(&exif, tag::LENS_MODEL);
            summary.iso = find(&exif, tag::ISO).and_then(|entry| tiff.uint(entry, 0));
            if let Some((num, den)) = find(&exif, tag::EXPOSURE_TIME).and_then(|e| tiff.rational(e))
            {
                summary.exposure = exposure_text(num, den);
            }
            if let Some((num, den)) = find(&exif, tag::F_NUMBER).and_then(|e| tiff.rational(e)) {
                summary.aperture = (den > 0.0).then(|| format!("f/{}", trim(num / den)));
            }
            if let Some((num, den)) = find(&exif, tag::FOCAL_LENGTH).and_then(|e| tiff.rational(e))
            {
                summary.focal_length = (den > 0.0).then(|| format!("{} mm", trim(num / den)));
            }
        }
        Some(summary)
    }

    /// The camera as one line: maker and model without the maker repeated.
    pub fn camera(&self) -> Option<String> {
        match (&self.make, &self.model) {
            (Some(make), Some(model)) if model.starts_with(make.as_str()) => Some(model.clone()),
            (Some(make), Some(model)) => Some(format!("{make} {model}")),
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            (None, None) => None,
        }
    }
}

fn exposure_text(num: f64, den: f64) -> Option<String> {
    if num <= 0.0 || den <= 0.0 {
        return None;
    }
    let seconds = num / den;
    Some(if seconds >= 1.0 {
        format!("{} s", trim(seconds))
    } else {
        format!("1/{} s", trim((1.0 / seconds).round()))
    })
}

/// A number with at most one decimal, without a trailing `.0`.
fn trim(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// The orientation value of an EXIF chunk (1 to 8), if the tag is present.
pub fn orientation(chunk: &[u8]) -> Option<u8> {
    let tiff = Tiff::new(chunk)?;
    let ifd0 = tiff.entries(tiff.first_ifd()?)?;
    let value = tiff.uint(find(&ifd0, tag::ORIENTATION)?, 0)?;
    u8::try_from(value)
        .ok()
        .filter(|value| (1..=8).contains(value))
}

/// Rewrites the orientation tag in place. Returns `false` when the chunk has
/// no orientation entry to rewrite.
pub fn set_orientation(chunk: &mut [u8], value: u8) -> bool {
    let Some((at, little)) = Tiff::new(chunk).and_then(|tiff| {
        let ifd0 = tiff.entries(tiff.first_ifd()?)?;
        let at = Tiff::short_position(find(&ifd0, tag::ORIENTATION)?)?;
        Some((at, chunk.first() == Some(&b'I')))
    }) else {
        return false;
    };
    let bytes = if little {
        u16::from(value).to_le_bytes()
    } else {
        u16::from(value).to_be_bytes()
    };
    chunk[at..at + 2].copy_from_slice(&bytes);
    true
}

/// A minimal EXIF chunk holding only an orientation tag.
pub(crate) fn orientation_chunk(value: u8) -> Vec<u8> {
    let mut chunk = b"MM\0\x2A\0\0\0\x08\0\x01".to_vec();
    chunk.extend_from_slice(&tag::ORIENTATION.to_be_bytes());
    chunk.extend_from_slice(&[0, 3, 0, 0, 0, 1, 0, value, 0, 0]);
    chunk.extend_from_slice(&[0; 4]);
    chunk
}

/// An element of the eight orientations: an optional horizontal mirror
/// followed by `turns` quarter turns clockwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Orientation {
    mirror: bool,
    turns: u8,
}

impl Orientation {
    /// No change.
    pub const IDENTITY: Self = Self {
        mirror: false,
        turns: 0,
    };

    /// The transformation an EXIF orientation value asks a viewer to apply.
    pub fn from_exif(value: u8) -> Option<Self> {
        let (mirror, turns) = match value {
            1 => (false, 0),
            2 => (true, 0),
            3 => (false, 2),
            4 => (true, 2),
            5 => (true, 3),
            6 => (false, 1),
            7 => (true, 1),
            8 => (false, 3),
            _ => return None,
        };
        Some(Self { mirror, turns })
    }

    /// The EXIF orientation value of this transformation.
    pub fn to_exif(self) -> u8 {
        match (self.mirror, self.turns % 4) {
            (false, 0) => 1,
            (true, 0) => 2,
            (false, 2) => 3,
            (true, 2) => 4,
            (true, 3) => 5,
            (false, 1) => 6,
            (true, 1) => 7,
            _ => 8,
        }
    }

    /// This transformation followed by a quarter turn clockwise.
    #[must_use]
    pub fn turn_right(self) -> Self {
        Self {
            mirror: self.mirror,
            turns: (self.turns + 1) % 4,
        }
    }

    /// This transformation followed by a quarter turn counter-clockwise.
    #[must_use]
    pub fn turn_left(self) -> Self {
        Self {
            mirror: self.mirror,
            turns: (self.turns + 3) % 4,
        }
    }

    /// This transformation followed by a horizontal mirror.
    #[must_use]
    pub fn mirror_horizontally(self) -> Self {
        // A mirror after `r` turns equals `-r` turns after a mirror.
        Self {
            mirror: !self.mirror,
            turns: (4 - self.turns % 4) % 4,
        }
    }

    /// This transformation followed by a vertical mirror (a horizontal mirror
    /// and a half turn).
    #[must_use]
    pub fn mirror_vertically(self) -> Self {
        let mirrored = self.mirror_horizontally();
        Self {
            mirror: mirrored.mirror,
            turns: (mirrored.turns + 2) % 4,
        }
    }

    /// The matching value of the `image` crate.
    pub fn to_image(self) -> image::metadata::Orientation {
        image::metadata::Orientation::from_exif(self.to_exif())
            .unwrap_or(image::metadata::Orientation::NoTransforms)
    }
}

/// A calendar date and time of day, as EXIF records it (no time zone).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Stamp {
    /// Year.
    pub year: u16,
    /// Month, 1 to 12.
    pub month: u8,
    /// Day, 1 to 31.
    pub day: u8,
    /// Hour, 0 to 23.
    pub hour: u8,
    /// Minute.
    pub minute: u8,
    /// Second.
    pub second: u8,
}

impl Stamp {
    /// Parses `YYYY:MM:DD HH:MM:SS` (any separators).
    pub fn parse(text: &str) -> Option<Self> {
        let numbers: Vec<u32> = text
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect();
        let field = |index: usize| numbers.get(index).copied().unwrap_or(0);
        let stamp = Self {
            year: u16::try_from(field(0)).ok()?,
            month: u8::try_from(field(1)).ok()?,
            day: u8::try_from(field(2)).ok()?,
            hour: u8::try_from(field(3)).ok()?,
            minute: u8::try_from(field(4)).ok()?,
            second: u8::try_from(field(5)).ok()?,
        };
        (stamp.year > 0 && (1..=12).contains(&stamp.month) && (1..=31).contains(&stamp.day))
            .then_some(stamp)
    }

    /// The UTC date and time of a file time.
    pub fn from_system_time(time: std::time::SystemTime) -> Option<Self> {
        let seconds = time.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        let days = i64::try_from(seconds / 86_400).ok()?;
        let rest = seconds % 86_400;
        // Civil-from-days (H. Hinnant), valid for every date after 1970.
        let shifted = days + 719_468;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let march_month = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * march_month + 2) / 5 + 1;
        let month = if march_month < 10 {
            march_month + 3
        } else {
            march_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Some(Self {
            year: u16::try_from(year).ok()?,
            month: u8::try_from(month).ok()?,
            day: u8::try_from(day).ok()?,
            hour: u8::try_from(rest / 3600).ok()?,
            minute: u8::try_from(rest % 3600 / 60).ok()?,
            second: u8::try_from(rest % 60).ok()?,
        })
    }

    /// `YYYY-MM-DD`.
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `HH-MM-SS`, safe in file names.
    pub fn time(&self) -> String {
        format!("{:02}-{:02}-{:02}", self.hour, self.minute, self.second)
    }
}

impl std::fmt::Display for Stamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {:02}:{:02}:{:02}",
            self.date(),
            self.hour,
            self.minute,
            self.second
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::build;

    #[test]
    fn orientation_group_matches_exif() {
        let o = |value| Orientation::from_exif(value).expect("valid");
        for value in 1..=8 {
            assert_eq!(o(value).to_exif(), value);
        }
        // Turning the upright picture right is EXIF 6, twice is 3, thrice 8.
        assert_eq!(Orientation::IDENTITY.turn_right().to_exif(), 6);
        assert_eq!(o(6).turn_right().to_exif(), 3);
        assert_eq!(o(3).turn_right().to_exif(), 8);
        assert_eq!(o(8).turn_right().to_exif(), 1);
        assert_eq!(o(6).turn_left().to_exif(), 1);
        assert_eq!(Orientation::IDENTITY.mirror_horizontally().to_exif(), 2);
        assert_eq!(Orientation::IDENTITY.mirror_vertically().to_exif(), 4);
        // Mirroring a right turn gives transpose (5): a mirror, then three
        // quarter turns.
        assert_eq!(o(6).mirror_horizontally().to_exif(), 5);
        for value in 1..=8 {
            assert_eq!(
                o(value).mirror_horizontally().mirror_horizontally(),
                o(value)
            );
            assert_eq!(o(value).mirror_vertically().mirror_vertically(), o(value));
        }
    }

    #[test]
    fn orientation_matches_the_image_crate_pixels() {
        use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
        let mut source = RgbaImage::new(3, 2);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([u8::try_from(x * 10 + y).expect("small"), 0, 0, 255]);
        }
        let source = DynamicImage::ImageRgba8(source);
        for value in 1..=8 {
            let orientation = Orientation::from_exif(value).expect("valid");
            let mut ours = source.clone();
            if orientation.mirror {
                ours = ours.fliph();
            }
            for _ in 0..orientation.turns {
                ours = ours.rotate90();
            }
            let mut theirs = source.clone();
            theirs.apply_orientation(orientation.to_image());
            assert_eq!(ours.dimensions(), theirs.dimensions(), "{value}");
            assert_eq!(
                ours.to_rgba8().into_raw(),
                theirs.to_rgba8().into_raw(),
                "{value}"
            );
        }
    }

    #[test]
    fn reads_and_patches_the_orientation() {
        let mut chunk = orientation_chunk(6);
        assert_eq!(orientation(&chunk), Some(6));
        assert!(set_orientation(&mut chunk, 3));
        assert_eq!(orientation(&chunk), Some(3));
        let mut empty = build(&[(tag::MAKE, 2, 4, b"Foo\0".to_vec())]);
        assert!(!set_orientation(&mut empty, 1));
    }

    #[test]
    fn summarizes_camera_fields() {
        let exif = build(&[
            (
                tag::DATE_TIME_ORIGINAL,
                2,
                20,
                b"2024:05:06 07:08:09\0".to_vec(),
            ),
            (tag::ISO, 3, 1, vec![0, 200]),
            (tag::F_NUMBER, 5, 1, vec![0, 0, 0, 28, 0, 0, 0, 10]),
            (tag::EXPOSURE_TIME, 5, 1, vec![0, 0, 0, 1, 0, 0, 0, 250]),
        ]);
        // IFD0 pointing at an EXIF directory stored at the end of the file.
        let ifd0 = |exif_at: u32| {
            build(&[
                (tag::MAKE, 2, 6, b"NIKON\0".to_vec()),
                (tag::MODEL, 2, 11, b"NIKON Z 6\0\0".to_vec()),
                (tag::EXIF_IFD, 4, 1, exif_at.to_be_bytes().to_vec()),
            ])
        };
        let exif_at = u32::try_from(ifd0(0).len()).expect("small");
        let mut file = ifd0(exif_at);
        // Re-base the EXIF directory's out-of-line values onto the new place.
        let directory = rebase(&exif[8..], exif_at as usize - 8);
        file.extend_from_slice(&directory);
        let summary = ExifSummary::read(&file).expect("summary");
        assert_eq!(summary.camera().as_deref(), Some("NIKON Z 6"));
        assert_eq!(summary.iso, Some(200));
        assert_eq!(summary.aperture.as_deref(), Some("f/2.8"));
        assert_eq!(summary.exposure.as_deref(), Some("1/250 s"));
        assert_eq!(
            summary.taken.map(|t| t.to_string()).as_deref(),
            Some("2024-05-06 07:08:09")
        );
    }

    /// Shifts the out-of-line offsets of a built directory by `delta`.
    fn rebase(directory: &[u8], delta: usize) -> Vec<u8> {
        let mut out = directory.to_vec();
        let count = usize::from(u16::from_be_bytes([out[0], out[1]]));
        for index in 0..count {
            let base = 2 + 12 * index;
            let kind = u16::from_be_bytes([out[base + 2], out[base + 3]]);
            let n = u32::from_be_bytes(out[base + 4..base + 8].try_into().expect("4")) as usize;
            let size = match kind {
                2 => n,
                3 => 2 * n,
                5 => 8 * n,
                _ => 4 * n,
            };
            if size > 4 {
                let at =
                    u32::from_be_bytes(out[base + 8..base + 12].try_into().expect("4")) as usize;
                let moved = u32::try_from(at + delta).expect("small");
                out[base + 8..base + 12].copy_from_slice(&moved.to_be_bytes());
            }
        }
        out
    }

    #[test]
    fn stamps_parse_and_convert() {
        assert_eq!(
            Stamp::parse("2023:12:31 23:59:58")
                .map(|s| s.date())
                .as_deref(),
            Some("2023-12-31")
        );
        assert_eq!(Stamp::parse("garbage"), None);
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let stamp = Stamp::from_system_time(time).expect("stamp");
        assert_eq!(stamp.to_string(), "2023-11-14 22:13:20");
    }
}
