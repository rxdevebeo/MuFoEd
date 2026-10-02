//! Deterministic ZIP writing (`STAGE-8-TASK.md` §3, W5).
//!
//! The reader in the parent module is deliberately paranoid; the writer is
//! deliberately boring: entries are stored in the order the caller adds them,
//! every timestamp is fixed, and no extra field is emitted unless the data
//! needs it. Two runs over the same parts therefore produce the same bytes,
//! which is what criterion SC-1 asserts.
//!
//! Scope limits, on purpose:
//!
//! - **no ZIP64.** A package that needs more than 4 GiB is rejected rather than
//!   silently truncated; `ResourceLimits` (`max_total_uncompressed`, default
//!   512 MiB) is well below that, so the rejection is a guard, not a limit the
//!   project can actually reach.
//! - **no data descriptors and no encryption.** Everything is known up front,
//!   so sizes and the CRC go straight into the local header.
//! - **stored or deflate only**, decided by whichever is smaller.

// Every 32-bit field below is written from a `usize` that has already been
// range-checked against the format limits: the entry count against `u16::MAX`,
// each size and offset against `u32::MAX`. The checks are the point; the casts
// only satisfy the type system afterwards.
#![allow(clippy::cast_possible_truncation)]

use crate::error::{LimitKind, Result, StrictError};
use crate::limits::ResourceLimits;
use crate::opc::path::canonicalize_part_name;
use crate::part::{Compression, PartId};

use super::{crc32_update, CENTRAL_SIG, EOCD_SIG, LOCAL_SIG, METHOD_DEFLATE, METHOD_STORED};

/// Fixed MS-DOS date: 1980-01-01, the earliest value the format can express.
///
/// Using the epoch instead of "now" is what makes the output reproducible; a
/// package with a real modification date would differ on every run.
const DOS_DATE_EPOCH: u16 = 0x0021;
/// Fixed MS-DOS time: 00:00:00.
const DOS_TIME_EPOCH: u16 = 0x0000;

/// The largest value a 32-bit ZIP size/offset field can hold.
const MAX_U32: usize = u32::MAX as usize;

/// Builds a ZIP archive in memory, deterministically.
#[derive(Clone, Debug)]
pub struct ZipWriter {
    entries: Vec<(String, Vec<u8>)>,
    limits: ResourceLimits,
    total: u64,
}

