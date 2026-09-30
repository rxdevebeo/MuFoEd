//! Minimal, auditable ZIP reader built directly on `miniz_oxide`.
//!
//! Implements manual EOCD / ZIP64 / central-directory / local-header parsing
//! with exact control over resource limits before any decompression starts
//! (ADR-0001, stage tasks S1.3–S1.5). The archive bytes are held in memory; the
//! *decompressed* parts are produced on demand by `PartReader`, never eagerly.
//!
//! Nothing in this module panics on malformed input: every failure is a
//! [`StrictError`].
//!
//! The counterpart [`write`] module serializes a package back to bytes
//! deterministically (`STAGE-8-TASK.md` §3, W5).

pub mod write;

use std::collections::HashMap;
use std::io::{self, Read};
use std::sync::Arc;

use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

use crate::error::{LimitKind, Result, StrictError};
use crate::limits::ResourceLimits;
use crate::opc::path::canonicalize_part_name;
use crate::part::{Compression, PartId};

const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const ZIP64_LOCATOR_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
const ZIP64_EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const CENTRAL_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const LOCAL_SIG: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

/// A parsed entry of the ZIP central directory.
#[derive(Clone, Debug)]
pub(crate) struct ZipEntry {
    pub(crate) id: PartId,
    pub(crate) compression: Compression,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
    pub(crate) local_header_offset: u64,
    pub(crate) crc32: u32,
}

/// A parsed ZIP archive held entirely in memory.
#[derive(Clone, Debug)]
pub(crate) struct ZipArchive {
    data: Arc<Vec<u8>>,
    entries: Vec<ZipEntry>,
    by_id: HashMap<PartId, usize>,
}

impl ZipArchive {
    /// Parses the archive and validates every resource limit.
    pub(crate) fn new(data: Arc<Vec<u8>>, limits: &ResourceLimits) -> Result<Self> {
        let directory = CentralDirectory::parse(data.as_slice(), limits)?;

        let mut entries = Vec::with_capacity(directory.entries.capacity());
        let mut by_id: HashMap<PartId, usize> = HashMap::with_capacity(directory.entries.len());
        let mut total_uncompressed: u64 = 0;

        for raw in directory.entries {
            if raw.name.ends_with('/') {
                // Directory placeholder entry; not a part.
                continue;
            }
            let id = canonicalize_part_name(&raw.name)?;
            if by_id.contains_key(&id) {
                return Err(StrictError::DuplicatePart(id));
            }
            if raw.uncompressed_size > limits.max_single_uncompressed {
                return Err(StrictError::LimitExceeded {
                    kind: LimitKind::SingleUncompressed,
                    limit: limits.max_single_uncompressed,
                    actual: raw.uncompressed_size,
                });
            }
            total_uncompressed = total_uncompressed
                .checked_add(raw.uncompressed_size)
                .ok_or_else(|| StrictError::InvalidZip("total size overflow".to_owned()))?;
            if total_uncompressed > limits.max_total_uncompressed {
                return Err(StrictError::LimitExceeded {
                    kind: LimitKind::TotalUncompressed,
                    limit: limits.max_total_uncompressed,
                    actual: total_uncompressed,
                });
            }
            // The compression ratio is a *secondary* heuristic: the absolute
            // limits above are the primary barrier, so a legitimate, highly
            // compressible document is not rejected merely for its ratio
            // (REWORK-CORE-1 C-1). `actual` reports the ratio itself, not bytes
            // (C-2).
            if raw.compression == Compression::Deflate {
                let ratio = raw
                    .uncompressed_size
                    .checked_div(raw.compressed_size)
                    .unwrap_or(u64::MAX);
                let ratio_exceeded = if raw.compressed_size == 0 {
                    raw.uncompressed_size > 0
                } else {
                    ratio > u64::from(limits.max_compression_ratio)
                };
                if ratio_exceeded {
                    return Err(StrictError::LimitExceeded {
                        kind: LimitKind::CompressionRatio,
                        limit: u64::from(limits.max_compression_ratio),
                        actual: ratio,
                    });
                }
            }
            by_id.insert(id.clone(), entries.len());
            entries.push(ZipEntry {
                id,
                compression: raw.compression,
                compressed_size: raw.compressed_size,
                uncompressed_size: raw.uncompressed_size,
                local_header_offset: raw.local_header_offset,
                crc32: raw.crc32,
            });
        }

        Ok(Self {
            data,
            entries,
            by_id,
        })
    }

