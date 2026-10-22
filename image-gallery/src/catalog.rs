//! Listing a folder's pictures, sorting and filtering them.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::Format;

/// One picture file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Full path.
    pub path: PathBuf,
    /// File name, for display and sorting.
    pub name: String,
    /// Format by extension.
    pub format: Format,
    /// Size in bytes.
    pub size: u64,
    /// Last modification time.
    pub modified: Option<SystemTime>,
}

/// What the gallery sorts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// File name, numbers in names compared by value.
    Name,
    /// Modification time.
    Modified,
    /// File size.
    Size,
    /// Format, then name.
    Type,
}

impl SortKey {
    /// Every key, for menus.
    pub const ALL: [Self; 4] = [Self::Name, Self::Modified, Self::Size, Self::Type];
}

/// Most files one listing returns, so a mistaken pick of a huge tree does
/// not exhaust memory.
pub const MAX_ENTRIES: usize = 200_000;
/// Deepest folder level a recursive listing enters.
const MAX_DEPTH: usize = 16;

/// Lists the pictures in `folder` (and its sub-folders if `recursive`),
/// sorted by name. Hidden files and unreadable entries are skipped.
pub fn scan(folder: &Path, recursive: bool) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut pending = vec![(folder.to_path_buf(), 0)];
    let mut first = true;
    while let Some((dir, depth)) = pending.pop() {
        let listing = match std::fs::read_dir(dir) {
            Ok(listing) => listing,
            // The chosen folder itself must be readable; sub-folders may not be.
            Err(error) if first => return Err(error),
            Err(_) => continue,
        };
        first = false;
        for item in listing.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = item.file_type() else {
                continue;
            };
            let path = item.path();
            if kind.is_dir() {
                if recursive && depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            let Some(format) = Format::from_path(&path) else {
                continue;
            };
            let metadata = item.metadata().ok();
            out.push(Entry {
                name: if depth == 0 {
                    name
                } else {
                    path.strip_prefix(folder)
                        .map_or(name, |relative| relative.to_string_lossy().into_owned())
                },
                path,
                format,
                size: metadata.as_ref().map_or(0, std::fs::Metadata::len),
                modified: metadata.and_then(|metadata| metadata.modified().ok()),
            });
            if out.len() >= MAX_ENTRIES {
                sort(&mut out, SortKey::Name, false);
                return Ok(out);
            }
        }
    }
    sort(&mut out, SortKey::Name, false);
    Ok(out)
}

/// Sorts entries in place; ties fall back to the name.
pub fn sort(entries: &mut [Entry], key: SortKey, descending: bool) {
    entries.sort_by(|a, b| {
        let primary = match key {
            SortKey::Name => Ordering::Equal,
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Type => a.format.cmp(&b.format),
        };
        let order = primary.then_with(|| natural_cmp(&a.name, &b.name));
        if descending {
            order.reverse()
        } else {
            order
        }
    });
}

/// Compares names the way people read them: case-insensitively, with runs of
/// digits compared by value (`img2` before `img10`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(first), Some(second)) if first.is_ascii_digit() && second.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(digit) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(digit);
                        chars.next();
                    }
                    digits
                };
                let (one, other) = (take(&mut left), take(&mut right));
                let (one_value, other_value) =
                    (one.trim_start_matches('0'), other.trim_start_matches('0'));
                let order = one_value
                    .len()
                    .cmp(&other_value.len())
                    .then_with(|| one_value.cmp(other_value))
                    .then_with(|| one.len().cmp(&other.len()));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(first), Some(second)) => {
                let order = first.to_lowercase().cmp(second.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                left.next();
                right.next();
            }
        }
    }
}

/// Whether `entry` matches a filter text: every blank-separated word must
/// occur in the name, case-insensitively.
pub fn matches(entry: &Entry, filter: &str) -> bool {
    let name = entry.name.to_lowercase();
    filter
        .split_whitespace()
        .all(|word| name.contains(&word.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut names = vec!["img10.png", "IMG2.png", "img1.png", "img02.png", "a.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            ["a.png", "img1.png", "IMG2.png", "img02.png", "img10.png"]
        );
    }

    #[test]
    fn scans_pictures_only() {
        let folder =
            std::env::temp_dir().join(format!("image-gallery-scan-{}", std::process::id()));
        let nested = folder.join("sub");
        std::fs::create_dir_all(&nested).expect("folders");
        for name in ["b.png", "a.JPG", "notes.txt", ".hidden.png"] {
            std::fs::write(folder.join(name), b"x").expect("file");
        }
        std::fs::write(nested.join("c.webp"), b"x").expect("file");
        let flat = scan(&folder, false).expect("scans");
        let names: Vec<_> = flat.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["a.JPG", "b.png"]);
        let deep = scan(&folder, true).expect("scans");
        assert_eq!(deep.len(), 3);
        assert!(deep.iter().any(|entry| entry.format == Format::WebP));
        assert!(matches(&flat[0], "a jp"));
        assert!(!matches(&flat[0], "b"));
        assert!(scan(&folder.join("missing"), false).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