impl Default for ZipWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipWriter {
    /// Creates an empty writer with the default resource limits.
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(ResourceLimits::default())
    }

    /// Creates an empty writer with an explicit resource budget.
    #[must_use]
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            entries: Vec::new(),
            limits,
            total: 0,
        }
    }

    /// Returns the number of entries added so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The canonical names of the parts added so far, in the order added.
    ///
    /// For the writer's own accounting: "which parts did this write actually
    /// emit" is a question about the *result*, and several of them are
    /// conditional — `word/numbering.xml` is written only when the model carries
    /// a numbering table — so a list of the parts a write *might* emit cannot
    /// answer it. This is the list that did.
    #[must_use]
    pub fn part_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|(name, _)| format!("/{name}"))
            .collect()
    }

    /// Returns `true` when no entry has been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Appends a part.
    ///
    /// The part name is canonicalized, so `/word/document.xml` and
    /// `word/document.xml` are the same entry; adding the same part twice is an
    /// error rather than a silent overwrite, because a duplicated part makes
    /// the reader's view order-dependent.
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::InvalidPartName`] for a name that is not a safe
    /// canonical OPC path, [`StrictError::DuplicatePart`] for a repeated part
    /// and [`StrictError::LimitExceeded`] when the entry count, the single-part
    /// size or the total size exceeds the configured budget.
    pub fn add_part(&mut self, part: &PartId, data: impl Into<Vec<u8>>) -> Result<()> {
        let canonical = canonicalize_part_name(part.as_str().trim_start_matches('/'))?;
        let name = canonical.as_str().trim_start_matches('/').to_owned();
        if self.entries.iter().any(|(existing, _)| *existing == name) {
            return Err(StrictError::DuplicatePart(canonical));
        }
        if self.entries.len() >= self.limits.max_zip_entries {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::ZipEntries,
                limit: self.limits.max_zip_entries as u64,
                actual: self.entries.len() as u64 + 1,
            });
        }
        let data = data.into();
        if data.len() as u64 > self.limits.max_single_uncompressed {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::SingleUncompressed,
                limit: self.limits.max_single_uncompressed,
                actual: data.len() as u64,
            });
        }
        self.total += data.len() as u64;
        if self.total > self.limits.max_total_uncompressed {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::TotalUncompressed,
                limit: self.limits.max_total_uncompressed,
                actual: self.total,
            });
        }
        self.entries.push((name, data));
        Ok(())
    }

    /// Serializes the archive.
    ///
    /// # Errors
    ///
    /// Returns [`StrictError::InvalidZip`] when the result would need a 32-bit
    /// size or offset field to overflow, and [`StrictError::LimitExceeded`]
    /// when the compressed result exceeds `max_compressed_input`.
    pub fn finish(self) -> Result<Vec<u8>> {
        if self.entries.len() > u16::MAX as usize {
            return Err(StrictError::InvalidZip(
                "more than 65535 entries require ZIP64".to_owned(),
            ));
        }

        let mut local = Vec::new();
        let mut central = Vec::new();
        for (name, data) in &self.entries {
            let offset = local.len();
            if offset > MAX_U32 {
                return Err(StrictError::InvalidZip(
                    "archive exceeds the 32-bit ZIP offset range (ZIP64 required)".to_owned(),
                ));
            }
            let crc = crc32_update(0, data);
            let compressed = compress(data);
            if compressed.len() > MAX_U32 {
                return Err(StrictError::InvalidZip(
                    "entry exceeds the 32-bit ZIP size range (ZIP64 required)".to_owned(),
                ));
            }
            write_local(&mut local, name, crc, &compressed, data.len());
            write_central(
                &mut central,
                name,
                crc,
                &compressed,
                data.len(),
                offset as u32,
            );
        }

        let central_offset = local.len();
        if central_offset > MAX_U32 || central.len() > MAX_U32 {
            return Err(StrictError::InvalidZip(
                "archive exceeds the 32-bit ZIP range (ZIP64 required)".to_owned(),
            ));
        }
        local.extend_from_slice(&central);
        write_eocd(
            &mut local,
            self.entries.len(),
            central.len(),
            central_offset,
        );

        if local.len() as u64 > self.limits.max_compressed_input {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::CompressedInput,
                limit: self.limits.max_compressed_input,
                actual: local.len() as u64,
            });
        }
        Ok(local)
    }
}

/// Deflates `data`, falling back to `None` when storing is not smaller.
///
/// The choice only depends on the two lengths, so it stays deterministic.
fn compress(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if data.is_empty() {
        return std::borrow::Cow::Borrowed(data);
    }
    let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
    if deflated.len() < data.len() {
        std::borrow::Cow::Owned(deflated)
    } else {
        std::borrow::Cow::Borrowed(data)
    }
}

fn method_for(compressed: &[u8], uncompressed_len: usize) -> u16 {
    if compressed.len() == uncompressed_len {
        METHOD_STORED
    } else {
        METHOD_DEFLATE
    }
}

fn write_local(out: &mut Vec<u8>, name: &str, crc: u32, compressed: &[u8], uncompressed: usize) {
    push_sig(out, LOCAL_SIG); // local file header
    push_u16(out, 20); // version needed: 2.0 (deflate)
    push_u16(out, 0x0800); // flags: UTF-8 names
    push_u16(out, method_for(compressed, uncompressed));
    push_u16(out, DOS_TIME_EPOCH);
    push_u16(out, DOS_DATE_EPOCH);
    push_u32(out, crc);
    push_u32(out, compressed.len() as u32);
    push_u32(out, uncompressed as u32);
    push_u16(out, name.len() as u16);
    push_u16(out, 0); // extra field length
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(compressed);
}

fn write_central(
    out: &mut Vec<u8>,
    name: &str,
    crc: u32,
    compressed: &[u8],
    uncompressed: usize,
    offset: u32,
) {
    push_sig(out, CENTRAL_SIG); // central directory header
    push_u16(out, 20); // version made by: 2.0, MS-DOS
    push_u16(out, 20); // version needed
    push_u16(out, 0x0800); // flags
    push_u16(out, method_for(compressed, uncompressed));
    push_u16(out, DOS_TIME_EPOCH);
    push_u16(out, DOS_DATE_EPOCH);
    push_u32(out, crc);
    push_u32(out, compressed.len() as u32);
    push_u32(out, uncompressed as u32);
    push_u16(out, name.len() as u16);
    push_u16(out, 0); // extra
    push_u16(out, 0); // comment
    push_u16(out, 0); // disk number start
    push_u16(out, 0); // internal attributes
    push_u32(out, 0); // external attributes
    push_u32(out, offset);
    out.extend_from_slice(name.as_bytes());
}

fn write_eocd(out: &mut Vec<u8>, entries: usize, central_size: usize, central_offset: usize) {
    push_sig(out, EOCD_SIG); // end of central directory
    push_u16(out, 0); // this disk
    push_u16(out, 0); // disk with the central directory
    push_u16(out, entries as u16);
    push_u16(out, entries as u16);
    push_u32(out, central_size as u32);
    push_u32(out, central_offset as u32);
    push_u16(out, 0); // comment length
}

fn push_sig(out: &mut Vec<u8>, signature: [u8; 4]) {
    out.extend_from_slice(&signature);
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The compression a writer would choose for `data`, exposed for the package
/// writer's own assertions.
#[must_use]
pub fn compression_for(data: &[u8]) -> Compression {
    match method_for(&compress(data), data.len()) {
        METHOD_STORED => Compression::Stored,
        _ => Compression::Deflate,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::Arc;

    use crate::limits::ResourceLimits;
    use crate::opc::{OpenOptions, Package};
    use crate::part::PartId;

    use super::ZipWriter;
    use crate::opc::ZipArchive;

    const CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

    const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    fn write(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new();
        for (name, data) in entries {
            writer
                .add_part(&PartId::new(format!("/{name}").as_str()), *data)
                .expect("add part");
        }
        writer.finish().expect("finish")
    }

    #[test]
    fn output_is_byte_identical_across_runs() {
        let entries: &[(&str, &[u8])] = &[
            ("[Content_Types].xml", b"<Types/>"),
            ("word/document.xml", b"<w:document/>"),
        ];
        assert_eq!(write(entries), write(entries));
    }

    #[test]
    fn a_duplicate_part_is_rejected() {
        let mut writer = ZipWriter::new();
        writer
            .add_part(&PartId::new("/word/document.xml"), b"a")
            .expect("first");
        // The same part with and without the leading slash is the same entry.
        let error = writer
            .add_part(&PartId::new("word/document.xml"), b"b")
            .expect_err("duplicate");
        assert!(error.to_string().contains("duplicate"), "{error}");
    }

    #[test]
    fn an_unsafe_part_name_is_rejected() {
        let mut writer = ZipWriter::new();
        let error = writer
            .add_part(&PartId::new("/word\\evil.xml"), b"a")
            .expect_err("traversal");
        assert!(error.to_string().contains("invalid part name"), "{error}");
    }

    #[test]
    fn entry_and_size_limits_are_enforced() {
        let limits = ResourceLimits {
            max_zip_entries: 1,
            max_single_uncompressed: 4,
            max_total_uncompressed: 8,
            ..ResourceLimits::default()
        };
        let mut writer = ZipWriter::with_limits(limits);
        writer
            .add_part(&PartId::new("/a.xml"), b"12345")
            .expect_err("single-part limit");
        writer
            .add_part(&PartId::new("/a.xml"), b"1234")
            .expect("fits");
        let error = writer
            .add_part(&PartId::new("/b.xml"), b"1")
            .expect_err("entry-count limit");
        assert!(error.to_string().contains("zip_entries"), "{error}");
    }

    #[test]
    fn an_empty_part_is_stored_and_readable() {
        let bytes = write(&[("word/empty.xml", b"")]);
        let archive = ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()).expect("read");
        let mut reader = archive
            .open_reader(&PartId::new("/word/empty.xml"), &ResourceLimits::default())
            .expect("entry");
        let mut out = Vec::new();
        reader.read_to_end(&mut out).expect("drain");
        assert!(out.is_empty());
    }

    #[test]
    fn a_written_package_reopens() {
        let bytes = write(&[
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", RELS),
            ("word/document.xml", b"<document/>"),
        ]);
        let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
        assert_eq!(
            package.main_document_part().expect("main part").as_str(),
            "/word/document.xml"
        );
        assert_eq!(
            package
                .read_part(&PartId::new("/word/document.xml"))
                .expect("part"),
            b"<document/>"
        );
    }

    #[test]
    fn compressible_content_is_deflated() {
        let payload = "a".repeat(4096);
        let bytes = write(&[("word/big.xml", payload.as_bytes())]);
        // The central directory records the deflate method (8) for a payload
        // that shrinks; the archive must be far smaller than the input.
        assert!(bytes.len() < payload.len() / 10, "{}", bytes.len());
        assert_eq!(&bytes[8..10], &[8, 0]);
    }
}
