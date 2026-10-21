//! Directory listings: entries, sorting and the background lister.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// One item of a listing: a file, a folder, or an archive member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The name shown (for archive members, the last path component).
    pub name: String,
    /// Where it is: a file system path, or the member's path inside the
    /// archive for archive listings.
    pub path: PathBuf,
    /// A folder.
    pub is_dir: bool,
    /// Size in bytes (folders: 0 until counted).
    pub size: u64,
    /// Last change.
    pub modified: Option<SystemTime>,
    /// Hidden (dot file, or the Windows hidden attribute).
    pub hidden: bool,
    /// Read-only.
    pub readonly: bool,
    /// A symbolic link.
    pub link: bool,
}

impl Entry {
    /// The extension, lower case, without the dot ("" for folders).
    pub fn extension(&self) -> String {
        if self.is_dir {
            return String::new();
        }
        Path::new(&self.name)
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    /// The name without its extension (whole name for folders).
    pub fn stem(&self) -> &str {
        if self.is_dir {
            return &self.name;
        }
        match self.name.rfind('.') {
            Some(dot) if dot > 0 => &self.name[..dot],
            _ => &self.name,
        }
    }

    /// Reads one file system entry.
    pub fn from_path(path: &Path) -> Option<Self> {
        let link = std::fs::symlink_metadata(path)
            .ok()?
            .file_type()
            .is_symlink();
        let metadata = std::fs::metadata(path)
            .or_else(|_| std::fs::symlink_metadata(path))
            .ok()?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        Some(Self {
            hidden: name.starts_with('.') || hidden_attribute(&metadata),
            readonly: metadata.permissions().readonly(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified: metadata.modified().ok(),
            path: path.to_path_buf(),
            name,
            link,
        })
    }
}

#[cfg(windows)]
fn hidden_attribute(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM
    metadata.file_attributes() & 0x6 != 0
}

#[cfg(not(windows))]
fn hidden_attribute(_: &std::fs::Metadata) -> bool {
    false
}

/// A column to sort by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortKey {
    /// Name, natural order.
    #[default]
    Name,
    /// Extension, then name.
    Extension,
    /// Size.
    Size,
    /// Date.
    Modified,
}

/// Sorts folders first, then files, by `key`.
pub fn sort(entries: &mut [Entry], key: SortKey, descending: bool) {
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            let primary = match key {
                SortKey::Name => Ordering::Equal,
                SortKey::Extension => a.extension().cmp(&b.extension()),
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Modified => a.modified.cmp(&b.modified),
            };
            let order = primary.then_with(|| image_gallery::catalog::natural_cmp(&a.name, &b.name));
            if descending {
                order.reverse()
            } else {
                order
            }
        })
    });
}

/// A listing in progress or done.
pub struct Listing {
    receiver: Receiver<std::io::Result<Vec<Entry>>>,
    cancel: Arc<AtomicBool>,
}

impl std::fmt::Debug for Listing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listing").finish_non_exhaustive()
    }
}

impl Listing {
    /// Lists `folder` on a worker thread; `wake` is called when done.
    pub fn start(folder: &Path, wake: impl Fn() + Send + 'static) -> Self {
        let (sender, receiver) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let folder = folder.to_path_buf();
        let _ = std::thread::Builder::new()
            .name("file-manager-list".into())
            .spawn(move || {
                let result = list(&folder, &stop);
                let _ = sender.send(result);
                wake();
            });
        Self { receiver, cancel }
    }

    /// The result, once there.
    pub fn poll(&self) -> Option<std::io::Result<Vec<Entry>>> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for Listing {
    fn drop(&mut self) {
        self.cancel.store(true, AtomicOrdering::Relaxed);
    }
}

/// Lists a folder (not recursively), stopping early when `cancel` is set.
pub fn list(folder: &Path, cancel: &AtomicBool) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(folder)? {
        if cancel.load(AtomicOrdering::Relaxed) {
            break;
        }
        let Ok(item) = item else {
            continue;
        };
        let path = item.path();
        let Ok(file_type) = item.file_type() else {
            continue;
        };
        // DirEntry metadata is cheap on Windows (from the directory scan);
        // links are followed for the size and kind.
        let metadata = if file_type.is_symlink() {
            std::fs::metadata(&path).or_else(|_| item.metadata())
        } else {
            item.metadata()
        };
        let Ok(metadata) = metadata else {
            continue;
        };
        let name = item.file_name().to_string_lossy().into_owned();
        out.push(Entry {
            hidden: name.starts_with('.') || hidden_attribute(&metadata),
            readonly: metadata.permissions().readonly(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified: metadata.modified().ok(),
            path,
            name,
            link: file_type.is_symlink(),
        });
    }
    Ok(out)
}

/// What a folder holds: files, folders and bytes, counted recursively.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Files.
    pub files: u64,
    /// Folders (not counting the top one).
    pub folders: u64,
    /// Bytes of all files.
    pub bytes: u64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Self) {
        self.files += other.files;
        self.folders += other.folders;
        self.bytes += other.bytes;
    }
}

/// Counts what `path` holds (a file counts itself). Links are not followed
/// into, so cycles cannot occur.
pub fn usage(path: &Path, cancel: &AtomicBool) -> Usage {
    let mut total = Usage::default();
    let mut pending = vec![path.to_path_buf()];
    while let Some(next) = pending.pop() {
        if cancel.load(AtomicOrdering::Relaxed) {
            break;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&next) else {
            continue;
        };
        if metadata.is_dir() {
            if next != path {
                total.folders += 1;
            }
            if let Ok(listing) = std::fs::read_dir(&next) {
                pending.extend(listing.flatten().map(|item| item.path()));
            }
        } else {
            total.files += 1;
            total.bytes += metadata.len();
        }
    }
    total
}

/// Bytes as B, KB, MB, GB, TB.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    // Display only: precision loss above 2^52 bytes is irrelevant.
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Bytes with thin-space thousands separators, as TC shows them.
pub fn grouped(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push('\u{2009}');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            size,
            modified: None,
            hidden: false,
            readonly: false,
            link: false,
        }
    }

    #[test]
    fn folders_come_first_and_names_sort_naturally() {
        let mut entries = vec![
            entry("b10.txt", false, 5),
            entry("zz", true, 0),
            entry("b2.txt", false, 50),
            entry("a.md", false, 1),
        ];
        sort(&mut entries, SortKey::Name, false);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zz", "a.md", "b2.txt", "b10.txt"]);
        sort(&mut entries, SortKey::Size, true);
        assert_eq!(entries[0].name, "zz");
        assert_eq!(entries[1].name, "b2.txt");
        sort(&mut entries, SortKey::Extension, false);
        assert_eq!(entries[1].name, "a.md");
        assert_eq!(entries[1].extension(), "md");
        assert_eq!(entries[1].stem(), "a");
    }

    #[test]
    fn lists_and_counts() {
        let folder = std::env::temp_dir().join(format!("file-manager-list-{}", std::process::id()));
        let nested = folder.join("sub");
        std::fs::create_dir_all(&nested).expect("folders");
        std::fs::write(folder.join("a.txt"), b"12345").expect("file");
        std::fs::write(nested.join("b.bin"), vec![0u8; 100]).expect("file");
        let cancel = AtomicBool::new(false);
        let entries = list(&folder, &cancel).expect("lists");
        assert_eq!(entries.len(), 2);
        let usage = usage(&folder, &cancel);
        assert_eq!(
            usage,
            Usage {
                files: 2,
                folders: 1,
                bytes: 105
            }
        );
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(grouped(1_234_567), "1\u{2009}234\u{2009}567");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
