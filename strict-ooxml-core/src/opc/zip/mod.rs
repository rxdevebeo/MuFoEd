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
//! The counterpart [`write`](mod@write) module serializes a package back to bytes
//! deterministically (`STAGE-8-TASK.md` §3, W5).

#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

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

/// General-purpose flag bits that mean the entry is encrypted: bit 0
/// (traditional `PKWARE` encryption) and bit 6 (strong encryption).
const FLAGS_ENCRYPTED: u16 = 0x0001 | 0x0040;

/// A parsed entry of the ZIP central directory.
#[derive(Clone, Debug)]
pub(crate) struct ZipEntry {
    pub(crate) id: PartId,
    pub(crate) compression: Compression,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
    pub(crate) crc32: u32,
    /// Where the entry's compressed bytes sit in the archive, resolved from
    /// its local header once, at open (`start..end`).
    data_start: usize,
    data_end: usize,
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
        // `local header offset..end of data` of every part, for the overlap
        // check after the loop.
        let mut spans: Vec<(usize, usize)> = Vec::with_capacity(directory.entries.len());

        for raw in directory.entries {
            if raw.name.ends_with('/') {
                // Directory placeholder entry; not a part.
                continue;
            }
            let id = canonicalize_part_name(&raw.name)?;
            if by_id.contains_key(&id) {
                return Err(StrictError::DuplicatePart(id));
            }
            // A stored entry is its own bytes: the two sizes are one size, and
            // an archive that declares two different ones has lied about at
            // least one of them (audit 3.11).
            if raw.compression == Compression::Stored
                && raw.compressed_size != raw.uncompressed_size
            {
                return Err(StrictError::InvalidZip(format!(
                    "stored entry {} declares compressed size {} but uncompressed size {}",
                    raw.name, raw.compressed_size, raw.uncompressed_size
                )));
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
                // Compared by multiplication, not by an integer quotient: 2001
                // bytes out of 2 is over a 1000:1 limit although `2001 / 2`
                // is 1000 (audit 3.14). Saturation pins the bound at
                // `u64::MAX`, which no declared size exceeds — correct, since
                // the true bound is larger still. A zero compressed size makes
                // the bound 0, so any output at all exceeds it, as before.
                let ratio_exceeded = raw.uncompressed_size
                    > raw
                        .compressed_size
                        .saturating_mul(u64::from(limits.max_compression_ratio));
                if ratio_exceeded {
                    // Rounded up, so the reported ratio is above the limit
                    // whenever the check is.
                    let ratio = if raw.compressed_size == 0 {
                        u64::MAX
                    } else {
                        raw.uncompressed_size.div_ceil(raw.compressed_size)
                    };
                    return Err(StrictError::LimitExceeded {
                        kind: LimitKind::CompressionRatio,
                        limit: u64::from(limits.max_compression_ratio),
                        actual: ratio,
                    });
                }
            }
            let (data_start, data_end) = local_data_range(data.as_slice(), &raw)?;
            let header_start = usize::try_from(raw.local_header_offset).map_err(|_| {
                StrictError::InvalidZip("local header offset out of range".to_owned())
            })?;
            spans.push((header_start, data_end));
            by_id.insert(id.clone(), entries.len());
            entries.push(ZipEntry {
                id,
                compression: raw.compression,
                compressed_size: raw.compressed_size,
                uncompressed_size: raw.uncompressed_size,
                crc32: raw.crc32,
                data_start,
                data_end,
            });
        }
        check_spans(spans, directory.cd_start)?;

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

    /// The compressed bytes of an entry, as resolved and checked at open.
    fn local_data(&self, entry: &ZipEntry) -> Result<&[u8]> {
        self.data
            .get(entry.data_start..entry.data_end)
            .ok_or_else(|| StrictError::InvalidZip("part data out of bounds".to_owned()))
    }
}

