//! The CC0 corpus, by its lock file (`docs/CC0_CORPUS_MIGRATION_PLAN.md` §4.3).
//!
//! The documents are never committed; `testdata-lock/cc0.toml` is, and it is
//! embedded here. `cargo run -p xtool -- corpus fetch --tier ci-core` puts the
//! documents under `testdata/`, and a test asks for one by its lock id:
//!
//! ```no_run
//! # fn test() {
//! let doc = strict_ooxml_testkit::corpus_doc!("cc0-docx-1/076");
//! let bytes = std::fs::read(&doc.path).expect("read");
//! # let _ = bytes;
//! # }
//! ```
//!
//! What happens when a document is absent is decided by the environment, not
//! by the test (`STRICT_OOXML_CORPUS`):
//!
//! * unset or `skip` — the test prints `SKIP …` and returns (a clean clone);
//! * `require` — the test panics (CI, after `fetch`), so a missing document can
//!   never turn into a silent green.
//!
//! A document whose bytes do not match the lock's SHA-256 is a panic in both
//! modes: a wrong file is not an absent one.
//!
//! The corpus root is `STRICT_OOXML_CORPUS_ROOT` when set, `testdata/` at the
//! workspace root otherwise.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

/// The lock file, as committed.
pub const LOCK: &str = include_str!("../../testdata-lock/cc0.toml");

/// What a test does when a corpus document is absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Print `SKIP …` and return.
    Skip,
    /// Panic: the corpus was supposed to be fetched.
    Require,
}

impl Mode {
    /// The mode `STRICT_OOXML_CORPUS` selects (`require`, otherwise skip).
    #[must_use]
    pub fn from_env() -> Self {
        mode_from(std::env::var("STRICT_OOXML_CORPUS").ok().as_deref())
    }
}

fn mode_from(value: Option<&str>) -> Mode {
    match value.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("require") => Mode::Require,
        _ => Mode::Skip,
    }
}

/// A named subset of the lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// The small set every CI run fetches (about 11 MB).
    CiCore,
    /// Every document of the lock (nightly).
    CiFull,
}

impl Tier {
    /// The tier's name in the lock file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CiCore => "ci-core",
            Self::CiFull => "ci-full",
        }
    }
}

/// One `[[doc]]` of the lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockEntry {
    /// Stable id tests refer to (`cc0-docx-1/076`).
    pub id: String,
    /// Path under the corpus root.
    pub path: String,
    /// SHA-256 of the document, lower-case hex.
    pub sha256: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Tiers the document belongs to.
    pub tiers: Vec<String>,
    /// Constructs the document was found to contain (for choosing witnesses).
    pub features: Vec<String>,
}

/// A corpus document that is present and matches its lock entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusDoc {
    /// The lock id.
    pub id: String,
    /// Where it is on disk.
    pub path: PathBuf,
    /// Constructs it contains.
    pub features: Vec<String>,
}

/// Parses a lock file.
///
/// # Errors
///
/// A message naming the first malformed entry.
pub fn parse_lock(text: &str) -> Result<Vec<LockEntry>, String> {
    let value: toml::Value =
        toml::from_str(text).map_err(|error| format!("invalid TOML: {error}"))?;
    if value.get("schema").and_then(toml::Value::as_integer) != Some(1) {
        return Err("lock schema must be 1".to_owned());
    }
    let docs = value
        .get("doc")
        .and_then(toml::Value::as_array)
        .ok_or("lock has no [[doc]] entries")?;
    docs.iter()
        .enumerate()
        .map(|(index, doc)| {
            let text = |key: &str| {
                doc.get(key)
                    .and_then(toml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| format!("doc #{index}: missing string '{key}'"))
            };
            let list = |key: &str| {
                doc.get(key)
                    .and_then(toml::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(toml::Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let bytes = doc
                .get("bytes")
                .and_then(toml::Value::as_integer)
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| format!("doc #{index}: missing 'bytes'"))?;
            Ok(LockEntry {
                id: text("id")?,
                path: text("path")?,
                sha256: text("sha256")?.to_ascii_lowercase(),
                bytes,
                tiers: list("tiers"),
                features: list("features"),
            })
        })
        .collect()
}

/// Every entry of the embedded lock.
///
/// # Panics
///
/// If the committed lock does not parse (a test of this crate pins that it does).
#[must_use]
pub fn entries() -> &'static [LockEntry] {
    static ENTRIES: OnceLock<Vec<LockEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| match parse_lock(LOCK) {
        Ok(entries) => entries,
        Err(error) => panic!("testdata-lock/cc0.toml: {error}"),
    })
}

/// The corpus root (`STRICT_OOXML_CORPUS_ROOT`, else `<workspace>/testdata`).
#[must_use]
pub fn root() -> PathBuf {
    match std::env::var_os("STRICT_OOXML_CORPUS_ROOT") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata"),
    }
}

/// The document with lock id `id`, if it has been fetched.
///
/// # Panics
///
/// If `id` is not in the lock (a typo in the test), or if the file on disk does
/// not match the lock's size and SHA-256.
#[must_use]
pub fn doc(id: &str) -> Option<CorpusDoc> {
    let entry = entries()
        .iter()
        .find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("corpus id '{id}' is not in testdata-lock/cc0.toml"));
    present(entry, &root())
}

