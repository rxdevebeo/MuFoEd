//! Archive parsing, volume handling and extraction.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::crypto::{
    decrypt_cbc, decrypt_cbc_128, derive_key, derive_key_rar3, Rar5Key, AES_BLOCK_SIZE,
};
use crate::error::{Error, Result};
use crate::format::*;
use crate::hash::DataHash;
use crate::unpack::Unpacker;

const UNPACK_MAX_DICT: u64 = 0x100_0000_0000;

enum VolumeSource {
    File(File),
    Memory(Vec<u8>),
}

impl VolumeSource {
    fn len(&self) -> Result<u64> {
        match self {
            VolumeSource::File(file) => Ok(file.metadata()?.len()),
            VolumeSource::Memory(data) => Ok(data.len() as u64),
        }
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        match self {
            VolumeSource::File(file) => {
                file.seek(SeekFrom::Start(offset))?;
                Ok(file.read(buf)?)
            }
            VolumeSource::Memory(data) => {
                let offset = offset as usize;
                if offset >= data.len() {
                    return Ok(0);
                }
                let n = buf.len().min(data.len() - offset);
                buf[..n].copy_from_slice(&data[offset..offset + n]);
                Ok(n)
            }
        }
    }

    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let mut done = 0;
        while done < buf.len() {
            let position = offset
                .checked_add(done as u64)
                .ok_or_else(|| Error::Corrupt("archive offset overflow".into()))?;
            let n = self.read_at(position, &mut buf[done..])?;
            if n == 0 {
                return Err(Error::Corrupt(format!(
                    "unexpected end of archive at offset {offset} ({} bytes requested)",
                    buf.len()
                )));
            }
            done += n;
        }
        Ok(())
    }
}

struct Volume {
    source: VolumeSource,
    len: u64,
}

impl Volume {
    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let source = VolumeSource::File(file);
        let len = source.len()?;
        Ok(Volume { source, len })
    }

    fn from_bytes(data: Vec<u8>) -> Self {
        let len = data.len() as u64;
        Volume {
            source: VolumeSource::Memory(data),
            len,
        }
    }
}

struct CryptHeaderInfo {
    salt: [u8; 16],
    lg2_count: u8,
    psw_check: Option<[u8; 8]>,
}

struct ParsedBlock {
    header_type: u8,
    flags: u64,
    data_size: u64,
    file: Option<Entry>,
    main_flags: Option<u64>,
    main_vol_number: Option<u64>,
    crypt: Option<CryptHeaderInfo>,
    endarc_flags: Option<u64>,
}

struct VolumeParse {
    blocks: Vec<ParsedBlock>,
    next_volume: bool,
    format: Format,
}

/// Options used when opening an archive.
#[derive(Clone, Debug)]
pub struct ArchiveOptions {
    /// Password used to decrypt headers and/or file data.
    pub password: Option<String>,
    /// Maximum number of leading bytes inspected for an embedded SFX
    /// signature. Defaults to [`DEFAULT_MAX_SFX_SCAN`] (4 MiB). Values below
    /// the 8-byte RAR 5 signature length are raised to 8, so a normal archive
    /// is still detected when scanning is effectively disabled.
    pub max_sfx_scan: u64,
}

impl Default for ArchiveOptions {
    fn default() -> Self {
        ArchiveOptions {
            password: None,
            max_sfx_scan: DEFAULT_MAX_SFX_SCAN,
        }
    }
}

impl ArchiveOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    pub fn with_max_sfx_scan(mut self, bytes: u64) -> Self {
        self.max_sfx_scan = bytes;
        self
    }
}

/// An opened RAR archive. All headers are parsed eagerly so that entries can
/// be inspected without touching the compressed data.
pub struct Archive {
    volumes: Vec<Volume>,
    pub entries: Vec<Entry>,
    pub info: ArchiveInfo,
    password: Option<String>,
    format: Format,
    main_header_seen: bool,
    max_sfx_scan: u64,
}

