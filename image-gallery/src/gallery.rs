//! The embeddable gallery widget.
//!
//! [`Gallery`] owns the browsing state: the folder, its pictures, the
//! selection, the previews and the running job. The host draws it with
//! [`Gallery::show`], or places the ribbon ([`Gallery::show_ribbon`]) and the
//! content ([`Gallery::show_content`]) itself, and answers the
//! [`GalleryEvent`]s: the widget never opens a file dialog of its own.
//! Every ribbon button is also a [`Command`] the host may bind to its own
//! controls through [`Gallery::execute`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::TextureHandle;
use serde::{Deserialize, Serialize};

use crate::background::{Download, Remover, TOOL_ID};
use crate::batch::{Conflict, Job, Outcome, Recipe, SkipReason, Task, TransferMode};
use crate::catalog::{self, Entry, SortKey};
use crate::ops::{Axis, Operation, QuarterTurn, Tools};
use crate::thumbs::{Info, Kind, Loader, Request, Stamp};
use crate::Locale;

mod model_view;
mod ribbon;

pub use model_view::ModelPanel;
mod side;
mod view;
mod viewer;

/// A request the host answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GalleryEvent {
    /// Show a folder dialog and pass the answer to [`Gallery::open_folder`].
    PickFolder,
    /// Show a folder dialog and pass the answer to
    /// [`Gallery::set_destination`] with the same purpose.
    PickDestination(Purpose),
    /// Open this picture in the host's picture editor.
    OpenInEditor(PathBuf),
    /// Show this file in the system's file manager.
    Reveal(PathBuf),
    /// The gallery now shows this folder (for the host's title or recent
    /// list).
    FolderOpened(PathBuf),
    /// A job wrote these files.
    Written(Vec<PathBuf>),
    /// Show a file dialog for `BiRefNet` ONNX weights and pass the answer to
    /// [`Gallery::set_background_model`].
    PickModel,
}

/// What a destination folder is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Purpose {
    /// The folder recipe results go to.
    Output,
    /// Copy the selection there.
    Copy,
    /// Move the selection there.
    Move,
}

/// Everything a ribbon button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Ask the host for a folder.
    OpenFolder,
    /// List the folder again.
    Refresh,
    /// Select every shown picture.
    SelectAll,
    /// Clear the selection.
    SelectNone,
    /// Select the others.
    InvertSelection,
    /// Turn the selection a quarter left, in place.
    RotateLeft,
    /// Turn the selection a quarter right, in place.
    RotateRight,
    /// Turn the selection upside down, in place.
    Rotate180,
    /// Mirror the selection left to right, in place.
    FlipHorizontal,
    /// Mirror the selection top to bottom, in place.
    FlipVertical,
    /// Remove EXIF, XMP, IPTC and comments from the selection, in place.
    StripMetadata,
    /// Run the current recipe on the selection.
    RunRecipe,
    /// Copy the selection to a folder the host asks for.
    CopyTo,
    /// Move the selection to a folder the host asks for.
    MoveTo,
    /// Rename the selection by the rename template.
    Rename,
    /// Open the focused picture in the viewer.
    OpenViewer,
    /// Back from the viewer to the previews.
    CloseViewer,
    /// Ask the host to open the focused picture in its editor.
    OpenInEditor,
    /// Write a copy of the selection with the background made transparent
    /// (PNG, next to the source).
    RemoveBackground,
    /// Download the `BiRefNet` weights of the chosen variant.
    DownloadModel,
    /// Stop the running job.
    CancelJob,
}

/// The ribbon's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RibbonTab {
    /// Folder, selection and view.
    Home,
    /// Quick in-place edits.
    Image,
    /// Recipes.
    Batch,
    /// Copy, move, rename.
    Files,
}

