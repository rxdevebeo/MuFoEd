//! The drive buttons: drive letters on Windows, the root, the home folder
//! and mounted volumes elsewhere.

use std::path::PathBuf;

/// One drive button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// The button's text (`C`, `/`, `~`, a volume name).
    pub label: String,
    /// The folder it opens.
    pub root: PathBuf,
}

/// The drives present now.
pub fn drives() -> Vec<Drive> {
    let mut out = Vec::new();
    if cfg!(windows) {
        // A and B are floppy letters: probing an empty one can stall, so
        // they appear only when the system says they exist.
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", char::from(letter)));
            if root.exists() {
                out.push(Drive {
                    label: char::from(letter).to_string(),
                    root,
                });
            }
        }
    } else {
        out.push(Drive {
            label: "/".into(),
            root: PathBuf::from("/"),
        });
        if let Some(home) = std::env::var_os("HOME") {
            out.push(Drive {
                label: "~".into(),
                root: PathBuf::from(home),
            });
        }
        for mount in mounts() {
            if !out.iter().any(|drive| drive.root == mount) {
                let label = mount.file_name().map_or_else(
                    || mount.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                out.push(Drive { label, root: mount });
            }
        }
    }
    out
}

/// Mounted volumes worth a button: removable media and data disks under
/// `/media`, `/mnt` and `/run/media`.
fn mounts() -> Vec<PathBuf> {
    let Ok(table) = std::fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };
    table
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(|point| point.replace("\\040", " "))
        .filter(|point| {
            point.starts_with("/media/")
                || point.starts_with("/mnt/")
                || point.starts_with("/run/media/")
        })
        .map(PathBuf::from)
        .collect()
}

/// The drive a path is on, for highlighting its button.
pub fn drive_of(path: &std::path::Path, drives: &[Drive]) -> Option<usize> {
    drives
        .iter()
        .enumerate()
        .filter(|(_, drive)| path.starts_with(&drive.root))
        .max_by_key(|(_, drive)| drive.root.as_os_str().len())
        .map(|(index, _)| index)
}

/// Free and total bytes of the volume holding `path`, where the platform
/// tells without native calls (Unix `df`); `None` elsewhere.
pub fn space(path: &std::path::Path) -> Option<(u64, u64)> {
    if cfg!(windows) {
        return None;
    }
    let output = std::process::Command::new("df")
        .arg("-Pk")
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    let total = fields.get(1)?.parse::<u64>().ok()? * 1024;
    let free = fields.get(3)?.parse::<u64>().ok()? * 1024;
    Some((free, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_a_drive_and_paths_find_it() {
        let drives = drives();
        assert!(!drives.is_empty());
        let current = std::env::current_dir().expect("current folder");
        assert!(drive_of(&current, &drives).is_some());
    }
}
