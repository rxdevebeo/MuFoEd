//! Independent folder tabs in each file panel, with backwards-compatible settings.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{FileManager, Location, Panel};
use crate::{entry::SortKey, Locale};

/// Persisted navigation and view settings of one folder tab.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FolderTabSettings {
    /// Folder path, or the archive file when `archive` is set.
    pub folder: PathBuf,
    /// Path within an archive; an empty string means its root.
    pub archive: Option<String>,
    /// Keep this tab's location fixed and protect it from closing until unpinned.
    pub pinned: bool,
    /// Sort column.
    pub sort: SortKey,
    /// Reverse the sort order.
    pub descending: bool,
    /// File mask in the quick filter.
    pub filter: String,
}

/// Ordered folder tabs and active tab for one panel.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PanelTabSettings {
    /// Tabs in display order. Empty migrates the old panel folder setting.
    pub tabs: Vec<FolderTabSettings>,
    /// Selected tab index.
    pub selected: usize,
}

struct Tab {
    id: u64,
    pinned: bool,
    panel: Panel,
}

impl Tab {
    fn new(settings: &FolderTabSettings, hidden: bool) -> Self {
        let mut panel = Panel::new(settings.folder.clone(), hidden);
        if let Some(inner) = &settings.archive {
            panel.location = Location::Archive {
                file: settings.folder.clone(),
                inner: inner.clone(),
            };
        }
        panel.sort = settings.sort;
        panel.descending = settings.descending;
        panel.filter.clone_from(&settings.filter);
        Self::from_panel(panel, settings.pinned)
    }

    fn from_panel(mut panel: Panel, pinned: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        panel.set_locked(pinned);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            pinned,
            panel,
        }
    }

    fn settings(&self) -> FolderTabSettings {
        let (folder, archive) = match self.panel.saved_location() {
            Location::Folder(folder) => (folder.clone(), None),
            Location::Archive { file, inner } => (file.clone(), Some(inner.clone())),
            Location::Results { .. } => (super::start_folder(None), None),
        };
        FolderTabSettings {
            folder,
            archive,
            pinned: self.pinned,
            sort: self.panel.sort,
            descending: self.panel.descending,
            filter: self.panel.filter.clone(),
        }
    }
}

/// Existing file operations always address the selected panel through Deref.
pub(super) struct PanelTabs {
    tabs: Vec<Tab>,
    selected: usize,
    reveal_selected: bool,
}

impl Deref for PanelTabs {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.tabs[self.selected].panel
    }
}

impl DerefMut for PanelTabs {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.tabs[self.selected].panel
    }
}

#[derive(Clone, Copy)]
struct DragTab {
    side: usize,
    id: u64,
}

#[derive(Clone, Copy)]
pub(super) enum TabAction {
    Select(u64),
    New,
    Duplicate(u64),
    Close(u64),
    Pin(u64),
    Move(u64, bool),
    OtherPanel(u64),
    Drop { source: usize, id: u64, before: u64 },
}

impl PanelTabs {
    fn follow_navigation(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        if let Some(panel) = self.tabs[self.selected].panel.fork_navigation(wake) {
            self.tabs.push(Tab::from_panel(panel, false));
            self.selected = self.tabs.len() - 1;
            self.reveal_selected = true;
        }
    }

    pub(super) fn go(
        &mut self,
        location: Location,
        focus: Option<String>,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) {
        self.tabs[self.selected].panel.go(location, focus, wake);
        self.follow_navigation(wake);
    }

    pub(super) fn back(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        self.tabs[self.selected].panel.back(wake);
        self.follow_navigation(wake);
    }

    pub(super) fn forward(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        self.tabs[self.selected].panel.forward(wake);
        self.follow_navigation(wake);
    }

