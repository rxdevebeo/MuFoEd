//! File operations on a worker thread: copy, move, delete (to the recycle
//! bin or for good), new folder, rename, pack, extract and space counting.
//!
//! Copies stream in 1 MiB steps, so files of any size neither stall the
//! interface nor fill memory; every step checks for cancellation.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::archive::{self, Kind};
use crate::entry::{self, Usage};
use crate::progress::{self, Progress};

/// What to do when a target name exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Conflict {
    /// Replace the existing file.
    Overwrite,
    /// Keep the existing file.
    #[default]
    Skip,
    /// Replace it only if it is older than the source.
    OverwriteOlder,
    /// Give the new file a numbered name.
    Rename,
}

/// An operation to run.
#[derive(Clone, Debug)]
pub enum Operation {
    /// Copy files and folders into a folder.
    Copy {
        /// What to copy.
        sources: Vec<PathBuf>,
        /// The folder to copy into.
        destination: PathBuf,
        /// What to do with existing names.
        conflict: Conflict,
    },
    /// Move files and folders into a folder.
    Move {
        /// What to move.
        sources: Vec<PathBuf>,
        /// The folder to move into.
        destination: PathBuf,
        /// What to do with existing names.
        conflict: Conflict,
    },
    /// Delete files and folders.
    Delete {
        /// What to delete.
        sources: Vec<PathBuf>,
        /// For good instead of to the recycle bin.
        permanent: bool,
    },
    /// Create a folder (with missing parents).
    MakeFolder(PathBuf),
    /// Rename or move one item.
    Rename {
        /// The item.
        from: PathBuf,
        /// Its new path.
        to: PathBuf,
    },
    /// Pack into a new archive.
    Pack {
        /// What to pack.
        sources: Vec<PathBuf>,
        /// The archive to create.
        archive: PathBuf,
        /// Its kind.
        kind: Kind,
    },
    /// Extract members of an archive.
    Extract {
        /// The archive.
        archive: PathBuf,
        /// Member paths (all when `None`).
        members: Option<Vec<String>>,
        /// The folder to extract into.
        destination: PathBuf,
        /// Replace existing files.
        overwrite: bool,
    },
    /// Count the space taken by each path.
    Usage(Vec<PathBuf>),
}

impl Operation {
    /// A short English title (a catalog message ID).
    pub fn title(&self) -> &'static str {
        match self {
            Self::Copy { .. } => "Copying",
            Self::Move { .. } => "Moving",
            Self::Delete {
                permanent: true, ..
            } => "Deleting",
            Self::Delete { .. } => "Moving to the recycle bin",
            Self::MakeFolder(_) => "Creating the folder",
            Self::Rename { .. } => "Renaming",
            Self::Pack { .. } => "Packing",
            Self::Extract { .. } => "Extracting",
            Self::Usage(_) => "Counting",
        }
    }
}

/// What an operation achieved.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Files processed.
    pub files: u64,
    /// Bytes processed.
    pub bytes: u64,
    /// Items left alone because their name existed.
    pub skipped: u64,
    /// Failures with their item.
    pub errors: Vec<(PathBuf, String)>,
    /// The operation was cancelled.
    pub cancelled: bool,
    /// Space per path, for [`Operation::Usage`].
    pub usage: Vec<(PathBuf, Usage)>,
    /// Paths created (new folders, archives, copies of the sources).
    pub created: Vec<PathBuf>,
}