/// Resolves an entry's compressed byte range from its local header.
///
/// The local header must name the entry exactly as the central directory
/// does (audit 3.16): the two disagreeing is how one archive shows a different
/// file list to a reader that walks local headers than to one that reads the
/// directory, and this reader does not get to pick which story is true.
fn local_data_range(data: &[u8], raw: &RawEntry) -> Result<(usize, usize)> {
    let offset = usize::try_from(raw.local_header_offset)
        .map_err(|_| StrictError::InvalidZip("local header offset out of range".to_owned()))?;
    let sig = slice_at(data, offset, 4)?;
    if sig != LOCAL_SIG {
        return Err(StrictError::InvalidZip(
            "bad local file header signature".to_owned(),
        ));
    }
    let name_len = usize::from(u16_at(data, offset_add(offset, 26)?)?);
    let extra_len = usize::from(u16_at(data, offset_add(offset, 28)?)?);
    let local_name = slice_at(data, offset_add(offset, 30)?, name_len)?;
    if local_name != raw.name.as_bytes() {
        return Err(StrictError::InvalidZip(format!(
            "local header name mismatch for {}",
            raw.name
        )));
    }
    let data_start = offset
        .checked_add(30)
        .and_then(|v| v.checked_add(name_len))
        .and_then(|v| v.checked_add(extra_len))
        .ok_or_else(|| StrictError::InvalidZip("local header overflow".to_owned()))?;
    let size = usize::try_from(raw.compressed_size)
        .map_err(|_| StrictError::InvalidZip("part too large".to_owned()))?;
    let end = data_start
        .checked_add(size)
        .ok_or_else(|| StrictError::InvalidZip("part size overflow".to_owned()))?;
    if end > data.len() {
        return Err(StrictError::InvalidZip(
            "part data out of bounds".to_owned(),
        ));
    }
    Ok((data_start, end))
}

/// Rejects entries whose bytes overlap, or run into the central directory
/// (audit 3.10).
///
/// Each span is `local header offset..end of compressed data`. Two parts that
/// share bytes are the classic overlapping-entry bomb — one small deflate
/// stream referenced a thousand times — and a part that reaches into the
/// directory is reading the directory as its own data; neither is a ZIP any
/// writer produces.
fn check_spans(mut spans: Vec<(usize, usize)>, cd_start: usize) -> Result<()> {
    spans.sort_unstable();
    let mut previous_end: usize = 0;
    for (start, end) in spans {
        if start < previous_end {
            return Err(StrictError::InvalidZip(format!(
                "entry at offset {start} overlaps the entry before it"
            )));
        }
        if end > cd_start {
            return Err(StrictError::InvalidZip(format!(
                "entry at offset {start} extends into the central directory"
            )));
        }
        previous_end = end;
    }
    Ok(())
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
    /// Offset of the first central-directory byte: no entry data may reach it.
    cd_start: usize,
}

