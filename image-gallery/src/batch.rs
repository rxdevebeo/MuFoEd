//! Batch processing: recipes, copying, moving and renaming many files on
//! worker threads, with progress and cancellation.
//!
//! A [`Recipe`] is a list of [`Operation`]s and an [`Output`]: the format,
//! where results go, how they are named and what happens when a name is
//! taken. Wherever the result can be produced without decoding, it is:
//!
//! - no operations and nothing to change: the bytes are copied;
//! - only quarter turns and mirrors of a JPEG: the EXIF orientation changes;
//! - only metadata removal from a JPEG or PNG: segments are dropped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::decode::{self, LoadOptions};
use crate::encode::{self, EncodeOptions, OutputFormat};
use crate::exif::Stamp;
use crate::naming::{self, NameContext};
use crate::ops::{self, Filter, Operation, ResizeMode, Tools};
use crate::{lossless, Error, Format};

/// Where results are written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    /// Next to the source.
    SameFolder,
    /// A folder of this name inside the source's folder (created as needed).
    Subfolder(String),
    /// This folder.
    Folder(PathBuf),
}

/// What happens when the target name is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// Leave the existing file and skip this one.
    Skip,
    /// Replace the existing file (in place when it is the source).
    Overwrite,
    /// Add ` (2)`, ` (3)`, ... to the new name.
    Unique,
}

/// How results are written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    /// The target format.
    pub format: OutputFormat,
    /// Per-format options and the metadata policy.
    pub encode: EncodeOptions,
    /// Where results go.
    pub destination: Destination,
    /// The file name template, without extension (see [`naming`]).
    pub template: String,
    /// The first value of `{n}`.
    pub start_index: usize,
    /// What happens when a name is taken.
    pub conflict: Conflict,
    /// Give results the source's modification time.
    pub keep_modified_time: bool,
    /// Record JPEG turns in the EXIF orientation instead of re-encoding.
    pub lossless_jpeg: bool,
    /// Apply the EXIF orientation to the pixels before processing.
    pub auto_orient: bool,
    /// Scale of vector sources (SVG) relative to their own size.
    pub vector_scale: f32,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            format: OutputFormat::Keep,
            encode: EncodeOptions::default(),
            destination: Destination::Subfolder("processed".into()),
            template: "{name}".into(),
            start_index: 1,
            conflict: Conflict::Unique,
            keep_modified_time: false,
            lossless_jpeg: true,
            auto_orient: true,
            vector_scale: 1.0,
        }
    }
}

/// A named list of operations and the output settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    /// Name shown in the preset list.
    pub name: String,
    /// The operations, in order.
    pub operations: Vec<Operation>,
    /// How results are written.
    pub output: Output,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            name: "Custom".into(),
            operations: Vec::new(),
            output: Output::default(),
        }
    }
}

impl Recipe {
    /// A recipe that changes the files in place: same folder, same name,
    /// same format, the original replaced.
    pub fn in_place(operations: Vec<Operation>) -> Self {
        Self {
            name: "In place".into(),
            operations,
            output: Output {
                destination: Destination::SameFolder,
                conflict: Conflict::Overwrite,
                keep_modified_time: true,
                ..Output::default()
            },
        }
    }