/// What the host may persist between sessions.
// Independent switches of the view, not a state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Longer side of a preview cell, in points.
    pub thumb_size: f32,
    /// Sort order.
    pub sort: SortKey,
    /// Reverse the order.
    pub descending: bool,
    /// Include sub-folders.
    pub recursive: bool,
    /// Show file names under the previews.
    pub show_names: bool,
    /// Turn JPEGs losslessly (EXIF orientation) in quick edits.
    pub lossless_jpeg: bool,
    /// The recipe being edited.
    pub recipe: Recipe,
    /// Recipes the user saved.
    pub presets: Vec<Recipe>,
    /// The rename template.
    pub rename_template: String,
    /// The first number of a rename.
    pub rename_start: usize,
    /// Background removal: the model, its variant, CPU or GPU.
    pub background: crate::background::Settings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thumb_size: 160.0,
            sort: SortKey::Name,
            descending: false,
            recursive: false,
            show_names: true,
            lossless_jpeg: true,
            recipe: Recipe::presets().remove(0),
            presets: Vec::new(),
            rename_template: "{name}".into(),
            rename_start: 1,
            background: crate::background::Settings::default(),
        }
    }
}

/// Smallest and largest preview cell.
const THUMB_RANGE: std::ops::RangeInclusive<f32> = 64.0..=384.0;
/// Previews kept in memory beyond those on screen.
const CACHE_LIMIT: usize = 3000;

enum SlotState {
    Loading,
    Ready(TextureHandle),
    Failed(String),
}

struct Slot {
    stamp: Stamp,
    /// The preview side requested, in pixels.
    side: u32,
    state: SlotState,
    info: Option<Info>,
    /// The last frame it was on screen.
    seen: u64,
}

struct Running {
    job: Job,
    title: String,
    /// Selected paths to follow when files are renamed or moved.
    follow: bool,
}

/// The summary of a finished job.
struct Report {
    title: String,
    written: usize,
    lossless: usize,
    skipped: Vec<(PathBuf, SkipReason)>,
    failed: Vec<(PathBuf, String)>,
}

/// The gallery widget.
pub struct Gallery {
    locale: Locale,
    settings: Settings,
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
    /// Positions in `entries` of the shown (filtered) pictures, in order.
    view: Vec<usize>,
    filter: String,
    scanning: Option<Receiver<(PathBuf, std::io::Result<Vec<Entry>>)>>,
    message: Option<String>,
    selection: HashSet<PathBuf>,
    /// The view position a Shift-click extends from.
    anchor: Option<usize>,
    /// The view position the keyboard acts on.
    focus: Option<usize>,
    scroll_to_focus: bool,
    thumbs: HashMap<PathBuf, Slot>,
    loader: Option<Loader>,
    viewer: Option<viewer::Viewer>,
    tab: RibbonTab,
    rename_plan: Option<Vec<(PathBuf, PathBuf)>>,
    job: Option<Running>,
    report: Option<Report>,
    tools: Tools,
    events: Vec<GalleryEvent>,
    frame: u64,
    last_time: f64,
    columns: usize,
    /// The context, for jobs to wake the interface.
    repaint: Option<egui::Context>,
    /// The grid's scroll offset and visible height, from the last frame.
    viewport: (f32, f32),
    /// The background remover behind [`crate::background::TOOL_ID`].
    remover: Arc<Remover>,
    download: Option<Download>,
    /// The 3D panel of the model in focus.
    model: ModelPanel,
}

impl std::fmt::Debug for Gallery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gallery")
            .field("folder", &self.folder)
            .field("pictures", &self.entries.len())
            .field("selected", &self.selection.len())
            .finish_non_exhaustive()
    }
}

impl Default for Gallery {
    fn default() -> Self {
        Self::new(Locale::default())
    }
}

impl Gallery {
    /// An empty gallery: no folder yet.
    pub fn new(locale: Locale) -> Self {
        Self {
            locale,
            settings: Settings::default(),
            folder: None,
            entries: Vec::new(),
            view: Vec::new(),
            filter: String::new(),
            scanning: None,
            message: None,
            selection: HashSet::new(),
            anchor: None,
            focus: None,
            scroll_to_focus: false,
            thumbs: HashMap::new(),
            loader: None,
            viewer: None,
            tab: RibbonTab::Home,
            rename_plan: None,
            job: None,
            report: None,
            tools: Tools::default(),
            events: Vec::new(),
            frame: 0,
            last_time: -1.0,
            columns: 1,
            repaint: None,
            viewport: (0.0, 0.0),
            remover: Arc::new(Remover::new(crate::background::Settings::default())),
            download: None,
            model: ModelPanel::default(),
        }
        .with_background_tool()
    }