/// A running operation.
pub struct Task {
    /// Its title (a catalog message ID).
    pub title: &'static str,
    /// Its progress.
    pub progress: Arc<Progress>,
    finished: Arc<AtomicBool>,
    report: Arc<Mutex<Report>>,
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl Task {
    /// Starts `operation`; `wake` is called now and then (to repaint).
    pub fn start(operation: Operation, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let progress = Arc::new(Progress::default());
        let finished = Arc::new(AtomicBool::new(false));
        let report = Arc::new(Mutex::new(Report::default()));
        let title = operation.title();
        let (worker_progress, worker_finished, worker_report) = (
            Arc::clone(&progress),
            Arc::clone(&finished),
            Arc::clone(&report),
        );
        let spawned = std::thread::Builder::new()
            .name("file-manager-operation".into())
            .spawn(move || {
                let mut result = Report::default();
                // A panic in a codec must not leave the task running forever.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&operation, &worker_progress, &mut result);
                }));
                if outcome.is_err() {
                    result
                        .errors
                        .push((PathBuf::new(), "the operation failed unexpectedly".into()));
                }
                result.cancelled = worker_progress.cancelled();
                *worker_report.lock().unwrap_or_else(PoisonError::into_inner) = result;
                worker_finished.store(true, Ordering::Release);
                wake();
            });
        if spawned.is_err() {
            finished.store(true, Ordering::Release);
        }
        Self {
            title,
            progress,
            finished,
            report,
        }
    }

    /// Whether it has ended.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Asks it to stop.
    pub fn cancel(&self) {
        self.progress.cancel.store(true, Ordering::Relaxed);
    }

    /// The report (complete once finished).
    pub fn report(&self) -> Report {
        self.report
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits for the end (tests and command-line use).
    pub fn wait(&self) {
        while !self.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

fn run(operation: &Operation, progress: &Progress, report: &mut Report) {
    match operation {
        Operation::Copy {
            sources,
            destination,
            conflict,
        } => transfer(sources, destination, *conflict, false, progress, report),
        Operation::Move {
            sources,
            destination,
            conflict,
        } => transfer(sources, destination, *conflict, true, progress, report),
        Operation::Delete { sources, permanent } => delete(sources, *permanent, progress, report),
        Operation::MakeFolder(path) => match std::fs::create_dir_all(path) {
            Ok(()) => report.created.push(path.clone()),
            Err(error) => report.errors.push((path.clone(), error.to_string())),
        },
        Operation::Rename { from, to } => {
            if to.exists() && !same_file(from, to) {
                report.errors.push((to.clone(), "the name is taken".into()));
            } else {
                match std::fs::rename(from, to) {
                    Ok(()) => report.created.push(to.clone()),
                    Err(error) => report.errors.push((from.clone(), error.to_string())),
                }
            }
        }
        Operation::Pack {
            sources,
            archive,
            kind,
        } => match archive::pack(sources, archive, *kind, progress) {
            Ok(()) => report.created.push(archive.clone()),
            Err(error) => report.errors.push((archive.clone(), error.to_string())),
        },
        Operation::Extract {
            archive,
            members,
            destination,
            overwrite,
        } => match archive::extract(
            archive,
            members.as_deref(),
            destination,
            *overwrite,
            progress,
        ) {
            Ok(count) => {
                report.files = count as u64;
                report.created.push(destination.clone());
            }
            Err(error) => report.errors.push((archive.clone(), error.to_string())),
        },
        Operation::Usage(paths) => {
            for path in paths {
                if progress.cancelled() {
                    break;
                }
                progress.set_current(&path.display().to_string());
                let usage = entry::usage(path, &progress.cancel);
                report.files += usage.files;
                report.bytes += usage.bytes;
                report.usage.push((path.clone(), usage));
            }
        }
    }
    report.bytes = report
        .bytes
        .max(progress.bytes_done.load(Ordering::Relaxed));
}

/// Whether two paths name the same file (a case-only rename on Windows).
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `name (2).ext`, `name (3).ext`, ... : the first free name.
fn numbered(target: &Path) -> PathBuf {
    let stem = target
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = target
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| target.with_file_name(format!("{stem} ({n}){extension}")))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| target.to_path_buf())
}

/// Where a file goes under `conflict`; `None` to skip it.
fn resolve(source: &Path, target: &Path, conflict: Conflict) -> Option<PathBuf> {
    if !target.exists() {
        return Some(target.to_path_buf());
    }
    match conflict {
        Conflict::Overwrite => Some(target.to_path_buf()),
        Conflict::Skip => None,
        Conflict::Rename => Some(numbered(target)),
        Conflict::OverwriteOlder => {
            let modified = |path: &Path| {
                std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
            };
            (modified(source) > modified(target)).then(|| target.to_path_buf())
        }
    }
}