impl Archive {
    /// Open the first volume of an archive. Additional volumes are discovered
    /// automatically when the end-of-archive record requests them.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_options(path, ArchiveOptions::default())
    }

    /// Open an archive whose headers are encrypted.
    pub fn open_with_password<P: AsRef<Path>>(path: P, password: &str) -> Result<Self> {
        Self::open_with_options(path, ArchiveOptions::default().with_password(password))
    }

    /// Open an archive with explicit options.
    pub fn open_with_options<P: AsRef<Path>>(path: P, options: ArchiveOptions) -> Result<Self> {
        Self::from_paths(vec![path.as_ref().to_path_buf()], options)
    }

    /// Open additional volumes explicitly. The first path must be the first
    /// volume of the set.
    pub fn open_volumes<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        Self::open_volumes_with_options(paths, ArchiveOptions::default())
    }

    pub fn open_volumes_with_password<P: AsRef<Path>>(paths: &[P], password: &str) -> Result<Self> {
        Self::open_volumes_with_options(paths, ArchiveOptions::default().with_password(password))
    }

    pub fn open_volumes_with_options<P: AsRef<Path>>(
        paths: &[P],
        options: ArchiveOptions,
    ) -> Result<Self> {
        if paths.is_empty() {
            return Err(Error::NotArchive);
        }
        Self::from_paths(
            paths.iter().map(|p| p.as_ref().to_path_buf()).collect(),
            options,
        )
    }

    fn from_paths(paths: Vec<PathBuf>, options: ArchiveOptions) -> Result<Self> {
        let mut volumes = Vec::new();
        for path in &paths {
            volumes.push(Volume::open(path)?);
        }
        Self::from_volumes(volumes, options, Some(paths))
    }

    /// Build an archive from a complete in-memory first volume (or a
    /// concatenation produced by the caller). Multi-volume archives must use
    /// one of the path based constructors.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Self::from_bytes_with_options(data, ArchiveOptions::default())
    }

    pub fn from_bytes_with_password(data: Vec<u8>, password: &str) -> Result<Self> {
        Self::from_bytes_with_options(data, ArchiveOptions::default().with_password(password))
    }

    /// Build an in-memory archive with explicit options.
    pub fn from_bytes_with_options(data: Vec<u8>, options: ArchiveOptions) -> Result<Self> {
        Self::from_volumes(vec![Volume::from_bytes(data)], options, None)
    }

    fn from_volumes(
        mut volumes: Vec<Volume>,
        options: ArchiveOptions,
        paths: Option<Vec<PathBuf>>,
    ) -> Result<Self> {
        let mut archive = Archive {
            volumes: std::mem::take(&mut volumes),
            entries: Vec::new(),
            info: ArchiveInfo::default(),
            password: options.password,
            format: Format::Rar50,
            main_header_seen: false,
            max_sfx_scan: options.max_sfx_scan.max(8),
        };

        let mut next_path = paths.as_ref().and_then(|p| p.first().cloned());

        let mut vi = 0usize;
        let mut discovered = 0usize;
        loop {
            let parsed = archive.read_volume(vi)?;
            if vi == 0 {
                archive.format = parsed.format;
                archive.info.format = Some(parsed.format);
            } else if parsed.format != archive.format {
                return Err(Error::Corrupt("volumes use different RAR formats".into()));
            }

            for block in parsed.blocks {
                archive.process_block(block);
            }

            let need_next = parsed.next_volume
                || archive
                    .entries
                    .last()
                    .map(|e| e.split_after)
                    .unwrap_or(false);
            if !need_next {
                break;
            }

            // If the caller supplied an explicit volume list, consume it.
            if vi + 1 < archive.volumes.len() {
                vi += 1;
                continue;
            }

            // Otherwise auto-discover the next volume by its name.
            let current_path = next_path.clone();
            let can_discover = paths
                .as_ref()
                .map(|p| p.len() == 1 && discovered < 10_000)
                .unwrap_or(false);
            if can_discover {
                if let Some(path) = current_path.as_deref().and_then(next_volume_path) {
                    match Volume::open(&path) {
                        Ok(volume) => {
                            archive.volumes.push(volume);
                            next_path = Some(path);
                            discovered += 1;
                            vi += 1;
                            continue;
                        }
                        Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                            // The set is incomplete. Keep the entries parsed so
                            // far; split files remain marked with split_after.
                            break;
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            break;
        }

        Ok(archive)
    }

    /// Set the password used for encrypted file data. Header encryption must
    /// be handled when opening the archive.
    pub fn set_password(&mut self, password: &str) {
        self.password = Some(password.to_owned());
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn volume_count(&self) -> usize {
        self.volumes.len()
    }

    /// Read and verify one entry into memory.
    pub fn read(&mut self, index: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let size = self
            .entries
            .get(index)
            .map(|e| e.unpacked_size.min(1 << 20) as usize)
            .unwrap_or(0);
        out.reserve(size);
        self.extract_to(index, &mut out)?;
        Ok(out)
    }

    /// Extract a single entry to a writer.
    pub fn extract_to<W: Write>(&mut self, index: usize, writer: &mut W) -> Result<()> {
        if index >= self.entries.len() {
            return Err(Error::Corrupt(format!(
                "entry index {index} is out of range"
            )));
        }

        let start = self.solid_stream_start(index);
        let mut unpacker = Unpacker::new();
        let mut discard = |_data: &[u8]| -> Result<()> { Ok(()) };

        for i in start..=index {
            let entry = self.entries[i].clone();
            let is_target = i == index;

            if entry.is_dir || entry.redirect.is_some() || entry.service {
                continue;
            }

            if entry.method == 0 {
                let data = self.read_packed_data(&entry)?;
                if is_target {
                    let mut hash = DataHash::new(entry.expected_hash().hash_type());
                    let limit = if entry.unpacked_size == u64::MAX {
                        data.len()
                    } else {
                        (entry.unpacked_size as usize).min(data.len())
                    };
                    hash.update(&data[..limit]);
                    writer.write_all(&data[..limit])?;
                    self.verify_entry(&entry, hash)?;
                }
                continue;
            }

            let data = self.read_packed_data(&entry)?;
            if is_target {
                let mut hash = DataHash::new(entry.expected_hash().hash_type());
                {
                    let mut sink = |chunk: &[u8]| -> Result<()> {
                        hash.update(chunk);
                        writer.write_all(chunk)?;
                        Ok(())
                    };
                    self.decode_entry(&mut unpacker, &entry, &data, &mut sink)?;
                }
                self.verify_entry(&entry, hash)?;
            } else {
                self.decode_entry(&mut unpacker, &entry, &data, &mut discard)?;
            }
        }
        Ok(())
    }

    /// Verify all entries without writing anything.
    pub fn test(&mut self) -> Result<()> {
        let mut unpacker = Unpacker::new();
        for index in 0..self.entries.len() {
            let entry = self.entries[index].clone();
            if entry.is_dir || entry.service || entry.redirect.is_some() {
                continue;
            }
            if entry.split_after {
                // Incomplete multi-volume set: the remaining data is missing.
                continue;
            }
            if entry.method == 0 {
                let data = self.read_packed_data(&entry)?;
                let mut hash = DataHash::new(entry.expected_hash().hash_type());
                let limit = if entry.unpacked_size == u64::MAX {
                    data.len()
                } else {
                    (entry.unpacked_size as usize).min(data.len())
                };
                hash.update(&data[..limit]);
                self.verify_entry(&entry, hash)?;
                continue;
            }

            let data = self.read_packed_data(&entry)?;
            let mut hash = DataHash::new(entry.expected_hash().hash_type());
            {
                let mut sink = |chunk: &[u8]| -> Result<()> {
                    hash.update(chunk);
                    Ok(())
                };
                self.decode_entry(&mut unpacker, &entry, &data, &mut sink)?;
            }
            self.verify_entry(&entry, hash)?;
        }
        Ok(())
    }

    /// Extract all entries into `dest`. The extraction is sequential, which
    /// is required for solid archives.
    pub fn extract_all<P: AsRef<Path>>(&mut self, dest: P) -> Result<()> {
        let dest = dest.as_ref();
        std::fs::create_dir_all(dest)?;

        let mut unpacker = Unpacker::new();

        for index in 0..self.entries.len() {
            let entry = self.entries[index].clone();
            if entry.is_dir {
                let path = safe_join(dest, &entry.name)?;
                check_parent_symlinks(dest, &path)?;
                if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                    if metadata.file_type().is_symlink() {
                        return Err(Error::Unsupported(
                            "refusing to replace a symlink with a directory".into(),
                        ));
                    }
                }
                std::fs::create_dir_all(&path)?;
                continue;
            }
            if let Some(redir) = &entry.redirect {
                self.extract_redirect(dest, &entry, redir)?;
                continue;
            }

            let path = safe_join(dest, &entry.name)?;
            check_parent_symlinks(dest, &path)?;
            if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                if metadata.file_type().is_symlink() {
                    return Err(Error::Unsupported(
                        "refusing to overwrite a symlink with a file".into(),
                    ));
                }
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if entry.method == 0 {
                let data = self.read_packed_data(&entry)?;
                let limit = if entry.unpacked_size == u64::MAX {
                    data.len()
                } else {
                    (entry.unpacked_size as usize).min(data.len())
                };
                let mut file = File::create(&path)?;
                file.write_all(&data[..limit])?;
                if let Some(mtime) = entry.mtime {
                    let _ = file.set_modified(mtime);
                }
                let mut hash = DataHash::new(entry.expected_hash().hash_type());
                hash.update(&data[..limit]);
                self.verify_entry(&entry, hash)?;
                continue;
            }

            let data = self.read_packed_data(&entry)?;
            let mut file = File::create(&path)?;
            let mut hash = DataHash::new(entry.expected_hash().hash_type());
            {
                let mut sink = |chunk: &[u8]| -> Result<()> {
                    hash.update(chunk);
                    file.write_all(chunk)?;
                    Ok(())
                };
                self.decode_entry(&mut unpacker, &entry, &data, &mut sink)?;
            }
            if let Some(mtime) = entry.mtime {
                let _ = file.set_modified(mtime);
            }
            self.verify_entry(&entry, hash)?;
        }
        Ok(())
    }

    fn extract_redirect(&self, dest: &Path, entry: &Entry, redir: &Redirect) -> Result<()> {
        let path = safe_join(dest, &entry.name)?;
        check_parent_symlinks(dest, &path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }

        #[cfg(unix)]
        {
            match redir.kind {
                RedirectKind::UnixSymlink
                | RedirectKind::WindowsSymlink
                | RedirectKind::Junction => {
                    std::os::unix::fs::symlink(&redir.name, &path)?;
                    return Ok(());
                }
                _ => {}
            }
        }

        #[cfg(not(unix))]
        let _ = redir;

        // Hard links and file copies are represented as regular files. We do
        // not have the redirect target's data at this point, so leave an
        // empty file rather than silently creating a wrong link.
        File::create(&path)?;
        Ok(())
    }

    fn decode_entry(
        &self,
        unpacker: &mut Unpacker,
        entry: &Entry,
        data: &[u8],
        sink: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        unpacker.init(entry.win_size, entry.solid)?;
        unpacker.set_dest_size(if entry.unpacked_size == u64::MAX {
            i64::MAX as u64
        } else {
            entry.unpacked_size
        });
        match self.format {
            Format::Rar50 => {
                if entry.unp_ver != 50 && entry.unp_ver != 70 {
                    return Err(Error::Unsupported(format!(
                        "RAR 5 decompression method version {}",
                        entry.unp_ver
                    )));
                }
                unpacker.set_extra_dist(entry.unp_ver == 70);
                unpacker.unpack5(data, entry.solid, sink)
            }
            Format::Rar15 => match entry.unp_ver {
                29 => unpacker.unpack29(data, entry.solid, sink),
                20 | 26 => Err(Error::Unsupported(
                    "RAR 2.x compression is not supported".into(),
                )),
                15 | 13 => Err(Error::Unsupported(
                    "RAR 1.5 compression is not supported".into(),
                )),
                other => Err(Error::Unsupported(format!(
                    "RAR 3.x decompression method version {other}"
                ))),
            },
            Format::Rar14 => Err(Error::Unsupported("RAR 1.4 archives".into())),
        }
    }

    fn verify_entry(&self, entry: &Entry, hash: DataHash) -> Result<()> {
        if entry.split_after {
            return Ok(());
        }
        let expected = entry.expected_hash();
        let hash_key = entry
            .crypt
            .as_ref()
            .filter(|crypt| crypt.use_hash_key)
            .map(|crypt| {
                let password = self.password.as_deref().unwrap_or_default();
                derive_key(password, &crypt.salt, crypt.lg2_count)
            })
            .transpose()?
            .map(|key| key.hash_key);

        hash.verify(&expected, hash_key.as_ref())
            .map_err(|_| Error::Checksum(entry.name.clone()))
    }

    fn solid_stream_start(&self, index: usize) -> usize {
        if !self.entries[index].solid {
            return index;
        }
        let mut start = index;
        while start > 0 {
            let current = &self.entries[start];
            if !current.solid {
                break;
            }
            start -= 1;
            if !self.entries[start].solid {
                // The non-solid entry begins the stream and must be decoded.
                break;
            }
        }
        start
    }

    fn read_packed_data(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        let total = entry
            .segments
            .iter()
            .try_fold(0u64, |acc, segment| acc.checked_add(segment.len))
            .ok_or_else(|| Error::Corrupt("packed size overflow".into()))?;
        let total_usize =
            usize::try_from(total).map_err(|_| Error::Unsupported("file is too large".into()))?;
        let mut out = Vec::new();
        out.try_reserve_exact(total_usize)
            .map_err(|_| Error::Unsupported("cannot allocate packed file buffer".into()))?;
        out.resize(total_usize, 0);

        let mut written = 0usize;
        for segment in &entry.segments {
            let len = segment.len as usize;
            let volume = &mut self.volumes[segment.volume];
            volume
                .source
                .read_exact_at(segment.offset, &mut out[written..written + len])?;
            written += len;
        }

        if let Some(crypt) = &entry.crypt {
            let password = self.password.as_deref().ok_or(Error::PasswordRequired)?;
            let key = derive_key(password, &crypt.salt, crypt.lg2_count)?;
            if let Some(stored) = crypt.psw_check {
                if stored != [0u8; 8] && stored != key.psw_check {
                    return Err(Error::WrongPassword);
                }
            }
            if out.len() % AES_BLOCK_SIZE != 0 {
                return Err(Error::Corrupt(
                    "encrypted file data is not block aligned".into(),
                ));
            }
            decrypt_cbc(&key.key, &crypt.init_v, &mut out)?;
        } else if entry.encrypted {
            if self.format == Format::Rar15 && entry.unp_ver >= 29 {
                let password = self.password.as_deref().ok_or(Error::PasswordRequired)?;
                let key = derive_key_rar3(password, entry.rar3_salt.as_ref());
                if out.len() % AES_BLOCK_SIZE != 0 {
                    return Err(Error::Corrupt(
                        "encrypted file data is not block aligned".into(),
                    ));
                }
                decrypt_cbc_128(&key.key, &key.iv, &mut out)?;
            } else {
                return Err(Error::Unsupported(
                    "encryption used by this archive version is not supported".into(),
                ));
            }
        }

        Ok(out)
    }

    // ------------------------------------------------------------------
    // Header parsing
    // ------------------------------------------------------------------

    fn read_volume(&mut self, vi: usize) -> Result<VolumeParse> {
        let password = self.password.clone();
        let (format, sfx_size, mark_size) =
            detect_signature(&mut self.volumes[vi], self.max_sfx_scan)?;

        if format == Format::Rar14 {
            return Err(Error::UnsupportedFormat(14));
        }

        let mut blocks = Vec::new();
        let mut pos = sfx_size + mark_size;
        let mut encrypted = false;
        let mut header_key: Option<Rar5Key> = None;
        let mut next_volume = false;
        let volume_len = self.volumes[vi].len;

        loop {
            if pos >= volume_len {
                break;
            }

            match format {
                Format::Rar50 => {
                    let (block, next_pos) = read_block50(
                        &mut self.volumes[vi],
                        vi,
                        pos,
                        volume_len,
                        encrypted,
                        header_key.as_ref(),
                    )?;

                    if block.header_type == HEAD_CRYPT {
                        let crypt = block
                            .crypt
                            .as_ref()
                            .ok_or_else(|| Error::Corrupt("missing encryption header".into()))?;
                        let pwd = password.as_deref().ok_or(Error::PasswordRequired)?;
                        let key = derive_key(pwd, &crypt.salt, crypt.lg2_count)?;
                        if let Some(stored) = crypt.psw_check {
                            if stored != [0u8; 8] && stored != key.psw_check {
                                return Err(Error::WrongPassword);
                            }
                        }
                        header_key = Some(key);
                        encrypted = true;
                        self.info.is_encrypted = true;
                    }

                    next_volume = match block.endarc_flags {
                        Some(flags) => flags & EHFL_NEXTVOLUME != 0,
                        None => false,
                    };
                    let end_of_archive = block.header_type == HEAD_ENDARC;
                    blocks.push(block);
                    pos = next_pos;
                    if end_of_archive {
                        break;
                    }
                }
                Format::Rar15 => {
                    let (block, next_pos) =
                        read_block15(&mut self.volumes[vi], vi, pos, volume_len, encrypted)?;
                    if block.header_type == HEAD_MAIN && block.flags & 0x0080 != 0 {
                        return Err(Error::Unsupported(
                            "encrypted RAR 4.x headers are not supported".into(),
                        ));
                    }
                    if block.header_type == HEAD_CRYPT || encrypted {
                        return Err(Error::Unsupported(
                            "encrypted RAR 4.x headers are not supported".into(),
                        ));
                    }
                    next_volume = match block.endarc_flags {
                        Some(flags) => flags & 0x0001 != 0,
                        None => false,
                    };
                    let end_of_archive = block.header_type == HEAD_ENDARC;
                    blocks.push(block);
                    pos = next_pos;
                    if end_of_archive {
                        break;
                    }
                }
                Format::Rar14 => unreachable!(),
            };

            if pos == 0 {
                break;
            }
        }

        Ok(VolumeParse {
            blocks,
            next_volume,
            format,
        })
    }

    fn process_block(&mut self, block: ParsedBlock) {
        match block.header_type {
            HEAD_MAIN => {
                if let Some(flags) = block.main_flags {
                    if !self.main_header_seen {
                        self.main_header_seen = true;
                        self.info.is_volume = flags & MHFL_VOLUME != 0;
                        self.info.is_solid = flags & MHFL_SOLID != 0;
                        self.info.is_locked = flags & MHFL_LOCK != 0;
                        self.info.has_recovery_record = flags & MHFL_PROTECT != 0;
                        self.info.volume_number = block.main_vol_number.unwrap_or(0) as u32;
                        self.info.is_first_volume =
                            self.info.is_volume && self.info.volume_number == 0;
                    }
                }
            }
            HEAD_FILE | HEAD_SERVICE => {
                if let Some(mut entry) = block.file {
                    entry.service = block.header_type == HEAD_SERVICE;
                    entry.packed_size = block.data_size;
                    if entry.encrypted {
                        self.info.is_encrypted = true;
                    }

                    if let Some(last) = self.entries.last_mut() {
                        if last.split_after && entry.split_before {
                            if let Some(segment) = entry.segments.first().copied() {
                                last.segments.push(segment);
                            }
                            last.packed_size += block.data_size;
                            last.split_after = entry.split_after;
                            // RAR stores the checksum of the complete file in
                            // the final fragment, so adopt it when merging.
                            last.crc32 = entry.crc32;
                            last.blake2 = entry.blake2;
                            last.method = entry.method;
                            last.unp_ver = entry.unp_ver;
                            last.win_size = entry.win_size;
                            last.mtime = entry.mtime.or(last.mtime);
                            return;
                        }
                    }

                    if !entry.service {
                        self.entries.push(entry);
                    }
                }
            }
            _ => {}
        }
    }
}

fn detect_signature(volume: &mut Volume, max_sfx_scan: u64) -> Result<(Format, u64, u64)> {
    let len = volume.len;
    if len < 7 {
        return Err(Error::NotArchive);
    }
    let scan_len = len.min(max_sfx_scan) as usize;
    let mut buf = vec![0u8; scan_len];
    volume.source.read_exact_at(0, &mut buf)?;

    if buf.len() >= 8 && buf[..8] == RAR5_SIGNATURE {
        return Ok((Format::Rar50, 0, 8));
    }
    if buf.len() >= 7 && buf[..7] == RAR15_SIGNATURE {
        return Ok((Format::Rar15, 0, 7));
    }
    if buf.len() >= 4 && buf[..4] == RAR14_SIGNATURE {
        return Ok((Format::Rar14, 0, 7));
    }

    for i in 1..buf.len() {
        if buf[i] != 0x52 {
            continue;
        }
        let rest = &buf[i..];
        if rest.len() >= 8 && rest[..8] == RAR5_SIGNATURE {
            return Ok((Format::Rar50, i as u64, 8));
        }
        if rest.len() >= 7 && rest[..7] == RAR15_SIGNATURE {
            return Ok((Format::Rar15, i as u64, 7));
        }
        if rest.len() >= 4 && rest[..4] == RAR14_SIGNATURE {
            return Ok((Format::Rar14, i as u64, 7));
        }
    }

    Err(Error::NotArchive)
}

fn align16(value: u64) -> u64 {
    (value + 15) & !15
}

fn read_block50(
    volume: &mut Volume,
    volume_index: usize,
    block_pos: u64,
    volume_len: u64,
    encrypted: bool,
    key: Option<&Rar5Key>,
) -> Result<(ParsedBlock, u64)> {
    let (plain, header_storage) = if encrypted {
        let key = key.ok_or(Error::PasswordRequired)?;
        let mut iv = [0u8; 16];
        if block_pos + 32 > volume_len {
            return Err(Error::Corrupt(format!(
                "truncated encrypted header at {block_pos} (volume length {volume_len})"
            )));
        }
        volume.source.read_exact_at(block_pos, &mut iv)?;

        let probe_len = ((volume_len - block_pos - 16).min(32) as usize) & !15;
        let mut probe = vec![0u8; probe_len];
        volume.source.read_exact_at(block_pos + 16, &mut probe)?;
        decrypt_cbc(&key.key, &iv, &mut probe)?;

        let (size_bytes, block_size) = parse_header_size(&probe, 4)?;
        let header_size = 4u64 + size_bytes as u64 + block_size;
        if !(7..=MAX_HEADER_SIZE_RAR5).contains(&header_size) {
            return Err(Error::Corrupt(format!(
                "invalid encrypted header size {header_size} at {block_pos} (size_bytes {size_bytes}, block_size {block_size})"
            )));
        }
        let storage = align16(header_size);
        let read_size =
            usize::try_from(storage).map_err(|_| Error::Corrupt("invalid header size".into()))?;
        let mut plain = vec![0u8; read_size];
        if block_pos + 16 + storage > volume_len {
            return Err(Error::Corrupt(format!(
                "truncated encrypted header at {block_pos}: size {header_size}, storage {storage}, volume length {volume_len}"
            )));
        }
        volume.source.read_exact_at(block_pos + 16, &mut plain)?;
        decrypt_cbc(&key.key, &iv, &mut plain)?;
        plain.truncate(header_size as usize);

        let stored_crc = u32::from_le_bytes([plain[0], plain[1], plain[2], plain[3]]);
        let actual_crc = crc32fast::hash(&plain[4..]);
        if stored_crc != actual_crc {
            return Err(Error::WrongPassword);
        }
        (plain, storage)
    } else {
        if block_pos + 7 > volume_len {
            return Err(Error::Corrupt("truncated header".into()));
        }
        let mut first = [0u8; 7];
        volume.source.read_exact_at(block_pos, &mut first)?;
        let (size_bytes, block_size) = parse_header_size(&first, 4)?;
        let header_size = 4u64 + size_bytes as u64 + block_size;
        if !(7..=MAX_HEADER_SIZE_RAR5).contains(&header_size) {
            return Err(Error::Corrupt(format!(
                "invalid header size {header_size} at {block_pos} (size_bytes {size_bytes}, block_size {block_size})"
            )));
        }
        let mut plain = vec![0u8; header_size as usize];
        volume.source.read_exact_at(block_pos, &mut plain)?;

        let stored_crc = u32::from_le_bytes([plain[0], plain[1], plain[2], plain[3]]);
        let actual_crc = crc32fast::hash(&plain[4..]);
        if stored_crc != actual_crc {
            return Err(Error::Corrupt("header CRC mismatch".into()));
        }
        (plain, header_size)
    };

    let mut parsed = parse_block50(&plain)?;
    let data_size = parsed.data_size;
    let data_offset = block_pos
        .checked_add(if encrypted {
            16 + header_storage
        } else {
            header_storage
        })
        .ok_or_else(|| Error::Corrupt("block position overflow".into()))?;
    let next_pos = data_offset
        .checked_add(data_size)
        .ok_or_else(|| Error::Corrupt("block position overflow".into()))?;
    if next_pos <= block_pos {
        return Err(Error::Corrupt("block position overflow".into()));
    }

    if let Some(entry) = parsed.file.as_mut() {
        entry.segments.push(DataSegment {
            volume: volume_index,
            offset: data_offset,
            len: data_size,
        });
    }
    Ok((parsed, next_pos))
}

fn parse_header_size(buf: &[u8], start: usize) -> Result<(usize, u64)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    let mut pos = start;
    while pos < buf.len() && shift < 64 {
        let byte = buf[pos];
        value |= ((byte & 0x7f) as u64) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            return Ok((pos - start, value));
        }
        shift += 7;
    }
    Err(Error::Corrupt("invalid variable length integer".into()))
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn get1(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            return Err(Error::Corrupt("unexpected end of header".into()));
        }
        let value = self.data[self.pos];
        self.pos += 1;
        Ok(value)
    }

    fn get2(&mut self) -> Result<u16> {
        let bytes = self.get_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn get4(&mut self) -> Result<u32> {
        let bytes = self.get_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn get_vint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0u32;
        while shift < 64 {
            let byte = self.get1()?;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
        Err(Error::Corrupt("invalid variable length integer".into()))
    }

    fn get_bytes(&mut self, size: usize) -> Result<&'a [u8]> {
        if self.pos + size > self.data.len() {
            return Err(Error::Corrupt("unexpected end of header".into()));
        }
        let bytes = &self.data[self.pos..self.pos + size];
        self.pos += size;
        Ok(bytes)
    }
}