    /// Returns the entry for a canonical part id.
    pub(crate) fn entry(&self, id: &PartId) -> Option<&ZipEntry> {
        self.by_id.get(id).and_then(|&idx| self.entries.get(idx))
    }

    /// Returns all parsed entries.
    pub(crate) fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Opens the entry's bytes as a streaming reader.
    pub(crate) fn open_reader(
        &self,
        id: &PartId,
        limits: &ResourceLimits,
    ) -> Result<PartReader<'_>> {
        let entry = self
            .entry(id)
            .ok_or_else(|| StrictError::MissingPart(id.clone()))?;
        let slice = self.local_data(entry)?;
        let inner = match entry.compression {
            Compression::Stored => {
                if slice.len() as u64 != entry.uncompressed_size {
                    return Err(StrictError::InvalidZip(format!(
                        "stored part size mismatch for {id}"
                    )));
                }
                Inner::Stored {
                    data: slice,
                    pos: 0,
                }
            }
            Compression::Deflate => Inner::Deflate {
                state: InflateState::new_boxed(DataFormat::Raw),
                input: slice,
                in_pos: 0,
                done: false,
            },
        };
        Ok(PartReader {
            inner,
            crc: 0,
            expected_crc: entry.crc32,
            expected_len: entry.uncompressed_size,
            produced: 0,
            max_len: limits.max_single_uncompressed,
            finished: false,
        })
    }

    /// Resolves the compressed byte range of an entry from its local header.
    fn local_data(&self, entry: &ZipEntry) -> Result<&[u8]> {
        let data: &[u8] = self.data.as_slice();
        let offset = usize::try_from(entry.local_header_offset)
            .map_err(|_| StrictError::InvalidZip("local header offset out of range".to_owned()))?;
        let sig = slice_at(data, offset, 4)?;
        if sig != LOCAL_SIG {
            return Err(StrictError::InvalidZip(
                "bad local file header signature".to_owned(),
            ));
        }
        let name_len = usize::from(u16_at(data, offset + 26)?);
        let extra_len = usize::from(u16_at(data, offset + 28)?);
        let data_start = offset
            .checked_add(30)
            .and_then(|v| v.checked_add(name_len))
            .and_then(|v| v.checked_add(extra_len))
            .ok_or_else(|| StrictError::InvalidZip("local header overflow".to_owned()))?;
        let size = usize::try_from(entry.compressed_size)
            .map_err(|_| StrictError::InvalidZip("part too large".to_owned()))?;
        let end = data_start
            .checked_add(size)
            .ok_or_else(|| StrictError::InvalidZip("part size overflow".to_owned()))?;
        data.get(data_start..end)
            .ok_or_else(|| StrictError::InvalidZip("part data out of bounds".to_owned()))
    }
}

/// Raw central-directory fields before limit validation.
struct RawEntry {
    name: String,
    compression: Compression,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    crc32: u32,
}

struct CentralDirectory {
    entries: Vec<RawEntry>,
}