/// The fetched documents of `tier`.
///
/// # Panics
///
/// In [`Mode::Require`], if any document of the tier is absent; in both modes,
/// if a present document does not match its lock entry.
#[must_use]
pub fn tier(tier: Tier) -> Vec<CorpusDoc> {
    let root = root();
    let wanted: Vec<&LockEntry> = entries()
        .iter()
        .filter(|entry| entry.tiers.iter().any(|name| name == tier.as_str()))
        .collect();
    let found: Vec<CorpusDoc> = wanted
        .iter()
        .filter_map(|entry| present(entry, &root))
        .collect();
    assert!(
        found.len() == wanted.len() || Mode::from_env() == Mode::Skip,
        "STRICT_OOXML_CORPUS=require: {} of {} '{}' documents are missing under {} \
             (run `cargo run -p xtool -- corpus fetch --tier {}`)",
        wanted.len() - found.len(),
        wanted.len(),
        tier.as_str(),
        root.display(),
        tier.as_str()
    );
    found
}

/// The entry's document under `root`, verified once per process.
fn present(entry: &LockEntry, root: &Path) -> Option<CorpusDoc> {
    static VERIFIED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let path = root.join(&entry.path);
    if !path.is_file() {
        return None;
    }
    let verified = VERIFIED.get_or_init(|| Mutex::new(HashSet::new()));
    let known = verified
        .lock()
        .map(|seen| seen.contains(&path))
        .unwrap_or(false);
    if !known {
        if let Err(error) = verify_file(&path, entry) {
            panic!("{error}");
        }
        if let Ok(mut seen) = verified.lock() {
            seen.insert(path.clone());
        }
    }
    Some(CorpusDoc {
        id: entry.id.clone(),
        path,
        features: entry.features.clone(),
    })
}

/// Checks `path` against the entry's size and SHA-256.
///
/// # Errors
///
/// A message naming the document and what differs.
pub fn verify_file(path: &Path, entry: &LockEntry) -> Result<(), String> {
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        hasher.update(&buffer[..read]);
    }
    if total != entry.bytes {
        return Err(format!(
            "corpus document {} ({}) has {total} bytes, the lock says {}",
            entry.id,
            path.display(),
            entry.bytes
        ));
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    if hex != entry.sha256 {
        return Err(format!(
            "corpus document {} ({}) has sha256 {hex}, the lock says {}",
            entry.id,
            path.display(),
            entry.sha256
        ));
    }
    Ok(())
}

/// Returns the fetched corpus document `id`, or skips the test.
///
/// In [`Mode::Skip`] an absent document prints `SKIP …` and `return`s from the
/// calling test; in [`Mode::Require`] it panics. See the module documentation.
#[macro_export]
macro_rules! corpus_doc {
    ($id:expr) => {
        match $crate::corpus::doc($id) {
            Some(doc) => doc,
            None => match $crate::corpus::Mode::from_env() {
                $crate::corpus::Mode::Skip => {
                    eprintln!(
                        "SKIP corpus doc {} not fetched (run `cargo run -p xtool -- corpus fetch --tier ci-core`)",
                        $id
                    );
                    return;
                }
                $crate::corpus::Mode::Require => panic!(
                    "STRICT_OOXML_CORPUS=require: corpus doc {} is not under {}",
                    $id,
                    $crate::corpus::root().display()
                ),
            },
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{entries, mode_from, parse_lock, verify_file, LockEntry, Mode, Tier};

    #[test]
    fn the_committed_lock_parses() {
        let all = entries();
        assert_eq!(all.len(), 300);
        let core = all
            .iter()
            .filter(|entry| entry.tiers.iter().any(|t| t == Tier::CiCore.as_str()))
            .count();
        assert_eq!(core, 26);
        let mut ids: Vec<&str> = all.iter().map(|entry| entry.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "ids are unique");
        assert!(all.iter().all(|entry| entry.sha256.len() == 64));
    }

    #[test]
    fn a_malformed_lock_is_refused() {
        assert!(parse_lock("schema = 2\n[[doc]]\n").is_err());
        assert!(parse_lock("schema = 1\n").is_err());
        assert!(parse_lock("schema = 1\n[[doc]]\nid = \"x\"\n").is_err());
    }

    #[test]
    fn the_mode_comes_from_the_value() {
        assert_eq!(mode_from(None), Mode::Skip);
        assert_eq!(mode_from(Some("skip")), Mode::Skip);
        assert_eq!(mode_from(Some("REQUIRE")), Mode::Require);
        assert_eq!(mode_from(Some(" require ")), Mode::Require);
    }

    #[test]
    fn a_wrong_file_is_named() {
        let dir = std::env::temp_dir().join(format!("testkit-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("abc.docx");
        std::fs::write(&path, b"abc").expect("write");
        let entry = LockEntry {
            id: "t/abc".to_owned(),
            path: "abc.docx".to_owned(),
            // sha256("abc")
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
            bytes: 3,
            tiers: Vec::new(),
            features: Vec::new(),
        };
        assert_eq!(verify_file(&path, &entry), Ok(()));
        std::fs::write(&path, b"abd").expect("tamper");
        let error = verify_file(&path, &entry).expect_err("hash mismatch");
        assert!(error.contains("sha256"), "{error}");
        std::fs::write(&path, b"abcd").expect("grow");
        let error = verify_file(&path, &entry).expect_err("size mismatch");
        assert!(error.contains("4 bytes"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