fn read_vint_at(data: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while shift < 64 {
        if *pos >= data.len() {
            return Err(Error::Corrupt("unexpected end of extra field".into()));
        }
        let byte = data[*pos];
        *pos += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
    Err(Error::Corrupt("invalid extra field integer".into()))
}

struct ExtraCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ExtraCursor<'a> {
    fn get_vint(&mut self) -> Result<u64> {
        read_vint_at(self.data, &mut self.pos)
    }

    fn get1(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            return Err(Error::Corrupt("unexpected end of extra field".into()));
        }
        let value = self.data[self.pos];
        self.pos += 1;
        Ok(value)
    }

    fn get4(&mut self) -> Result<u32> {
        if self.pos + 4 > self.data.len() {
            return Err(Error::Corrupt("unexpected end of extra field".into()));
        }
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    fn get8(&mut self) -> Result<u64> {
        if self.pos + 8 > self.data.len() {
            return Err(Error::Corrupt("unexpected end of extra field".into()));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    fn get_bytes(&mut self, size: usize) -> Result<&'a [u8]> {
        if self.pos + size > self.data.len() {
            return Err(Error::Corrupt("unexpected end of extra field".into()));
        }
        let bytes = &self.data[self.pos..self.pos + size];
        self.pos += size;
        Ok(bytes)
    }
}

fn parse_block50(plain: &[u8]) -> Result<ParsedBlock> {
    let mut cursor = Cursor::new(&plain[4..]);
    // Skip the header size varint; the block body follows it.
    let _header_size = cursor.get_vint()?;
    let header_type = cursor.get_vint()? as u8;
    let flags = cursor.get_vint()?;

    let mut extra_size = 0u64;
    if flags & HFL_EXTRA != 0 {
        extra_size = cursor.get_vint()?;
        if extra_size >= plain.len() as u64 {
            return Err(Error::Corrupt(
                "extra area is larger than the header".into(),
            ));
        }
    }
    let mut data_size = 0u64;
    if flags & HFL_DATA != 0 {
        data_size = cursor.get_vint()?;
    }

    let extra_start = plain.len().saturating_sub(extra_size as usize);
    let extra = if extra_size > 0 {
        Some(&plain[extra_start..])
    } else {
        None
    };

    let mut block = ParsedBlock {
        header_type,
        flags,
        data_size,
        file: None,
        main_flags: None,
        main_vol_number: None,
        crypt: None,
        endarc_flags: None,
    };

    match header_type {
        HEAD_MAIN => {
            block.main_flags = Some(cursor.get_vint()?);
            if block.main_flags.unwrap() & MHFL_VOLNUMBER != 0 {
                block.main_vol_number = Some(cursor.get_vint()?);
            }
        }
        HEAD_CRYPT => {
            let version = cursor.get_vint()?;
            if version > 0 {
                return Err(Error::Unsupported(format!(
                    "encryption header version {version}"
                )));
            }
            let enc_flags = cursor.get_vint()?;
            let lg2_count = cursor.get1()?;
            let mut salt = [0u8; 16];
            salt.copy_from_slice(cursor.get_bytes(16)?);
            let mut psw_check = None;
            if enc_flags & CHFL_CRYPT_PSWCHECK != 0 {
                let mut check = [0u8; 8];
                check.copy_from_slice(cursor.get_bytes(8)?);
                let csum = cursor.get4()?;
                let digest = Sha256::digest(check);
                let valid = digest[..4] == csum.to_le_bytes();
                if valid {
                    psw_check = Some(check);
                }
            }
            block.crypt = Some(CryptHeaderInfo {
                salt,
                lg2_count,
                psw_check,
            });
        }
        HEAD_FILE | HEAD_SERVICE => {
            let file_flags = cursor.get_vint()?;
            let mut unpacked_size = cursor.get_vint()?;
            if file_flags & FHFL_UNPUNKNOWN != 0 {
                unpacked_size = u64::MAX;
            }
            let attributes = cursor.get_vint()? as u32;
            let mtime = if file_flags & FHFL_UTIME != 0 {
                unix_time(cursor.get4()? as u64)
            } else {
                None
            };
            let crc32 = if file_flags & FHFL_CRC32 != 0 {
                Some(cursor.get4()?)
            } else {
                None
            };
            let comp_info = cursor.get_vint()?;
            let host_os = cursor.get_vint()? as u8;
            let name_size = cursor.get_vint()? as usize;
            let available = cursor.remaining().min(name_size);
            let name_bytes = cursor.get_bytes(available)?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();

            let method = ((comp_info >> 7) & 7) as u8;
            let raw_ver = (comp_info & 0x3f) as u32;
            let mut unp_ver = match raw_ver {
                0 => 50,
                1 => 70,
                _ => 9999,
            };
            let is_dir = file_flags & FHFL_DIRECTORY != 0;

            let mut win_size = 0u64;
            if !is_dir && raw_ver <= 1 {
                let shift_mask = if raw_ver == 0 { 0x0f } else { 0x1f };
                win_size = 0x2_0000u64 << ((comp_info >> 10) & shift_mask);
                if raw_ver == 1 {
                    win_size += win_size / 32 * ((comp_info >> 15) & 0x1f);
                    if comp_info & FCI_RAR5_COMPAT != 0 {
                        unp_ver = 50;
                    }
                    if win_size > UNPACK_MAX_DICT {
                        unp_ver = 9999;
                    }
                }
            }

            let host_os = match host_os {
                0 => HostOs::Windows,
                1 => HostOs::Unix,
                other => HostOs::Unknown(other),
            };

            let mut entry = Entry {
                name,
                is_dir,
                unpacked_size,
                packed_size: data_size,
                method,
                solid: header_type == HEAD_FILE && (comp_info & FCI_SOLID) != 0,
                encrypted: false,
                split_before: flags & HFL_SPLITBEFORE != 0,
                split_after: flags & HFL_SPLITAFTER != 0,
                crc32,
                blake2: None,
                mtime,
                ctime: None,
                atime: None,
                host_os,
                attributes,
                redirect: None,
                sub_data: Vec::new(),
                version: None,
                owner: None,
                owner_name: None,
                group_name: None,
                service: header_type == HEAD_SERVICE,
                unp_ver,
                win_size,
                crypt: None,
                rar3_salt: None,
                segments: Vec::new(),
            };

            if let Some(extra) = extra {
                parse_extra50(extra, &mut entry)?;
            }
            block.file = Some(entry);
        }
        HEAD_ENDARC => {
            block.endarc_flags = Some(cursor.get_vint()?);
        }
        _ => {}
    }

    Ok(block)
}

fn parse_extra50(data: &[u8], entry: &mut Entry) -> Result<()> {
    let mut pos = 0usize;
    while pos + 2 <= data.len() {
        let field_size = match read_vint_at(data, &mut pos) {
            Ok(value) => value,
            Err(_) => break,
        };
        if field_size == 0 || pos >= data.len() {
            break;
        }
        let next_pos = pos + field_size as usize;
        if next_pos > data.len() {
            break;
        }
        let field_type = match read_vint_at(data, &mut pos) {
            Ok(value) => value,
            Err(_) => break,
        };
        let mut field = ExtraCursor {
            data: &data[pos..next_pos],
            pos: 0,
        };

        let result = match field_type {
            FHEXTRA_CRYPT => parse_crypt_extra(&mut field, entry),
            FHEXTRA_HASH => parse_hash_extra(&mut field, entry),
            FHEXTRA_HTIME => parse_time_extra(&mut field, entry),
            FHEXTRA_VERSION => parse_version_extra(&mut field, entry),
            FHEXTRA_REDIR => parse_redir_extra(&mut field, entry),
            FHEXTRA_UOWNER => parse_owner_extra(&mut field, entry),
            FHEXTRA_SUBDATA => {
                entry.sub_data = data[pos..next_pos].to_vec();
                Ok(())
            }
            _ => Ok(()),
        };

        // Extra fields are advisory. A malformed one stops parsing but does
        // not invalidate an otherwise readable archive.
        if result.is_err() {
            break;
        }

        pos = next_pos;
    }
    Ok(())
}

fn parse_crypt_extra(field: &mut ExtraCursor<'_>, entry: &mut Entry) -> Result<()> {
    let version = field.get_vint()?;
    if version > 0 {
        entry.encrypted = true;
        return Ok(());
    }
    let flags = field.get_vint()?;
    let lg2_count = field.get1()?;
    let mut salt = [0u8; 16];
    salt.copy_from_slice(field.get_bytes(16)?);
    let mut init_v = [0u8; 16];
    init_v.copy_from_slice(field.get_bytes(16)?);

    let mut psw_check = None;
    if flags & FHEXTRA_CRYPT_PSWCHECK != 0 {
        let mut check = [0u8; 8];
        check.copy_from_slice(field.get_bytes(8)?);
        let csum = field.get4()?;
        let digest = Sha256::digest(check);
        if digest[..4] == csum.to_le_bytes() && check != [0u8; 8] {
            psw_check = Some(check);
        }
    }

    entry.crypt = Some(FileCrypt {
        salt,
        init_v,
        lg2_count,
        psw_check,
        use_hash_key: flags & FHEXTRA_CRYPT_HASHMAC != 0,
    });
    entry.encrypted = true;
    Ok(())
}

fn parse_hash_extra(field: &mut ExtraCursor<'_>, entry: &mut Entry) -> Result<()> {
    let hash_type = field.get_vint()?;
    if hash_type == 0 {
        let mut digest = [0u8; 32];
        digest.copy_from_slice(field.get_bytes(32)?);
        entry.blake2 = Some(digest);
    }
    Ok(())
}

fn parse_time_extra(field: &mut ExtraCursor<'_>, entry: &mut Entry) -> Result<()> {
    let flags = field.get1()? as u64;
    let unix = flags & FHEXTRA_HTIME_UNIXTIME != 0;
    let mut mtime = None;
    let mut ctime = None;
    let mut atime = None;

    let read_time = |field: &mut ExtraCursor<'_>, unix: bool| -> Result<SystemTime> {
        if unix {
            unix_time(field.get4()? as u64)
                .ok_or_else(|| Error::Corrupt("invalid Unix time".into()))
        } else {
            win_time(field.get8()?).ok_or_else(|| Error::Corrupt("invalid Windows time".into()))
        }
    };

    if flags & FHEXTRA_HTIME_MTIME != 0 {
        mtime = Some(read_time(field, unix)?);
    }
    if flags & FHEXTRA_HTIME_CTIME != 0 {
        ctime = Some(read_time(field, unix)?);
    }
    if flags & FHEXTRA_HTIME_ATIME != 0 {
        atime = Some(read_time(field, unix)?);
    }
    if unix && flags & FHEXTRA_HTIME_UNIX_NS != 0 {
        if flags & FHEXTRA_HTIME_MTIME != 0 {
            let ns = field.get4()? & 0x3fff_ffff;
            if ns < 1_000_000_000 {
                mtime = mtime.map(|t| t + Duration::from_nanos(ns as u64));
            }
        }
        if flags & FHEXTRA_HTIME_CTIME != 0 {
            let ns = field.get4()? & 0x3fff_ffff;
            if ns < 1_000_000_000 {
                ctime = ctime.map(|t| t + Duration::from_nanos(ns as u64));
            }
        }
        if flags & FHEXTRA_HTIME_ATIME != 0 {
            let ns = field.get4()? & 0x3fff_ffff;
            if ns < 1_000_000_000 {
                atime = atime.map(|t| t + Duration::from_nanos(ns as u64));
            }
        }
    }

    if mtime.is_some() {
        entry.mtime = mtime;
    }
    if ctime.is_some() {
        entry.ctime = ctime;
    }
    if atime.is_some() {
        entry.atime = atime;
    }
    Ok(())
}