fn transfer(
    sources: &[PathBuf],
    destination: &Path,
    conflict: Conflict,
    moving: bool,
    progress: &Progress,
    report: &mut Report,
) {
    if let Err(error) = std::fs::create_dir_all(destination) {
        report
            .errors
            .push((destination.to_path_buf(), error.to_string()));
        return;
    }
    // Totals first, for a meaningful progress bar.
    let mut total = Usage::default();
    for source in sources {
        total += entry::usage(source, &progress.cancel);
    }
    progress.bytes_total.store(total.bytes, Ordering::Relaxed);
    progress.files_total.store(total.files, Ordering::Relaxed);
    for source in sources {
        if progress.cancelled() {
            return;
        }
        let Some(name) = source.file_name() else {
            continue;
        };
        let mut target = destination.join(name);
        if same_file(source, &target) {
            if moving {
                continue;
            }
            // Copying onto itself: a numbered copy beside it.
            target = numbered(&target);
        }
        if destination.starts_with(source) {
            report
                .errors
                .push((source.clone(), "cannot copy a folder into itself".into()));
            continue;
        }
        // A move within one volume is a rename: instant at any size.
        if moving && !target.exists() && std::fs::rename(source, &target).is_ok() {
            let usage = entry::usage(&target, &AtomicBool::new(false));
            progress.add_bytes(usage.bytes);
            progress
                .files_done
                .fetch_add(usage.files.max(1), Ordering::Relaxed);
            report.files += usage.files.max(1);
            report.created.push(target);
            continue;
        }
        let (errors_before, skipped_before) = (report.errors.len(), report.skipped);
        copy_tree(source, &target, conflict, progress, report);
        // The source goes only when everything in it arrived: skipped files
        // stay where they were.
        let complete = report.errors.len() == errors_before && report.skipped == skipped_before;
        if moving && complete && !progress.cancelled() {
            let removed = if source.is_dir() {
                std::fs::remove_dir_all(source)
            } else {
                std::fs::remove_file(source)
            };
            if let Err(error) = removed {
                report.errors.push((source.clone(), error.to_string()));
            }
        }
        report.created.push(target);
    }
}

fn copy_tree(
    source: &Path,
    target: &Path,
    conflict: Conflict,
    progress: &Progress,
    report: &mut Report,
) {
    let mut pending = vec![(source.to_path_buf(), target.to_path_buf())];
    while let Some((from, to)) = pending.pop() {
        if progress.cancelled() {
            return;
        }
        let metadata = match std::fs::symlink_metadata(&from) {
            Ok(metadata) => metadata,
            Err(error) => {
                report.errors.push((from, error.to_string()));
                continue;
            }
        };
        if metadata.is_dir() {
            if let Err(error) = std::fs::create_dir_all(&to) {
                report.errors.push((to, error.to_string()));
                continue;
            }
            match std::fs::read_dir(&from) {
                Ok(listing) => {
                    for item in listing.flatten() {
                        pending.push((item.path(), to.join(item.file_name())));
                    }
                }
                Err(error) => report.errors.push((from, error.to_string())),
            }
            continue;
        }
        let Some(target) = resolve(&from, &to, conflict) else {
            report.skipped += 1;
            progress.add_bytes(metadata.len());
            progress.file_done();
            continue;
        };
        progress.set_current(&from.display().to_string());
        match copy_file(&from, &target, progress) {
            Ok(bytes) => {
                report.files += 1;
                report.bytes += bytes;
                progress.file_done();
            }
            Err(error) => {
                let _ = std::fs::remove_file(&target);
                if !progress.cancelled() {
                    report.errors.push((from, error.to_string()));
                }
            }
        }
    }
}

