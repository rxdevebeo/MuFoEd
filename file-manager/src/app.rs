//! The file manager widget: two panels, the function-key bar, dialogs,
//! the quick view and viewer, search and the command line.

use std::path::{Path, PathBuf};

use egui::Key;

use crate::archive::{self, Kind};
use crate::drives::{self, Drive};
use crate::entry::Entry;
use crate::ops::{Conflict, Operation, Task};
use crate::search::Query;
use crate::terminal::Shell;
use crate::Locale;

mod dialogs;
mod panel;
mod search_ui;
mod terminal_ui;
mod view;

use dialogs::Dialog;
use panel::{Location, Panel, PanelAction};

/// The extensions the host's editor (`MuFoEd`) opens: F4 and Enter send
/// these as [`Event::OpenInEditor`].
pub const EDITOR_EXTENSIONS: &[&str] = &[
    "docx", "docm", "dotx", "rtf", "html", "htm", "tex", "ltx", "pdf", "djvu", "djv",
];

/// A request the host answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Open this document in the host's editor.
    OpenInEditor(PathBuf),
    /// Open this file with the system's default program.
    OpenWithSystem(PathBuf),
    /// Edit this file in a plain text editor (F4 on a non-document).
    EditText(PathBuf),
}

/// What the host may persist.
// Independent switches, not a state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The left panel's folder.
    pub left: Option<PathBuf>,
    /// The right panel's folder.
    pub right: Option<PathBuf>,
    /// Show hidden and system files.
    pub show_hidden: bool,
    /// The shell of the command line.
    pub shell: Shell,
    /// The command log is open.
    pub terminal_open: bool,
    /// Height of the command log.
    pub terminal_height: f32,
    /// Ask before moving to the recycle bin.
    pub confirm_delete: bool,
    /// The archive kind offered for packing.
    pub pack_kind: Kind,
    /// The last search.
    pub search: Query,
    /// The quick view is on.
    pub quick_view: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            left: None,
            right: None,
            show_hidden: false,
            shell: Shell::System,
            terminal_open: false,
            terminal_height: 180.0,
            confirm_delete: true,
            pack_kind: Kind::Zip,
            search: Query::default(),
            quick_view: false,
        }
    }
}

/// The file manager.
pub struct FileManager {
    locale: Locale,
    settings: Settings,
    panels: [Panel; 2],
    active: usize,
    drives: Vec<Drive>,
    drives_read: std::time::Instant,
    task: Option<Task>,
    /// Panels to refresh when the task ends.
    task_refresh: bool,
    dialog: Option<Dialog>,
    terminal: terminal_ui::TerminalState,
    view: view::ViewState,
    search: Option<search_ui::SearchState>,
    events: Vec<Event>,
    message: Option<String>,
    wake: Waker,
    /// Archive members extracted for viewing or editing, kept while open.
    temporaries: Vec<archive::TempFolder>,
}

impl std::fmt::Debug for FileManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileManager")
            .field("left", &self.panels[0].location.display())
            .field("right", &self.panels[1].location.display())
            .finish_non_exhaustive()
    }
}

/// Repaints the host from worker threads.
#[derive(Clone, Default)]
struct Waker(std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>);

impl Waker {
    fn wake(&self) {
        if let Some(ctx) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            ctx.request_repaint();
        }
    }

    fn set(&self, ctx: &egui::Context) {
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(ctx.clone());
        }
    }

    fn function(&self) -> impl Fn() + Clone + Send + Sync + 'static {
        let waker = self.clone();
        move || waker.wake()
    }
}

/// The folder a panel starts in: the setting, else the home folder.
fn start_folder(setting: Option<&PathBuf>) -> PathBuf {
    setting
        .filter(|path| path.is_dir())
        .cloned()
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
        })
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Whether a key was pressed with exactly these modifiers; consumes it.
fn take(ctx: &egui::Context, key: Key, ctrl: bool, shift: bool, alt: bool) -> bool {
    ctx.input_mut(|input| {
        let before = input.events.len();
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key { key: pressed, pressed: true, modifiers, .. }
                    if *pressed == key && modifiers.command == ctrl && modifiers.shift == shift && modifiers.alt == alt
            )
        });
        before != input.events.len()
    })
}