fn parse_version_extra(field: &mut ExtraCursor<'_>, entry: &mut Entry) -> Result<()> {
    let _flags = field.get_vint()?;
    let version = field.get_vint()?;
    if version != 0 {
        entry.version = Some(version as u32);
        entry.name = format!("{};{}", entry.name, version);
    }
    Ok(())
}

fn parse_redir_extra(field: &mut ExtraCursor<'_>, entry: &mut Entry) -> Result<()> {
    let redir_type = field.get_vint()?;
    let flags = field.get_vint()?;
    let name_size = field.get_vint()? as usize;
    let name_bytes = field.get_bytes(name_size)?;
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    let kind = match redir_type {
        1 => RedirectKind::UnixSymlink,
        2 => RedirectKind::WindowsSymlink,
        3 => RedirectKind::Junction,
        4 => RedirectKind::HardLink,
        5 => RedirectKind::FileCopy,
        other => RedirectKind::Unknown(other),
    };
    entry.redirect = Some(Redirect {
        kind,
        name: name.replace('\\', "/"),
        is_dir: flags & FHEXTRA_REDIR_DIR != 0,
    });
    Ok(())
}

fn parse_owner_extra(field: &mut ExtraCursor<'_>, entry: &mut Entry) -> Result<()> {
    let flags = field.get_vint()?;
    let mut owner = UnixOwner {
        owner_numeric: flags & FHEXTRA_UOWNER_NUMUID != 0,
        group_numeric: flags & FHEXTRA_UOWNER_NUMGID != 0,
        owner_id: None,
        group_id: None,
    };
    let mut owner_name = None;
    let mut group_name = None;
    if flags & FHEXTRA_UOWNER_UNAME != 0 {
        let len = field.get_vint()? as usize;
        let bytes = field.get_bytes(len.min(255))?;
        owner_name = Some(String::from_utf8_lossy(bytes).into_owned());
    }
    if flags & FHEXTRA_UOWNER_GNAME != 0 {
        let len = field.get_vint()? as usize;
        let bytes = field.get_bytes(len.min(255))?;
        group_name = Some(String::from_utf8_lossy(bytes).into_owned());
    }
    if owner.owner_numeric {
        owner.owner_id = Some(field.get_vint()? as u32);
    }
    if owner.group_numeric {
        owner.group_id = Some(field.get_vint()? as u32);
    }
    entry.owner = Some(owner);
    entry.owner_name = owner_name;
    entry.group_name = group_name;
    Ok(())
}