    /// Registers the built-in background remover as a recipe tool.
    fn with_background_tool(mut self) -> Self {
        let remover = Arc::clone(&self.remover);
        self.tools
            .register(TOOL_ID, "Remove background", move |image| {
                remover.remove(&image)
            });
        self
    }

    /// The host's answer to [`GalleryEvent::PickModel`].
    pub fn set_background_model(&mut self, path: impl Into<PathBuf>) {
        self.settings.background.model = Some(path.into());
    }

    /// Whether the background remover's model file is present.
    pub fn background_model_found(&self) -> bool {
        self.settings.background.model_path().is_file()
    }

    /// The interface language.
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// Changes the interface language.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// The settings worth persisting.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Restores persisted settings.
    pub fn set_settings(&mut self, settings: Settings) {
        let relist = settings.recursive != self.settings.recursive;
        self.settings = settings;
        self.settings.thumb_size = self
            .settings
            .thumb_size
            .clamp(*THUMB_RANGE.start(), *THUMB_RANGE.end());
        if relist {
            self.refresh();
        } else {
            self.resort();
        }
    }

    /// Registers a host tool (such as a model-backed background remover);
    /// recipes can then use it as [`Operation::Tool`].
    pub fn register_tool(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        tool: impl Fn(image::DynamicImage) -> Result<image::DynamicImage, crate::Error>
            + Send
            + Sync
            + 'static,
    ) {
        self.tools.register(id, label, tool);
    }

    /// The folder shown, if any.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// Shows `folder`. The listing happens in the background.
    pub fn open_folder(&mut self, folder: impl Into<PathBuf>) {
        let folder = folder.into();
        if self.folder.as_ref() != Some(&folder) {
            self.selection.clear();
            self.anchor = None;
            self.focus = None;
            self.viewer = None;
            self.thumbs.clear();
            if let Some(loader) = &self.loader {
                loader.reset();
            }
        }
        self.folder = Some(folder.clone());
        self.events.push(GalleryEvent::FolderOpened(folder));
        self.refresh();
    }

    /// The host's answer to [`GalleryEvent::PickDestination`].
    pub fn set_destination(&mut self, purpose: Purpose, folder: impl Into<PathBuf>) {
        let folder = folder.into();
        match purpose {
            Purpose::Output => {
                self.settings.recipe.output.destination = crate::batch::Destination::Folder(folder);
            }
            Purpose::Copy | Purpose::Move => {
                let mode = if purpose == Purpose::Copy {
                    TransferMode::Copy
                } else {
                    TransferMode::Move
                };
                let title = if mode == TransferMode::Copy {
                    "Copy"
                } else {
                    "Move"
                };
                self.start(
                    Task::Transfer {
                        folder,
                        mode,
                        conflict: Conflict::Unique,
                    },
                    title,
                    mode == TransferMode::Move,
                );
            }
        }
    }

    /// The selected pictures, in display order.
    pub fn selection(&self) -> Vec<PathBuf> {
        self.view
            .iter()
            .map(|&index| &self.entries[index].path)
            .filter(|path| self.selection.contains(*path))
            .cloned()
            .collect()
    }

    /// Selects exactly these pictures (those not shown are ignored).
    pub fn select(&mut self, paths: &[PathBuf]) {
        let shown: HashSet<&PathBuf> = self
            .view
            .iter()
            .map(|&index| &self.entries[index].path)
            .collect();
        self.selection = paths
            .iter()
            .filter(|path| shown.contains(path))
            .cloned()
            .collect();
    }

