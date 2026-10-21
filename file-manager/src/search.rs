//! File search on a worker thread: by name mask, contained text, size,
//! age and depth. Results arrive while the search runs.

use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use crate::entry::Entry;
use crate::mask::Mask;

/// What to look for.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Query {
    /// Where to start.
    pub root: PathBuf,
    /// Name mask (see [`Mask`]); empty matches all.
    pub mask: String,
    /// Text the file must contain; empty to skip the content check.
    pub text: String,
    /// Match the text's case exactly.
    pub case_sensitive: bool,
    /// Smallest size in bytes.
    pub min_size: Option<u64>,
    /// Largest size in bytes.
    pub max_size: Option<u64>,
    /// Changed within this many days.
    pub within_days: Option<u32>,
    /// Deepest folder level searched (0 = the root only).
    pub depth: Option<usize>,
    /// Report matching folders too.
    pub folders: bool,
    /// Enter hidden folders and report hidden files.
    pub hidden: bool,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            mask: "*".into(),
            text: String::new(),
            case_sensitive: false,
            min_size: None,
            max_size: None,
            within_days: None,
            depth: None,
            folders: false,
            hidden: true,
        }
    }
}

/// A running or finished search.
pub struct Search {
    results: Arc<Mutex<Vec<Entry>>>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    scanned: Arc<AtomicU64>,
    current: Arc<Mutex<PathBuf>>,
}

impl std::fmt::Debug for Search {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Search").finish_non_exhaustive()
    }
}

/// Largest number of results kept.
const MAX_RESULTS: usize = 100_000;

impl Search {
    /// Starts searching; `wake` is called now and then.
    pub fn start(query: Query, wake: impl Fn() + Send + 'static) -> Self {
        let search = Self {
            results: Arc::default(),
            cancel: Arc::default(),
            finished: Arc::default(),
            scanned: Arc::default(),
            current: Arc::default(),
        };
        let (results, cancel, finished, scanned, current) = (
            Arc::clone(&search.results),
            Arc::clone(&search.cancel),
            Arc::clone(&search.finished),
            Arc::clone(&search.scanned),
            Arc::clone(&search.current),
        );
        let spawned = std::thread::Builder::new()
            .name("file-manager-search".into())
            .spawn(move || {
                let worker = Worker {
                    query: &query,
                    mask: Mask::parse(&query.mask),
                    needle: needle(&query),
                    cutoff: query.within_days.and_then(|days| {
                        SystemTime::now().checked_sub(Duration::from_secs(u64::from(days) * 86_400))
                    }),
                    results: &results,
                    cancel: &cancel,
                    scanned: &scanned,
                    current: &current,
                    wake: &wake,
                };
                worker.run();
                finished.store(true, Ordering::Release);
                wake();
            });
        if spawned.is_err() {
            search.finished.store(true, Ordering::Release);
        }
        search
    }

