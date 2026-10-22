//! Archives, all in pure Rust: ZIP, 7z, TAR and its compressed variants
//! (gzip, bzip2, xz, zstd), and single compressed files.
//!
//! Archives are browsed like folders ([`list`] and [`children`]),
//! extracted ([`extract`]) and created ([`pack`]). Member paths are
//! checked before extraction: nothing is written outside the destination.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::entry::Entry;
use crate::progress::{self, Progress};

/// What an archive operation failed on.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    /// Reading or writing failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The archive format reported a problem.
    #[error("{0}")]
    Format(String),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

fn format_error(error: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::Format(error.to_string())
}

/// The archive kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// ZIP (also JAR, APK and similar).
    Zip,
    /// 7-Zip.
    SevenZ,
    /// Uncompressed TAR.
    Tar,
    /// TAR + gzip.
    TarGz,
    /// TAR + bzip2 (read only).
    TarBz2,
    /// TAR + xz.
    TarXz,
    /// TAR + zstd (read only).
    TarZst,
    /// One gzip-compressed file.
    Gz,
    /// One bzip2-compressed file (read only).
    Bz2,
    /// One xz-compressed file.
    Xz,
    /// One zstd-compressed file (read only).
    Zst,
}

impl Kind {
    /// The kind of a file name.
    pub fn of(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        let ends = |suffixes: &[&str]| suffixes.iter().any(|suffix| name.ends_with(suffix));
        Some(if ends(&[".tar.gz", ".tgz"]) {
            Self::TarGz
        } else if ends(&[".tar.bz2", ".tbz2", ".tbz"]) {
            Self::TarBz2
        } else if ends(&[".tar.xz", ".txz"]) {
            Self::TarXz
        } else if ends(&[".tar.zst", ".tzst"]) {
            Self::TarZst
        } else if ends(&[".tar"]) {
            Self::Tar
        } else if ends(&[
            ".zip", ".jar", ".apk", ".xpi", ".cbz", ".epub", ".nupkg", ".whl",
        ]) {
            Self::Zip
        } else if ends(&[".7z", ".cb7"]) {
            Self::SevenZ
        } else if ends(&[".gz"]) {
            Self::Gz
        } else if ends(&[".bz2"]) {
            Self::Bz2
        } else if ends(&[".xz"]) {
            Self::Xz
        } else if ends(&[".zst"]) {
            Self::Zst
        } else {
            return None;
        })
    }

    /// Kinds that [`pack`] writes, in menu order.
    pub const WRITABLE: [Self; 6] = [
        Self::Zip,
        Self::SevenZ,
        Self::Tar,
        Self::TarGz,
        Self::TarXz,
        Self::Gz,
    ];

    /// The usual extension (with the dot).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => ".zip",
            Self::SevenZ => ".7z",
            Self::Tar => ".tar",
            Self::TarGz => ".tar.gz",
            Self::TarBz2 => ".tar.bz2",
            Self::TarXz => ".tar.xz",
            Self::TarZst => ".tar.zst",
            Self::Gz => ".gz",
            Self::Bz2 => ".bz2",
            Self::Xz => ".xz",
            Self::Zst => ".zst",
        }
    }

    /// A display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::SevenZ => "7z",
            Self::Tar => "TAR",
            Self::TarGz => "TAR.GZ",
            Self::TarBz2 => "TAR.BZ2",
            Self::TarXz => "TAR.XZ",
            Self::TarZst => "TAR.ZST",
            Self::Gz => "GZ",
            Self::Bz2 => "BZ2",
            Self::Xz => "XZ",
            Self::Zst => "ZST",
        }
    }

    fn is_tar(self) -> bool {
        matches!(
            self,
            Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz | Self::TarZst
        )
    }
}

/// One member of an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Path inside the archive, `/`-separated, without a trailing slash.
    pub path: String,
    /// A folder.
    pub is_dir: bool,
    /// Uncompressed size.
    pub size: u64,
    /// Last change.
    pub modified: Option<SystemTime>,
}