impl CentralDirectory {
    #[allow(clippy::too_many_lines)]
    fn parse(data: &[u8], limits: &ResourceLimits) -> Result<Self> {
        let eocd = find_eocd(data)?;
        let mut entries_total = u64::from(u16_at(data, eocd + 10)?);
        let mut cd_size = u64::from(u32_at(data, eocd + 12)?);
        let mut cd_offset = u64::from(u32_at(data, eocd + 16)?);

        let needs_zip64 =
            entries_total == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF;
        if needs_zip64 {
            let (total, size, offset) = parse_zip64(data, eocd)?;
            entries_total = total;
            cd_size = size;
            cd_offset = offset;
        }

        if entries_total > limits.max_zip_entries as u64 {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::ZipEntries,
                limit: limits.max_zip_entries as u64,
                actual: entries_total,
            });
        }
        if entries_total > limits.max_parts as u64 {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::Parts,
                limit: limits.max_parts as u64,
                actual: entries_total,
            });
        }

        let cd_start = usize::try_from(cd_offset).map_err(|_| {
            StrictError::InvalidZip("central directory offset too large".to_owned())
        })?;
        let cd_len = usize::try_from(cd_size)
            .map_err(|_| StrictError::InvalidZip("central directory too large".to_owned()))?;
        let cd_end = cd_start
            .checked_add(cd_len)
            .ok_or_else(|| StrictError::InvalidZip("central directory overflow".to_owned()))?;
        if cd_end > data.len() {
            return Err(StrictError::InvalidZip(
                "central directory out of bounds".to_owned(),
            ));
        }

        let mut entries = Vec::with_capacity(usize::try_from(entries_total.min(4096)).unwrap_or(0));
        let mut pos = cd_start;
        for _ in 0..entries_total {
            if pos + 4 > data.len() || data[pos..pos + 4] != CENTRAL_SIG {
                return Err(StrictError::InvalidZip(
                    "bad central directory entry signature".to_owned(),
                ));
            }
            let flags = u16_at(data, pos + 8)?;
            let method = u16_at(data, pos + 10)?;
            let crc32 = u32_at(data, pos + 16)?;
            let mut compressed_size = u64::from(u32_at(data, pos + 20)?);
            let mut uncompressed_size = u64::from(u32_at(data, pos + 24)?);
            let name_len = usize::from(u16_at(data, pos + 28)?);
            let extra_len = usize::from(u16_at(data, pos + 30)?);
            let comment_len = usize::from(u16_at(data, pos + 32)?);
            let mut local_header_offset = u64::from(u32_at(data, pos + 42)?);

            let name_start = pos + 46;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or_else(|| StrictError::InvalidZip("name length overflow".to_owned()))?;
            let extra_end = name_end
                .checked_add(extra_len)
                .ok_or_else(|| StrictError::InvalidZip("extra length overflow".to_owned()))?;
            let next = extra_end
                .checked_add(comment_len)
                .ok_or_else(|| StrictError::InvalidZip("comment length overflow".to_owned()))?;
            if next > data.len() {
                return Err(StrictError::InvalidZip(
                    "central directory entry out of bounds".to_owned(),
                ));
            }

            let name_bytes = data
                .get(name_start..name_end)
                .ok_or_else(|| StrictError::InvalidZip("part name out of bounds".to_owned()))?;
            let name = std::str::from_utf8(name_bytes)
                .map_err(|_| {
                    StrictError::InvalidZip("non-UTF-8 part name (CP437 unsupported)".to_owned())
                })?
                .to_owned();

            let extra = data.get(name_end..extra_end).unwrap_or(&[]);
            let zip64 = Zip64Extra::parse(extra)?;
            if let Some(v) = zip64.uncompressed_size {
                uncompressed_size = v;
            }
            if let Some(v) = zip64.compressed_size {
                compressed_size = v;
            }
            if let Some(v) = zip64.local_header_offset {
                local_header_offset = v;
            }

            let compression = match method {
                METHOD_STORED => Compression::Stored,
                METHOD_DEFLATE => Compression::Deflate,
                other => return Err(StrictError::UnsupportedCompression(other)),
            };

            // Data descriptor bit (bit 3) is fine: authoritative sizes come from
            // the central directory, not the local header.
            let _ = flags;

            entries.push(RawEntry {
                name,
                compression,
                compressed_size,
                uncompressed_size,
                local_header_offset,
                crc32,
            });
            pos = next;
        }

        Ok(Self { entries })
    }
}

#[derive(Default)]
struct Zip64Extra {
    uncompressed_size: Option<u64>,
    compressed_size: Option<u64>,
    local_header_offset: Option<u64>,
}

impl Zip64Extra {
    /// Parses the `0x0001` extra field; field order is fixed by the spec.
    fn parse(extra: &[u8]) -> Result<Self> {
        let mut out = Self::default();
        let mut pos = 0;
        while pos + 4 <= extra.len() {
            let id = u16_at(extra, pos)?;
            let size = usize::from(u16_at(extra, pos + 2)?);
            let body_start = pos + 4;
            let body_end = body_start
                .checked_add(size)
                .ok_or_else(|| StrictError::InvalidZip("zip64 extra overflow".to_owned()))?;
            let body = extra
                .get(body_start..body_end)
                .ok_or_else(|| StrictError::InvalidZip("zip64 extra out of bounds".to_owned()))?;
            if id == 0x0001 {
                let mut p = 0;
                if body.len() >= p + 8 {
                    out.uncompressed_size = Some(u64_at(body, p)?);
                    p += 8;
                }
                if body.len() >= p + 8 {
                    out.compressed_size = Some(u64_at(body, p)?);
                    p += 8;
                }
                if body.len() >= p + 8 {
                    out.local_header_offset = Some(u64_at(body, p)?);
                }
            }
            pos = body_end;
        }
        Ok(out)
    }
}