    /// Ready-made recipes for common jobs. Their names are catalog message
    /// IDs.
    pub fn presets() -> Vec<Self> {
        let jpeg = |quality| EncodeOptions {
            jpeg_quality: quality,
            ..EncodeOptions::default()
        };
        let resize = |mode| Operation::Resize {
            mode,
            filter: Filter::Lanczos,
            enlarge: false,
        };
        vec![
            Self {
                name: "Web: JPEG, 1920 px".into(),
                operations: vec![resize(ResizeMode::LongSide { size: 1920 })],
                output: Output {
                    format: OutputFormat::To(Format::Jpeg),
                    encode: jpeg(85),
                    destination: Destination::Subfolder("web".into()),
                    ..Output::default()
                },
            },
            Self {
                name: "Thumbnails 320 px".into(),
                operations: vec![resize(ResizeMode::Fit {
                    width: 320,
                    height: 320,
                })],
                output: Output {
                    format: OutputFormat::To(Format::Jpeg),
                    encode: EncodeOptions {
                        keep_exif: false,
                        ..jpeg(80)
                    },
                    destination: Destination::Subfolder("thumbnails".into()),
                    template: "{name}_thumb".into(),
                    ..Output::default()
                },
            },
            Self {
                name: "Square 1080 px".into(),
                operations: vec![resize(ResizeMode::Fill {
                    width: 1080,
                    height: 1080,
                })],
                output: Output {
                    format: OutputFormat::To(Format::Jpeg),
                    encode: jpeg(90),
                    destination: Destination::Subfolder("square".into()),
                    ..Output::default()
                },
            },
            Self {
                name: "Convert to PNG".into(),
                operations: Vec::new(),
                output: Output {
                    format: OutputFormat::To(Format::Png),
                    destination: Destination::SameFolder,
                    ..Output::default()
                },
            },
            Self {
                name: "Convert to WebP".into(),
                operations: Vec::new(),
                output: Output {
                    format: OutputFormat::To(Format::WebP),
                    destination: Destination::SameFolder,
                    ..Output::default()
                },
            },
            Self {
                name: "Convert to JPEG".into(),
                operations: Vec::new(),
                output: Output {
                    format: OutputFormat::To(Format::Jpeg),
                    encode: jpeg(92),
                    destination: Destination::SameFolder,
                    ..Output::default()
                },
            },
        ]
    }

    /// The recipe as JSON, for the host to keep.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// A recipe from JSON; missing fields take their defaults.
    ///
    /// # Errors
    ///
    /// Returns the parser's message for malformed input.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }
}

/// Why a file was left alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The target name is taken and the policy is to skip.
    Exists,
    /// The recipe would not change the file.
    NothingToDo,
    /// The job was cancelled first.
    Cancelled,
}

/// What became of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A result was written.
    Written {
        /// The source file.
        source: PathBuf,
        /// The written file.
        target: PathBuf,
        /// Whether it was produced without re-encoding the pixels.
        lossless: bool,
    },
    /// The file was left alone.
    Skipped {
        /// The source file.
        source: PathBuf,
        /// Why.
        reason: SkipReason,
    },
    /// Processing failed.
    Failed {
        /// The source file.
        source: PathBuf,
        /// The error, in English.
        error: String,
    },
}

impl Outcome {
    /// The source file.
    pub fn source(&self) -> &Path {
        match self {
            Self::Written { source, .. }
            | Self::Skipped { source, .. }
            | Self::Failed { source, .. } => source,
        }
    }

    /// The written file, if any.
    pub fn target(&self) -> Option<&Path> {
        match self {
            Self::Written { target, .. } => Some(target),
            _ => None,
        }
    }

    fn failed(source: &Path, error: &Error) -> Self {
        Self::Failed {
            source: source.to_path_buf(),
            error: error.to_string(),
        }
    }
}

/// Target names handed out in one job, so two sources never write the same
/// file.
#[derive(Debug, Default)]
pub struct Claims(Mutex<HashSet<PathBuf>>);

impl Claims {
    /// Reserves a name for `wanted` under `conflict`; `source` may be
    /// replaced in place. `None` means "skip".
    fn claim(&self, wanted: &Path, source: &Path, conflict: Conflict) -> Option<PathBuf> {
        let mut guard = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let claimed: &mut HashSet<PathBuf> = &mut guard;
        let chosen = {
            let seen: &HashSet<PathBuf> = claimed;
            let taken = |path: &Path| seen.contains(path) || (path != source && path.exists());
            if !taken(wanted) {
                wanted.to_path_buf()
            } else if conflict == Conflict::Skip {
                return None;
            } else if conflict == Conflict::Overwrite && !seen.contains(wanted) {
                wanted.to_path_buf()
            } else {
                unique(wanted, taken)?
            }
        };
        claimed.insert(chosen.clone());
        Some(chosen)
    }
}