/// The days since 1970-01-01 of a civil date (H. Hinnant).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn zip_time(time: zip::DateTime) -> Option<SystemTime> {
    let days = days_from_civil(
        i64::from(time.year()),
        i64::from(time.month()),
        i64::from(time.day()),
    );
    let seconds = days * 86_400
        + i64::from(time.hour()) * 3600
        + i64::from(time.minute()) * 60
        + i64::from(time.second());
    u64::try_from(seconds)
        .ok()
        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
}

/// A system time as a ZIP time (UTC; ZIP has no zone).
fn to_zip_time(time: SystemTime) -> Option<zip::DateTime> {
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let stamp =
        image_gallery::exif::Stamp::from_system_time(UNIX_EPOCH + Duration::from_secs(seconds))?;
    zip::DateTime::from_date_and_time(
        stamp.year,
        stamp.month,
        stamp.day,
        stamp.hour,
        stamp.minute,
        stamp.second,
    )
    .ok()
}

/// A decompressed stream of a compressed TAR or single file. xz needs a
/// writer to decompress into, so it goes through a temporary file.
fn decompressed(path: &Path, kind: Kind) -> Result<Box<dyn Read>, ArchiveError> {
    let file = BufReader::new(File::open(path)?);
    Ok(match kind {
        Kind::TarGz | Kind::Gz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        Kind::TarBz2 | Kind::Bz2 => Box::new(bzip2_rs::DecoderReader::new(file)),
        Kind::TarZst | Kind::Zst => {
            Box::new(ruzstd::decoding::StreamingDecoder::new(file).map_err(format_error)?)
        }
        Kind::TarXz | Kind::Xz => {
            let temporary = TempFile::new("xz")?;
            {
                let mut input = file;
                let mut output = BufWriter::new(File::create(&temporary.0)?);
                lzma_rs::xz_decompress(&mut input, &mut output).map_err(format_error)?;
                output.flush()?;
            }
            Box::new(TempReader {
                file: File::open(&temporary.0)?,
                _temporary: temporary,
            })
        }
        _ => Box::new(file),
    })
}

/// A temporary file removed on drop.
struct TempFile(PathBuf);

impl TempFile {
    fn new(tag: &str) -> std::io::Result<Self> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "file-manager-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        File::create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct TempReader {
    file: File,
    _temporary: TempFile,
}

impl Read for TempReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

/// The member name of a single compressed file: its name without the
/// compression extension.
fn single_name(path: &Path, kind: Kind) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = name.to_lowercase();
    match lower.strip_suffix(kind.extension()) {
        Some(stem) if !stem.is_empty() => name[..stem.len()].to_owned(),
        _ => format!("{name}.out"),
    }
}

/// Every member of an archive.
pub fn list(path: &Path) -> Result<Vec<Member>, ArchiveError> {
    let kind = Kind::of(path).ok_or_else(|| ArchiveError::Format("not an archive".into()))?;
    let mut members = Vec::new();
    match kind {
        Kind::Zip => {
            let mut archive =
                zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(format_error)?;
            for index in 0..archive.len() {
                let file = archive.by_index(index).map_err(format_error)?;
                let name = file.name().map_err(format_error)?.replace('\\', "/");
                members.push(Member {
                    path: name.trim_end_matches('/').to_owned(),
                    is_dir: file.is_dir(),
                    size: file.size(),
                    modified: file.last_modified().and_then(zip_time),
                });
            }
        }
        Kind::SevenZ => {
            let reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
                .map_err(format_error)?;
            for entry in &reader.archive().files {
                members.push(Member {
                    path: entry
                        .name
                        .replace('\\', "/")
                        .trim_end_matches('/')
                        .to_owned(),
                    is_dir: entry.is_directory,
                    size: entry.size,
                    modified: entry
                        .has_last_modified_date
                        .then(|| SystemTime::from(entry.last_modified_date)),
                });
            }
        }
        kind if kind.is_tar() => {
            let mut archive = tar::Archive::new(decompressed(path, kind)?);
            for entry in archive.entries()? {
                let entry = entry?;
                let header = entry.header();
                members.push(Member {
                    path: entry
                        .path()?
                        .to_string_lossy()
                        .replace('\\', "/")
                        .trim_end_matches('/')
                        .to_owned(),
                    is_dir: header.entry_type().is_dir(),
                    size: header.size().unwrap_or(0),
                    modified: header
                        .mtime()
                        .ok()
                        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds)),
                });
            }
        }
        _ => {
            // One compressed file: the size is known after decompressing.
            let mut stream = decompressed(path, kind)?;
            let size = std::io::copy(&mut stream, &mut std::io::sink())?;
            members.push(Member {
                path: single_name(path, kind),
                is_dir: false,
                size,
                modified: std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok(),
            });
        }
    }
    members.retain(|member| !member.path.is_empty());
    Ok(members)
}