// ----------------------------------------------------------------------
// RAR 1.5 - 4.x
// ----------------------------------------------------------------------

fn read_block15(
    volume: &mut Volume,
    volume_index: usize,
    block_pos: u64,
    volume_len: u64,
    encrypted: bool,
) -> Result<(ParsedBlock, u64)> {
    if block_pos + 7 > volume_len {
        return Err(Error::Corrupt("truncated header".into()));
    }
    let mut first = [0u8; 7];
    volume.source.read_exact_at(block_pos, &mut first)?;
    let head_size = u16::from_le_bytes([first[5], first[6]]) as u64;
    if head_size < 7 {
        return Err(Error::Corrupt("invalid RAR 4 header size".into()));
    }
    let mut plain = vec![0u8; head_size as usize];
    volume.source.read_exact_at(block_pos, &mut plain)?;

    let header_type = plain[2];
    let flags = u16::from_le_bytes([plain[3], plain[4]]);
    let mut block = ParsedBlock {
        header_type: match header_type {
            HEAD3_MAIN => HEAD_MAIN,
            HEAD3_FILE => HEAD_FILE,
            HEAD3_SERVICE => HEAD_SERVICE,
            HEAD3_ENDARC => HEAD_ENDARC,
            other => other,
        },
        flags: flags as u64,
        data_size: 0,
        file: None,
        main_flags: None,
        main_vol_number: None,
        crypt: None,
        endarc_flags: None,
    };

    let mut cursor = Cursor::new(&plain[7..]);

    match block.header_type {
        HEAD_MAIN => {
            let high_pos_av = cursor.get2()?;
            let pos_av = cursor.get4()?;
            let mut arc_flags = 0u64;
            if flags & 0x0001 != 0 {
                arc_flags |= MHFL_VOLUME;
            }
            if flags & 0x0008 != 0 {
                arc_flags |= MHFL_SOLID;
            }
            if flags & 0x0004 != 0 {
                arc_flags |= MHFL_LOCK;
            }
            if flags & 0x0040 != 0 {
                arc_flags |= MHFL_PROTECT;
            }
            if pos_av != 0 || high_pos_av != 0 {
                block.main_vol_number = Some(0);
            }
            block.main_flags = Some(arc_flags);
            block.data_size = 0;
        }
        HEAD_FILE | HEAD_SERVICE => {
            let data_size = cursor.get4()?;
            let low_unp_size = cursor.get4()?;
            let host_os = cursor.get1()?;
            let crc32 = cursor.get4()?;
            let file_time = cursor.get4()?;
            let unp_ver = cursor.get1()?;
            let method_raw = cursor.get1()?;
            let name_size = cursor.get2()? as usize;
            let attributes = cursor.get4()?;

            let large = flags & LHD_LARGE != 0;
            let (high_pack, high_unp) = if large {
                (cursor.get4()? as u64, cursor.get4()? as u64)
            } else {
                (0, 0)
            };
            let pack_size = (high_pack << 32) | data_size as u64;
            let unpacked_size = if low_unp_size == u32::MAX && high_unp == u32::MAX as u64 {
                u64::MAX
            } else {
                (high_unp << 32) | low_unp_size as u64
            };

            let available = cursor.remaining().min(name_size);
            let name_bytes = cursor.get_bytes(available)?;
            let name = decode_rar4_name(name_bytes, flags);
            let is_dir = (flags & LHD_WINDOWMASK) == LHD_DIRECTORY
                || (unp_ver < 20 && attributes & 0x10 != 0);

            let method = method_raw.wrapping_sub(0x30);
            let win_size = if is_dir {
                0
            } else {
                0x1_0000u64 << ((flags & LHD_WINDOWMASK) >> 5)
            };

            let mut rar3_salt = None;
            if flags & LHD_SALT != 0 {
                let mut salt = [0u8; 8];
                salt.copy_from_slice(cursor.get_bytes(8)?);
                rar3_salt = Some(salt);
            }

            let host_os = match host_os {
                3 | 5 => HostOs::Unix,
                0..=4 => HostOs::Windows,
                other => HostOs::Unknown(other),
            };

            let mut entry = Entry {
                name,
                is_dir,
                unpacked_size,
                packed_size: pack_size,
                method,
                solid: flags & LHD_SOLID != 0,
                encrypted: flags & LHD_PASSWORD != 0,
                split_before: flags & LHD_SPLIT_BEFORE != 0,
                split_after: flags & LHD_SPLIT_AFTER != 0,
                crc32: Some(crc32),
                blake2: None,
                mtime: dos_time(file_time),
                ctime: None,
                atime: None,
                host_os,
                attributes,
                redirect: None,
                sub_data: Vec::new(),
                version: None,
                owner: None,
                owner_name: None,
                group_name: None,
                service: block.header_type == HEAD_SERVICE,
                unp_ver: unp_ver as u32,
                win_size,
                crypt: None,
                rar3_salt,
                segments: Vec::new(),
            };

            if host_os == HostOs::Unix && attributes & 0xF000 == 0xA000 {
                entry.redirect = Some(Redirect {
                    kind: RedirectKind::UnixSymlink,
                    name: String::new(),
                    is_dir: false,
                });
            }
            if flags & LHD_VERSION != 0 {
                // Versioned names are stored in the name as ";N" already in
                // RAR 4.x, nothing extra to do here.
            }

            block.data_size = pack_size;
            block.file = Some(entry);
        }
        HEAD_ENDARC => {
            block.endarc_flags = Some(flags as u64);
        }
        _ => {
            if flags & LONG_BLOCK != 0 {
                let size = cursor.get4()? as u64;
                block.data_size = size;
            }
        }
    }

    let _ = encrypted;
    let data_offset = block_pos
        .checked_add(head_size)
        .ok_or_else(|| Error::Corrupt("block position overflow".into()))?;
    let next_pos = data_offset
        .checked_add(block.data_size)
        .ok_or_else(|| Error::Corrupt("block position overflow".into()))?;
    if let Some(entry) = block.file.as_mut() {
        entry.segments.push(DataSegment {
            volume: volume_index,
            offset: data_offset,
            len: block.data_size,
        });
    }
    Ok((block, next_pos))
}