/// `name (2).ext`, `name (3).ext`, ... : the first that is not `taken`.
fn unique(wanted: &Path, taken: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let stem = wanted.file_stem()?.to_string_lossy().into_owned();
    let extension = wanted
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    (2..100_000)
        .map(|n| wanted.with_file_name(format!("{stem} ({n}){extension}")))
        .find(|candidate| !taken(candidate))
}

/// The folder results of `source` go to, created if needed.
fn target_folder(source: &Path, destination: &Destination) -> Result<PathBuf, Error> {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let folder = match destination {
        Destination::SameFolder => return Ok(parent.to_path_buf()),
        Destination::Subfolder(name) => parent.join(naming::sanitize(name)),
        Destination::Folder(folder) => folder.clone(),
    };
    std::fs::create_dir_all(&folder).map_err(|error| Error::io(&folder, error))?;
    Ok(folder)
}

/// The extension a result gets: the source's own spelling when the format
/// stays, else the format's preferred one.
fn target_extension(source: &Path, source_format: Format, target: Format) -> String {
    let own = source
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned());
    match own {
        Some(own) if target == source_format && Format::from_extension(&own) == Some(target) => own,
        _ => target.extension().to_owned(),
    }
}

struct Naming<'a> {
    source: &'a Path,
    index: usize,
    size: Option<(u32, u32)>,
    taken: Option<Stamp>,
    camera: Option<String>,
}

impl Naming<'_> {
    fn render(&self, template: &str) -> String {
        let stem = self
            .source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = self
            .source
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = self
            .source
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let taken = self.taken.or_else(|| {
            std::fs::metadata(self.source)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(Stamp::from_system_time)
        });
        naming::render(
            template,
            &NameContext {
                stem: &stem,
                extension: &extension,
                index: self.index,
                width: self.size.map(|size| size.0),
                height: self.size.map(|size| size.1),
                taken,
                camera: self.camera.as_deref(),
                folder: &folder,
            },
        )
    }
}

/// Processes one file with a recipe. `index` is the file's position in the
/// job (0-based); `{n}` counts from the recipe's start index.
pub fn process(
    source: &Path,
    index: usize,
    recipe: &Recipe,
    tools: &Tools,
    claims: &Claims,
) -> Outcome {
    match crate::error::contain(|| process_inner(source, index, recipe, tools, claims)) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::failed(source, &error),
    }
}

// The pipeline, step by step.
#[allow(clippy::too_many_lines)]
fn process_inner(
    source: &Path,
    index: usize,
    recipe: &Recipe,
    tools: &Tools,
    claims: &Claims,
) -> Result<Outcome, Error> {
    let output = &recipe.output;
    let data = std::fs::read(source).map_err(|error| Error::io(source, error))?;
    let source_format =
        decode::detect(&data, Format::from_path(source)).ok_or(Error::UnknownFormat)?;
    let target_format = output.format.resolve(source_format);
    let same_format = target_format == source_format;
    let operations = &recipe.operations;
    let options = &output.encode;

    // Results that need no decoding.
    let shortcut = if !same_format {
        None
    } else if operations.is_empty() && options.keep_exif && options.keep_icc {
        Some(data.clone())
    } else if operations.is_empty() && !options.keep_exif && options.keep_icc {
        match source_format {
            Format::Jpeg => lossless::strip_jpeg(&data),
            Format::Png => lossless::strip_png(&data),
            _ => None,
        }
    } else if source_format == Format::Jpeg
        && output.lossless_jpeg
        && options.keep_exif
        && !operations.is_empty()
        && operations.iter().all(Operation::is_reorientation)
    {
        lossless::reorient_jpeg(&data, operations)
    } else {
        None
    };

    let folder = target_folder(source, &output.destination)?;
    let extension = target_extension(source, source_format, target_format);
    let mut naming = Naming {
        source,
        index: output.start_index.saturating_add(index),
        size: None,
        taken: None,
        camera: None,
    };

    let (bytes, lossless) = if let Some(bytes) = shortcut {
        let needs_size = output.template.contains("{w}") || output.template.contains("{h}");
        let needs_exif = output.template.contains("{date}")
            || output.template.contains("{time}")
            || output.template.contains("{camera}");
        if needs_size || needs_exif {
            if let Ok(probe) = decode::probe_bytes(&bytes, Some(source_format)) {
                naming.size = probe.size;
                naming.taken = probe.summary.as_ref().and_then(|summary| summary.taken);
                naming.camera = probe.summary.and_then(|summary| summary.camera());
            }
        }
        (bytes, true)
    } else {
        let load = LoadOptions {
            auto_orient: output.auto_orient,
            vector_scale: output.vector_scale,
            ..LoadOptions::default()
        };
        let decoded = decode::load_bytes(&data, Some(source_format), &load)?;
        let image = ops::apply_all(decoded.image, operations, tools)?;
        naming.size = Some((image.width(), image.height()));
        naming.taken = decoded.summary.as_ref().and_then(|summary| summary.taken);
        naming.camera = decoded.summary.and_then(|summary| summary.camera());
        let bytes = encode::encode(
            &image,
            target_format,
            options,
            decoded.icc.as_deref(),
            decoded.exif.as_deref(),
        )?;
        (bytes, false)
    };

    let wanted = folder.join(format!("{}.{extension}", naming.render(&output.template)));
    if wanted == source && lossless && bytes == data {
        return Ok(Outcome::Skipped {
            source: source.to_path_buf(),
            reason: SkipReason::NothingToDo,
        });
    }
    let Some(target) = claims.claim(&wanted, source, output.conflict) else {
        return Ok(Outcome::Skipped {
            source: source.to_path_buf(),
            reason: SkipReason::Exists,
        });
    };
    encode::write_atomic(&target, &bytes)?;
    if output.keep_modified_time {
        copy_modified_time(source, &target);
    }
    Ok(Outcome::Written {
        source: source.to_path_buf(),
        target,
        lossless,
    })
}