/// The listing of one folder (`inner`, `""` for the top) of an archive,
/// with the folders that only appear in member paths.
pub fn children(members: &[Member], inner: &str) -> Vec<Entry> {
    let prefix = if inner.is_empty() {
        String::new()
    } else {
        format!("{}/", inner.trim_end_matches('/'))
    };
    let mut out: Vec<Entry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for member in members {
        let Some(rest) = member.path.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let (name, deeper) = match rest.split_once('/') {
            Some((name, _)) => (name, true),
            None => (rest, false),
        };
        if !seen.insert(name.to_owned()) {
            // An explicit folder member after implied ones: keep its date.
            if !deeper && member.is_dir {
                if let Some(entry) = out.iter_mut().find(|entry| entry.name == name) {
                    entry.modified = member.modified;
                }
            }
            continue;
        }
        out.push(Entry {
            name: name.to_owned(),
            path: PathBuf::from(format!("{prefix}{name}")),
            is_dir: deeper || member.is_dir,
            size: if deeper { 0 } else { member.size },
            modified: if deeper { None } else { member.modified },
            hidden: false,
            readonly: false,
            link: false,
        });
    }
    out
}

/// A member path as a relative path that stays inside its destination.
pub fn safe_relative(member: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(&member.replace('\\', "/")).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Whether `member` is selected: it is one of `selected` or lies inside one.
fn chosen(member: &str, selected: Option<&[String]>) -> Option<String> {
    let Some(selected) = selected else {
        return Some(member.to_owned());
    };
    selected.iter().find_map(|root| {
        let root = root.trim_end_matches('/');
        // The part kept: from the selected item's own name onwards.
        let parent_len = root.rfind('/').map_or(0, |slash| slash + 1);
        (member == root || member.starts_with(&format!("{root}/")))
            .then(|| member[parent_len..].to_owned())
    })
}

/// Extracts the members (all when `selected` is `None`; folders take their
/// contents) into `destination`, keeping paths relative to the selected
/// items' folder. Existing files are replaced only when `overwrite`.
/// Returns the files written.
pub fn extract(
    archive: &Path,
    selected: Option<&[String]>,
    destination: &Path,
    overwrite: bool,
    progress: &Progress,
) -> Result<usize, ArchiveError> {
    let kind = Kind::of(archive).ok_or_else(|| ArchiveError::Format("not an archive".into()))?;
    let mut written = 0;
    let mut write_member =
        |name: &str, is_dir: bool, reader: &mut dyn Read| -> Result<(), ArchiveError> {
            if progress.cancelled() {
                return Err(ArchiveError::Cancelled);
            }
            // Some archivers store Windows separators.
            let name = name.replace('\\', "/");
            let Some(relative) =
                chosen(name.trim_end_matches('/'), selected).and_then(|kept| safe_relative(&kept))
            else {
                return Ok(());
            };
            let target = destination.join(relative);
            progress.set_current(&name);
            if is_dir {
                std::fs::create_dir_all(&target)?;
                return Ok(());
            }
            if target.exists() && !overwrite {
                return Ok(());
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = BufWriter::new(File::create(&target)?);
            progress::copy(reader, &mut file, progress).map_err(|error| {
                if progress.cancelled() {
                    ArchiveError::Cancelled
                } else {
                    ArchiveError::Io(error)
                }
            })?;
            file.flush()?;
            progress.file_done();
            written += 1;
            Ok(())
        };
    match kind {
        Kind::Zip => {
            let mut zip =
                zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(format_error)?;
            let total: u64 = (0..zip.len())
                .filter_map(|index| zip.by_index(index).ok().map(|file| file.size()))
                .sum();
            progress
                .bytes_total
                .store(total, std::sync::atomic::Ordering::Relaxed);
            for index in 0..zip.len() {
                let mut file = zip.by_index(index).map_err(format_error)?;
                let name = file.name().map_err(format_error)?.into_owned();
                let is_dir = file.is_dir();
                write_member(&name, is_dir, &mut file)?;
            }
        }
        Kind::SevenZ => {
            let mut reader =
                sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty())
                    .map_err(format_error)?;
            let total: u64 = reader.archive().files.iter().map(|entry| entry.size).sum();
            progress
                .bytes_total
                .store(total, std::sync::atomic::Ordering::Relaxed);
            let mut failure = None;
            reader
                .for_each_entries(|entry, stream| {
                    match write_member(&entry.name, entry.is_directory, stream) {
                        Ok(()) => Ok(true),
                        Err(error) => {
                            failure = Some(error);
                            Ok(false)
                        }
                    }
                })
                .map_err(format_error)?;
            if let Some(error) = failure {
                return Err(error);
            }
        }
        kind if kind.is_tar() => {
            let mut tar = tar::Archive::new(decompressed(archive, kind)?);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let name = entry.path()?.to_string_lossy().replace('\\', "/");
                let kind = entry.header().entry_type();
                if kind.is_file() || kind.is_dir() || kind == tar::EntryType::Continuous {
                    write_member(&name, kind.is_dir(), &mut entry)?;
                }
            }
        }
        _ => {
            let name = single_name(archive, kind);
            let mut stream = decompressed(archive, kind)?;
            write_member(&name, false, &mut stream)?;
        }
    }
    Ok(written)
}

