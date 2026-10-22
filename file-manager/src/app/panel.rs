//! One file panel: where it is, what it lists, the cursor and selection,
//! and how it is drawn.

// Column widths and row positions are small positive point values.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};

use crate::archive::{self, Member};
use crate::entry::{self, Entry, Listing, SortKey, Usage};
use crate::mask::Mask;
use crate::Locale;

/// Where a panel is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Location {
    /// A folder.
    Folder(PathBuf),
    /// A folder inside an archive (`inner` is `""` at the top).
    Archive { file: PathBuf, inner: String },
    /// Search results "fed" to the panel.
    Results { title: String, entries: Vec<Entry> },
}

impl Location {
    /// The text of the path bar.
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Folder(path) => path.display().to_string(),
            Self::Archive { file, inner } if inner.is_empty() => {
                format!("{}{}", file.display(), std::path::MAIN_SEPARATOR)
            }
            Self::Archive { file, inner } => format!(
                "{}{}{}",
                file.display(),
                std::path::MAIN_SEPARATOR,
                inner.replace('/', std::path::MAIN_SEPARATOR_STR)
            ),
            Self::Results { title, .. } => title.clone(),
        }
    }

    /// The real folder (for the terminal, new folders and copy targets).
    pub(crate) fn folder(&self) -> Option<&Path> {
        match self {
            Self::Folder(path) => Some(path),
            Self::Archive { .. } | Self::Results { .. } => None,
        }
    }

    /// The folder containing this place, and the name to put the cursor on.
    fn parent(&self) -> Option<(Location, String)> {
        match self {
            Self::Folder(path) => {
                let parent = path.parent()?;
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some((Self::Folder(parent.to_path_buf()), name))
            }
            Self::Archive { file, inner } if inner.is_empty() => {
                let name = file.file_name()?.to_string_lossy().into_owned();
                Some((Self::Folder(file.parent()?.to_path_buf()), name))
            }
            Self::Archive { file, inner } => {
                let (parent, name) = inner.rsplit_once('/').unwrap_or(("", inner));
                Some((
                    Self::Archive {
                        file: file.clone(),
                        inner: parent.to_owned(),
                    },
                    name.to_owned(),
                ))
            }
            Self::Results { .. } => None,
        }
    }
}

/// What a click or key in a panel asks the manager to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PanelAction {
    /// The panel was clicked: make it active.
    Activate,
    /// Open the row at this index (Enter, double-click).
    Open(usize),
    /// Context menu commands.
    View,
    Edit,
    CopyPath,
    Delete,
    Pack,
    Unpack,
    Properties,
    Rename,
}

/// An archive listing being read: the archive and the answer to come.
type MemberLoading = (PathBuf, Receiver<Result<Vec<Member>, String>>);

enum Navigation {
    Go(Location, Option<String>),
    Back,
    Forward,
}

/// A file panel.
// Location locking, sort direction, scroll and visibility are independent options.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Panel {
    locked: bool,
    pending_navigation: Option<Navigation>,
    pub(crate) location: Location,
    all: Vec<Entry>,
    /// Sorted and filtered rows; `..` first when there is a parent.
    pub(crate) rows: Vec<Entry>,
    listing: Option<Listing>,
    members: Option<(PathBuf, Arc<Vec<Member>>)>,
    member_loading: Option<MemberLoading>,
    pub(crate) cursor: usize,
    pub(crate) selected: HashSet<PathBuf>,
    /// The row a Shift-click extends from.
    anchor: Option<usize>,
    pub(crate) sort: SortKey,
    pub(crate) descending: bool,
    /// Quick filter (a mask).
    pub(crate) filter: String,
    back: Vec<Location>,
    forward: Vec<Location>,
    pub(crate) path_edit: Option<String>,
    pub(crate) error: Option<String>,
    /// Folder sizes counted with Space or Alt+Shift+Enter.
    pub(crate) sizes: HashMap<PathBuf, Usage>,
    pub(crate) scroll_to_cursor: bool,
    viewport: (f32, f32),
    /// The name to put the cursor on when the listing arrives.
    focus_name: Option<String>,
    show_hidden: bool,
}