fn copy_modified_time(source: &Path, target: &Path) {
    let Ok(time) = std::fs::metadata(source).and_then(|metadata| metadata.modified()) else {
        return;
    };
    if let Ok(file) = std::fs::File::options().write(true).open(target) {
        let _ = file.set_modified(time);
    }
}

/// Copy or move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    /// Copy, keeping the source.
    Copy,
    /// Move: the source is removed once the copy is complete.
    Move,
}

/// Copies or moves one file into `folder`.
pub fn transfer(
    source: &Path,
    folder: &Path,
    mode: TransferMode,
    conflict: Conflict,
    claims: &Claims,
) -> Outcome {
    transfer_inner(source, folder, mode, conflict, claims)
        .unwrap_or_else(|error| Outcome::failed(source, &error))
}

fn transfer_inner(
    source: &Path,
    folder: &Path,
    mode: TransferMode,
    conflict: Conflict,
    claims: &Claims,
) -> Result<Outcome, Error> {
    std::fs::create_dir_all(folder).map_err(|error| Error::io(folder, error))?;
    let name = source
        .file_name()
        .ok_or_else(|| Error::Invalid("the source has no file name".into()))?;
    let wanted = folder.join(name);
    if wanted == source {
        return Ok(Outcome::Skipped {
            source: source.to_path_buf(),
            reason: SkipReason::NothingToDo,
        });
    }
    let Some(target) = claims.claim(&wanted, source, conflict) else {
        return Ok(Outcome::Skipped {
            source: source.to_path_buf(),
            reason: SkipReason::Exists,
        });
    };
    match mode {
        TransferMode::Copy => {
            std::fs::copy(source, &target).map_err(|error| Error::io(&target, error))?;
        }
        TransferMode::Move => {
            if std::fs::rename(source, &target).is_err() {
                // Another volume: copy, then remove the source.
                std::fs::copy(source, &target).map_err(|error| Error::io(&target, error))?;
                std::fs::remove_file(source).map_err(|error| Error::io(source, error))?;
            }
        }
    }
    Ok(Outcome::Written {
        source: source.to_path_buf(),
        target,
        lossless: true,
    })
}