/// Returns the absolute offset of the End-Of-Central-Directory record.
fn find_eocd(data: &[u8]) -> Result<usize> {
    if data.len() < 22 {
        return Err(StrictError::InvalidZip("archive too small".to_owned()));
    }
    let last = data.len() - 22;
    let first = data.len().saturating_sub(22 + 65_535);
    let mut end = last + 1;
    while end > first {
        let hay = &data[first..end];
        match memchr::memrchr(b'P', hay) {
            Some(p) => {
                let abs = first + p;
                if abs + 4 <= data.len() && data[abs..abs + 4] == EOCD_SIG {
                    return Ok(abs);
                }
                end = abs;
            }
            None => break,
        }
    }
    Err(StrictError::InvalidZip(
        "end of central directory not found".to_owned(),
    ))
}

/// Parses the ZIP64 end-of-central-directory record (total, size, offset).
fn parse_zip64(data: &[u8], eocd: usize) -> Result<(u64, u64, u64)> {
    let locator = eocd
        .checked_sub(20)
        .ok_or_else(|| StrictError::InvalidZip("ZIP64 locator missing".to_owned()))?;
    if slice_at(data, locator, 4)? != ZIP64_LOCATOR_SIG {
        return Err(StrictError::InvalidZip(
            "ZIP64 locator signature invalid".to_owned(),
        ));
    }
    let record_offset = u64_at(data, locator + 8)?;
    let record = usize::try_from(record_offset)
        .map_err(|_| StrictError::InvalidZip("ZIP64 record offset too large".to_owned()))?;
    if slice_at(data, record, 4)? != ZIP64_EOCD_SIG {
        return Err(StrictError::InvalidZip(
            "ZIP64 record signature invalid".to_owned(),
        ));
    }
    let total = u64_at(data, record + 32)?;
    let cd_size = u64_at(data, record + 40)?;
    let cd_offset = u64_at(data, record + 48)?;
    Ok((total, cd_size, cd_offset))
}

fn slice_at(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| StrictError::InvalidZip("offset overflow".to_owned()))?;
    data.get(offset..end)
        .ok_or_else(|| StrictError::InvalidZip(format!("truncated at offset {offset}")))
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    let s = slice_at(data, offset, 2)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let s = slice_at(data, offset, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(data: &[u8], offset: usize) -> Result<u64> {
    let s = slice_at(data, offset, 8)?;
    Ok(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

/// Streaming, CRC-checking reader for a single part.
pub(crate) struct PartReader<'a> {
    inner: Inner<'a>,
    crc: u32,
    expected_crc: u32,
    expected_len: u64,
    produced: u64,
    max_len: u64,
    finished: bool,
}

enum Inner<'a> {
    Stored {
        data: &'a [u8],
        pos: usize,
    },
    Deflate {
        state: Box<InflateState>,
        input: &'a [u8],
        in_pos: usize,
        done: bool,
    },
}

impl Inner<'_> {
    /// Fills `out` with up to `out.len()` bytes; returns 0 at end of stream.
    fn read_into(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match self {
            Inner::Stored { data, pos } => {
                let remaining = data.len().saturating_sub(*pos);
                let n = remaining.min(out.len());
                if n == 0 {
                    return Ok(0);
                }
                out[..n].copy_from_slice(&data[*pos..*pos + n]);
                *pos += n;
                Ok(n)
            }
            Inner::Deflate {
                state,
                input,
                in_pos,
                done,
            } => {
                if *done {
                    return Ok(0);
                }
                if out.is_empty() {
                    return Ok(0);
                }
                loop {
                    let result = inflate(state, &input[*in_pos..], out, MZFlush::None);
                    *in_pos += result.bytes_consumed;
                    match result.status {
                        Ok(MZStatus::StreamEnd) => *done = true,
                        Ok(_) => {}
                        Err(_) => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "corrupt DEFLATE stream",
                            ))
                        }
                    }
                    if result.bytes_written > 0 {
                        return Ok(result.bytes_written);
                    }
                    if *done {
                        return Ok(0);
                    }
                    if result.bytes_consumed == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "truncated DEFLATE stream",
                        ));
                    }
                }
            }
        }
    }
}

