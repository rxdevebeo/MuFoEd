//! Content identities with a bounded stat cache for immutable model files.
//! Rehash after path, size, creation time or last-write time changes. A caller that
//! edits a file while preserving all timestamps must explicitly call `invalidate`.
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}
type Cache = HashMap<PathBuf, (Stamp, String)>;
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
/// Invalidate cached metadata after an application deliberately rewrites model files.
pub fn invalidate() {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}
/// Hash model bytes, including names/content of a flat prepared model directory.
/// The caller must serialize publication/replacement against active inference.
pub fn file(path: &Path) -> Result<String, String> {
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if path.is_dir() {
        let mut files = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if files.len() > 10000 {
            return Err("Model directory exceeds fingerprint limit".into());
        }
        files.sort();
        let mut hash = Sha256::new();
        for entry in files {
            if entry.is_file() {
                hash.update(
                    entry
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_bytes(),
                );
                hash.update(file(&entry)?.as_bytes());
            }
        }
        return Ok(format!("{:x}", hash.finalize()));
    }
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    let stamp = Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    };
    if let Some(value) = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&path)
        .filter(|(old, _)| *old == stamp)
        .map(|(_, digest)| digest.clone())
    {
        return Ok(value);
    }
    let mut source = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = source.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let after = source.metadata().map_err(|e| e.to_string())?;
    if after.len() != stamp.len || after.modified().ok() != stamp.modified {
        return Err("Model file changed while hashing; retry after publication completes".into());
    }
    let digest = format!("{:x}", hash.finalize());
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(path, (stamp, digest.clone()));
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_identity_invalidates_for_replaced_weights_and_explicit_rewrite() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model");
        std::fs::write(&path, b"first").unwrap();
        let first = file(&path).unwrap();
        assert_eq!(first, file(&path).unwrap());
        std::fs::write(&path, b"second weights").unwrap();
        assert_ne!(first, file(&path).unwrap());
        let before = path.metadata().unwrap().modified().unwrap();
        let digest = file(&path).unwrap();
        std::fs::write(&path, b"same size edit").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(before))
            .unwrap();
        invalidate();
        assert_ne!(digest, file(&path).unwrap());
    }
}