    pub(super) fn up(&mut self, wake: &(impl Fn() + Clone + Send + 'static)) {
        self.tabs[self.selected].panel.up(wake);
        self.follow_navigation(wake);
    }

    pub(super) fn top_ui(
        &mut self,
        ui: &mut egui::Ui,
        drives: &[crate::drives::Drive],
        locale: &Locale,
        id: &str,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) -> bool {
        let activated = self.tabs[self.selected]
            .panel
            .top_ui(ui, drives, locale, id, wake);
        self.follow_navigation(wake);
        activated
    }

    pub(super) fn new(
        settings: &PanelTabSettings,
        fallback: PathBuf,
        hidden: bool,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) -> Self {
        let tabs = if settings.tabs.is_empty() {
            vec![Tab::new(
                &FolderTabSettings {
                    folder: fallback,
                    ..Default::default()
                },
                hidden,
            )]
        } else {
            settings
                .tabs
                .iter()
                .map(|tab| {
                    let mut settings = tab.clone();
                    if settings.folder.as_os_str().is_empty() {
                        settings.folder.clone_from(&fallback);
                    }
                    Tab::new(&settings, hidden)
                })
                .collect()
        };
        let selected = settings.selected.min(tabs.len() - 1);
        let mut result = Self {
            tabs,
            selected,
            reveal_selected: true,
        };
        // Keep pinned tabs together while preserving the selected tab and relative order.
        let selected_id = result.selected_id();
        result.tabs.sort_by_key(|tab| !tab.pinned);
        result.select(selected_id);
        result.refresh(wake);
        result
    }

    pub(super) fn settings(&self) -> PanelTabSettings {
        PanelTabSettings {
            tabs: self.tabs.iter().map(Tab::settings).collect(),
            selected: self.selected,
        }
    }

    pub(super) fn selected_id(&self) -> u64 {
        self.tabs[self.selected].id
    }

    fn index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn select(&mut self, id: u64) {
        if let Some(index) = self.index(id) {
            self.reveal_selected |= self.selected != index;
            self.selected = index;
        }
    }

    pub(super) fn cycle(&mut self, direction: isize) {
        self.reveal_selected = true;
        self.selected = if direction < 0 {
            (self.selected + self.tabs.len() - 1) % self.tabs.len()
        } else {
            (self.selected + 1) % self.tabs.len()
        };
    }

    pub(super) fn poll_tabs(&mut self, hidden: bool) {
        for tab in &mut self.tabs {
            tab.panel.poll();
            tab.panel.set_show_hidden(hidden);
        }
    }

    fn close(&mut self, id: u64) -> bool {
        let Some(index) = self.index(id) else {
            return false;
        };
        if self.tabs.len() == 1 || self.tabs[index].pinned {
            return false;
        }
        let selected = self.selected_id();
        self.tabs.remove(index);
        self.reveal_selected = true;
        self.selected = index.min(self.tabs.len() - 1);
        if selected != id {
            self.select(selected);
        }
        true
    }

    fn pin(&mut self, id: u64) {
        let Some(index) = self.index(id) else {
            return;
        };
        let selected = self.selected_id();
        let mut tab = self.tabs.remove(index);
        tab.pinned = !tab.pinned;
        tab.panel.set_locked(tab.pinned);
        let target = self.tabs.iter().take_while(|tab| tab.pinned).count();
        self.tabs.insert(target, tab);
        self.select(selected);
    }

    fn reorder(&mut self, id: u64, before: u64) {
        let (Some(from), Some(to)) = (self.index(id), self.index(before)) else {
            return;
        };
        if from == to || self.tabs[from].pinned != self.tabs[to].pinned {
            return;
        }
        let selected = self.selected_id();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.select(selected);
    }

    fn add(
        &mut self,
        mut settings: FolderTabSettings,
        hidden: bool,
        wake: &(impl Fn() + Clone + Send + 'static),
    ) {
        settings.pinned = false;
        self.tabs.push(Tab::new(&settings, hidden));
        self.selected = self.tabs.len() - 1;
        self.reveal_selected = true;
        self.refresh(wake);
    }

    fn tab_menu(&self, ui: &mut egui::Ui, locale: &Locale, index: usize) -> Option<TabAction> {
        let tab = &self.tabs[index];
        let mut action = None;
        for (label, command, enabled) in [
            ("Duplicate folder tab", TabAction::Duplicate(tab.id), true),
            (
                if tab.pinned {
                    "Unpin folder tab"
                } else {
                    "Pin folder tab"
                },
                TabAction::Pin(tab.id),
                true,
            ),
            (
                "Move tab left",
                TabAction::Move(tab.id, false),
                index > 0 && self.tabs[index - 1].pinned == tab.pinned,
            ),
            (
                "Move tab right",
                TabAction::Move(tab.id, true),
                index + 1 < self.tabs.len() && self.tabs[index + 1].pinned == tab.pinned,
            ),
            (
                "Move to other panel",
                TabAction::OtherPanel(tab.id),
                self.tabs.len() > 1,
            ),
            (
                "Close folder tab",
                TabAction::Close(tab.id),
                !tab.pinned && self.tabs.len() > 1,
            ),
        ] {
            if ui
                .add_enabled(enabled, egui::Button::new(locale.text(label)))
                .clicked()
            {
                action = Some(command);
                ui.close();
            }
        }
        action
    }

    /// One compact horizontal strip above the path bar.
    pub(super) fn tabs_ui(
        &mut self,
        ui: &mut egui::Ui,
        locale: &Locale,
        side: usize,
    ) -> Option<TabAction> {
        let mut action = None;
        let reveal = std::mem::take(&mut self.reveal_selected);
        ui.push_id(("folder-tabs", side), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button("+")
                    .on_hover_text(locale.text("New folder tab (Ctrl+T)"))
                    .clicked()
                {
                    action = Some(TabAction::New);
                }
                egui::ScrollArea::horizontal()
                    .id_salt("strip")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (index, tab) in self.tabs.iter().enumerate() {
                                let saved = tab.settings();
                                let name = saved.folder.file_name().map_or_else(
                                    || saved.folder.display().to_string(),
                                    |name| name.to_string_lossy().into_owned(),
                                );
                                let title = if tab.pinned {
                                    format!("    {name}")
                                } else {
                                    name
                                };
                                let response = ui
                                    .push_id(tab.id, |ui| {
                                        let response = ui.add(
                                            egui::Button::selectable(index == self.selected, title)
                                                .sense(egui::Sense::click_and_drag()),
                                        );
                                        if tab.pinned {
                                            paint_pin(ui, &response, index == self.selected);
                                        }
                                        if reveal && index == self.selected {
                                            response.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        if !tab.pinned
                                            && self.tabs.len() > 1
                                            && ui
                                                .small_button("×")
                                                .on_hover_text(
                                                    locale.text("Close folder tab (Ctrl+W)"),
                                                )
                                                .clicked()
                                        {
                                            action = Some(TabAction::Close(tab.id));
                                        }
                                        response
                                    })
                                    .inner
                                    .on_hover_text(tab.panel.location.display());
                                if response.clicked() {
                                    action = Some(TabAction::Select(tab.id));
                                }
                                if response.middle_clicked() {
                                    action = Some(TabAction::Close(tab.id));
                                }
                                response.dnd_set_drag_payload(DragTab { side, id: tab.id });
                                if let Some(payload) = response.dnd_hover_payload::<DragTab>() {
                                    if payload.id != tab.id {
                                        ui.painter().rect_stroke(
                                            response.rect,
                                            2.0,
                                            ui.visuals().selection.stroke,
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                                if let Some(payload) = response.dnd_release_payload::<DragTab>() {
                                    action = Some(TabAction::Drop {
                                        source: payload.side,
                                        id: payload.id,
                                        before: tab.id,
                                    });
                                }
                                response.context_menu(|ui| {
                                    if let Some(command) = self.tab_menu(ui, locale, index) {
                                        action = Some(command);
                                    }
                                });
                            }
                        });
                    });
            });
        });
        action
    }
}