    /// The results so far.
    pub fn results(&self) -> Vec<Entry> {
        self.results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many results there are so far.
    pub fn count(&self) -> usize {
        self.results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Items looked at.
    pub fn scanned(&self) -> u64 {
        self.scanned.load(Ordering::Relaxed)
    }

    /// The folder being searched.
    pub fn current(&self) -> PathBuf {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Stops the search.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether it has ended.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Waits for the end (tests).
    pub fn wait(&self) {
        while !self.is_finished() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The text to find as bytes, folded to lower case unless case matters.
fn needle(query: &Query) -> Option<Vec<u8>> {
    if query.text.is_empty() {
        return None;
    }
    Some(if query.case_sensitive {
        query.text.as_bytes().to_vec()
    } else {
        query.text.to_lowercase().into_bytes()
    })
}

struct Worker<'a> {
    query: &'a Query,
    mask: Mask,
    needle: Option<Vec<u8>>,
    cutoff: Option<SystemTime>,
    results: &'a Mutex<Vec<Entry>>,
    cancel: &'a AtomicBool,
    scanned: &'a AtomicU64,
    current: &'a Mutex<PathBuf>,
    wake: &'a (dyn Fn() + Send),
}

impl Worker<'_> {
    fn run(&self) {
        let mut pending = vec![(self.query.root.clone(), 0usize)];
        let mut last_wake = std::time::Instant::now();
        while let Some((folder, depth)) = pending.pop() {
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            folder.clone_into(&mut self.current.lock().unwrap_or_else(PoisonError::into_inner));
            let Ok(entries) = crate::entry::list(&folder, self.cancel) else {
                continue;
            };
            for entry in entries {
                self.scanned.fetch_add(1, Ordering::Relaxed);
                if entry.hidden && !self.query.hidden {
                    continue;
                }
                if entry.is_dir && !entry.link && self.query.depth.is_none_or(|limit| depth < limit)
                {
                    pending.push((entry.path.clone(), depth + 1));
                }
                if self.matches(&entry) {
                    let mut results = self.results.lock().unwrap_or_else(PoisonError::into_inner);
                    if results.len() >= MAX_RESULTS {
                        return;
                    }
                    results.push(entry);
                }
            }
            if last_wake.elapsed() > Duration::from_millis(150) {
                (self.wake)();
                last_wake = std::time::Instant::now();
            }
        }
    }

    fn matches(&self, entry: &Entry) -> bool {
        if entry.is_dir {
            return self.query.folders && self.needle.is_none() && self.mask.matches(&entry.name);
        }
        if !self.mask.matches(&entry.name) {
            return false;
        }
        if self.query.min_size.is_some_and(|min| entry.size < min)
            || self.query.max_size.is_some_and(|max| entry.size > max)
        {
            return false;
        }
        if let Some(cutoff) = self.cutoff {
            if entry.modified.is_none_or(|modified| modified < cutoff) {
                return false;
            }
        }
        match &self.needle {
            Some(needle) => contains(&entry.path, needle, !self.query.case_sensitive, self.cancel),
            None => true,
        }
    }
}

/// Whether the file holds `needle`, reading it in 1 MiB blocks that overlap
/// by the needle's length so a match across blocks is found.
pub fn contains(
    path: &std::path::Path,
    needle: &[u8],
    fold_case: bool,
    cancel: &AtomicBool,
) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut block = vec![0u8; (1 << 20) + needle.len()];
    let mut kept = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let read = match file.read(&mut block[kept..]) {
            Ok(0) | Err(_) => return false,
            Ok(read) => read,
        };
        let filled = kept + read;
        if fold_case {
            block[kept..filled].make_ascii_lowercase();
        }
        if block[..filled]
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return true;
        }
        kept = needle.len().saturating_sub(1).min(filled);
        block.copy_within(filled - kept..filled, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_name_text_size_and_depth() {
        let root = std::env::temp_dir().join(format!("file-manager-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).expect("folders");
        std::fs::write(root.join("top.txt"), b"nothing here").expect("file");
        std::fs::write(root.join("a/mid.txt"), b"The Needle is here").expect("file");
        std::fs::write(root.join("a/b/deep.log"), vec![b'x'; 2000]).expect("file");
        let run = |query: Query| {
            let search = Search::start(query, || {});
            search.wait();
            let mut names: Vec<String> = search
                .results()
                .into_iter()
                .map(|entry| entry.name)
                .collect();
            names.sort();
            names
        };
        let base = Query {
            root: root.clone(),
            ..Query::default()
        };
        assert_eq!(
            run(Query {
                mask: "*.txt".into(),
                ..base.clone()
            }),
            ["mid.txt", "top.txt"]
        );
        assert_eq!(
            run(Query {
                text: "needle".into(),
                ..base.clone()
            }),
            ["mid.txt"]
        );
        assert!(run(Query {
            text: "needle".into(),
            case_sensitive: true,
            ..base.clone()
        })
        .is_empty());
        assert_eq!(
            run(Query {
                min_size: Some(1000),
                ..base.clone()
            }),
            ["deep.log"]
        );
        assert_eq!(
            run(Query {
                depth: Some(0),
                ..base.clone()
            }),
            ["top.txt"]
        );
        assert_eq!(
            run(Query {
                mask: "a".into(),
                folders: true,
                ..base.clone()
            }),
            ["a"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn text_across_block_edges_is_found() {
        let path = std::env::temp_dir().join(format!("file-manager-edge-{}", std::process::id()));
        let mut data = vec![b'.'; (1 << 20) - 3];
        data.extend_from_slice(b"needle");
        std::fs::write(&path, &data).expect("file");
        assert!(contains(&path, b"needle", false, &AtomicBool::new(false)));
        assert!(!contains(
            &path,
            b"haystack",
            false,
            &AtomicBool::new(false)
        ));
        let _ = std::fs::remove_file(&path);
    }
}