impl CentralDirectory {
    #[allow(clippy::too_many_lines)]
    fn parse(data: &[u8], limits: &ResourceLimits) -> Result<Self> {
        let eocd = find_eocd(data)?;
        let mut entries_total = u64::from(u16_at(data, offset_add(eocd, 10)?)?);
        let mut cd_size = u64::from(u32_at(data, offset_add(eocd, 12)?)?);
        let mut cd_offset = u64::from(u32_at(data, offset_add(eocd, 16)?)?);

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
            let sig = pos
                .checked_add(4)
                .and_then(|sig_end| data.get(pos..sig_end));
            if sig != Some(CENTRAL_SIG.as_slice()) {
                return Err(StrictError::InvalidZip(
                    "bad central directory entry signature".to_owned(),
                ));
            }
            let flags = u16_at(data, offset_add(pos, 8)?)?;
            let method = u16_at(data, offset_add(pos, 10)?)?;
            let crc32 = u32_at(data, offset_add(pos, 16)?)?;
            let mut compressed_size = u64::from(u32_at(data, offset_add(pos, 20)?)?);
            let mut uncompressed_size = u64::from(u32_at(data, offset_add(pos, 24)?)?);
            let name_len = usize::from(u16_at(data, offset_add(pos, 28)?)?);
            let extra_len = usize::from(u16_at(data, offset_add(pos, 30)?)?);
            let comment_len = usize::from(u16_at(data, offset_add(pos, 32)?)?);
            let mut local_header_offset = u64::from(u32_at(data, offset_add(pos, 42)?)?);

            let name_start = offset_add(pos, 46)?;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or_else(|| StrictError::InvalidZip("name length overflow".to_owned()))?;
            let extra_end = name_end
                .checked_add(extra_len)
                .ok_or_else(|| StrictError::InvalidZip("extra length overflow".to_owned()))?;
            let next = extra_end
                .checked_add(comment_len)
                .ok_or_else(|| StrictError::InvalidZip("comment length overflow".to_owned()))?;
            // Bounded by the directory's *declared* end, not the archive's:
            // an entry that spills past it is reading the end record (or
            // whatever follows) as its own name and extra fields (audit 3.9).
            if next > cd_end {
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
            let zip64 = Zip64Extra::parse(
                extra,
                Zip64Fields {
                    uncompressed_size: uncompressed_size == u64::from(u32::MAX),
                    compressed_size: compressed_size == u64::from(u32::MAX),
                    local_header_offset: local_header_offset == u64::from(u32::MAX),
                },
            )?;
            if let Some(v) = zip64.uncompressed_size {
                uncompressed_size = v;
            }
            if let Some(v) = zip64.compressed_size {
                compressed_size = v;
            }
            if let Some(v) = zip64.local_header_offset {
                local_header_offset = v;
            }

            // Data descriptor bit (bit 3) is fine: authoritative sizes come from
            // the central directory, not the local header. Encryption is not:
            // the bytes would inflate to noise, or to a CRC error that names
            // the wrong cause (hostile 2.10).
            if flags & FLAGS_ENCRYPTED != 0 {
                return Err(StrictError::InvalidZip(format!(
                    "encrypted entry {name} is not supported"
                )));
            }

            let compression = match method {
                METHOD_STORED => Compression::Stored,
                METHOD_DEFLATE => Compression::Deflate,
                other => return Err(StrictError::UnsupportedCompression(other)),
            };

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

        Ok(Self { entries, cd_start })
    }
}

#[derive(Default)]
struct Zip64Extra {
    uncompressed_size: Option<u64>,
    compressed_size: Option<u64>,
    local_header_offset: Option<u64>,
}

/// Which 32-bit central-directory fields hold the `0xFFFFFFFF` sentinel.
#[derive(Clone, Copy)]
struct Zip64Fields {
    uncompressed_size: bool,
    compressed_size: bool,
    local_header_offset: bool,
}

impl Zip64Extra {
    /// Parses the `0x0001` extra field.
    ///
    /// The field carries **only** the values whose 32-bit header field is the
    /// `0xFFFFFFFF` sentinel, in the fixed order uncompressed size, compressed
    /// size, local header offset (APPNOTE 4.5.3). A writer that moves only the
    /// offset to ZIP64 writes an 8-byte body holding the offset, so reading the
    /// slots positionally would put it into the uncompressed size.
    fn parse(extra: &[u8], wanted: Zip64Fields) -> Result<Self> {
        let mut out = Self::default();
        let mut pos: usize = 0;
        while pos.checked_add(4).is_some_and(|end| end <= extra.len()) {
            let id = u16_at(extra, pos)?;
            let size = usize::from(u16_at(extra, offset_add(pos, 2)?)?);
            let body_start = offset_add(pos, 4)?;
            let body_end = body_start
                .checked_add(size)
                .ok_or_else(|| StrictError::InvalidZip("zip64 extra overflow".to_owned()))?;
            let body = extra
                .get(body_start..body_end)
                .ok_or_else(|| StrictError::InvalidZip("zip64 extra out of bounds".to_owned()))?;
            if id == 0x0001 {
                let mut p: usize = 0;
                let mut take = |wanted: bool| -> Result<Option<u64>> {
                    let end = match p.checked_add(8) {
                        Some(end) if wanted && end <= body.len() => end,
                        _ => return Ok(None),
                    };
                    let value = u64_at(body, p)?;
                    p = end;
                    Ok(Some(value))
                };
                out.uncompressed_size = take(wanted.uncompressed_size)?;
                out.compressed_size = take(wanted.compressed_size)?;
                out.local_header_offset = take(wanted.local_header_offset)?;
            }
            pos = body_end;
        }
        Ok(out)
    }
}

/// Returns the absolute offset of the End-Of-Central-Directory record.
fn find_eocd(data: &[u8]) -> Result<usize> {
    let last = data
        .len()
        .checked_sub(22)
        .ok_or_else(|| StrictError::InvalidZip("archive too small".to_owned()))?;
    let first = data.len().saturating_sub(22 + 65_535);
    let mut end = offset_add(last, 1)?;
    while end > first {
        let Some(hay) = data.get(first..end) else {
            break;
        };
        match memchr::memrchr(b'P', hay) {
            Some(p) => {
                let abs = offset_add(first, p)?;
                let sig = abs
                    .checked_add(4)
                    .and_then(|sig_end| data.get(abs..sig_end));
                if sig == Some(EOCD_SIG.as_slice()) {
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
    let record_offset = u64_at(data, offset_add(locator, 8)?)?;
    let record = usize::try_from(record_offset)
        .map_err(|_| StrictError::InvalidZip("ZIP64 record offset too large".to_owned()))?;
    if slice_at(data, record, 4)? != ZIP64_EOCD_SIG {
        return Err(StrictError::InvalidZip(
            "ZIP64 record signature invalid".to_owned(),
        ));
    }
    let total = u64_at(data, offset_add(record, 32)?)?;
    let cd_size = u64_at(data, offset_add(record, 40)?)?;
    let cd_offset = u64_at(data, offset_add(record, 48)?)?;
    Ok((total, cd_size, cd_offset))
}

/// `base + delta` as an archive offset; overflow means a malformed archive.
fn offset_add(base: usize, delta: usize) -> Result<usize> {
    base.checked_add(delta)
        .ok_or_else(|| StrictError::InvalidZip("offset overflow".to_owned()))
}

fn slice_at(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| StrictError::InvalidZip("offset overflow".to_owned()))?;
    data.get(offset..end)
        .ok_or_else(|| StrictError::InvalidZip(format!("truncated at offset {offset}")))
}

/// The `N` bytes at `offset`, as an array.
fn array_at<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    let bytes = slice_at(data, offset, N)?;
    <[u8; N]>::try_from(bytes)
        .map_err(|_| StrictError::InvalidZip(format!("truncated at offset {offset}")))
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(array_at(data, offset)?))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(array_at(data, offset)?))
}

fn u64_at(data: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(array_at(data, offset)?))
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
                let remaining = data.get(*pos..).unwrap_or_default();
                let n = remaining.len().min(out.len());
                if n == 0 {
                    return Ok(0);
                }
                let (Some(target), Some(source)) = (out.get_mut(..n), remaining.get(..n)) else {
                    return Ok(0);
                };
                target.copy_from_slice(source);
                // `pos + n <= data.len()`: `n` is bounded by the remaining slice.
                *pos = pos.saturating_add(n);
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
                    let pending = input.get(*in_pos..).unwrap_or_default();
                    let result = inflate(state, pending, out, MZFlush::None);
                    // Bounded by `input.len()`: inflate consumes at most `pending`.
                    *in_pos = in_pos.saturating_add(result.bytes_consumed);
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
        let window = buf.get_mut(..want).unwrap_or_default();
        let n = self.inner.read_into(window)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "part shorter than declared size",
            ));
        }
        self.crc = crc32_update(self.crc, window.get(..n).unwrap_or_default());
        // Bounded by `expected_len`; saturation keeps the limit check sound.
        self.produced = self.produced.saturating_add(n as u64);
        // Defence in depth only: `ZipArchive::new` already refuses an entry
        // whose declared size is over `max_single_uncompressed`, and reads stop
        // at that declared size, so this does not fire for an archive it built.
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
        // `index <= 0xFF < CRC_TABLE.len()`, so the fallback is dead (and elided).
        let entry = CRC_TABLE.get(index).copied().unwrap_or_default();
        crc = crc.wrapping_shr(8) ^ entry;
    }
    !crc
}