impl FileManager {
    pub(super) fn folder_tab_action(&mut self, side: usize, action: TabAction) {
        let wake = self.wake.function();
        let hidden = self.settings.show_hidden;
        let active_side = if matches!(action, TabAction::OtherPanel(_)) {
            1 - side
        } else {
            side
        };
        let previous = self.panels[side].selected_id();
        let group = &mut self.panels[side];
        match action {
            TabAction::Select(id) => group.select(id),
            TabAction::New => {
                let saved = group.tabs[group.selected].settings();
                group.add(saved, hidden, &wake);
            }
            TabAction::Duplicate(id) => {
                if let Some(index) = group.index(id) {
                    let saved = group.tabs[index].settings();
                    group.add(saved, hidden, &wake);
                }
            }
            TabAction::Close(id) => {
                group.close(id);
            }
            TabAction::Pin(id) => group.pin(id),
            TabAction::Move(id, right) => {
                if let Some(index) = group.index(id) {
                    let target = if right {
                        index.checked_add(1)
                    } else {
                        index.checked_sub(1)
                    };
                    if let Some(other) = target.and_then(|i| group.tabs.get(i)).map(|tab| tab.id) {
                        group.reorder(id, other);
                    }
                }
            }
            TabAction::OtherPanel(id) => self.transfer_folder_tab(side, 1 - side, id, None),
            TabAction::Drop { source, id, before } => {
                if source == side {
                    group.reorder(id, before);
                } else if source < 2 {
                    self.transfer_folder_tab(source, side, id, Some(before));
                }
            }
        }
        if self.panels[side].selected_id() != previous && !self.panels[side].is_loading() {
            self.panels[side].refresh(&wake);
        }
        let changed = self.active != active_side || self.panels[side].selected_id() != previous;
        // A click on the covered panel's tabs returns it from quick view to files.
        if self.active != active_side {
            self.view.quick = false;
        }
        self.active = active_side;
        if changed {
            self.view.clear_quick();
        }
    }