    /// Whether a job is running.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// The events since the last call.
    pub fn take_events(&mut self) -> Vec<GalleryEvent> {
        std::mem::take(&mut self.events)
    }

    /// Whether `command` would do anything now.
    pub fn can_execute(&self, command: Command) -> bool {
        let busy = self.job.is_some();
        let targets = !self.selection.is_empty() || self.focus.is_some();
        match command {
            Command::OpenFolder => true,
            Command::Refresh => self.folder.is_some(),
            Command::SelectAll | Command::InvertSelection => !self.view.is_empty(),
            Command::SelectNone => !self.selection.is_empty(),
            Command::RotateLeft
            | Command::RotateRight
            | Command::Rotate180
            | Command::FlipHorizontal
            | Command::FlipVertical
            | Command::StripMetadata
            | Command::RunRecipe
            | Command::CopyTo
            | Command::MoveTo
            | Command::Rename => targets && !busy,
            Command::OpenViewer | Command::OpenInEditor => targets,
            Command::RemoveBackground => targets && !busy && self.background_model_found(),
            Command::DownloadModel => self.download.is_none() && !self.background_model_found(),
            Command::CloseViewer => self.viewer.is_some(),
            Command::CancelJob => busy || self.download.is_some(),
        }
    }

    /// Carries out a command.
    pub fn execute(&mut self, command: Command) {
        if !self.can_execute(command) {
            return;
        }
        let turn = |operation| Recipe::in_place(vec![operation]);
        match command {
            Command::OpenFolder => self.events.push(GalleryEvent::PickFolder),
            Command::Refresh => self.refresh(),
            Command::SelectAll => {
                self.selection = self
                    .view
                    .iter()
                    .map(|&index| self.entries[index].path.clone())
                    .collect();
            }
            Command::SelectNone => self.selection.clear(),
            Command::InvertSelection => {
                self.selection = self
                    .view
                    .iter()
                    .map(|&index| &self.entries[index].path)
                    .filter(|path| !self.selection.contains(*path))
                    .cloned()
                    .collect();
            }
            Command::RotateLeft => {
                self.quick(turn(Operation::Rotate(QuarterTurn::Left)), "Rotate left");
            }
            Command::RotateRight => {
                self.quick(turn(Operation::Rotate(QuarterTurn::Right)), "Rotate right");
            }
            Command::Rotate180 => {
                self.quick(turn(Operation::Rotate(QuarterTurn::Half)), "Rotate 180°");
            }
            Command::FlipHorizontal => {
                self.quick(turn(Operation::Flip(Axis::Horizontal)), "Flip horizontally");
            }
            Command::FlipVertical => {
                self.quick(turn(Operation::Flip(Axis::Vertical)), "Flip vertically");
            }
            Command::StripMetadata => {
                let mut recipe = Recipe::in_place(Vec::new());
                recipe.output.encode.keep_exif = false;
                self.quick(recipe, "Remove metadata");
            }
            Command::RunRecipe => {
                let recipe = self.settings.recipe.clone();
                self.start(Task::Process(Box::new(recipe)), "Batch", false);
            }
            Command::CopyTo => self
                .events
                .push(GalleryEvent::PickDestination(Purpose::Copy)),
            Command::MoveTo => self
                .events
                .push(GalleryEvent::PickDestination(Purpose::Move)),
            Command::Rename => {
                // Planned afresh: the files may have changed since the preview.
                let plan = self.plan_rename();
                self.rename_plan = None;
                self.start(Task::Rename(plan), "Rename", true);
            }
            Command::OpenViewer => {
                if let Some(position) = self.focus.or_else(|| self.first_selected()) {
                    self.open_viewer(position);
                }
            }
            Command::CloseViewer => self.viewer = None,
            Command::OpenInEditor => {
                if let Some(path) = self
                    .focus_path()
                    .or_else(|| self.selection().into_iter().next())
                {
                    self.events.push(GalleryEvent::OpenInEditor(path));
                }
            }
            Command::RemoveBackground => {
                let recipe = Recipe::cutout();
                self.start(Task::Process(Box::new(recipe)), "Remove background", false);
            }
            Command::DownloadModel => self.start_download(),
            Command::CancelJob => {
                if let Some(running) = &self.job {
                    running.job.cancel();
                }
                if let Some(download) = &self.download {
                    download.cancel();
                }
            }
        }
    }