/// CRC-32 (IEEE 802.3) lookup table, generated at compile time.
static CRC_TABLE: [u32; 256] = build_crc_table();

#[allow(clippy::cast_possible_truncation)]
#[allow(
    clippy::indexing_slicing,
    reason = "`index < 256 == table.len()` by the loop condition; `get_mut` is not const"
)]
const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index: usize = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit: u32 = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                crc.wrapping_shr(1) ^ 0xEDB8_8320
            } else {
                crc.wrapping_shr(1)
            };
            bit = bit.wrapping_add(1);
        }
        table[index] = crc;
        index = index.wrapping_add(1);
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

    /// AUD-24: OPC part names are compared ASCII case-insensitively, so two
    /// ZIP entries differing only in case name the same part.
    #[test]
    fn case_variant_duplicate_parts_are_rejected() {
        let bytes = build_test_zip(&[
            ("word/document.xml", b"a", false),
            ("WORD/DOCUMENT.XML", b"b", false),
        ]);
        assert!(matches!(
            ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()),
            Err(StrictError::DuplicatePart(_))
        ));
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
        // The local header is at the start of the archive.
        push_u32(&mut central, 0);
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
        // The local header is at the start of the archive.
        push_u32(&mut central, 0);
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

    /// The archive offset of the central directory, read from the end record
    /// of an archive built by `build_test_zip` (no comment, no ZIP64).
    fn cd_offset_of(bytes: &[u8]) -> usize {
        let at = bytes.len() - 22 + 16;
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
    }

    fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn invalid_zip_message(bytes: Vec<u8>) -> String {
        match ZipArchive::new(Arc::new(bytes), &ResourceLimits::default()) {
            Err(StrictError::InvalidZip(message)) => message,
            other => panic!("expected InvalidZip, got {other:?}"),
        }
    }

    /// Audit 3.9: an entry is bounded by the directory's declared end, not by
    /// the end of the archive.
    #[test]
    fn a_directory_entry_past_the_declared_directory_end_is_rejected() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello", false)]);
        // One byte less of declared directory: the entry's name now spills
        // into the end record.
        let size_at = bytes.len() - 22 + 12;
        let size = u32::from_le_bytes(bytes[size_at..size_at + 4].try_into().unwrap());
        put_u32(&mut bytes, size_at, size - 1);
        let message = invalid_zip_message(bytes);
        assert!(message.contains("central directory entry"), "{message}");
    }

    /// Audit 3.11: a stored entry whose two sizes differ is refused at open,
    /// not when (or if) the part is read.
    #[test]
    fn a_stored_entry_with_two_different_sizes_is_rejected_at_open() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello", false)]);
        let cd = cd_offset_of(&bytes);
        put_u32(&mut bytes, cd + 20, 4); // compressed size
        let message = invalid_zip_message(bytes);
        assert!(message.contains("stored entry a.txt"), "{message}");
    }

    /// Audit 3.14: the ratio is compared exactly, not through an integer
    /// quotient that rounds 1000.5:1 down to an allowed 1000:1.
    #[test]
    fn a_ratio_just_over_the_limit_is_not_rounded_down() {
        let limits = ResourceLimits {
            max_compression_ratio: 1000,
            ..ResourceLimits::default()
        };
        let exactly = build_declared_deflate_zip("doc.xml", 2, 2000);
        assert!(ZipArchive::new(Arc::new(exactly), &limits).is_ok());

        let over = build_declared_deflate_zip("doc.xml", 2, 2001);
        match ZipArchive::new(Arc::new(over), &limits).unwrap_err() {
            StrictError::LimitExceeded {
                kind: LimitKind::CompressionRatio,
                limit,
                actual,
            } => {
                assert_eq!(limit, 1000);
                assert_eq!(actual, 1001, "rounded up, so it reads as over the limit");
            }
            other => panic!("expected CompressionRatio, got {other:?}"),
        }

        // A zero compressed size with output is still over any limit.
        let zero = build_declared_deflate_zip("doc.xml", 0, 1);
        assert!(matches!(
            ZipArchive::new(Arc::new(zero), &limits),
            Err(StrictError::LimitExceeded {
                kind: LimitKind::CompressionRatio,
                actual: u64::MAX,
                ..
            })
        ));
    }

    /// Audit 3.16: the local header must name the entry the directory names.
    #[test]
    fn a_local_header_naming_another_file_is_rejected() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello", false)]);
        // The local name starts right after the 30-byte local header.
        bytes[30] = b'b';
        let message = invalid_zip_message(bytes);
        assert!(message.contains("local header name mismatch"), "{message}");
    }

    /// Audit 3.10: two entries may not share bytes.
    #[test]
    fn overlapping_entries_are_rejected() {
        // `a.txt` spans 0..40, `b.txt` 40..80. Declaring `a.txt` 45 bytes long
        // (both sizes, so it stays a consistent stored entry) makes it cover
        // all of `b.txt`.
        let mut bytes = build_test_zip(&[("a.txt", b"hello", false), ("b.txt", b"world", false)]);
        let cd = cd_offset_of(&bytes);
        assert_eq!(cd, 80);
        put_u32(&mut bytes, cd + 20, 45);
        put_u32(&mut bytes, cd + 24, 45);
        let message = invalid_zip_message(bytes);
        assert!(message.contains("overlaps"), "{message}");
    }

    /// Audit 3.10: an entry's data may not run into the central directory.
    #[test]
    fn an_entry_reaching_into_the_central_directory_is_rejected() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello", false)]);
        let cd = cd_offset_of(&bytes);
        put_u32(&mut bytes, cd + 20, 6);
        put_u32(&mut bytes, cd + 24, 6);
        let message = invalid_zip_message(bytes);
        assert!(message.contains("central directory"), "{message}");
    }

    /// Hostile 2.10: an encrypted entry is refused by name, not read as noise.
    #[test]
    fn an_encrypted_entry_is_rejected() {
        let mut bytes = build_test_zip(&[("a.txt", b"hello", false)]);
        let cd = cd_offset_of(&bytes);
        bytes[cd + 8] |= 0x01; // general-purpose flag bit 0
        let message = invalid_zip_message(bytes);
        assert!(message.contains("encrypted entry a.txt"), "{message}");
    }

    fn zip64_extra(values: &[u64]) -> Vec<u8> {
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x0001u16.to_le_bytes());
        extra.extend_from_slice(&((values.len() * 8) as u16).to_le_bytes());
        for value in values {
            extra.extend_from_slice(&value.to_le_bytes());
        }
        extra
    }

    /// APPNOTE 4.5.3: the ZIP64 extra holds only the sentinel fields, in order.
    #[test]
    fn zip64_extra_reads_only_the_fields_whose_header_value_is_the_sentinel() {
        use super::{Zip64Extra, Zip64Fields};
        let only_offset = Zip64Fields {
            uncompressed_size: false,
            compressed_size: false,
            local_header_offset: true,
        };
        let parsed = Zip64Extra::parse(&zip64_extra(&[0x1_0000_0000]), only_offset).expect("parse");
        assert_eq!(parsed.local_header_offset, Some(0x1_0000_0000));
        assert_eq!(parsed.uncompressed_size, None);
        assert_eq!(parsed.compressed_size, None);

        let only_compressed = Zip64Fields {
            uncompressed_size: false,
            compressed_size: true,
            local_header_offset: false,
        };
        let parsed = Zip64Extra::parse(&zip64_extra(&[7]), only_compressed).expect("parse");
        assert_eq!(parsed.compressed_size, Some(7));
        assert_eq!(parsed.uncompressed_size, None);

        let all = Zip64Fields {
            uncompressed_size: true,
            compressed_size: true,
            local_header_offset: true,
        };
        let parsed = Zip64Extra::parse(&zip64_extra(&[1, 2, 3]), all).expect("parse");
        assert_eq!(
            (
                parsed.uncompressed_size,
                parsed.compressed_size,
                parsed.local_header_offset
            ),
            (Some(1), Some(2), Some(3))
        );

        // A sentinel without its value in the extra stays unresolved rather than
        // borrowing a neighbour's slot.
        let parsed = Zip64Extra::parse(&zip64_extra(&[9]), all).expect("parse");
        assert_eq!(parsed.uncompressed_size, Some(9));
        assert_eq!(parsed.compressed_size, None);
        assert_eq!(parsed.local_header_offset, None);
    }
}