    fn transfer_folder_tab(&mut self, from: usize, to: usize, id: u64, before: Option<u64>) {
        let source = &mut self.panels[from];
        let Some(index) = source.index(id) else {
            return;
        };
        if source.tabs.len() == 1 {
            return;
        }
        let selected = source.selected_id();
        let tab = source.tabs.remove(index);
        source.selected = index.min(source.tabs.len() - 1);
        if selected != id {
            source.select(selected);
        }
        let target = &mut self.panels[to];
        let pinned = target.tabs.iter().take_while(|tab| tab.pinned).count();
        let index = before
            .and_then(|id| target.index(id))
            .unwrap_or(target.tabs.len());
        let index = if tab.pinned {
            index.min(pinned)
        } else {
            index.max(pinned)
        };
        target.tabs.insert(index, tab);
        target.select(id);
        let wake = self.wake.function();
        self.panels[to].refresh(&wake);
    }
}

/// A small pushpin drawn as lines, independent of the installed fonts.
fn paint_pin(ui: &egui::Ui, response: &egui::Response, selected: bool) {
    let center = egui::pos2(response.rect.left() + 9.0, response.rect.center().y);
    let colour = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    let stroke = egui::Stroke::new(1.0, colour);
    for (a, b) in [
        ((-3.0, -4.0), (3.0, -4.0)),
        ((-2.0, -4.0), (-2.0, 0.0)),
        ((2.0, -4.0), (2.0, 0.0)),
        ((-4.0, 1.0), (4.0, 1.0)),
        ((0.0, 1.0), (0.0, 5.0)),
    ] {
        ui.painter().line_segment(
            [center + egui::vec2(a.0, a.1), center + egui::vec2(b.0, b.1)],
            stroke,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    fn manager() -> FileManager {
        FileManager::new(Locale::default(), Settings::default())
    }

    #[test]
    fn pinned_folders_fork_on_navigation_and_remain_fixed_after_restart() {
        for side in 0..2 {
            let mut manager = manager();
            manager.active = side;
            let original = manager.current_folder().unwrap().to_path_buf();
            let pinned = manager.panels[side].selected_id();
            manager.folder_tab_action(side, TabAction::Pin(pinned));
            manager.open_folder(&original);
            assert_eq!(manager.panels[side].tabs.len(), 1); // Refresh is not a new location.
            let next = original.join("pinned-navigation-test");
            manager.open_folder(&next);
            assert_eq!(manager.current_folder(), Some(next.as_path()));
            assert_eq!(manager.panels[side].tabs.len(), 2);
            assert!(!manager.panels[side].tabs[1].pinned);
            assert_eq!(
                manager.panels[side].tabs[0].panel.location.folder(),
                Some(original.as_path())
            );
            let settings = manager.settings();
            let restored = FileManager::new(Locale::default(), settings.clone());
            assert_eq!(restored.settings(), settings);
            manager.folder_tab_action(side, TabAction::Select(pinned));
            manager.folder_tab_action(side, TabAction::Pin(pinned));
            manager.open_folder(next);
            assert_eq!(manager.panels[side].tabs.len(), 2); // Unpinned tabs navigate normally.
        }
    }

    #[test]
    fn pinned_history_and_parent_navigation_fork_without_consuming_history() {
        for direction in [0, 1, 2] {
            let mut manager = manager();
            let original = manager.current_folder().unwrap().to_path_buf();
            let child = original.join("pinned-history-test");
            manager.open_folder(&child);
            if direction == 1 {
                manager.panels[0].back(&|| {});
            }
            let location = manager.panels[0].location.clone();
            let pinned = manager.panels[0].selected_id();
            manager.folder_tab_action(0, TabAction::Pin(pinned));
            match direction {
                0 => manager.active_panel().back(&|| {}),
                1 => manager.active_panel().forward(&|| {}),
                _ => manager.active_panel().up(&|| {}),
            }
            let target = if direction == 1 { &child } else { &original };
            assert_eq!(manager.current_folder(), Some(target.as_path()));
            assert_eq!(manager.panels[0].tabs.len(), 2);
            assert_eq!(manager.panels[0].tabs[0].panel.location, location);
            manager.folder_tab_action(0, TabAction::Select(pinned));
            match direction {
                1 => manager.active_panel().forward(&|| {}),
                _ => manager.active_panel().back(&|| {}),
            }
            assert_eq!(manager.panels[0].tabs.len(), 3); // Original history still usable.
        }
    }

    #[test]
    fn pinned_internal_navigation_archives_and_results_open_unpinned_tabs() {
        let mut manager = manager();
        let original = manager.panels[0].location.clone();
        let pinned = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::Pin(pinned));
        // Drive/path bar navigation happens inside Panel, then the wrapper dispatches it.
        let next = std::env::temp_dir().join("pinned-archive.zip");
        manager.panels[0].tabs[0].panel.go(
            Location::Archive {
                file: next.clone(),
                inner: String::new(),
            },
            None,
            &|| {},
        );
        assert_eq!(manager.panels[0].location, original);
        manager.panels[0].follow_navigation(&|| {});
        assert_eq!(manager.panels[0].tabs.len(), 2);
        let archive = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::Pin(archive));
        manager.panels[0].go(
            Location::Archive {
                file: next,
                inner: "images".into(),
            },
            None,
            &|| {},
        );
        assert_eq!(manager.panels[0].tabs.len(), 3);
        assert!(!manager.panels[0].tabs[2].pinned);
        manager.folder_tab_action(0, TabAction::Select(pinned));
        manager.panels[0].go(
            Location::Results {
                title: "Search".into(),
                entries: Vec::new(),
            },
            None,
            &|| {},
        );
        assert_eq!(manager.panels[0].tabs.len(), 4);
        assert_eq!(manager.panels[0].tabs[0].panel.location, original);
    }

    #[test]
    fn restart_restores_both_panels_tabs_views_and_active_tab() {
        let mut manager = manager();
        let first = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::Pin(first));
        manager.folder_tab_action(0, TabAction::New);
        manager.panels[0].sort = SortKey::Size;
        manager.panels[0].descending = true;
        // A temporarily unavailable path must survive restart, not silently disappear.
        manager.open_folder(std::env::temp_dir().join("mufoed-unavailable-tab-test"));
        manager.panels[0].filter = "*.png".into();
        manager.folder_tab_action(1, TabAction::New);
        manager.panels[1].sort = SortKey::Modified;
        manager.panels[1].descending = true;
        let settings = manager.settings();
        let json = serde_json::to_string(&settings).unwrap();
        let restored = FileManager::new(Locale::default(), serde_json::from_str(&json).unwrap());
        assert_eq!(restored.settings(), settings);
        assert_eq!(restored.active, 1);
        assert_eq!(restored.panels[0].selected, 1);
        assert_eq!(restored.panels[0].filter, "*.png");
        assert!(restored.panels[0].tabs[0].pinned);
        assert!(!restored.panels[0].tabs[0].panel.is_loading());
        assert!(restored.panels[0].tabs[1].panel.is_loading());
    }

    #[test]
    fn pinned_tabs_are_protected_and_reorder_preserves_selection() {
        let mut manager = manager();
        let a = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::New);
        let b = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::New);
        let c = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::Pin(c));
        assert_eq!(manager.panels[0].tabs[0].id, c);
        manager.folder_tab_action(0, TabAction::Close(c));
        assert_eq!(manager.panels[0].tabs.len(), 3);
        manager.folder_tab_action(0, TabAction::Move(b, false));
        assert_eq!(
            manager.panels[0]
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<_>>(),
            vec![c, b, a]
        );
        assert_eq!(manager.panels[0].selected_id(), c);
        manager.folder_tab_action(0, TabAction::Move(b, false));
        assert_eq!(manager.panels[0].tabs[0].id, c); // Cannot cross pinned boundary.
        manager.folder_tab_action(0, TabAction::Close(b));
        manager.folder_tab_action(0, TabAction::Close(a));
        manager.folder_tab_action(0, TabAction::Pin(c));
        manager.folder_tab_action(0, TabAction::Close(c));
        assert_eq!(manager.panels[0].tabs.len(), 1);
    }

    #[test]
    fn switching_and_transfer_keep_navigation_and_view_settings() {
        let mut manager = manager();
        let a = manager.panels[0].selected_id();
        let folder = manager.current_folder().unwrap().to_path_buf();
        manager.folder_tab_action(0, TabAction::New);
        let b = manager.panels[0].selected_id();
        let next = folder.join("tab-history-test");
        manager.open_folder(&next);
        manager.panels[0].sort = SortKey::Extension;
        manager.panels[0].filter = "*.rs".into();
        manager.folder_tab_action(0, TabAction::Select(a));
        assert_eq!(manager.current_folder(), Some(folder.as_path()));
        assert_eq!(manager.panels[0].sort, SortKey::Name);
        manager.folder_tab_action(0, TabAction::OtherPanel(b));
        assert_eq!(manager.active, 1);
        assert_eq!(manager.current_folder(), Some(next.as_path()));
        assert_eq!(manager.panels[1].sort, SortKey::Extension);
        assert_eq!(manager.panels[1].filter, "*.rs");
        manager.panels[1].back(&|| {});
        assert_eq!(manager.current_folder(), Some(folder.as_path()));
        assert_eq!(manager.panels[0].tabs.len(), 1);
        let only = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::OtherPanel(only));
        assert_eq!(manager.panels[0].tabs.len(), 1);
    }

    #[test]
    fn legacy_settings_and_invalid_selection_migrate_safely() {
        let folder = std::env::temp_dir();
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "left": folder, "right": folder, "active_panel": 999,
            "folder_tabs": [{"selected": 99}, {}]
        }))
        .unwrap();
        let manager = FileManager::new(Locale::default(), settings);
        assert_eq!(manager.panels[0].location.folder(), Some(folder.as_path()));
        assert_eq!(manager.panels[1].location.folder(), Some(folder.as_path()));
        assert_eq!(manager.active, 1);
        assert_eq!(manager.panels[0].selected, 0);
    }

    #[test]
    fn dragging_tabs_between_panels_preserves_pins_and_quick_view_state() {
        let mut manager = manager();
        let pinned = manager.panels[0].selected_id();
        manager.folder_tab_action(0, TabAction::New);
        manager.folder_tab_action(0, TabAction::Pin(pinned));
        manager.view.quick = true;
        manager.folder_tab_action(0, TabAction::Select(pinned));
        assert!(manager.view.quick);
        let target = manager.panels[1].selected_id();
        manager.folder_tab_action(
            1,
            TabAction::Drop {
                source: 0,
                id: pinned,
                before: target,
            },
        );
        assert_eq!(manager.panels[0].tabs.len(), 1);
        assert_eq!(manager.panels[1].tabs[0].id, pinned);
        assert!(manager.panels[1].tabs[0].pinned);
        assert_eq!(manager.panels[1].selected_id(), pinned);
        assert!(!manager.view.quick);
        assert_eq!(manager.active, 1);
    }

    #[test]
    fn archived_location_and_search_results_persist_a_real_location() {
        let mut manager = manager();
        let archive = std::env::temp_dir().join("tab-test.zip");
        manager.panels[0].go(
            Location::Archive {
                file: archive.clone(),
                inner: "images".into(),
            },
            None,
            &|| {},
        );
        let saved = manager.settings();
        let restored = FileManager::new(Locale::default(), saved.clone());
        assert_eq!(restored.settings(), saved);
        manager.panels[0].go(
            Location::Results {
                title: "Search".into(),
                entries: Vec::new(),
            },
            None,
            &|| {},
        );
        assert_eq!(manager.settings().folder_tabs[0].tabs[0].folder, archive);
        assert_eq!(
            manager.settings().folder_tabs[0].tabs[0].archive.as_deref(),
            Some("images")
        );
    }

    #[test]
    fn tab_strip_and_shortcuts_work_with_narrow_panels() {
        let mut manager = manager();
        let ctx = egui::Context::default();
        for _ in 0..12 {
            manager.folder_tab_action(0, TabAction::New);
        }
        for key in [egui::Key::Tab, egui::Key::W, egui::Key::T] {
            let previous = manager.panels[0].selected_id();
            let count = manager.panels[0].tabs.len();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 500.0),
                )),
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                manager.show(ui);
            });
            match key {
                egui::Key::Tab => assert_ne!(manager.panels[0].selected_id(), previous),
                egui::Key::W => assert_eq!(manager.panels[0].tabs.len(), count - 1),
                egui::Key::T => assert_eq!(manager.panels[0].tabs.len(), count + 1),
                _ => unreachable!(),
            }
        }
    }
}