impl FileManager {
    /// A file manager with `settings`.
    pub fn new(locale: Locale, settings: Settings) -> Self {
        let wake = Waker::default();
        let function = wake.function();
        let left = Panel::new(
            start_folder(settings.left.as_ref()),
            settings.show_hidden,
            &function,
        );
        let right = Panel::new(
            start_folder(settings.right.as_ref()),
            settings.show_hidden,
            &function,
        );
        let terminal = terminal_ui::TerminalState::new(&settings);
        Self {
            locale,
            panels: [left, right],
            active: 0,
            drives: drives::drives(),
            drives_read: std::time::Instant::now(),
            task: None,
            task_refresh: false,
            dialog: None,
            terminal,
            view: view::ViewState::default(),
            search: None,
            events: Vec::new(),
            message: None,
            wake,
            temporaries: Vec::new(),
            settings,
        }
    }

    /// The settings, with the panels' current folders.
    pub fn settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        settings.left = self.panels[0].location.folder().map(Path::to_path_buf);
        settings.right = self.panels[1].location.folder().map(Path::to_path_buf);
        settings.terminal_open = self.terminal.open;
        settings.terminal_height = self.terminal.height;
        settings.quick_view = self.view.quick;
        settings
    }

    /// Changes the interface language.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// The folder of the active panel, if it shows one.
    pub fn current_folder(&self) -> Option<&Path> {
        self.panels[self.active].location.folder()
    }

    /// Opens `folder` in the active panel.
    pub fn open_folder(&mut self, folder: impl Into<PathBuf>) {
        let function = self.wake.function();
        self.panels[self.active].go(Location::Folder(folder.into()), None, &function);
    }

    /// The events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Draws the manager filling `ui`; returns the events for the host.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<Event> {
        let ctx = ui.ctx().clone();
        self.wake.set(&ctx);
        self.update();
        let locale = self.locale.clone();
        if self.dialog.is_none() && !ctx.egui_wants_keyboard_input() {
            self.keyboard(&ctx);
        }
        egui::Panel::bottom(egui::Id::new("file-manager-keys"))
            .exact_size(30.0)
            .show(ui, |ui| self.function_bar(ui, &locale));
        if self.task.is_some() {
            egui::Panel::bottom(egui::Id::new("file-manager-task"))
                .exact_size(30.0)
                .show(ui, |ui| self.task_ui(ui, &locale));
        }
        self.terminal_panel(ui, &locale);
        egui::CentralPanel::default().show(ui, |ui| {
            if self.view.viewer.is_some() {
                self.viewer_ui(ui, &locale);
            } else {
                self.panels_ui(ui, &locale);
            }
        });
        self.dialog_ui(&ctx, &locale);
        self.search_window(&ctx, &locale);
        self.take_events()
    }

    /// Once per frame: listings, the task, the drive list.
    fn update(&mut self) {
        for panel in &mut self.panels {
            panel.poll();
            panel.set_show_hidden(self.settings.show_hidden);
        }
        if self.drives_read.elapsed() > std::time::Duration::from_secs(5) {
            self.drives = drives::drives();
            self.drives_read = std::time::Instant::now();
        }
        if self.task.as_ref().is_some_and(Task::is_finished) {
            if let Some(task) = self.task.take() {
                self.finish_task(&task);
            }
        }
        self.terminal_poll();
    }

    fn finish_task(&mut self, task: &Task) {
        let report = task.report();
        for (path, usage) in &report.usage {
            for panel in &mut self.panels {
                panel.sizes.insert(path.clone(), *usage);
            }
        }
        if self.task_refresh {
            let function = self.wake.function();
            for panel in &mut self.panels {
                if !matches!(panel.location, Location::Results { .. }) {
                    panel.refresh(&function);
                }
            }
        }
        if !report.errors.is_empty() {
            self.dialog = Some(Dialog::Report {
                title: task.title,
                report,
            });
        } else if report.cancelled {
            self.message = Some(self.locale.text("Cancelled").to_owned());
        }
    }

    /// Starts an operation (one at a time).
    fn run(&mut self, operation: Operation) {
        if self.task.is_some() {
            self.message = Some(self.locale.text("Another operation is running").to_owned());
            return;
        }
        self.task_refresh = !matches!(operation, Operation::Usage(_));
        self.message = None;
        self.task = Some(Task::start(operation, self.wake.function()));
    }

    fn active_panel(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    /// The other panel's folder, the default target of copies.
    fn other_folder(&self) -> Option<PathBuf> {
        self.panels[1 - self.active]
            .location
            .folder()
            .map(Path::to_path_buf)
    }

    #[allow(clippy::too_many_lines)]
    fn keyboard(&mut self, ctx: &egui::Context) {
        let function = self.wake.function();
        let page = self.panels[self.active].page();
        // Most specific combinations first.
        let pressed = |key, ctrl, shift, alt| take(ctx, key, ctrl, shift, alt);
        if pressed(Key::Tab, false, false, false) {
            self.active = 1 - self.active;
        } else if pressed(Key::F7, false, false, true) {
            self.open_search();
        } else if pressed(Key::F5, false, false, true) {
            self.pack_dialog();
        } else if pressed(Key::F9, false, false, true) {
            self.unpack_dialog();
        } else if pressed(Key::F6, false, true, false) {
            self.rename_dialog();
        } else if pressed(Key::F8, false, true, false) || pressed(Key::Delete, false, true, false) {
            self.delete_dialog(true);
        } else if pressed(Key::F3, false, false, false) {
            self.view_current();
        } else if pressed(Key::F4, false, false, false) {
            self.edit_current();
        } else if pressed(Key::F5, false, false, false) {
            self.transfer_dialog(false);
        } else if pressed(Key::F6, false, false, false) {
            self.transfer_dialog(true);
        } else if pressed(Key::F7, false, false, false) {
            self.dialog = Some(Dialog::MakeFolder {
                name: String::new(),
            });
        } else if pressed(Key::F8, false, false, false) || pressed(Key::Delete, false, false, false)
        {
            self.delete_dialog(false);
        } else if pressed(Key::Q, true, false, false) {
            self.view.quick = !self.view.quick;
        } else if pressed(Key::R, true, false, false) {
            self.active_panel().refresh(&function);
        } else if pressed(Key::A, true, false, false) {
            self.active_panel().select_mask("*", true, true);
        } else if pressed(Key::H, true, false, false) {
            self.settings.show_hidden = !self.settings.show_hidden;
        } else if pressed(Key::U, true, false, false) {
            self.panels.swap(0, 1);
        } else if pressed(Key::Backtick, true, false, false) {
            self.terminal.open = !self.terminal.open;
        } else if pressed(Key::F3, true, false, false) {
            self.sort_by(crate::entry::SortKey::Name);
        } else if pressed(Key::F4, true, false, false) {
            self.sort_by(crate::entry::SortKey::Extension);
        } else if pressed(Key::F5, true, false, false) {
            self.sort_by(crate::entry::SortKey::Modified);
        } else if pressed(Key::F6, true, false, false) {
            self.sort_by(crate::entry::SortKey::Size);
        } else if pressed(Key::ArrowLeft, false, false, true) {
            self.active_panel().back(&function);
        } else if pressed(Key::ArrowRight, false, false, true) {
            self.active_panel().forward(&function);
        } else if pressed(Key::Enter, false, true, true) {
            self.count_all();
        } else if pressed(Key::Enter, true, false, false) {
            self.name_to_command_line();
        } else if pressed(Key::Enter, false, true, false) {
            if let Some(entry) = self.panels[self.active]
                .current()
                .filter(|entry| !Panel::is_up(entry))
            {
                self.events.push(Event::OpenWithSystem(entry.path.clone()));
            }
        } else if pressed(Key::Enter, false, false, false)
            || pressed(Key::PageDown, true, false, false)
        {
            let cursor = self.panels[self.active].cursor;
            self.open_row(cursor);
        } else if pressed(Key::PageUp, true, false, false) {
            self.active_panel().up(&function);
        } else if pressed(Key::Backspace, false, false, false) {
            if self.terminal.input.is_empty() {
                self.active_panel().up(&function);
            } else {
                self.terminal.input.pop();
            }
        } else if pressed(Key::Insert, false, false, false) {
            let panel = self.active_panel();
            panel.toggle_current();
            panel.move_cursor(1, false);
        } else if pressed(Key::Space, false, false, false) && self.terminal.input.is_empty() {
            self.space();
        } else {
            self.cursor_keys(ctx, page);
        }
        self.typed_text(ctx);
    }

    fn cursor_keys(&mut self, ctx: &egui::Context, page: isize) {
        let moves = [
            (Key::ArrowUp, -1),
            (Key::ArrowDown, 1),
            (Key::PageUp, -page),
            (Key::PageDown, page),
            (Key::Home, isize::MIN / 2),
            (Key::End, isize::MAX / 2),
        ];
        for (key, step) in moves {
            if take(ctx, key, false, false, false) {
                self.active_panel().move_cursor(step, false);
            } else if take(ctx, key, false, true, false) {
                self.active_panel().move_cursor(step, true);
            }
        }
    }

    /// Typed characters go to the command line; on an empty one `+`, `-`
    /// and `*` are the selection commands, as on the number pad in TC.
    fn typed_text(&mut self, ctx: &egui::Context) {
        let typed: Vec<String> = ctx.input_mut(|input| {
            let mut out = Vec::new();
            input.events.retain(|event| match event {
                egui::Event::Text(text) => {
                    out.push(text.clone());
                    false
                }
                _ => true,
            });
            out
        });
        for text in typed {
            if self.terminal.input.is_empty() {
                match text.as_str() {
                    "+" => {
                        self.dialog = Some(Dialog::SelectMask {
                            select: true,
                            mask: "*.*".into(),
                            folders: false,
                        });
                        continue;
                    }
                    "-" => {
                        self.dialog = Some(Dialog::SelectMask {
                            select: false,
                            mask: "*.*".into(),
                            folders: false,
                        });
                        continue;
                    }
                    "*" => {
                        self.active_panel().invert(false);
                        continue;
                    }
                    " " => continue,
                    _ => {}
                }
            }
            self.terminal.input.push_str(&text);
            self.terminal.focus = true;
        }
    }

    fn sort_by(&mut self, key: crate::entry::SortKey) {
        let panel = self.active_panel();
        if panel.sort == key {
            panel.descending = !panel.descending;
        } else {
            panel.sort = key;
            panel.descending = false;
        }
        panel.rebuild();
    }

    /// Space: select the row and, for a folder, count its contents.
    fn space(&mut self) {
        let panel = &mut self.panels[self.active];
        let Some(entry) = panel
            .current()
            .filter(|entry| !Panel::is_up(entry))
            .cloned()
        else {
            return;
        };
        panel.toggle_current();
        if entry.is_dir && entry.path.exists() {
            self.run(Operation::Usage(vec![entry.path]));
        }
    }

    /// Counts every folder of the active panel (Alt+Shift+Enter).
    fn count_all(&mut self) {
        let folders: Vec<PathBuf> = self.panels[self.active]
            .rows
            .iter()
            .filter(|entry| entry.is_dir && !Panel::is_up(entry) && entry.path.exists())
            .map(|entry| entry.path.clone())
            .collect();
        if !folders.is_empty() {
            self.run(Operation::Usage(folders));
        }
    }

    fn name_to_command_line(&mut self) {
        if let Some(entry) = self.panels[self.active]
            .current()
            .filter(|entry| !Panel::is_up(entry))
        {
            let name = if entry.name.contains(' ') {
                format!("\"{}\" ", entry.name)
            } else {
                format!("{} ", entry.name)
            };
            self.terminal.input.push_str(&name);
            self.terminal.focus = true;
        }
    }

    /// Enter on a row: into folders and archives, documents to the editor,
    /// other files to the system.
    fn open_row(&mut self, index: usize) {
        let function = self.wake.function();
        let panel = &mut self.panels[self.active];
        let Some(entry) = panel.rows.get(index).cloned() else {
            return;
        };
        panel.cursor = index;
        if Panel::is_up(&entry) {
            if matches!(panel.location, Location::Results { .. }) {
                panel.back(&function);
            } else {
                panel.up(&function);
            }
            return;
        }
        match panel.location.clone() {
            Location::Archive { file, .. } => {
                if entry.is_dir {
                    let inner = entry.path.to_string_lossy().into_owned();
                    panel.go(Location::Archive { file, inner }, None, &function);
                } else {
                    self.open_member(&file, &entry.path.to_string_lossy(), false);
                }
            }
            Location::Results { .. } if !entry.is_dir => {
                // A search result: go to it.
                if let Some(parent) = entry.path.parent() {
                    panel.go(
                        Location::Folder(parent.to_path_buf()),
                        Some(entry.name.clone()),
                        &function,
                    );
                }
            }
            _ => {
                if entry.is_dir {
                    panel.go(Location::Folder(entry.path.clone()), None, &function);
                } else if archive::Kind::of(&entry.path).is_some() && !is_editor_file(&entry.path) {
                    panel.go(
                        Location::Archive {
                            file: entry.path.clone(),
                            inner: String::new(),
                        },
                        None,
                        &function,
                    );
                } else if is_editor_file(&entry.path) {
                    self.events.push(Event::OpenInEditor(entry.path));
                } else {
                    self.events.push(Event::OpenWithSystem(entry.path));
                }
            }
        }
    }

    /// Opens an archive member: extracted to a temporary folder first.
    fn open_member(&mut self, archive: &Path, member: &str, edit: bool) {
        match archive::extract_to_temporary(archive, member) {
            Ok((path, folder)) => {
                self.temporaries.push(folder);
                let event = if is_editor_file(&path) {
                    Event::OpenInEditor(path)
                } else if edit {
                    Event::EditText(path)
                } else {
                    Event::OpenWithSystem(path)
                };
                self.events.push(event);
            }
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    /// F4: documents to the editor, other files to a text editor.
    fn edit_current(&mut self) {
        let panel = &self.panels[self.active];
        let Some(entry) = panel
            .current()
            .filter(|entry| !Panel::is_up(entry) && !entry.is_dir)
            .cloned()
        else {
            return;
        };
        if let Location::Archive { file, .. } = &panel.location {
            let file = file.clone();
            self.open_member(&file, &entry.path.to_string_lossy(), true);
        } else if is_editor_file(&entry.path) {
            self.events.push(Event::OpenInEditor(entry.path));
        } else {
            self.events.push(Event::EditText(entry.path));
        }
    }

    fn transfer_dialog(&mut self, moving: bool) {
        let panel = &self.panels[self.active];
        let sources = panel.targets();
        if sources.is_empty() {
            return;
        }
        let from_archive = match &panel.location {
            Location::Archive { file, .. } => Some(file.clone()),
            _ => None,
        };
        let destination = self
            .other_folder()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.dialog = Some(Dialog::Transfer {
            moving: moving && from_archive.is_none(),
            sources,
            from_archive,
            destination,
            conflict: Conflict::Skip,
        });
    }

    fn rename_dialog(&mut self) {
        let panel = &self.panels[self.active];
        if panel.location.folder().is_none() {
            return;
        }
        if let Some(entry) = panel.current().filter(|entry| !Panel::is_up(entry)) {
            self.dialog = Some(Dialog::Rename {
                from: entry.path.clone(),
                name: entry.name.clone(),
            });
        }
    }

    fn delete_dialog(&mut self, permanent: bool) {
        let panel = &self.panels[self.active];
        if panel.location.folder().is_none() && !matches!(panel.location, Location::Results { .. })
        {
            self.message = Some(
                self.locale
                    .text("Archives are read only here: unpack, change, pack again")
                    .to_owned(),
            );
            return;
        }
        let sources: Vec<PathBuf> = panel
            .targets()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if sources.is_empty() {
            return;
        }
        if permanent || self.settings.confirm_delete {
            self.dialog = Some(Dialog::Delete { sources, permanent });
        } else {
            self.run(Operation::Delete {
                sources,
                permanent: false,
            });
        }
    }

    fn pack_dialog(&mut self) {
        let panel = &self.panels[self.active];
        let Some(folder) = panel.location.folder() else {
            return;
        };
        let sources: Vec<PathBuf> = panel
            .targets()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if sources.is_empty() {
            return;
        }
        let name = match sources.as_slice() {
            [single] => single
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            _ => folder.file_name().map_or_else(
                || "archive".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
        };
        let destination = self.other_folder().unwrap_or_else(|| folder.to_path_buf());
        self.dialog = Some(Dialog::Pack {
            sources,
            name,
            kind: self.settings.pack_kind,
            destination: destination.display().to_string(),
        });
    }

    // Inside an archive or on one: two cases of one command.
    #[allow(clippy::single_match_else)]
    fn unpack_dialog(&mut self) {
        let panel = &self.panels[self.active];
        let destination = self
            .other_folder()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        match &panel.location {
            Location::Archive { file, .. } => {
                let members: Vec<String> = panel
                    .targets()
                    .into_iter()
                    .map(|entry| entry.path.to_string_lossy().into_owned())
                    .collect();
                self.dialog = Some(Dialog::Unpack {
                    archive: file.clone(),
                    members: (!members.is_empty()).then_some(members),
                    destination,
                    overwrite: false,
                });
            }
            _ => {
                let Some(entry) = panel
                    .current()
                    .filter(|entry| Kind::of(&entry.path).is_some())
                else {
                    self.message =
                        Some(self.locale.text("Put the cursor on an archive").to_owned());
                    return;
                };
                self.dialog = Some(Dialog::Unpack {
                    archive: entry.path.clone(),
                    members: None,
                    destination,
                    overwrite: false,
                });
            }
        }
    }

    fn apply_panel_actions(&mut self, side: usize, actions: Vec<PanelAction>) {
        for action in actions {
            if action != PanelAction::Activate {
                self.active = side;
            }
            match action {
                PanelAction::Activate => self.active = side,
                PanelAction::Open(index) => self.open_row(index),
                PanelAction::View => self.view_current(),
                PanelAction::Edit => self.edit_current(),
                PanelAction::Rename => self.rename_dialog(),
                PanelAction::Delete => self.delete_dialog(false),
                PanelAction::Pack => self.pack_dialog(),
                PanelAction::Unpack => self.unpack_dialog(),
                PanelAction::Properties => self.space(),
                PanelAction::CopyPath => {
                    if let Some(entry) = self.panels[side].current() {
                        let text = match &self.panels[side].location {
                            Location::Archive { file, .. } => {
                                format!("{}/{}", file.display(), entry.path.display())
                            }
                            _ => entry.path.display().to_string(),
                        };
                        self.view.copied = Some(text);
                    }
                }
            }
        }
    }

    fn panels_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        if let Some(text) = self.view.copied.take() {
            ui.ctx().copy_text(text);
        }
        let function = self.wake.function();
        let quick = self.view.quick;
        let active = self.active;
        let mut all_actions: [Vec<PanelAction>; 2] = [Vec::new(), Vec::new()];
        let stroke_active = ui.visuals().selection.bg_fill;
        let stroke_idle = ui.visuals().widgets.noninteractive.bg_stroke.color;
        ui.columns(2, |columns| {
            for (side, column) in columns.iter_mut().enumerate() {
                let is_active = side == active;
                let frame = egui::Frame::new().inner_margin(4).stroke(egui::Stroke::new(
                    1.0,
                    if is_active {
                        stroke_active
                    } else {
                        stroke_idle
                    },
                ));
                frame.show(column, |ui| {
                    ui.set_min_height(ui.available_height());
                    if quick && !is_active {
                        self.quick_view_ui(ui, locale);
                        return;
                    }
                    let id = if side == 0 { "left" } else { "right" };
                    let panel = &mut self.panels[side];
                    if panel.top_ui(ui, &self.drives, locale, id, &function) {
                        all_actions[side].push(PanelAction::Activate);
                    }
                    let status_height = 22.0;
                    let rows_height = (ui.available_height() - status_height).max(40.0);
                    ui.allocate_ui(egui::vec2(ui.available_width(), rows_height), |ui| {
                        all_actions[side].extend(panel.ui(ui, is_active, locale, id));
                    });
                    let (selected_files, selected_bytes, files, bytes) = panel.summary();
                    ui.small(locale.format(
                        "{} of {} selected, {} of {}",
                        &[
                            selected_files.to_string(),
                            files.to_string(),
                            crate::entry::human_size(selected_bytes),
                            crate::entry::human_size(bytes),
                        ],
                    ));
                });
            }
        });
        for (side, actions) in all_actions.into_iter().enumerate() {
            self.apply_panel_actions(side, actions);
        }
    }

    fn function_bar(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        ui.horizontal_centered(|ui| {
            let mut key = |label: &str, text: &str| {
                ui.button(format!("{label} {}", locale.text(text)))
                    .clicked()
            };
            if key("F3", "View") {
                self.view_current();
            }
            if key("F4", "Edit") {
                self.edit_current();
            }
            if key("F5", "Copy") {
                self.transfer_dialog(false);
            }
            if key("F6", "Move") {
                self.transfer_dialog(true);
            }
            if key("F7", "New folder") {
                self.dialog = Some(Dialog::MakeFolder {
                    name: String::new(),
                });
            }
            if key("F8", "Delete") {
                self.delete_dialog(false);
            }
            if key("Alt+F5", "Pack") {
                self.pack_dialog();
            }
            if key("Alt+F9", "Unpack") {
                self.unpack_dialog();
            }
            if key("Alt+F7", "Search") {
                self.open_search();
            }
            let quick = self.view.quick;
            if ui
                .selectable_label(quick, format!("Ctrl+Q {}", locale.text("Quick view")))
                .clicked()
            {
                self.view.quick = !quick;
            }
            ui.checkbox(&mut self.settings.show_hidden, locale.text("Hidden"));
            if let Some(message) = &self.message {
                ui.separator();
                ui.colored_label(ui.visuals().warn_fg_color, message);
            }
        });
    }

    fn task_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        let Some(task) = &self.task else {
            return;
        };
        let mut cancel = false;
        ui.horizontal_centered(|ui| {
            let progress = &task.progress;
            let done = progress
                .bytes_done
                .load(std::sync::atomic::Ordering::Relaxed);
            let total = progress
                .bytes_total
                .load(std::sync::atomic::Ordering::Relaxed);
            let files = progress
                .files_done
                .load(std::sync::atomic::Ordering::Relaxed);
            ui.label(locale.text(task.title));
            ui.add(
                egui::ProgressBar::new(progress.fraction())
                    .desired_width(260.0)
                    .text(format!(
                        "{} / {} · {files}",
                        crate::entry::human_size(done),
                        crate::entry::human_size(total)
                    )),
            );
            cancel = ui.button(locale.text("Cancel")).clicked();
            ui.add(
                egui::Label::new(egui::RichText::new(progress.current()).small().weak()).truncate(),
            );
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
        if cancel {
            task.cancel();
        }
    }
}

/// Whether the host's editor opens a file.
fn is_editor_file(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            EDITOR_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// The entries of a selection as paths.
fn paths(entries: &[Entry]) -> Vec<PathBuf> {
    entries.iter().map(|entry| entry.path.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_files_are_documents() {
        assert!(is_editor_file(Path::new("a/Report.DOCX")));
        assert!(is_editor_file(Path::new("b.pdf")));
        assert!(!is_editor_file(Path::new("c.txt")));
        assert_eq!(paths(&[]), Vec::<PathBuf>::new());
    }

    #[test]
    fn settings_round_trip() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).expect("serializes");
        assert_eq!(
            serde_json::from_str::<Settings>(&json).expect("parses"),
            settings
        );
    }
}