/// The new names a rename by `template` would give `sources`, in order:
/// unique among themselves and against other files in their folders. The
/// extension is kept.
pub fn plan_renames(
    sources: &[PathBuf],
    template: &str,
    start_index: usize,
) -> Vec<(PathBuf, PathBuf)> {
    let needs_probe = ["{w}", "{h}", "{date}", "{time}", "{camera}"]
        .iter()
        .any(|field| template.contains(field));
    let moving: HashSet<&Path> = sources.iter().map(PathBuf::as_path).collect();
    let mut chosen: HashSet<PathBuf> = HashSet::new();
    let mut plan = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let mut naming = Naming {
            source,
            index: start_index.saturating_add(index),
            size: None,
            taken: None,
            camera: None,
        };
        if needs_probe {
            if let Ok(probe) = decode::probe(source) {
                naming.size = probe.size;
                naming.taken = probe.summary.as_ref().and_then(|summary| summary.taken);
                naming.camera = probe.summary.and_then(|summary| summary.camera());
            }
        }
        let extension = source
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let wanted = source.with_file_name(format!("{}{extension}", naming.render(template)));
        let taken =
            |path: &Path| chosen.contains(path) || (path.exists() && !moving.contains(path));
        let target = if taken(&wanted) {
            unique(&wanted, taken).unwrap_or_else(|| source.clone())
        } else {
            wanted
        };
        chosen.insert(target.clone());
        plan.push((source.clone(), target));
    }
    plan
}

/// Carries out a rename plan in two steps (through temporary names), so
/// names may be swapped or shifted within the set.
pub fn rename(plan: &[(PathBuf, PathBuf)]) -> Vec<Outcome> {
    let mut staged = Vec::with_capacity(plan.len());
    for (index, (source, target)) in plan.iter().enumerate() {
        if source == target {
            staged.push(Err(Outcome::Skipped {
                source: source.clone(),
                reason: SkipReason::NothingToDo,
            }));
            continue;
        }
        let temporary =
            source.with_file_name(format!(".rename-{}-{index}.tmp", std::process::id()));
        staged.push(match std::fs::rename(source, &temporary) {
            Ok(()) => Ok(temporary),
            Err(error) => Err(Outcome::failed(source, &Error::io(source, error))),
        });
    }
    plan.iter()
        .zip(staged)
        .map(|((source, target), staged)| match staged {
            Err(outcome) => outcome,
            Ok(temporary) => match std::fs::rename(&temporary, target) {
                Ok(()) => Outcome::Written {
                    source: source.clone(),
                    target: target.clone(),
                    lossless: true,
                },
                Err(error) => {
                    let _ = std::fs::rename(&temporary, source);
                    Outcome::failed(source, &Error::io(target, error))
                }
            },
        })
        .collect()
}

/// A job's work.
#[derive(Clone, Debug)]
pub enum Task {
    /// Run a recipe.
    Process(Box<Recipe>),
    /// Copy or move into a folder.
    Transfer {
        /// The destination.
        folder: PathBuf,
        /// Copy or move.
        mode: TransferMode,
        /// What happens when a name is taken.
        conflict: Conflict,
    },
    /// Rename by a plan from [`plan_renames`].
    Rename(Vec<(PathBuf, PathBuf)>),
}

#[derive(Default)]
struct Shared {
    total: usize,
    done: AtomicUsize,
    cancel: AtomicBool,
    finished: AtomicBool,
    outcomes: Mutex<Vec<Outcome>>,
}

/// A running (or finished) batch job.
pub struct Job {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (done, total) = self.progress();
        f.debug_struct("Job")
            .field("done", &done)
            .field("total", &total)
            .finish_non_exhaustive()
    }
}

impl Job {
    /// Starts `task` over `items` on a background thread. `notify` is called
    /// after every file and at the end (to repaint the host).
    pub fn start(
        task: Task,
        items: Vec<PathBuf>,
        tools: Tools,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let total = match &task {
            Task::Rename(plan) => plan.len(),
            _ => items.len(),
        };
        let shared = Arc::new(Shared {
            total,
            ..Shared::default()
        });
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("image-gallery-batch".into())
            .spawn(move || {
                run(&task, &items, &tools, &worker, &notify);
                worker.finished.store(true, Ordering::Release);
                notify();
            });
        if spawned.is_err() {
            shared.finished.store(true, Ordering::Release);
        }
        Self { shared }
    }