impl Read for PartReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.finished || buf.is_empty() {
            return Ok(0);
        }
        let remaining = self.expected_len.saturating_sub(self.produced);
        if remaining == 0 {
            return self.finish();
        }
        let want = buf
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let n = self.inner.read_into(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "part shorter than declared size",
            ));
        }
        self.crc = crc32_update(self.crc, &buf[..n]);
        self.produced += n as u64;
        if self.produced > self.max_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "part exceeds max_single_uncompressed",
            ));
        }
        Ok(n)
    }
}

impl PartReader<'_> {
    fn finish(&mut self) -> io::Result<usize> {
        self.finished = true;
        if self.produced != self.expected_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "part length mismatch",
            ));
        }
        if self.crc != self.expected_crc {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "part CRC-32 mismatch",
            ));
        }
        Ok(0)
    }
}

/// Table-driven CRC-32 (IEEE 802.3) update.
///
/// Uses a 256-entry lookup table built at compile time; roughly 4-6x faster
/// than the bit-at-a-time variant previously used (rework P5).
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut crc = !crc;
    for &byte in data {
        let index = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC_TABLE[index];
    }
    !crc
}

/// CRC-32 (IEEE 802.3) lookup table, generated at compile time.
static CRC_TABLE: [u32; 256] = build_crc_table();