fn latin1_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn decode_unicode_name(name: &[u8], enc: &[u8]) -> Option<String> {
    if enc.is_empty() {
        return None;
    }
    let mut out: Vec<u16> = Vec::new();
    let mut enc_pos = 0usize;
    let high_byte = enc[enc_pos];
    enc_pos += 1;
    let mut flags = 0u8;
    let mut flag_bits = 0u32;
    let name_size = name.len();

    while enc_pos < enc.len() {
        if flag_bits == 0 {
            flags = enc[enc_pos];
            enc_pos += 1;
            flag_bits = 8;
        }
        match flags >> 6 {
            0 => {
                if enc_pos >= enc.len() {
                    break;
                }
                out.push(enc[enc_pos] as u16);
                enc_pos += 1;
            }
            1 => {
                if enc_pos >= enc.len() {
                    break;
                }
                out.push(enc[enc_pos] as u16 | ((high_byte as u16) << 8));
                enc_pos += 1;
            }
            2 => {
                if enc_pos + 1 >= enc.len() {
                    break;
                }
                out.push(enc[enc_pos] as u16 | ((enc[enc_pos + 1] as u16) << 8));
                enc_pos += 2;
            }
            _ => {
                if enc_pos >= enc.len() {
                    break;
                }
                let mut length = enc[enc_pos];
                enc_pos += 1;
                if length & 0x80 != 0 {
                    if enc_pos >= enc.len() {
                        break;
                    }
                    let correction = enc[enc_pos];
                    enc_pos += 1;
                    length = (length & 0x7f) + 2;
                    let mut dec_pos = out.len();
                    while length > 0 && dec_pos < name_size {
                        let byte = name.get(dec_pos).copied().unwrap_or(0);
                        out.push(
                            (byte.wrapping_add(correction)) as u16 | ((high_byte as u16) << 8),
                        );
                        length -= 1;
                        dec_pos += 1;
                    }
                } else {
                    length += 2;
                    let mut dec_pos = out.len();
                    while length > 0 && dec_pos < name_size {
                        let byte = name.get(dec_pos).copied().unwrap_or(0);
                        out.push(byte as u16);
                        length -= 1;
                        dec_pos += 1;
                    }
                }
            }
        }
        flags <<= 2;
        flag_bits -= 2;
    }
    Some(String::from_utf16_lossy(&out))
}