/// Reads one member into memory (for viewing), up to `limit` bytes.
pub fn read_member(archive: &Path, member: &str, limit: usize) -> Result<Vec<u8>, ArchiveError> {
    let folder = TempFolder::new()?;
    let progress = Progress::default();
    extract(
        archive,
        Some(&[member.to_owned()]),
        &folder.0,
        true,
        &progress,
    )?;
    let name = Path::new(member)
        .file_name()
        .ok_or_else(|| ArchiveError::Format("no such member".into()))?;
    let mut data = Vec::new();
    File::open(folder.0.join(name))?
        .take(limit as u64)
        .read_to_end(&mut data)?;
    Ok(data)
}

/// Extracts one member to a temporary folder kept until the returned guard
/// is dropped (for opening it in a viewer or editor).
pub fn extract_to_temporary(
    archive: &Path,
    member: &str,
) -> Result<(PathBuf, TempFolder), ArchiveError> {
    let folder = TempFolder::new()?;
    let progress = Progress::default();
    extract(
        archive,
        Some(&[member.to_owned()]),
        &folder.0,
        true,
        &progress,
    )?;
    let name = Path::new(member)
        .file_name()
        .ok_or_else(|| ArchiveError::Format("no such member".into()))?;
    Ok((folder.0.join(name), folder))
}

/// A temporary folder removed on drop.
#[derive(Debug)]
pub struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> std::io::Result<Self> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "file-manager-extract-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The files and folders to pack: every source, folders walked, with the
/// name each gets inside the archive (relative to the sources' folder).
fn walk(sources: &[PathBuf]) -> Vec<(PathBuf, String, bool)> {
    let mut out = Vec::new();
    for source in sources {
        let base = source.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut pending = vec![source.clone()];
        while let Some(path) = pending.pop() {
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let name = path
                .strip_prefix(&base)
                .unwrap_or(&path)
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if metadata.is_dir() {
                out.push((path.clone(), name, true));
                if let Ok(listing) = std::fs::read_dir(&path) {
                    let mut children: Vec<PathBuf> =
                        listing.flatten().map(|item| item.path()).collect();
                    children.sort();
                    pending.extend(children.into_iter().rev());
                }
            } else if metadata.is_file() {
                out.push((path, name, false));
            }
        }
    }
    out
}