    /// Files done and files in all.
    pub fn progress(&self) -> (usize, usize) {
        (self.shared.done.load(Ordering::Acquire), self.shared.total)
    }

    /// Asks the job to stop; files already started finish.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Release);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::Acquire)
    }

    /// Whether every file has been dealt with.
    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    /// The outcomes so far.
    pub fn outcomes(&self) -> Vec<Outcome> {
        self.shared
            .outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits for the job to finish (for tests and command-line use).
    pub fn wait(&self) {
        while !self.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

fn run(
    task: &Task,
    items: &[PathBuf],
    tools: &Tools,
    shared: &Shared,
    notify: &(impl Fn() + Sync),
) {
    let record = |outcome: Outcome| {
        shared
            .outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(outcome);
        shared.done.fetch_add(1, Ordering::AcqRel);
        notify();
    };
    let cancelled = |source: &Path| Outcome::Skipped {
        source: source.to_path_buf(),
        reason: SkipReason::Cancelled,
    };
    let claims = Claims::default();
    match task {
        Task::Process(recipe) => items.par_iter().enumerate().for_each(|(index, source)| {
            record(if shared.cancel.load(Ordering::Acquire) {
                cancelled(source)
            } else {
                process(source, index, recipe, tools, &claims)
            });
        }),
        Task::Transfer {
            folder,
            mode,
            conflict,
        } => items.par_iter().for_each(|source| {
            record(if shared.cancel.load(Ordering::Acquire) {
                cancelled(source)
            } else {
                transfer(source, folder, *mode, *conflict, &claims)
            });
        }),
        Task::Rename(plan) => {
            for outcome in rename(plan) {
                record(outcome);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::QuarterTurn;
    use image::{DynamicImage, GenericImageView, Rgb, RgbImage};

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("image-gallery-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("folder");
            Self(path)
        }

        fn picture(&self, name: &str, width: u32, height: u32) -> PathBuf {
            let path = self.0.join(name);
            DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([90, 120, 200])))
                .save(&path)
                .expect("saves");
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn run_job(task: Task, items: Vec<PathBuf>) -> Vec<Outcome> {
        let job = Job::start(task, items, Tools::default(), || {});
        job.wait();
        job.outcomes()
    }

    #[test]
    fn web_preset_resizes_and_converts_into_a_subfolder() {
        let folder = Folder::new("web");
        let sources = vec![
            folder.picture("a.png", 4000, 2000),
            folder.picture("b.bmp", 100, 50),
        ];
        let recipe = Recipe::presets().remove(0);
        let outcomes = run_job(Task::Process(Box::new(recipe)), sources);
        assert_eq!(outcomes.len(), 2);
        let a = folder.0.join("web").join("a.jpg");
        let b = folder.0.join("web").join("b.jpg");
        assert!(
            outcomes.iter().all(|outcome| outcome.target().is_some()),
            "{outcomes:?}"
        );
        assert_eq!(image::image_dimensions(a).expect("a"), (1920, 960));
        // Not enlarged.
        assert_eq!(image::image_dimensions(b).expect("b"), (100, 50));
    }

    #[test]
    fn in_place_jpeg_turns_are_lossless() {
        let folder = Folder::new("turn");
        let source = folder.picture("photo.jpg", 40, 20);
        let before = std::fs::read(&source).expect("reads");
        let recipe = Recipe::in_place(vec![Operation::Rotate(QuarterTurn::Right)]);
        let outcome = process(&source, 0, &recipe, &Tools::default(), &Claims::default());
        assert_eq!(
            outcome,
            Outcome::Written {
                source: source.clone(),
                target: source.clone(),
                lossless: true
            }
        );
        let after = std::fs::read(&source).expect("reads");
        assert!(after.len() > before.len() && after.len() < before.len() + 64);
        let decoded = decode::load(&source, &LoadOptions::default()).expect("decodes");
        assert_eq!(decoded.image.dimensions(), (20, 40));
    }

    #[test]
    fn conflicts_follow_the_policy() {
        let folder = Folder::new("conflict");
        let a = folder.picture("x.png", 8, 8);
        let b = folder.picture("x.bmp", 8, 8);
        let mut recipe = Recipe {
            operations: vec![Operation::Invert],
            ..Recipe::default()
        };
        recipe.output.format = OutputFormat::To(Format::Png);
        recipe.output.destination = Destination::Subfolder("out".into());
        // Two sources, one name: the second gets a number.
        let outcomes = run_job(
            Task::Process(Box::new(recipe.clone())),
            vec![a.clone(), b.clone()],
        );
        let mut targets: Vec<_> = outcomes
            .iter()
            .filter_map(|outcome| outcome.target().and_then(Path::file_name))
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        targets.sort();
        assert_eq!(targets, ["x (2).png", "x.png"]);
        // Skip leaves existing results alone.
        recipe.output.conflict = Conflict::Skip;
        let outcome = process(&a, 0, &recipe, &Tools::default(), &Claims::default());
        assert!(matches!(
            outcome,
            Outcome::Skipped {
                reason: SkipReason::Exists,
                ..
            }
        ));
    }

    #[test]
    fn templates_name_results() {
        let folder = Folder::new("names");
        let source = folder.picture("shot.png", 30, 10);
        let mut recipe = Recipe::default();
        recipe.output.template = "{folder}_{n:3}_{w}x{h}".into();
        recipe.output.start_index = 5;
        recipe.operations = vec![Operation::Rotate(QuarterTurn::Left)];
        let outcome = process(&source, 2, &recipe, &Tools::default(), &Claims::default());
        let target = outcome.target().expect("written");
        let expected = format!(
            "{}_007_10x30.png",
            folder.0.file_name().expect("name").to_string_lossy()
        );
        assert_eq!(
            target.file_name().expect("name").to_string_lossy(),
            expected
        );
    }

    #[test]
    fn copies_moves_and_renames() {
        let folder = Folder::new("transfer");
        let a = folder.picture("a.png", 4, 4);
        let b = folder.picture("b.png", 4, 4);
        let copies = folder.0.join("copies");
        let outcomes = run_job(
            Task::Transfer {
                folder: copies.clone(),
                mode: TransferMode::Copy,
                conflict: Conflict::Unique,
            },
            vec![a.clone(), b.clone()],
        );
        assert_eq!(outcomes.len(), 2);
        assert!(copies.join("a.png").exists() && a.exists());

        // Swap names through the two-step rename.
        let plan = vec![(a.clone(), b.clone()), (b.clone(), a.clone())];
        let outcomes = rename(&plan);
        assert!(
            outcomes.iter().all(|outcome| outcome.target().is_some()),
            "{outcomes:?}"
        );

        let planned = plan_renames(&[a.clone(), b.clone()], "photo", 1);
        let names: Vec<_> = planned
            .iter()
            .map(|(_, target)| {
                target
                    .file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["photo.png", "photo (2).png"]);

        let moved = folder.0.join("moved");
        let outcomes = run_job(
            Task::Transfer {
                folder: moved.clone(),
                mode: TransferMode::Move,
                conflict: Conflict::Unique,
            },
            vec![a.clone()],
        );
        assert!(outcomes[0].target().is_some());
        assert!(!a.exists() && moved.join("a.png").exists());
    }

    #[test]
    fn cancelled_jobs_skip_the_rest() {
        let folder = Folder::new("cancel");
        let sources: Vec<_> = (0..4)
            .map(|n| folder.picture(&format!("{n}.png"), 4, 4))
            .collect();
        let job = Job::start(
            Task::Process(Box::default()),
            sources,
            Tools::default(),
            || {},
        );
        job.cancel();
        job.wait();
        assert_eq!(job.progress(), (4, 4));
        assert!(job.is_cancelled());
    }

    #[test]
    fn recipes_round_trip() {
        for recipe in Recipe::presets() {
            assert_eq!(
                Recipe::from_json(&recipe.to_json()).expect("parses"),
                recipe
            );
        }
        let partial = Recipe::from_json(r#"{"name": "Mine"}"#).expect("defaults fill in");
        assert_eq!(partial.output, Output::default());
    }
}
