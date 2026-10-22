//! A bounds-checked reader of TIFF structures: EXIF chunks and the TIFF-based
//! camera RAW files. Every offset comes from the file and is checked; a
//! malformed structure yields `None`, never a panic.

/// Tags this crate reads.
pub(crate) mod tag {
    pub(crate) const COMPRESSION: u16 = 0x0103;
    pub(crate) const PHOTOMETRIC: u16 = 0x0106;
    pub(crate) const MAKE: u16 = 0x010F;
    pub(crate) const MODEL: u16 = 0x0110;
    pub(crate) const STRIP_OFFSETS: u16 = 0x0111;
    pub(crate) const ORIENTATION: u16 = 0x0112;
    pub(crate) const STRIP_BYTE_COUNTS: u16 = 0x0117;
    pub(crate) const SOFTWARE: u16 = 0x0131;
    pub(crate) const DATE_TIME: u16 = 0x0132;
    pub(crate) const SUB_IFDS: u16 = 0x014A;
    pub(crate) const JPEG_OFFSET: u16 = 0x0201;
    pub(crate) const JPEG_LENGTH: u16 = 0x0202;
    pub(crate) const EXPOSURE_TIME: u16 = 0x829A;
    pub(crate) const F_NUMBER: u16 = 0x829D;
    pub(crate) const EXIF_IFD: u16 = 0x8769;
    pub(crate) const GPS_IFD: u16 = 0x8825;
    pub(crate) const ISO: u16 = 0x8827;
    pub(crate) const DATE_TIME_ORIGINAL: u16 = 0x9003;
    pub(crate) const FOCAL_LENGTH: u16 = 0x920A;
    pub(crate) const LENS_MODEL: u16 = 0xA434;
}

/// Most entries a directory may declare before it is taken for garbage.
const MAX_ENTRIES: usize = 4096;

/// A TIFF structure over borrowed bytes.
#[derive(Clone, Copy)]
pub(crate) struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

/// One directory entry. `at` is where the value's bytes begin (inline or not),
/// already checked to lie inside the data.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub(crate) tag: u16,
    kind: u16,
    pub(crate) count: u32,
    at: usize,
}

impl<'a> Tiff<'a> {
    /// Reads the header. Accepts the variants of the magic number that
    /// Olympus (`IIRO`, `IIRS`) and Panasonic (`IIU\0`) RAW files use.
    pub(crate) fn new(data: &'a [u8]) -> Option<Self> {
        let little = match data.get(..4)? {
            [b'I', b'I', 42 | b'U', 0] | [b'I', b'I', b'R', b'O' | b'S'] => true,
            [b'M', b'M', 0, 42] => false,
            _ => return None,
        };
        Some(Self { data, little })
    }