/// Packs `sources` into a new archive of `kind`.
pub fn pack(
    sources: &[PathBuf],
    archive: &Path,
    kind: Kind,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    let items = walk(sources);
    let total: u64 = items
        .iter()
        .filter(|(_, _, is_dir)| !is_dir)
        .filter_map(|(path, _, _)| std::fs::metadata(path).ok().map(|metadata| metadata.len()))
        .sum();
    progress
        .bytes_total
        .store(total, std::sync::atomic::Ordering::Relaxed);
    progress.files_total.store(
        items.iter().filter(|item| !item.2).count() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    let result = pack_items(&items, archive, kind, progress);
    if result.is_err() {
        let _ = std::fs::remove_file(archive);
    }
    result
}

#[allow(clippy::too_many_lines)]
fn pack_items(
    items: &[(PathBuf, String, bool)],
    archive: &Path,
    kind: Kind,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    let check = || {
        if progress.cancelled() {
            Err(ArchiveError::Cancelled)
        } else {
            Ok(())
        }
    };
    let copy = |reader: &mut dyn Read, writer: &mut dyn Write| {
        progress::copy(reader, writer, progress).map_err(|error| {
            if progress.cancelled() {
                ArchiveError::Cancelled
            } else {
                ArchiveError::Io(error)
            }
        })
    };
    match kind {
        Kind::Zip => {
            let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(archive)?));
            for (path, name, is_dir) in items {
                check()?;
                progress.set_current(name);
                let mut options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .large_file(true);
                if let Some(time) = std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(to_zip_time)
                {
                    options = options.last_modified_time(time);
                }
                if *is_dir {
                    zip.add_directory(format!("{name}/"), options)
                        .map_err(format_error)?;
                } else {
                    zip.start_file(name.clone(), options)
                        .map_err(format_error)?;
                    copy(&mut File::open(path)?, &mut zip)?;
                    progress.file_done();
                }
            }
            zip.finish().map_err(format_error)?.flush()?;
        }
        Kind::SevenZ => {
            let mut writer = sevenz_rust2::ArchiveWriter::create(archive).map_err(format_error)?;
            for (path, name, is_dir) in items {
                check()?;
                progress.set_current(name);
                if *is_dir {
                    writer
                        .push_archive_entry::<File>(
                            sevenz_rust2::ArchiveEntry::new_directory(name),
                            None,
                        )
                        .map_err(format_error)?;
                } else {
                    let entry = sevenz_rust2::ArchiveEntry::from_path(path, name.clone());
                    let file = CountingReader {
                        inner: File::open(path)?,
                        progress,
                    };
                    writer
                        .push_archive_entry(entry, Some(file))
                        .map_err(format_error)?;
                    progress.file_done();
                }
            }
            writer.finish()?;
        }
        Kind::Tar | Kind::TarGz | Kind::TarXz => {
            let output: Box<dyn Write> = match kind {
                Kind::TarGz => Box::new(flate2::write::GzEncoder::new(
                    BufWriter::new(File::create(archive)?),
                    flate2::Compression::default(),
                )),
                // xz compresses from a reader: the TAR goes to a temporary file first.
                Kind::TarXz => Box::new(BufWriter::new(File::create(
                    archive.with_extension("tar-xz-part"),
                )?)),
                _ => Box::new(BufWriter::new(File::create(archive)?)),
            };
            let mut builder = tar::Builder::new(output);
            builder.follow_symlinks(false);
            for (path, name, is_dir) in items {
                check()?;
                progress.set_current(name);
                if *is_dir {
                    builder.append_dir(name, path)?;
                } else {
                    let mut header = tar::Header::new_gnu();
                    let metadata = std::fs::metadata(path)?;
                    header.set_metadata(&metadata);
                    let mut file = CountingReader {
                        inner: File::open(path)?,
                        progress,
                    };
                    builder.append_data(&mut header, name, &mut file)?;
                    progress.file_done();
                }
            }
            let mut output = builder.into_inner()?;
            output.flush()?;
            drop(output);
            if kind == Kind::TarXz {
                let part = archive.with_extension("tar-xz-part");
                let result = xz_file(&part, archive);
                let _ = std::fs::remove_file(&part);
                result?;
            }
        }
        Kind::Gz => {
            let [(path, name, false)] = items else {
                return Err(ArchiveError::Format("gzip holds exactly one file".into()));
            };
            progress.set_current(name);
            let mut encoder = flate2::write::GzEncoder::new(
                BufWriter::new(File::create(archive)?),
                flate2::Compression::default(),
            );
            copy(&mut File::open(path)?, &mut encoder)?;
            encoder.finish()?.flush()?;
            progress.file_done();
        }
        Kind::Xz => {
            let [(path, name, false)] = items else {
                return Err(ArchiveError::Format("xz holds exactly one file".into()));
            };
            progress.set_current(name);
            let mut input = BufReader::new(CountingReader {
                inner: File::open(path)?,
                progress,
            });
            let mut output = BufWriter::new(File::create(archive)?);
            lzma_rs::xz_compress(&mut input, &mut output)?;
            output.flush()?;
            progress.file_done();
        }
        Kind::TarBz2 | Kind::TarZst | Kind::Bz2 | Kind::Zst => {
            return Err(ArchiveError::Format(format!(
                "{} archives are read only",
                kind.name()
            )));
        }
    }
    Ok(())
}