/// Copies one file through a temporary name, keeping its modification time.
fn copy_file(from: &Path, to: &Path, progress: &Progress) -> std::io::Result<u64> {
    let partial = to.with_file_name(format!(
        ".{}.part",
        to.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let result = write_partial(from, &partial, progress).and_then(|bytes| {
        std::fs::rename(&partial, to)?;
        Ok(bytes)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn write_partial(from: &Path, partial: &Path, progress: &Progress) -> std::io::Result<u64> {
    let mut reader = File::open(from)?;
    let mut writer = BufWriter::with_capacity(1 << 20, File::create(partial)?);
    let bytes = progress::copy(&mut reader, &mut writer, progress)?;
    writer.flush()?;
    let file = writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?;
    if let Ok(modified) = std::fs::metadata(from).and_then(|metadata| metadata.modified()) {
        let _ = file.set_modified(modified);
    }
    Ok(bytes)
}

fn delete(sources: &[PathBuf], permanent: bool, progress: &Progress, report: &mut Report) {
    progress
        .files_total
        .store(sources.len() as u64, Ordering::Relaxed);
    if !permanent {
        // One call: the recycle bin groups the items into one undo step.
        match trash::delete_all(sources) {
            Ok(()) => report.files = sources.len() as u64,
            Err(error) => report.errors.push((PathBuf::new(), error.to_string())),
        }
        progress
            .files_done
            .store(sources.len() as u64, Ordering::Relaxed);
        return;
    }
    for source in sources {
        if progress.cancelled() {
            return;
        }
        progress.set_current(&source.display().to_string());
        let removed = match std::fs::symlink_metadata(source) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(source),
            Ok(_) => std::fs::remove_file(source),
            Err(error) => Err(error),
        };
        match removed {
            Ok(()) => report.files += 1,
            Err(error) => report.errors.push((source.clone(), error.to_string())),
        }
        progress.file_done();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("file-manager-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("left/folder/deep")).expect("folders");
        std::fs::create_dir_all(root.join("right")).expect("folder");
        std::fs::write(root.join("left/one.txt"), b"one").expect("file");
        std::fs::write(root.join("left/folder/deep/two.bin"), vec![7u8; 3 << 20]).expect("file");
        root
    }

    fn run_task(operation: Operation) -> Report {
        let task = Task::start(operation, || {});
        task.wait();
        task.report()
    }

    #[test]
    fn copies_large_files_and_trees() {
        let root = tree("copy");
        let report = run_task(Operation::Copy {
            sources: vec![root.join("left/one.txt"), root.join("left/folder")],
            destination: root.join("right"),
            conflict: Conflict::Skip,
        });
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.files, 2);
        assert_eq!(
            std::fs::metadata(root.join("right/folder/deep/two.bin"))
                .expect("copied")
                .len(),
            3 << 20
        );
        // Again: skipped, then renamed.
        let again = run_task(Operation::Copy {
            sources: vec![root.join("left/one.txt")],
            destination: root.join("right"),
            conflict: Conflict::Skip,
        });
        assert_eq!(again.skipped, 1);
        run_task(Operation::Copy {
            sources: vec![root.join("left/one.txt")],
            destination: root.join("right"),
            conflict: Conflict::Rename,
        });
        assert!(root.join("right/one (2).txt").exists());
        // Into itself is refused.
        let refused = run_task(Operation::Copy {
            sources: vec![root.join("left")],
            destination: root.join("left/folder"),
            conflict: Conflict::Skip,
        });
        assert_eq!(refused.errors.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn moves_renames_makes_and_deletes() {
        let root = tree("move");
        let report = run_task(Operation::Move {
            sources: vec![root.join("left/folder")],
            destination: root.join("right"),
            conflict: Conflict::Skip,
        });
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(!root.join("left/folder").exists());
        assert!(root.join("right/folder/deep/two.bin").exists());
        run_task(Operation::Rename {
            from: root.join("left/one.txt"),
            to: root.join("left/uno.txt"),
        });
        assert!(root.join("left/uno.txt").exists());
        run_task(Operation::MakeFolder(root.join("left/new/nested")));
        assert!(root.join("left/new/nested").is_dir());
        let usage = run_task(Operation::Usage(vec![root.join("right")]));
        assert_eq!(usage.usage[0].1.bytes, 3 << 20);
        let deleted = run_task(Operation::Delete {
            sources: vec![root.join("right/folder")],
            permanent: true,
        });
        assert_eq!(deleted.files, 1);
        assert!(!root.join("right/folder").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelling_stops_and_leaves_no_partial_file() {
        let root = tree("cancel");
        let task = Task::start(
            Operation::Copy {
                sources: vec![root.join("left/folder")],
                destination: root.join("right"),
                conflict: Conflict::Overwrite,
            },
            || {},
        );
        task.cancel();
        task.wait();
        assert!(task.report().cancelled);
        let leftovers: Vec<_> = std::fs::read_dir(root.join("right"))
            .expect("lists")
            .flatten()
            .filter(|item| item.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