    /// Draws the ribbon above the content, filling `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<GalleryEvent> {
        self.update(ui.ctx());
        let fill = ui.visuals().panel_fill;
        egui::Panel::top(egui::Id::new("image-gallery-ribbon"))
            .frame(
                egui::Frame::new()
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| self.ribbon_ui(ui));
        self.content_ui(ui);
        self.take_events()
    }

    /// Draws only the ribbon, for hosts that place it in their own band.
    pub fn show_ribbon(&mut self, ui: &mut egui::Ui) {
        self.update(ui.ctx());
        self.ribbon_ui(ui);
    }

    /// Draws everything but the ribbon: previews or viewer, the side panel
    /// and the status bar.
    pub fn show_content(&mut self, ui: &mut egui::Ui) -> Vec<GalleryEvent> {
        self.update(ui.ctx());
        self.content_ui(ui);
        self.take_events()
    }

    fn content_ui(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom(egui::Id::new("image-gallery-status"))
            .exact_size(26.0)
            .show(ui, |ui| self.status_ui(ui));
        egui::Panel::right(egui::Id::new("image-gallery-side"))
            .resizable(true)
            .default_size(360.0)
            .size_range(220.0..=1000.0)
            .show(ui, |ui| self.side_ui(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            if self.viewer.is_some() {
                self.viewer_ui(ui);
            } else {
                self.grid_ui(ui);
            }
        });
    }

    /// Once per frame: listing, previews and job progress.
    fn update(&mut self, ctx: &egui::Context) {
        let time = ctx.input(|input| input.time);
        if (time - self.last_time).abs() < f64::EPSILON {
            return;
        }
        self.last_time = time;
        self.frame += 1;
        if self.repaint.is_none() {
            self.repaint = Some(ctx.clone());
        }
        self.poll_scan();
        self.poll_previews(ctx);
        self.poll_job();
        self.remover.configure(&self.settings.background);
        self.poll_download();
        if self.thumbs.len() > CACHE_LIMIT {
            let keep_after = self.frame.saturating_sub(120);
            self.thumbs.retain(|_, slot| slot.seen >= keep_after);
        }
    }

    fn refresh(&mut self) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let (sender, receiver) = channel();
        let recursive = self.settings.recursive;
        let spawned = std::thread::Builder::new()
            .name("image-gallery-scan".into())
            .spawn(move || {
                let result = catalog::scan(&folder, recursive);
                let _ = sender.send((folder, result));
            });
        match spawned {
            Ok(_) => self.scanning = Some(receiver),
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    fn poll_scan(&mut self) {
        let Some(receiver) = &self.scanning else {
            return;
        };
        let Ok((folder, result)) = receiver.try_recv() else {
            return;
        };
        self.scanning = None;
        if self.folder.as_ref() != Some(&folder) {
            return;
        }
        match result {
            Ok(entries) => {
                let focused = self.focus_path();
                self.entries = entries;
                self.message = None;
                let present: HashSet<&PathBuf> =
                    self.entries.iter().map(|entry| &entry.path).collect();
                self.selection.retain(|path| present.contains(path));
                self.resort();
                self.focus = focused.and_then(|path| self.position_of(&path));
            }
            Err(error) => {
                self.entries.clear();
                self.view.clear();
                self.message = Some(format!("{}: {error}", folder.display()));
            }
        }
    }

    /// Sorts and filters `entries` into `view`.
    fn resort(&mut self) {
        let focused = self.focus_path();
        catalog::sort(
            &mut self.entries,
            self.settings.sort,
            self.settings.descending,
        );
        self.view = (0..self.entries.len())
            .filter(|&index| catalog::matches(&self.entries[index], &self.filter))
            .collect();
        self.anchor = None;
        self.focus = focused.and_then(|path| self.position_of(&path));
        self.rename_plan = None;
    }

    fn position_of(&self, path: &Path) -> Option<usize> {
        self.view
            .iter()
            .position(|&index| self.entries[index].path == path)
    }

    fn entry_at(&self, position: usize) -> Option<&Entry> {
        self.view.get(position).map(|&index| &self.entries[index])
    }

    fn focus_path(&self) -> Option<PathBuf> {
        self.focus
            .and_then(|position| self.entry_at(position))
            .map(|entry| entry.path.clone())
    }

    fn first_selected(&self) -> Option<usize> {
        self.view
            .iter()
            .position(|&index| self.selection.contains(&self.entries[index].path))
    }

    /// The files a command acts on: the selection, or the focused picture.
    fn targets(&self) -> Vec<PathBuf> {
        let selected = self.selection();
        if selected.is_empty() {
            self.focus_path().into_iter().collect()
        } else {
            selected
        }
    }

    fn quick(&mut self, mut recipe: Recipe, title: &str) {
        recipe.output.lossless_jpeg = self.settings.lossless_jpeg;
        self.start(Task::Process(Box::new(recipe)), title, false);
    }

    fn start(&mut self, task: Task, title: &str, follow: bool) {
        if self.job.is_some() {
            return;
        }
        let items = self.targets();
        if items.is_empty() && !matches!(task, Task::Rename(_)) {
            return;
        }
        let wake = self.repaint.clone();
        let job = Job::start(task, items, self.tools.clone(), move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        });
        self.report = None;
        self.job = Some(Running {
            job,
            title: title.to_owned(),
            follow,
        });
    }

    fn poll_job(&mut self) {
        let finished = self
            .job
            .as_ref()
            .is_some_and(|running| running.job.is_finished());
        if !finished {
            return;
        }
        let Some(running) = self.job.take() else {
            return;
        };
        let outcomes = running.job.outcomes();
        let mut report = Report {
            title: running.title,
            written: 0,
            lossless: 0,
            skipped: Vec::new(),
            failed: Vec::new(),
        };
        let mut written = Vec::new();
        for outcome in outcomes {
            match outcome {
                Outcome::Written {
                    source,
                    target,
                    lossless,
                } => {
                    report.written += 1;
                    report.lossless += usize::from(lossless);
                    self.thumbs.remove(&source);
                    self.thumbs.remove(&target);
                    if running.follow && self.selection.remove(&source) {
                        self.selection.insert(target.clone());
                    }
                    written.push(target);
                }
                Outcome::Skipped { source, reason } => report.skipped.push((source, reason)),
                Outcome::Failed { source, error } => report.failed.push((source, error)),
            }
        }
        if let Some(viewer) = &mut self.viewer {
            if written.contains(&viewer.path) {
                viewer.reload();
            }
        }
        self.report = Some(report);
        if !written.is_empty() {
            self.events.push(GalleryEvent::Written(written));
        }
        self.refresh();
    }

    fn start_download(&mut self) {
        let wake = self.repaint.clone();
        match Download::start(&self.settings.background, move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        }) {
            Ok(download) => {
                self.download = Some(download);
                self.message = None;
            }
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    fn poll_download(&mut self) {
        let Some(download) = &self.download else {
            return;
        };
        let state = download.state();
        if !state.finished {
            return;
        }
        if let Some(error) = state.error {
            self.message = Some(error);
        } else {
            self.settings.background.model = Some(download.target().to_path_buf());
        }
        self.download = None;
    }

    fn plan_rename(&self) -> Vec<(PathBuf, PathBuf)> {
        crate::batch::plan_renames(
            &self.targets(),
            &self.settings.rename_template,
            self.settings.rename_start,
        )
    }

    /// The loader, started on first use.
    fn loader(&mut self, ctx: &egui::Context) -> &Loader {
        self.loader.get_or_insert_with(|| {
            let ctx = ctx.clone();
            let threads = std::thread::available_parallelism()
                .map_or(2, std::num::NonZeroUsize::get)
                .clamp(2, 6)
                - 1;
            Loader::new(threads, move || ctx.request_repaint())
        })
    }

    /// Asks for the preview of the picture at `position` if it is missing,
    /// stale or too small, and marks it as on screen.
    fn want_preview(&mut self, ctx: &egui::Context, position: usize, side: u32) {
        let Some(entry) = self.entry_at(position) else {
            return;
        };
        let stamp = Stamp {
            size: entry.size,
            modified: entry.modified,
        };
        let path = entry.path.clone();
        let frame = self.frame;
        if let Some(slot) = self.thumbs.get_mut(&path) {
            slot.seen = frame;
            let current = slot.stamp == stamp
                && (slot.side >= side || matches!(slot.state, SlotState::Failed(_)));
            if current {
                return;
            }
        }
        let generation = self.loader(ctx).generation();
        self.thumbs.insert(
            path.clone(),
            Slot {
                stamp,
                side,
                state: SlotState::Loading,
                info: None,
                seen: frame,
            },
        );
        self.loader(ctx).request(Request {
            path,
            stamp,
            kind: Kind::Preview(side),
            generation,
        });
    }

    fn poll_previews(&mut self, ctx: &egui::Context) {
        let Some(loader) = &self.loader else {
            return;
        };
        let generation = loader.generation();
        for answer in loader.answers() {
            if answer.request.generation != generation {
                continue;
            }
            match answer.request.kind {
                Kind::Preview(side) => {
                    let Some(slot) = self.thumbs.get_mut(&answer.request.path) else {
                        continue;
                    };
                    if slot.stamp != answer.request.stamp || slot.side != side {
                        continue;
                    }
                    match answer.result {
                        Ok(pixels) => {
                            let image =
                                egui::ColorImage::from_rgba_unmultiplied(pixels.size, &pixels.rgba);
                            let name = format!("image-gallery:{}", answer.request.path.display());
                            slot.state = SlotState::Ready(ctx.load_texture(
                                name,
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                            slot.info = Some(pixels.info);
                        }
                        Err(error) => slot.state = SlotState::Failed(error),
                    }
                }
                Kind::Full(_) => {
                    if let Some(viewer) = &mut self.viewer {
                        viewer.receive(ctx, answer);
                    }
                }
            }
        }
    }

    fn info_of(&self, path: &Path) -> Option<&Info> {
        self.thumbs.get(path).and_then(|slot| slot.info.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_with(names: &[&str]) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "image-gallery-widget-{}-{}",
            std::process::id(),
            names.len()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("folder");
        for name in names {
            image::RgbImage::new(4, 4)
                .save(folder.join(name))
                .expect("saves");
        }
        folder
    }

    fn wait_for_listing(gallery: &mut Gallery) {
        for _ in 0..400 {
            gallery.poll_scan();
            if gallery.scanning.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("listing did not finish");
    }

    #[test]
    fn lists_filters_and_selects() {
        let folder = folder_with(&["b.png", "a.png", "c.bmp"]);
        let mut gallery = Gallery::default();
        gallery.open_folder(&folder);
        wait_for_listing(&mut gallery);
        assert_eq!(gallery.view.len(), 3);
        assert_eq!(
            gallery.take_events(),
            vec![GalleryEvent::FolderOpened(folder.clone())]
        );
        gallery.execute(Command::SelectAll);
        assert_eq!(gallery.selection().len(), 3);
        assert_eq!(
            gallery.selection()[0]
                .file_name()
                .and_then(std::ffi::OsStr::to_str),
            Some("a.png")
        );
        gallery.execute(Command::InvertSelection);
        assert!(gallery.selection().is_empty());
        gallery.filter = "png".into();
        gallery.resort();
        assert_eq!(gallery.view.len(), 2);
        gallery.execute(Command::OpenFolder);
        assert_eq!(gallery.take_events(), vec![GalleryEvent::PickFolder]);
        assert!(!gallery.can_execute(Command::RotateLeft));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).expect("serializes");
        let back: Settings = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, settings);
    }
}