/// Compresses the file `input` into `output` with xz.
fn xz_file(input: &Path, output: &Path) -> Result<(), ArchiveError> {
    let mut reader = BufReader::new(File::open(input)?);
    let mut writer = BufWriter::new(File::create(output)?);
    lzma_rs::xz_compress(&mut reader, &mut writer)?;
    writer.flush()?;
    Ok(())
}

/// A reader that counts bytes into the progress and stops when cancelled.
struct CountingReader<'a, R> {
    inner: R,
    progress: &'a Progress,
}

impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.progress.cancelled() {
            return Err(progress::cancelled_error());
        }
        let read = self.inner.read(buffer)?;
        self.progress.add_bytes(read as u64);
        Ok(read)
    }
}

impl<R: Seek> Seek for CountingReader<'_, R> {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "file-manager-archive-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src/inner")).expect("folders");
        std::fs::write(path.join("src/a.txt"), b"alpha").expect("file");
        std::fs::write(path.join("src/inner/b.txt"), vec![b'b'; 5000]).expect("file");
        path
    }

    #[test]
    fn every_writable_kind_round_trips() {
        let root = folder("round");
        for kind in Kind::WRITABLE {
            let archive = root.join(format!("out{}", kind.extension()));
            let sources = if kind == Kind::Gz {
                vec![root.join("src/a.txt")]
            } else {
                vec![root.join("src")]
            };
            pack(&sources, &archive, kind, &Progress::default())
                .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            assert_eq!(Kind::of(&archive), Some(kind));
            let members = list(&archive).unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            if kind == Kind::Gz {
                assert_eq!(members[0].path, "out");
                assert_eq!(members[0].size, 5);
                continue;
            }
            assert!(
                members
                    .iter()
                    .any(|member| member.path == "src/inner/b.txt" && member.size == 5000),
                "{kind:?}: {members:?}"
            );
            let top = children(&members, "");
            assert_eq!(top.len(), 1);
            assert!(top[0].is_dir);
            let inside = children(&members, "src");
            assert_eq!(inside.len(), 2, "{kind:?}: {inside:?}");
            let target = root.join(format!("x-{}", kind.name()));
            let written = extract(
                &archive,
                Some(&["src/inner".to_owned()]),
                &target,
                false,
                &Progress::default(),
            )
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            assert_eq!(written, 1, "{kind:?}");
            assert_eq!(
                std::fs::read(target.join("inner/b.txt"))
                    .expect("extracted")
                    .len(),
                5000
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hostile_paths_are_refused() {
        assert_eq!(safe_relative("a/b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(safe_relative("../etc/passwd"), None);
        assert_eq!(safe_relative("/etc/passwd"), None);
        assert_eq!(safe_relative("./a"), Some(PathBuf::from("a")));
        assert_eq!(
            chosen("dir/sub/f", Some(&["dir/sub".to_owned()])),
            Some("sub/f".to_owned())
        );
        assert_eq!(chosen("dir/other", Some(&["dir/sub".to_owned()])), None);
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }
}