/// The name of the parent row.
const UP: &str = "..";
/// Height of a row.
pub(crate) const ROW: f32 = 18.0;

impl Panel {
    /// Persist a real location even when this tab temporarily shows search results.
    pub(super) fn saved_location(&self) -> &Location {
        if matches!(self.location, Location::Results { .. }) {
            self.back
                .iter()
                .rev()
                .find(|location| !matches!(location, Location::Results { .. }))
                .unwrap_or(&self.location)
        } else {
            &self.location
        }
    }

    /// Restored background tabs start reading only when first shown.
    pub(super) fn new(folder: PathBuf, show_hidden: bool) -> Self {
        Self {
            locked: false,
            pending_navigation: None,
            location: Location::Folder(folder),
            all: Vec::new(),
            rows: Vec::new(),
            listing: None,
            members: None,
            member_loading: None,
            cursor: 0,
            selected: HashSet::new(),
            anchor: None,
            sort: SortKey::Name,
            descending: false,
            filter: String::new(),
            back: Vec::new(),
            forward: Vec::new(),
            path_edit: None,
            error: None,
            sizes: HashMap::new(),
            scroll_to_cursor: true,
            viewport: (0.0, 0.0),
            focus_name: None,
            show_hidden,
        }
    }

    /// Whether a listing is being read.
    pub(crate) fn is_loading(&self) -> bool {
        self.listing.is_some() || self.member_loading.is_some()
    }

    /// Goes to `location`, remembering the current one for Back.
    pub(crate) fn go(
        &mut self,
        location: Location,
        focus: Option<String>,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) {
        if self.locked && location != self.location {
            self.pending_navigation = Some(Navigation::Go(location, focus));
            return;
        }
        if location != self.location {
            let previous = std::mem::replace(&mut self.location, location.clone());
            self.back.push(previous);
            if self.back.len() > 200 {
                self.back.remove(0);
            }
            self.forward.clear();
            self.selected.clear();
            self.filter.clear();
        }
        self.focus_name = focus;
        self.load(location, wake);
    }

    /// Back in history.
    pub(crate) fn back(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        if self.locked {
            if self
                .back
                .last()
                .is_some_and(|location| *location != self.location)
            {
                self.pending_navigation = Some(Navigation::Back);
            }
            return;
        }
        if let Some(location) = self.back.pop() {
            let current = std::mem::replace(&mut self.location, location.clone());
            self.forward.push(current);
            self.selected.clear();
            self.load(location, wake);
        }
    }

    /// Forward in history.
    pub(crate) fn forward(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        if self.locked {
            if self
                .forward
                .last()
                .is_some_and(|location| *location != self.location)
            {
                self.pending_navigation = Some(Navigation::Forward);
            }
            return;
        }
        if let Some(location) = self.forward.pop() {
            let current = std::mem::replace(&mut self.location, location.clone());
            self.back.push(current);
            self.selected.clear();
            self.load(location, wake);
        }
    }

    /// Up one level.
    pub(crate) fn up(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        if let Some((parent, name)) = self.location.parent() {
            self.go(parent, Some(name), wake);
        }
    }

    pub(super) fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Navigate a new, unlocked panel while keeping this panel and its history intact.
    pub(super) fn fork_navigation(
        &mut self,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) -> Option<Self> {
        let navigation = self.pending_navigation.take()?;
        let mut panel = Self::new(PathBuf::new(), self.show_hidden);
        panel.location = self.location.clone();
        panel.sort = self.sort;
        panel.descending = self.descending;
        panel.filter.clone_from(&self.filter);
        panel.back.clone_from(&self.back);
        panel.forward.clone_from(&self.forward);
        match navigation {
            Navigation::Go(location, focus) => panel.go(location, focus, wake),
            Navigation::Back => panel.back(wake),
            Navigation::Forward => panel.forward(wake),
        }
        Some(panel)
    }