fn decode_rar4_name(raw: &[u8], flags: u16) -> String {
    let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let ansi = &raw[..nul];
    if flags & LHD_UNICODE != 0 && nul + 1 < raw.len() {
        if let Some(decoded) = decode_unicode_name(ansi, &raw[nul + 1..]) {
            if !decoded.is_empty() {
                return decoded;
            }
        }
    }
    latin1_to_string(ansi)
}

// ----------------------------------------------------------------------
// Volumes and paths
// ----------------------------------------------------------------------

fn next_volume_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();

    if let Some(part_pos) = lower.rfind(".part") {
        let after = &name[part_pos + 5..];
        if let Some(dot) = after.rfind(".rar") {
            let digits = &after[..dot];
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                let number: u64 = digits.parse().ok()?;
                let width = digits.len();
                let next = format!(
                    "{}.part{:0width$}.rar",
                    &name[..part_pos],
                    number + 1,
                    width = width
                );
                return Some(path.with_file_name(next));
            }
        }
    }

    if let Some(stem) = lower.strip_suffix(".rar") {
        let stem = &name[..stem.len()];
        return Some(path.with_file_name(format!("{stem}.r00")));
    }

    // Old style RAR volume names: .r00 .. .r99, .s00 .. .s99, ...
    if lower.len() >= 4 {
        let bytes = lower.as_bytes();
        let ext = &bytes[bytes.len() - 3..];
        if ext[0].is_ascii_lowercase()
            && ext[0] >= b'r'
            && ext[1..].iter().all(|b| b.is_ascii_digit())
        {
            let letter = ext[0];
            let number: u32 = lower[lower.len() - 2..].parse().ok()?;
            let (new_letter, new_number) = if number >= 99 {
                ((letter + 1) as char, 0)
            } else {
                (letter as char, number + 1)
            };
            let file_name = path.file_name()?.to_string_lossy().into_owned();
            let prefix = &file_name[..file_name.len() - 3];
            return Some(path.with_file_name(format!("{prefix}{new_letter}{new_number:02}")));
        }
    }

    None
}

fn check_parent_symlinks(dest: &Path, path: &Path) -> Result<()> {
    let relative = match path.strip_prefix(dest) {
        Ok(relative) => relative,
        Err(_) => return Ok(()),
    };
    let components: Vec<_> = relative.components().collect();
    let mut current = dest.to_path_buf();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        if let Ok(metadata) = std::fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(Error::Unsupported(format!(
                    "refusing to extract through symlink {}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn safe_join(dest: &Path, name: &str) -> Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    let mut out = dest.to_path_buf();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::Unsupported(format!(
                    "unsafe path in archive: {name}"
                )));
            }
        }
    }
    Ok(out)
}