    /// The bytes the structure was read from.
    pub(crate) fn data(&self) -> &'a [u8] {
        self.data
    }

    pub(crate) fn u16_at(&self, at: usize) -> Option<u16> {
        let bytes: [u8; 2] = self.data.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    pub(crate) fn u32_at(&self, at: usize) -> Option<u32> {
        let bytes: [u8; 4] = self.data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    /// Offset of the first directory.
    pub(crate) fn first_ifd(&self) -> Option<usize> {
        usize::try_from(self.u32_at(4)?).ok()
    }

    /// The entries of the directory at `ifd`; entries whose values fall
    /// outside the data are dropped.
    pub(crate) fn entries(&self, ifd: usize) -> Option<Vec<Entry>> {
        let count = usize::from(self.u16_at(ifd)?);
        if count > MAX_ENTRIES {
            return None;
        }
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let base = ifd.checked_add(2 + 12 * index)?;
            let tag = self.u16_at(base)?;
            let kind = self.u16_at(base + 2)?;
            let count = self.u32_at(base + 4)?;
            let Some(size) = type_size(kind).and_then(|size| size.checked_mul(count as usize))
            else {
                continue;
            };
            let at = if size <= 4 {
                base + 8
            } else {
                match self
                    .u32_at(base + 8)
                    .and_then(|at| usize::try_from(at).ok())
                {
                    Some(at) => at,
                    None => continue,
                }
            };
            if at
                .checked_add(size)
                .is_some_and(|end| end <= self.data.len())
            {
                entries.push(Entry {
                    tag,
                    kind,
                    count,
                    at,
                });
            }
        }
        Some(entries)
    }

    /// Offset of the directory that follows the one at `ifd`, if any.
    pub(crate) fn next_ifd(&self, ifd: usize) -> Option<usize> {
        let count = usize::from(self.u16_at(ifd)?);
        let next = self.u32_at(ifd.checked_add(2 + 12 * count)?)?;
        usize::try_from(next).ok().filter(|&next| next != 0)
    }

    /// The `index`-th value of an integer entry.
    pub(crate) fn uint(&self, entry: &Entry, index: u32) -> Option<u32> {
        if index >= entry.count {
            return None;
        }
        let index = index as usize;
        match entry.kind {
            1 | 7 => self.data.get(entry.at + index).copied().map(u32::from),
            3 => self.u16_at(entry.at + 2 * index).map(u32::from),
            4 | 13 => self.u32_at(entry.at + 4 * index),
            _ => None,
        }
    }

    /// Where the value of a 16-bit entry is stored, for patching in place.
    pub(crate) fn short_position(entry: &Entry) -> Option<usize> {
        (entry.kind == 3 && entry.count >= 1).then_some(entry.at)
    }

    /// Text of an ASCII entry, without the terminator and surrounding blanks.
    pub(crate) fn ascii(&self, entry: &Entry) -> Option<String> {
        if entry.kind != 2 {
            return None;
        }
        let bytes = self.data.get(entry.at..entry.at + entry.count as usize)?;
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        let text = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    /// The first value of a rational entry as numerator and denominator.
    pub(crate) fn rational(&self, entry: &Entry) -> Option<(f64, f64)> {
        match entry.kind {
            5 => Some((
                f64::from(self.u32_at(entry.at)?),
                f64::from(self.u32_at(entry.at + 4)?),
            )),
            10 => Some((
                f64::from(self.u32_at(entry.at)?.cast_signed()),
                f64::from(self.u32_at(entry.at + 4)?.cast_signed()),
            )),
            _ => None,
        }
    }

    /// Every directory reachable from the first one: the chain, sub-directories
    /// and the EXIF directory. Cycles and runaway depth are cut off.
    pub(crate) fn all_ifds(&self) -> Vec<usize> {
        let mut seen = Vec::new();
        let mut pending: Vec<usize> = self.first_ifd().into_iter().collect();
        while let Some(ifd) = pending.pop() {
            if seen.contains(&ifd) || seen.len() >= 64 {
                continue;
            }
            seen.push(ifd);
            if let Some(next) = self.next_ifd(ifd) {
                pending.push(next);
            }
            for entry in self.entries(ifd).unwrap_or_default() {
                if entry.tag == tag::SUB_IFDS || entry.tag == tag::EXIF_IFD {
                    for index in 0..entry.count.min(16) {
                        if let Some(child) = self.uint(&entry, index) {
                            pending.push(child as usize);
                        }
                    }
                }
            }
        }
        seen
    }
}

/// Finds the entry with `tag`.
pub(crate) fn find(entries: &[Entry], tag: u16) -> Option<&Entry> {
    entries.iter().find(|entry| entry.tag == tag)
}

fn type_size(kind: u16) -> Option<usize> {
    match kind {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 | 13 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

/// Builds a little TIFF for tests: big-endian, one directory of `(tag, kind,
/// count, value-bytes)` entries, values longer than four bytes appended.
#[cfg(test)]
pub(crate) fn build(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"MM\0\x2A\0\0\0\x08".to_vec();
    let count = u16::try_from(entries.len()).expect("few entries");
    out.extend_from_slice(&count.to_be_bytes());
    let mut extra_at = 8 + 2 + 12 * entries.len() + 4;
    let mut extra = Vec::new();
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        if value.len() <= 4 {
            let mut inline = value.clone();
            inline.resize(4, 0);
            out.extend_from_slice(&inline);
        } else {
            let at = u32::try_from(extra_at).expect("small test file");
            out.extend_from_slice(&at.to_be_bytes());
            extra.extend_from_slice(value);
            extra_at += value.len();
        }
    }
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&extra);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_of_every_width() {
        let data = build(&[
            (tag::ORIENTATION, 3, 1, vec![0, 6]),
            (tag::MAKE, 2, 6, b"Canon\0".to_vec()),
            (tag::EXPOSURE_TIME, 5, 1, vec![0, 0, 0, 1, 0, 0, 0, 125]),
        ]);
        let tiff = Tiff::new(&data).expect("header");
        let entries = tiff
            .entries(tiff.first_ifd().expect("ifd"))
            .expect("entries");
        let orientation = find(&entries, tag::ORIENTATION).expect("orientation");
        assert_eq!(tiff.uint(orientation, 0), Some(6));
        assert_eq!(tiff.uint(orientation, 1), None);
        let make = find(&entries, tag::MAKE).expect("make");
        assert_eq!(tiff.ascii(make).as_deref(), Some("Canon"));
        let exposure = find(&entries, tag::EXPOSURE_TIME).expect("exposure");
        assert_eq!(tiff.rational(exposure), Some((1.0, 125.0)));
    }

    #[test]
    fn garbage_is_rejected_without_panicking() {
        assert!(Tiff::new(b"MM").is_none());
        let mut data = build(&[(tag::MAKE, 2, 200, vec![0; 200])]);
        data.truncate(40);
        let tiff = Tiff::new(&data).expect("header");
        let entries = tiff.entries(8).expect("directory");
        assert!(entries.is_empty(), "value outside the data is dropped");
        assert!(tiff.entries(1_000_000).is_none());
        // A directory pointing at itself is visited once.
        let looped = build(&[(tag::EXIF_IFD, 4, 1, vec![0, 0, 0, 8])]);
        assert_eq!(Tiff::new(&looped).expect("header").all_ifds(), vec![8]);
    }
}