    /// Puts the cursor on `name` once the next listing arrives.
    pub(crate) fn focus_after(&mut self, name: String) {
        self.focus_name = Some(name);
    }

    /// Reads the listing again, keeping the cursor on the same name (or the
    /// one [`Panel::focus_after`] asked for).
    pub(crate) fn refresh(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        if self.focus_name.is_none() {
            self.focus_name = self.current().map(|entry| entry.name.clone());
        }
        if let Location::Archive { file, .. } = &self.location {
            // Re-read the archive too: it may have changed.
            if self
                .members
                .as_ref()
                .is_some_and(|(cached, _)| cached == file)
            {
                self.members = None;
            }
        }
        let location = self.location.clone();
        self.load(location, wake);
    }

    fn load(&mut self, location: Location, wake: &(impl Fn() + Clone + Send + 'static)) {
        self.error = None;
        self.path_edit = None;
        match location {
            Location::Folder(folder) => {
                self.member_loading = None;
                let wake = wake.clone();
                self.listing = Some(Listing::start(&folder, wake));
            }
            Location::Archive { file, .. } => {
                self.listing = None;
                if self
                    .members
                    .as_ref()
                    .is_some_and(|(cached, _)| *cached == file)
                {
                    self.rebuild();
                } else {
                    let (sender, receiver) = channel();
                    let source = file.clone();
                    let wake = wake.clone();
                    let _ = std::thread::Builder::new()
                        .name("file-manager-archive".into())
                        .spawn(move || {
                            let _ = sender
                                .send(archive::list(&source).map_err(|error| error.to_string()));
                            wake();
                        });
                    self.member_loading = Some((file, receiver));
                }
            }
            Location::Results { entries, .. } => {
                self.listing = None;
                self.member_loading = None;
                self.all = entries;
                self.rebuild();
            }
        }
    }

    /// Takes finished listings in.
    pub(crate) fn poll(&mut self) {
        if let Some(listing) = &self.listing {
            if let Some(result) = listing.poll() {
                self.listing = None;
                match result {
                    Ok(entries) => self.all = entries,
                    Err(error) => {
                        self.all.clear();
                        self.error = Some(error.to_string());
                    }
                }
                self.rebuild();
            }
        }
        if let Some((file, receiver)) = &self.member_loading {
            if let Ok(result) = receiver.try_recv() {
                let file = file.clone();
                self.member_loading = None;
                match result {
                    Ok(members) => {
                        self.members = Some((file, Arc::new(members)));
                        self.rebuild();
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.all.clear();
                        self.rebuild();
                    }
                }
            }
        }
    }

    /// Shows or hides hidden files.
    pub(crate) fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.rebuild();
        }
    }

    /// Sorts, filters and lays out the rows, keeping the cursor's name.
    pub(crate) fn rebuild(&mut self) {
        let keep = self
            .focus_name
            .take()
            .or_else(|| self.current().map(|entry| entry.name.clone()));
        if let Location::Archive { file, inner } = &self.location {
            if let Some((cached, members)) = &self.members {
                if cached == file {
                    self.all = archive::children(members, inner);
                }
            }
        }
        let mask = Mask::parse(&self.filter);
        let mut rows: Vec<Entry> = self
            .all
            .iter()
            .filter(|entry| self.show_hidden || !entry.hidden)
            .filter(|entry| self.filter.is_empty() || entry.is_dir || mask.matches(&entry.name))
            .cloned()
            .collect();
        entry::sort(&mut rows, self.sort, self.descending);
        if self.location.parent().is_some() || matches!(self.location, Location::Results { .. }) {
            rows.insert(
                0,
                Entry {
                    name: UP.into(),
                    path: PathBuf::new(),
                    is_dir: true,
                    size: 0,
                    modified: None,
                    hidden: false,
                    readonly: false,
                    link: false,
                },
            );
        }
        self.rows = rows;
        let present: HashSet<&PathBuf> = self.rows.iter().map(|entry| &entry.path).collect();
        self.selected.retain(|path| present.contains(path));
        self.cursor = keep
            .and_then(|name| self.rows.iter().position(|entry| entry.name == name))
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
        self.scroll_to_cursor = true;
    }

    /// The row under the cursor.
    pub(crate) fn current(&self) -> Option<&Entry> {
        self.rows.get(self.cursor)
    }

    /// Whether a row is the `..` row.
    pub(crate) fn is_up(entry: &Entry) -> bool {
        entry.name == UP && entry.path.as_os_str().is_empty()
    }

    /// The items commands act on: the selection, or the row under the
    /// cursor.
    pub(crate) fn targets(&self) -> Vec<Entry> {
        let selected: Vec<Entry> = self
            .rows
            .iter()
            .filter(|entry| self.selected.contains(&entry.path))
            .cloned()
            .collect();
        if selected.is_empty() {
            self.current()
                .filter(|entry| !Self::is_up(entry))
                .cloned()
                .into_iter()
                .collect()
        } else {
            selected
        }
    }

    /// Toggles the selection of the cursor's row.
    pub(crate) fn toggle_current(&mut self) {
        if let Some(entry) = self.current().filter(|entry| !Self::is_up(entry)) {
            let path = entry.path.clone();
            if !self.selected.remove(&path) {
                self.selected.insert(path);
            }
        }
    }

    /// Selects (or deselects) every row matching a mask.
    pub(crate) fn select_mask(&mut self, mask: &str, select: bool, folders: bool) {
        let mask = Mask::parse(mask);
        for entry in &self.rows {
            if Self::is_up(entry) || (entry.is_dir && !folders) || !mask.matches(&entry.name) {
                continue;
            }
            if select {
                self.selected.insert(entry.path.clone());
            } else {
                self.selected.remove(&entry.path);
            }
        }
    }

    /// Inverts the selection of files (and folders when `folders`).
    pub(crate) fn invert(&mut self, folders: bool) {
        for entry in &self.rows {
            if Self::is_up(entry) || (entry.is_dir && !folders) {
                continue;
            }
            if !self.selected.remove(&entry.path) {
                self.selected.insert(entry.path.clone());
            }
        }
    }

    /// Moves the cursor by `step` rows; with `extend`, selects on the way.
    pub(crate) fn move_cursor(&mut self, step: isize, extend: bool) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let target = self
            .cursor
            .cast_signed()
            .saturating_add(step)
            .clamp(0, last.cast_signed())
            .cast_unsigned();
        if extend {
            let (low, high) = (self.cursor.min(target), self.cursor.max(target));
            let select = !self
                .rows
                .get(self.cursor)
                .is_some_and(|entry| self.selected.contains(&entry.path));
            for index in low..=high {
                let path = self.rows[index].path.clone();
                if Self::is_up(&self.rows[index]) {
                    continue;
                }
                if select {
                    self.selected.insert(path);
                } else {
                    self.selected.remove(&path);
                }
            }
        }
        self.cursor = target;
        self.scroll_to_cursor = true;
    }

    /// Rows that fit in the visible part (for Page Up / Down).
    pub(crate) fn page(&self) -> isize {
        ((self.viewport.1 / ROW).floor() as isize - 1).max(1)
    }

    /// Selected files and bytes, and the totals, for the status line.
    pub(crate) fn summary(&self) -> (usize, u64, usize, u64) {
        let mut out = (0, 0, 0, 0);
        for entry in self.rows.iter().filter(|entry| !Self::is_up(entry)) {
            let size = self.size_of(entry);
            out.2 += 1;
            out.3 += size;
            if self.selected.contains(&entry.path) {
                out.0 += 1;
                out.1 += size;
            }
        }
        out
    }

    /// A row's size: counted folder sizes included.
    pub(crate) fn size_of(&self, entry: &Entry) -> u64 {
        if entry.is_dir {
            self.sizes.get(&entry.path).map_or(0, |usage| usage.bytes)
        } else {
            entry.size
        }
    }

    /// Draws the panel; returns what the user asked for.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        active: bool,
        locale: &Locale,
        id: &str,
    ) -> Vec<PanelAction> {
        let mut actions = Vec::new();
        let visuals = ui.visuals().clone();
        let width = ui.available_width();
        // Columns: name grows; the rest have fixed widths and drop out
        // when the panel is narrow.
        let show_date = width > 360.0;
        let show_attributes = width > 470.0;
        let extension_width = 56.0;
        let size_width = 92.0;
        let date_width = if show_date { 124.0 } else { 0.0 };
        let attribute_width = if show_attributes { 40.0 } else { 0.0 };
        let name_width =
            (width - extension_width - size_width - date_width - attribute_width - 12.0).max(60.0);
        let columns = [
            (name_width, locale.text("Name"), Some(SortKey::Name)),
            (
                extension_width,
                locale.text("Ext"),
                Some(SortKey::Extension),
            ),
            (size_width, locale.text("Size"), Some(SortKey::Size)),
            (date_width, locale.text("Date"), Some(SortKey::Modified)),
            (attribute_width, locale.text("Attr"), None),
        ];
        // Header.
        let (header, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let painter = ui.painter_at(header);
        painter.rect_filled(header, 0.0, visuals.faint_bg_color);
        let mut x = header.min.x + 4.0;
        for (column_width, title, key) in columns {
            if column_width <= 0.0 {
                continue;
            }
            let rect = Rect::from_min_size(pos2(x, header.min.y), vec2(column_width, ROW));
            let response = ui.interact(rect, ui.id().with((id, "header", title)), Sense::click());
            let mut label = title.to_owned();
            if key == Some(self.sort) {
                label.push_str(if self.descending { " ▼" } else { " ▲" });
            }
            painter.text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(12.0),
                visuals.strong_text_color(),
            );
            if let Some(key) = key {
                if response.clicked() {
                    if self.sort == key {
                        self.descending = !self.descending;
                    } else {
                        self.sort = key;
                        self.descending = false;
                    }
                    self.rebuild();
                    actions.push(PanelAction::Activate);
                }
            }
            x += column_width;
        }
        // Rows.
        let mut area = egui::ScrollArea::vertical()
            .id_salt((id, "rows"))
            .auto_shrink([false, false]);
        if self.scroll_to_cursor {
            self.scroll_to_cursor = false;
            let pitch = ROW + ui.spacing().item_spacing.y;
            let top = self.cursor as f32 * pitch;
            let (offset, height) = self.viewport;
            if top < offset {
                area = area.vertical_scroll_offset(top);
            } else if height > 0.0 && top + ROW > offset + height {
                area = area.vertical_scroll_offset(top + ROW - height);
            }
        }
        let monospace = FontId::monospace(12.0);
        let proportional = FontId::proportional(13.0);
        let selected_colour = Color32::from_rgb(230, 70, 60);
        let output = area.show_rows(ui, ROW, self.rows.len(), |ui, range| {
            for index in range {
                let entry = &self.rows[index];
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let painter = ui.painter_at(rect);
                let is_cursor = index == self.cursor;
                let is_selected = self.selected.contains(&entry.path) && !Self::is_up(entry);
                if is_cursor && active {
                    painter.rect_filled(rect, 2.0, visuals.selection.bg_fill);
                } else if is_cursor {
                    painter.rect_stroke(
                        rect.shrink(0.5),
                        2.0,
                        Stroke::new(1.0, visuals.selection.bg_fill),
                        StrokeKind::Inside,
                    );
                } else if index % 2 == 1 {
                    painter.rect_filled(rect, 0.0, visuals.faint_bg_color.gamma_multiply(0.5));
                }
                let colour = if is_selected {
                    selected_colour
                } else if entry.hidden {
                    visuals.weak_text_color()
                } else if is_cursor && active {
                    visuals.selection.stroke.color
                } else {
                    visuals.text_color()
                };
                let mut x = rect.min.x + 4.0;
                let cell = |x: f32, column_width: f32| {
                    Rect::from_min_size(pos2(x, rect.min.y), vec2(column_width - 6.0, ROW))
                };
                // Name.
                let name = if Self::is_up(entry) {
                    format!("[{UP}]")
                } else if entry.is_dir {
                    format!("[{}]", entry.name)
                } else {
                    entry.stem().to_owned()
                };
                super::icons::paint(
                    &painter,
                    Rect::from_min_size(pos2(x, rect.center().y - 8.0), vec2(16.0, 16.0)),
                    entry,
                    Self::is_up(entry),
                    colour,
                );
                clipped_text(
                    &painter,
                    cell(x + 22.0, name_width - 22.0),
                    &name,
                    proportional.clone(),
                    colour,
                    false,
                );
                x += name_width;
                if !entry.is_dir {
                    clipped_text(
                        &painter,
                        cell(x, extension_width),
                        &entry.extension(),
                        proportional.clone(),
                        colour,
                        false,
                    );
                }
                x += extension_width;
                let size = if Self::is_up(entry) {
                    locale.text("<UP>").to_owned()
                } else if entry.is_dir {
                    self.sizes.get(&entry.path).map_or_else(
                        || locale.text("<DIR>").to_owned(),
                        |usage| entry::grouped(usage.bytes),
                    )
                } else {
                    entry::grouped(entry.size)
                };
                clipped_text(
                    &painter,
                    cell(x, size_width),
                    &size,
                    monospace.clone(),
                    colour,
                    true,
                );
                x += size_width;
                if show_date {
                    let date = entry
                        .modified
                        .and_then(image_gallery::exif::Stamp::from_system_time)
                        .map(|stamp| {
                            format!("{} {:02}:{:02}", stamp.date(), stamp.hour, stamp.minute)
                        })
                        .unwrap_or_default();
                    clipped_text(
                        &painter,
                        cell(x, date_width),
                        &date,
                        monospace.clone(),
                        colour,
                        false,
                    );
                    x += date_width;
                }
                if show_attributes && !Self::is_up(entry) {
                    let attributes = format!(
                        "{}{}{}",
                        if entry.readonly { 'r' } else { '-' },
                        if entry.hidden { 'h' } else { '-' },
                        if entry.link { 'l' } else { '-' }
                    );
                    clipped_text(
                        &painter,
                        cell(x, attribute_width),
                        &attributes,
                        monospace.clone(),
                        colour,
                        false,
                    );
                }
                // Mouse.
                if response.clicked() {
                    let modifiers = ui.input(|input| input.modifiers);
                    actions.push(PanelAction::Activate);
                    if modifiers.command {
                        if !Self::is_up(entry) {
                            let path = entry.path.clone();
                            if !self.selected.remove(&path) {
                                self.selected.insert(path);
                            }
                        }
                        self.anchor = Some(index);
                    } else if modifiers.shift {
                        let anchor = self.anchor.unwrap_or(self.cursor);
                        for row in anchor.min(index)..=anchor.max(index) {
                            if !Self::is_up(&self.rows[row]) {
                                self.selected.insert(self.rows[row].path.clone());
                            }
                        }
                    } else {
                        self.anchor = Some(index);
                    }
                    self.cursor = index;
                }
                if response.double_clicked() {
                    actions.push(PanelAction::Open(index));
                }
                if response.secondary_clicked() {
                    self.cursor = index;
                    actions.push(PanelAction::Activate);
                }
                response.context_menu(|ui| {
                    let mut item = |key: &str, action: PanelAction| {
                        if ui.button(locale.text(key)).clicked() {
                            actions.push(action);
                            ui.close();
                        }
                    };
                    item("Open", PanelAction::Open(index));
                    item("View (F3)", PanelAction::View);
                    item("Edit (F4)", PanelAction::Edit);
                    item("Rename (Shift+F6)", PanelAction::Rename);
                    item("Copy path", PanelAction::CopyPath);
                    item("Pack (Alt+F5)", PanelAction::Pack);
                    item("Unpack (Alt+F9)", PanelAction::Unpack);
                    item("Count space (Space)", PanelAction::Properties);
                    item("Delete (F8)", PanelAction::Delete);
                });
            }
        });
        self.viewport = (output.state.offset.y, output.inner_rect.height());
        if output.inner_rect.contains(
            ui.input(|input| input.pointer.interact_pos())
                .unwrap_or(pos2(-1.0, -1.0)),
        ) && ui.input(|input| input.pointer.any_pressed())
        {
            actions.push(PanelAction::Activate);
        }
        actions
    }

    /// Draws the drive buttons, the path bar and the filter above the rows.
    #[allow(clippy::too_many_lines, clippy::single_match_else)]
    pub(crate) fn top_ui(
        &mut self,
        ui: &mut egui::Ui,
        drives: &[crate::drives::Drive],
        locale: &Locale,
        id: &str,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) -> bool {
        let mut activated = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let current = self
                .location
                .folder()
                .and_then(|folder| crate::drives::drive_of(folder, drives));
            for (index, drive) in drives.iter().enumerate() {
                let button = egui::Button::new(egui::RichText::new(&drive.label).monospace())
                    .selected(current == Some(index))
                    .small();
                if ui
                    .add(button)
                    .on_hover_text(drive.root.display().to_string())
                    .clicked()
                {
                    self.go(Location::Folder(drive.root.clone()), None, wake);
                    activated = true;
                }
            }
            ui.separator();
            if ui
                .small_button("◀")
                .on_hover_text(locale.text("Back (Alt+Left)"))
                .clicked()
            {
                self.back(wake);
                activated = true;
            }
            if ui
                .small_button("▶")
                .on_hover_text(locale.text("Forward (Alt+Right)"))
                .clicked()
            {
                self.forward(wake);
                activated = true;
            }
            if ui
                .small_button("⬆")
                .on_hover_text(locale.text("Up (Backspace)"))
                .clicked()
            {
                self.up(wake);
                activated = true;
            }
            if self.is_loading() {
                ui.spinner();
            }
        });
        // The path bar: click to type a path, Enter to go.
        let mut go_to = None;
        let mut close_edit = false;
        match &mut self.path_edit {
            Some(text) => {
                let response = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(f32::INFINITY)
                        .id_salt((id, "path")),
                );
                if response.lost_focus() {
                    if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        go_to = Some(PathBuf::from(text.trim()));
                    }
                    close_edit = true;
                } else if !response.has_focus() {
                    response.request_focus();
                }
            }
            None => {
                let text = self.location.display();
                let response = ui.add(
                    egui::Label::new(egui::RichText::new(&text).monospace().strong())
                        .truncate()
                        .sense(Sense::click()),
                );
                if response.clicked() {
                    self.path_edit = Some(text);
                    activated = true;
                }
                response.on_hover_text(locale.text("Click to type a path"));
            }
        }
        if close_edit {
            self.path_edit = None;
        }
        if let Some(path) = go_to {
            if path.is_dir() {
                self.go(Location::Folder(path), None, wake);
            } else if path.is_file() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
                if let Some(parent) = path.parent() {
                    self.go(Location::Folder(parent.to_path_buf()), name, wake);
                }
            } else {
                self.error = Some(format!(
                    "{}: {}",
                    locale.text("No such folder"),
                    path.display()
                ));
            }
        }
        ui.horizontal(|ui| {
            ui.label(locale.text("Filter"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(locale.text("*.jpg *.png | thumb*"))
                    .desired_width(f32::INFINITY)
                    .id_salt((id, "filter")),
            );
            if response.changed() {
                self.rebuild();
            }
            if response.gained_focus() {
                activated = true;
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        activated
    }
}

/// Text cut to a cell, right-aligned when `right`.
fn clipped_text(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    font: FontId,
    colour: Color32,
    right: bool,
) {
    if text.is_empty() {
        return;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width().max(1.0));
    let galley = painter.layout_job(job);
    let x = if right {
        rect.max.x - galley.size().x
    } else {
        rect.min.x
    };
    let y = rect.center().y - galley.size().y / 2.0;
    painter
        .with_clip_rect(rect.expand(1.0))
        .galley(pos2(x, y), galley, colour);
}