#[allow(clippy::cast_possible_truncation)]
const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_possible_truncation)]

    use super::{crc32_update, ZipArchive, EOCD_SIG};
    use crate::error::{LimitKind, StrictError};
    use crate::limits::ResourceLimits;
    use std::io::Read;
    use std::sync::Arc;

    /// Builds a minimal ZIP archive with stored and deflated entries.
    pub(crate) fn build_test_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        // (name, content, deflate?)
        use miniz_oxide::deflate::compress_to_vec;
        let mut local = Vec::new();
        let mut central = Vec::new();
        let mut offsets = Vec::new();
        for (name, content, deflate) in entries {
            let offset = local.len() as u32;
            offsets.push(offset);
            let (method, stored): (u16, Vec<u8>) = if *deflate {
                (8, compress_to_vec(content, 6))
            } else {
                (0, content.to_vec())
            };
            let crc = crc32_update(0, content);
            local.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            local.extend_from_slice(&20u16.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&method.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(&crc.to_le_bytes());
            local.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            local.extend_from_slice(&(content.len() as u32).to_le_bytes());
            local.extend_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(name.as_bytes());
            local.extend_from_slice(&stored);
        }
        let cd_offset = local.len() as u32;
        for ((name, content, deflate), offset) in entries.iter().zip(offsets) {
            let (method, stored_len): (u16, u32) = if *deflate {
                let v = compress_to_vec(content, 6);
                (8, v.len() as u32)
            } else {
                (0, content.len() as u32)
            };
            let crc = crc32_update(0, content);
            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&stored_len.to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_size = central.len() as u32;
        let mut out = local;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIG);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn reads_stored_and_deflated_parts() {
        let bytes = build_test_zip(&[
            ("a.txt", b"hello world", false),
            ("dir/b.txt", b"compressed content compressed content", true),
        ]);
        let archive = ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()).unwrap();
        assert_eq!(archive.entries().len(), 2);

        let a = crate::part::PartId::new("/a.txt");
        let mut reader = archive.open_reader(&a, &ResourceLimits::default()).unwrap();
        let mut s = String::new();
        reader.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello world");

        let b = crate::part::PartId::new("/dir/b.txt");
        let mut reader = archive.open_reader(&b, &ResourceLimits::default()).unwrap();
        let mut s = String::new();
        reader.read_to_string(&mut s).unwrap();
        assert_eq!(s, "compressed content compressed content");
    }

    #[test]
    fn detects_bad_signature() {
        let mut bytes = build_test_zip(&[("a.txt", b"x", false)]);
        let len = bytes.len();
        bytes[len - 22] = 0;
        assert!(matches!(
            ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()),
            Err(StrictError::InvalidZip(_))
        ));
    }

    #[test]
    fn unrelated_kind_names_are_accepted() {
        // Regression guard: the error model must stay usable.
        let _ = StrictError::InvalidPartName("x".to_owned());
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u64(out: &mut Vec<u8>, value: u64) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// Builds a one-entry ZIP64 archive (store), exercising the ZIP64 branches.
    fn build_zip64_stored(name: &str, content: &[u8]) -> Vec<u8> {
        let crc = crc32_update(0, content);
        let mut out = Vec::new();
        // Local header (32-bit sizes are unused by the reader).
        out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        push_u16(&mut out, 45);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u32(&mut out, crc);
        push_u32(&mut out, content.len() as u32);
        push_u32(&mut out, content.len() as u32);
        push_u16(&mut out, name.len() as u16);
        push_u16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(content);

        let cd_offset = out.len() as u64;
        let mut central = Vec::new();
        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        push_u16(&mut central, 45);
        push_u16(&mut central, 45);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, crc);
        push_u32(&mut central, 0xFFFF_FFFF);
        push_u32(&mut central, 0xFFFF_FFFF);
        push_u16(&mut central, name.len() as u16);
        push_u16(&mut central, 28);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, 0xFFFF_FFFF);
        central.extend_from_slice(name.as_bytes());
        push_u16(&mut central, 0x0001);
        push_u16(&mut central, 24);
        push_u64(&mut central, content.len() as u64);
        push_u64(&mut central, content.len() as u64);
        push_u64(&mut central, 0);
        let cd_size = central.len() as u64;
        out.extend_from_slice(&central);

        let zip64_eocd_offset = out.len() as u64;
        out.extend_from_slice(&[0x50, 0x4b, 0x06, 0x06]);
        push_u64(&mut out, 44);
        push_u16(&mut out, 45);
        push_u16(&mut out, 45);
        push_u32(&mut out, 0);
        push_u32(&mut out, 0);
        push_u64(&mut out, 1);
        push_u64(&mut out, 1);
        push_u64(&mut out, cd_size);
        push_u64(&mut out, cd_offset);
        // ZIP64 locator.
        out.extend_from_slice(&[0x50, 0x4b, 0x06, 0x07]);
        push_u32(&mut out, 0);
        push_u64(&mut out, zip64_eocd_offset);
        push_u32(&mut out, 1);
        // EOCD with ZIP64 sentinels.
        out.extend_from_slice(&EOCD_SIG);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0xFFFF);
        push_u16(&mut out, 0xFFFF);
        push_u32(&mut out, 0xFFFF_FFFF);
        push_u32(&mut out, 0xFFFF_FFFF);
        push_u16(&mut out, 0);
        out
    }

    /// Builds a one-entry archive with an arbitrary compression method.
    fn build_zip_with_method(name: &str, content: &[u8], method: u16) -> Vec<u8> {
        let crc = crc32_update(0, content);
        let mut out = Vec::new();
        out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        push_u16(&mut out, 20);
        push_u16(&mut out, 0);
        push_u16(&mut out, method);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u32(&mut out, crc);
        push_u32(&mut out, content.len() as u32);
        push_u32(&mut out, content.len() as u32);
        push_u16(&mut out, name.len() as u16);
        push_u16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(content);
        let cd_offset = out.len() as u32;
        let mut central = Vec::new();
        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0);
        push_u16(&mut central, method);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, crc);
        push_u32(&mut central, content.len() as u32);
        push_u32(&mut central, content.len() as u32);
        push_u16(&mut central, name.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, cd_offset);
        central.extend_from_slice(name.as_bytes());
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIG);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 1);
        push_u16(&mut out, 1);
        push_u32(&mut out, cd_size);
        push_u32(&mut out, cd_offset);
        push_u16(&mut out, 0);
        out
    }

    fn build_empty_zip() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&EOCD_SIG);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u32(&mut out, 0);
        push_u32(&mut out, 0);
        push_u16(&mut out, 0);
        out
    }

    #[test]
    fn parses_zip64_archive() {
        let bytes = build_zip64_stored("word/document.xml", b"hello zip64");
        let archive = ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()).unwrap();
        assert_eq!(archive.entries().len(), 1);
        let id = crate::part::PartId::new("/word/document.xml");
        let mut reader = archive
            .open_reader(&id, &ResourceLimits::default())
            .unwrap();
        let mut s = String::new();
        reader.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello zip64");
    }

    #[test]
    fn parses_empty_archive() {
        let archive =
            ZipArchive::new(Arc::new(build_empty_zip()), &ResourceLimits::default()).unwrap();
        assert!(archive.entries().is_empty());
    }

    #[test]
    fn rejects_truncated_archive() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello world", false)]);
        bytes.truncate(bytes.len() / 2);
        assert!(matches!(
            ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()),
            Err(StrictError::InvalidZip(_))
        ));
    }

    #[test]
    fn rejects_unsupported_compression() {
        let bytes = build_zip_with_method("a.txt", b"data", 99);
        assert!(matches!(
            ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()),
            Err(StrictError::UnsupportedCompression(99))
        ));
    }

    #[test]
    fn detects_bad_crc_on_read() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello world", false)]);
        // Corrupt one data byte (after the 30-byte local header + 5-byte name).
        bytes[35] ^= 0xFF;
        let archive = ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()).unwrap();
        let id = crate::part::PartId::new("/a.txt");
        let mut reader = archive
            .open_reader(&id, &ResourceLimits::default())
            .unwrap();
        let mut s = String::new();
        assert!(reader.read_to_string(&mut s).is_err());
    }

    /// Builds a one-entry deflate archive whose central directory **declares**
    /// arbitrary compressed/uncompressed sizes (the data is never read).
    fn build_declared_deflate_zip(name: &str, compressed: u32, uncompressed: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        push_u16(&mut out, 20);
        push_u16(&mut out, 0);
        push_u16(&mut out, 8);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u32(&mut out, 0);
        push_u32(&mut out, compressed);
        push_u32(&mut out, uncompressed);
        push_u16(&mut out, name.len() as u16);
        push_u16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend(std::iter::repeat_n(0u8, compressed as usize));
        let cd_offset = out.len() as u32;
        let mut central = Vec::new();
        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0);
        push_u16(&mut central, 8);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, compressed);
        push_u32(&mut central, uncompressed);
        push_u16(&mut central, name.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, cd_offset);
        central.extend_from_slice(name.as_bytes());
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIG);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 1);
        push_u16(&mut out, 1);
        push_u32(&mut out, cd_size);
        push_u32(&mut out, cd_offset);
        push_u16(&mut out, 0);
        out
    }

    #[test]
    fn default_ratio_allows_repetitive_but_legitimate_documents() {
        // 227:1 is exactly the legitimate stress-document ratio (REWORK-CORE-1).
        let bytes = build_declared_deflate_zip("doc.xml", 1_000, 227_000);
        assert!(ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()).is_ok());
    }

    #[test]
    fn compression_bomb_is_rejected_and_reports_the_ratio() {
        // Within the absolute limits but with a 100000:1 ratio.
        let bytes = build_declared_deflate_zip("bomb", 1_000, 100_000_000);
        let error = ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()).unwrap_err();
        match error {
            StrictError::LimitExceeded {
                kind: LimitKind::CompressionRatio,
                limit,
                actual,
            } => {
                assert_eq!(limit, 1000);
                assert_eq!(actual, 100_000, "actual must be the ratio, not bytes");
            }
            other => panic!("expected CompressionRatio, got {other:?}"),
        }
    }

    #[test]
    fn strict_ratio_override_rejects_a_legitimate_document() {
        let bytes = build_declared_deflate_zip("doc.xml", 1_000, 227_000);
        let limits = ResourceLimits {
            max_compression_ratio: 200,
            ..ResourceLimits::default()
        };
        match ZipArchive::new(Arc::new(bytes), &limits).unwrap_err() {
            StrictError::LimitExceeded {
                kind: LimitKind::CompressionRatio,
                actual,
                ..
            } => assert_eq!(actual, 227),
            other => panic!("expected CompressionRatio, got {other:?}"),
        }
    }
}
