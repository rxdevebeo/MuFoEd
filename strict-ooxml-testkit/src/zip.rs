//! A ZIP writer for test inputs.
//!
//! Deliberately its own code, CRC included: a package built with the reader's
//! own writer would agree with the reader by construction.

/// Compression method of one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Method 0, the bytes as they are.
    Stored,
    /// Method 8, raw DEFLATE.
    Deflated,
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    data: Vec<u8>,
    method: Method,
}

/// Builds a ZIP archive entry by entry, in insertion order.
#[derive(Debug, Clone)]
pub struct ZipBuilder {
    entries: Vec<Entry>,
    method: Method,
}

impl Default for ZipBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipBuilder {
    /// An empty archive whose entries are stored.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            method: Method::Stored,
        }
    }

    /// Sets the method for entries added afterwards with [`entry`](Self::entry).
    #[must_use]
    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    /// Adds an entry with the current default method. Names are not checked:
    /// duplicates, case variants and odd paths are what hostile tests need.
    #[must_use]
    pub fn entry(self, name: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        let method = self.method;
        self.entry_with(name, data, method)
    }

    /// Adds an entry with an explicit method.
    #[must_use]
    pub fn entry_with(
        mut self,
        name: impl Into<String>,
        data: impl Into<Vec<u8>>,
        method: Method,
    ) -> Self {
        self.entries.push(Entry {
            name: name.into(),
            data: data.into(),
            method,
        });
        self
    }

    /// Writes the archive.
    ///
    /// # Panics
    ///
    /// If a size, an offset or the entry count does not fit the classic
    /// (non-ZIP64) format. Test inputs never need ZIP64.
    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for entry in &self.entries {
            let offset = fit_u32(out.len(), "local header offset");
            let crc = crc32(&entry.data);
            let (method, payload) = match entry.method {
                Method::Stored => (0u16, entry.data.clone()),
                Method::Deflated => (8u16, miniz_oxide::deflate::compress_to_vec(&entry.data, 6)),
            };
            let compressed = fit_u32(payload.len(), "compressed size");
            let uncompressed = fit_u32(entry.data.len(), "uncompressed size");
            let name_len = fit_u16(entry.name.len(), "name length");

            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // time
            out.extend_from_slice(&0x0021u16.to_le_bytes()); // date: 1980-01-01
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&compressed.to_le_bytes());
            out.extend_from_slice(&uncompressed.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra length
            out.extend_from_slice(entry.name.as_bytes());
            out.extend_from_slice(&payload);

            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // time
            central.extend_from_slice(&0x0021u16.to_le_bytes()); // date
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&compressed.to_le_bytes());
            central.extend_from_slice(&uncompressed.to_le_bytes());
            central.extend_from_slice(&name_len.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra length
            central.extend_from_slice(&0u16.to_le_bytes()); // comment length
            central.extend_from_slice(&0u16.to_le_bytes()); // disk number
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(entry.name.as_bytes());
        }
        let cd_offset = fit_u32(out.len(), "central directory offset");
        let cd_size = fit_u32(central.len(), "central directory size");
        let count = fit_u16(self.entries.len(), "entry count");
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // this disk
        out.extend_from_slice(&0u16.to_le_bytes()); // disk with the directory
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment length
        out
    }
}

fn fit_u32(value: usize, what: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| panic!("{what} {value} needs ZIP64"))
}

fn fit_u16(value: usize, what: &str) -> u16 {
    u16::try_from(value).unwrap_or_else(|_| panic!("{what} {value} needs ZIP64"))
}

/// CRC-32 (IEEE 802.3, reflected, polynomial `0xEDB88320`), bit by bit.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
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

    #[test]
    fn crc32_matches_the_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn stored_archive_has_the_expected_layout() {
        let bytes = ZipBuilder::new().entry("a.txt", b"hello".to_vec()).build();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        assert_eq!(&bytes[30..35], b"a.txt");
        assert_eq!(&bytes[35..40], b"hello");
        let eocd = bytes.len() - 22;
        assert_eq!(&bytes[eocd..eocd + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]), 1);
    }

    #[test]
    fn deflated_entry_inflates_back() {
        let data = b"abcabcabcabcabcabcabcabc".repeat(10);
        let bytes = ZipBuilder::new()
            .entry_with("x", data.clone(), Method::Deflated)
            .build();
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 8);
        let size = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let start = 30 + 1;
        let end = start + usize::try_from(size).unwrap();
        let inflated = miniz_oxide::inflate::decompress_to_vec(&bytes[start..end]).unwrap();
        assert_eq!(inflated, data);
    }
}
