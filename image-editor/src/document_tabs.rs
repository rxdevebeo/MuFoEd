//! Standalone document workspace. Each tab owns pixels, history, view and recovery.
use crate::recovery_host::RecoveryHost;
use image_editor::{
    ui::{EditorSettings, ImageEditor},
    Document, EditError, Raster,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

pub(super) type TabId = u64;
pub(super) struct DocumentTab {
    pub id: TabId,
    pub editor: ImageEditor,
    pub recovery: RecoveryHost,
    project: Option<PathBuf>,
    epoch: u64,
    revision: u64,
    placeholder: bool,
}
impl DocumentTab {
    fn sync_epoch(&mut self) {
        if self.epoch != self.editor.project_epoch() {
            self.epoch = self.editor.project_epoch();
            self.project = None;
        }
    }
    pub(super) fn name(&self) -> String {
        self.recovery
            .source()
            .and_then(Path::file_name)
            .map_or_else(
                || format!("Без имени {}", self.id),
                |n| n.to_string_lossy().into_owned(),
            )
    }
    pub(super) fn project(&self) -> Option<&Path> {
        if self.epoch == self.editor.project_epoch() {
            self.project.as_deref()
        } else {
            None
        }
    }
    pub(super) fn source(&self) -> Option<&Path> {
        if self.epoch == self.editor.project_epoch() {
            self.recovery.source()
        } else {
            None
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum TabAction {
    Select(TabId),
    New,
    Close(TabId),
    CloseOthers(TabId),
    CloseAll,
    Save(TabId),
    SaveAs(TabId),
    SaveAll,
}
pub(super) struct Workspace {
    pub tabs: Vec<DocumentTab>,
    pub active: usize,
    retired: Vec<DocumentTab>,
    directory: PathBuf,
    next: TabId,
    pub settings: EditorSettings,
    revision: u64,
    closing: VecDeque<TabId>,
    closing_window: bool,
}
fn same_path(a: &Path, b: &Path) -> bool {
    let a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}
fn is_psd(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("psd"))
}
impl Workspace {
    pub(super) fn new(directory: PathBuf, editor: ImageEditor) -> Self {
        let settings = editor.settings();
        let mut workspace = Self {
            tabs: Vec::new(),
            active: 0,
            retired: Vec::new(),
            directory,
            next: 1,
            settings,
            revision: 0,
            closing: VecDeque::new(),
            closing_window: false,
        };
        let tab = workspace.make_tab(editor, None, true);
        workspace.tabs.push(tab);
        workspace
    }
    fn make_tab(
        &mut self,
        editor: ImageEditor,
        source: Option<PathBuf>,
        placeholder: bool,
    ) -> DocumentTab {
        let id = self.next;
        self.next += 1;
        let epoch = editor.project_epoch();
        let project = source.clone().filter(|p| is_psd(p));
        let recovery = RecoveryHost::new_tab(&self.directory, epoch, source);
        DocumentTab {
            id,
            editor,
            recovery,
            project,
            epoch,
            revision: self.revision,
            placeholder,
        }
    }
    pub(super) fn fresh_editor(&self) -> ImageEditor {
        let mut editor = ImageEditor::default();
        if let Err(error) = editor.apply_settings(self.settings.clone()) {
            editor.set_status(error.to_string());
        }
        editor
    }
    pub(super) fn active(&self) -> &DocumentTab {
        &self.tabs[self.active]
    }
    pub(super) fn active_mut(&mut self) -> &mut DocumentTab {
        &mut self.tabs[self.active]
    }
    pub(super) fn tab(&self, id: TabId) -> Option<&DocumentTab> {
        self.tabs.iter().find(|t| t.id == id)
    }
    pub(super) fn select(&mut self, id: TabId) {
        if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
            if index != self.active {
                self.active_mut().editor.prepare_for_host_action();
                self.active = index;
            }
        }
    }
    pub(super) fn cycle(&mut self, backwards: bool) {
        let count = self.tabs.len();
        let index = if backwards {
            (self.active + count - 1) % count
        } else {
            (self.active + 1) % count
        };
        self.select(self.tabs[index].id);
    }
    pub(super) fn keyboard_navigation(&mut self, ui: &egui::Ui) {
        if !self.active().editor.document_commands_enabled() {
            return;
        }
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.ctrl
            && !modifiers.alt
            && ui.input_mut(|i| i.consume_key(modifiers, egui::Key::Tab))
        {
            self.cycle(modifiers.shift);
        }
    }
    pub(super) fn new_document(&mut self) {
        self.synchronize_settings();
        self.active_mut().editor.prepare_for_host_action();
        let tab = self.make_tab(self.fresh_editor(), None, false);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }
    fn insert(&mut self, tab: DocumentTab) {
        let replace = self.active().placeholder
            && self.active().source().is_none()
            && !self.active().editor.has_unsaved_changes()
            && !self.active().editor.background_active()
            && !self.active().editor.has_staged_result();
        self.active_mut().editor.prepare_for_host_action();
        if replace {
            let mut old = std::mem::replace(&mut self.tabs[self.active], tab);
            old.recovery.close();
            self.retired.push(old);
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
    }
    pub(super) fn open(&mut self, path: &Path) -> Result<TabId, EditError> {
        if let Some(id) = self
            .tabs
            .iter()
            .find(|t| t.source().is_some_and(|p| same_path(p, path)))
            .map(|t| t.id)
        {
            self.select(id);
            return Ok(id);
        }
        let path = std::fs::canonicalize(path)?;
        let document = if is_psd(&path)
            || path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            Document::open_project(&path)?
        } else {
            Document::from_raster(Raster::open(&path)?)?
        };
        self.synchronize_settings();
        let mut editor = self.fresh_editor();
        editor.replace(document);
        let tab = self.make_tab(editor, Some(path), false);
        let id = tab.id;
        self.insert(tab);
        Ok(id)
    }
    pub(super) fn insert_recovered(&mut self, editor: ImageEditor, recovery: RecoveryHost) {
        let epoch = editor.project_epoch();
        let tab = DocumentTab {
            id: self.next,
            editor,
            recovery,
            project: None,
            epoch,
            revision: self.revision,
            placeholder: false,
        };
        self.next += 1;
        self.insert(tab);
    }
    pub(super) fn save_to(&mut self, id: TabId, path: &Path) -> Result<(), EditError> {
        if !is_psd(path) {
            return Err(EditError::Invalid(
                "Сохраните проект с расширением .psd; PNG доступен через экспорт".into(),
            ));
        }
        if self
            .tabs
            .iter()
            .any(|t| t.id != id && t.source().is_some_and(|p| same_path(p, path)))
        {
            return Err(EditError::Invalid(
                "Этот файл открыт в другой вкладке. Выберите другое имя".into(),
            ));
        }
        let tab = self
            .tabs
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| EditError::Invalid("Вкладка уже закрыта".into()))?;
        tab.editor.prepare_for_host_action();
        tab.editor.document().save_project(path)?;
        let path = std::fs::canonicalize(path)?;
        tab.editor.mark_saved();
        tab.project = Some(path.clone());
        tab.epoch = tab.editor.project_epoch();
        tab.recovery.bind_source(tab.epoch, Some(path));
        tab.placeholder = false;
        tab.editor.set_status("PSD-проект сохранён");
        Ok(())
    }
    pub(super) fn synchronize_settings(&mut self) {
        let active = self.active();
        let current = active.editor.settings();
        if active.revision == self.revision && current != self.settings {
            self.settings = current;
            self.revision += 1;
            self.active_mut().revision = self.revision;
        }
        for tab in &mut self.tabs {
            if tab.revision != self.revision
                && !tab.editor.background_active()
                && tab.editor.apply_settings(self.settings.clone()).is_ok()
            {
                tab.revision = self.revision;
            }
        }
    }
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        self.synchronize_settings();
        for tab in &mut self.tabs {
            tab.sync_epoch();
            tab.editor.poll_background(ctx);
            tab.recovery.poll(&mut tab.editor);
        }
        for tab in &mut self.retired {
            tab.recovery.poll(&mut tab.editor);
        }
        self.retired.retain(|t| !t.recovery.ready_to_close());
    }
    pub(super) fn close_pending(&self) -> bool {
        !self.closing.is_empty()
    }
    pub(super) fn request_close(&mut self, ids: Vec<TabId>, window: bool) {
        if self.close_pending() {
            return;
        }
        self.active_mut().editor.prepare_for_host_action();
        self.closing = ids.into();
        self.closing_window = window;
    }
    pub(super) fn cancel_close(&mut self) {
        self.closing.clear();
        self.closing_window = false;
    }
    pub(super) fn pending_tab(&self) -> Option<TabId> {
        self.closing.front().copied()
    }
    /// Skip already clean tabs. Return true once closing the whole window was accepted.
    pub(super) fn advance_close(&mut self) -> bool {
        while let Some(id) = self.pending_tab() {
            if self.tab(id).is_some_and(|t| {
                t.editor.has_unsaved_changes()
                    || t.editor.background_active()
                    || t.editor.has_staged_result()
            }) {
                self.select(id);
                return false;
            }
            self.close_confirmed(id);
        }
        std::mem::take(&mut self.closing_window)
    }
    pub(super) fn close_confirmed(&mut self, id: TabId) {
        if self.pending_tab() != Some(id) {
            return;
        }
        self.closing.pop_front();
        if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
            let mut tab = self.tabs.remove(index);
            tab.editor.cancel_background();
            tab.recovery.close();
            self.retired.push(tab);
            if self.tabs.is_empty() {
                let tab = self.make_tab(self.fresh_editor(), None, true);
                self.tabs.push(tab);
            }
            if index < self.active {
                self.active -= 1;
            }
            self.active = self.active.min(self.tabs.len() - 1);
        }
    }
    pub(super) fn ids(&self) -> Vec<TabId> {
        self.tabs.iter().map(|t| t.id).collect()
    }
    pub(super) fn begin_shutdown(&mut self) {
        for tab in self.tabs.iter_mut().chain(&mut self.retired) {
            tab.editor.cancel_background();
            tab.recovery.close();
        }
    }
    pub(super) fn cancel_shutdown(&mut self) {
        for tab in &mut self.tabs {
            tab.recovery.cancel_close();
        }
    }
    pub(super) fn ready_to_close(&self) -> bool {
        self.tabs
            .iter()
            .chain(&self.retired)
            .all(|t| t.recovery.ready_to_close())
    }
    pub(super) fn tab_bar(&self, ui: &mut egui::Ui, enabled: bool) -> Option<TabAction> {
        let mut action = None;
        let active = self.active().id;
        let scroll = ui.ctx().data_mut(|data| {
            let key = egui::Id::new("last-active-document-tab");
            let changed = data.get_temp::<TabId>(key) != Some(active);
            data.insert_temp(key, active);
            changed
        });
        egui::Panel::top("image-document-tabs")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(36, 36, 40))
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([28.0, 28.0], egui::Button::new("+"))
                            .on_hover_text("Новый документ · Ctrl+N")
                            .clicked()
                        {
                            action = Some(TabAction::New);
                        }
                        egui::ScrollArea::horizontal()
                            .id_salt("document-tabs-scroll")
                            .min_scrolled_height(0.0)
                            .max_height(36.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for tab in &self.tabs {
                                        if let Some(next) = tab_button(ui, tab, active, scroll) {
                                            action = Some(next);
                                        }
                                    }
                                });
                            });
                    });
                });
            });
        action
    }
}
fn tab_button(
    ui: &mut egui::Ui,
    tab: &DocumentTab,
    active: TabId,
    scroll: bool,
) -> Option<TabAction> {
    let mut action = None;
    ui.push_id(tab.id, |ui| {
        let fill = if tab.id == active {
            egui::Color32::from_rgb(48, 64, 87)
        } else {
            egui::Color32::from_rgb(45, 45, 49)
        };
        egui::Frame::new()
            .fill(fill)
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let dirty = if tab.editor.has_unsaved_changes() {
                        "● "
                    } else {
                        ""
                    };
                    let state = if tab.editor.background_active() {
                        "  …"
                    } else if tab.editor.has_staged_result() {
                        "  ✓"
                    } else {
                        ""
                    };
                    let label = format!("{dirty}{}{state}", tab.name());
                    let response = ui
                        .add_sized(
                            [140.0, 24.0],
                            egui::Button::new(egui::RichText::new(label).size(13.0))
                                .frame(false)
                                .truncate(),
                        )
                        .on_hover_text(
                            tab.source()
                                .map_or_else(|| tab.name(), |p| p.display().to_string()),
                        );
                    if tab.id == active && scroll {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() {
                        action = Some(TabAction::Select(tab.id));
                    }
                    if response.clicked_by(egui::PointerButton::Middle) {
                        action = Some(TabAction::Close(tab.id));
                    }
                    response.context_menu(|ui| {
                        for (label, next) in [
                            ("Сохранить", TabAction::Save(tab.id)),
                            ("Сохранить как…", TabAction::SaveAs(tab.id)),
                            ("Сохранить все", TabAction::SaveAll),
                            ("Закрыть", TabAction::Close(tab.id)),
                            ("Закрыть остальные", TabAction::CloseOthers(tab.id)),
                            ("Закрыть все", TabAction::CloseAll),
                        ] {
                            if ui.button(label).clicked() {
                                action = Some(next);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .add_sized([22.0, 22.0], egui::Button::new("×").frame(false))
                        .on_hover_text("Закрыть · Ctrl+W")
                        .clicked()
                    {
                        action = Some(TabAction::Close(tab.id));
                    }
                });
            });
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace(root: &Path) -> Workspace {
        Workspace::new(
            root.join("recovery"),
            ImageEditor::new(Document::new(8, 6).unwrap()),
        )
    }
    fn image(root: &Path, name: &str, color: [u8; 4]) -> PathBuf {
        let path = root.join(name);
        Raster::solid(8, 6, color).unwrap().save_png(&path).unwrap();
        path
    }
    #[test]
    fn ctrl_tab_and_ctrl_shift_tab_move_in_opposite_directions_and_dialogs_block_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = workspace(dir.path());
        w.new_document();
        w.new_document();
        let ids = w.ids();
        w.select(ids[0]);
        let ctx = egui::Context::default();
        for (shift, expected) in [(true, ids[2]), (false, ids[0])] {
            let modifiers = egui::Modifiers {
                ctrl: true,
                command: true,
                shift,
                ..Default::default()
            };
            let _ = ctx.run_ui(
                egui::RawInput {
                    modifiers,
                    events: vec![egui::Event::Key {
                        key: egui::Key::Tab,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                    ..Default::default()
                },
                |ui| w.keyboard_navigation(ui),
            );
            assert_eq!(w.active().id, expected);
        }
        w.active_mut().editor.open_settings();
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |ui| w.keyboard_navigation(ui),
        );
        assert_eq!(w.active().id, ids[0]);
    }
    #[test]
    fn opening_files_keeps_independent_dirty_documents_and_reuses_existing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let a = image(dir.path(), "a.png", [20, 30, 40, 255]);
        let b = image(dir.path(), "b.png", [60, 70, 80, 255]);
        let mut w = workspace(dir.path());
        let first = w.open(&a).unwrap();
        assert_eq!(w.tabs.len(), 1, "unused startup canvas is replaced");
        w.active_mut().editor.import_layer_file(&b).unwrap();
        let modified = w.active().editor.document().clone();
        let second = w.open(&b).unwrap();
        assert_ne!(first, second);
        assert_eq!(w.tabs.len(), 2);
        w.open(&a).unwrap();
        assert_eq!(w.tabs.len(), 2);
        assert_eq!(w.active().id, first);
        assert_eq!(w.active().editor.document(), &modified);
        assert!(w.active().editor.has_unsaved_changes());
        let active = w.active().id;
        assert!(w.open(&dir.path().join("missing.png")).is_err());
        assert_eq!(w.active().id, active);
        assert_eq!(w.tabs.len(), 2);
    }
    #[test]
    fn psd_save_as_then_save_preserves_source_and_does_not_clean_other_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let a = image(dir.path(), "a.png", [20, 30, 40, 255]);
        let original = std::fs::read(&a).unwrap();
        let mut w = workspace(dir.path());
        let id = w.open(&a).unwrap();
        assert!(w.active().project().is_none());
        w.active_mut()
            .editor
            .restore_recovered(Document::new(8, 6).unwrap());
        w.new_document();
        let second = w.active().id;
        w.active_mut()
            .editor
            .restore_recovered(Document::new(5, 7).unwrap());
        let path = dir.path().join("project.psd");
        w.save_to(id, &path).unwrap();
        assert!(!w.tab(id).unwrap().editor.has_unsaved_changes());
        assert!(w.tab(second).unwrap().editor.has_unsaved_changes());
        assert_eq!(
            Document::open_project(&path).unwrap(),
            *w.tab(id).unwrap().editor.document()
        );
        assert_eq!(std::fs::read(&a).unwrap(), original);
        assert_eq!(
            w.tab(id).unwrap().project().unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert!(
            w.save_to(second, &path).is_err(),
            "cannot overwrite another open tab"
        );
        assert!(
            w.save_to(second, &a).is_err(),
            "saving is PSD, never overwrite imported raster"
        );
        w.select(id);
        w.active_mut().editor.replace(Document::new(3, 4).unwrap());
        assert!(
            w.active().project().is_none(),
            "replacing canvas invalidates its prior save path immediately"
        );
    }
    #[test]
    fn cancelled_or_failed_save_keeps_close_queue_and_dirty_content_intact() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = workspace(dir.path());
        w.active_mut()
            .editor
            .restore_recovered(Document::new(3, 4).unwrap());
        let first = w.active().id;
        w.new_document();
        w.active_mut()
            .editor
            .restore_recovered(Document::new(5, 6).unwrap());
        let second = w.active().id;
        w.request_close(w.ids(), true);
        assert!(!w.advance_close());
        assert_eq!(w.pending_tab(), Some(first));
        assert!(w
            .save_to(first, &dir.path().join("absent/fail.psd"))
            .is_err());
        assert_eq!(w.pending_tab(), Some(first));
        assert!(w.tab(first).unwrap().editor.has_unsaved_changes());
        w.cancel_close();
        assert!(!w.close_pending());
        assert_eq!(w.tabs.len(), 2);
        w.request_close(vec![second, first], true);
        assert!(!w.advance_close());
        w.close_confirmed(second);
        assert!(!w.advance_close());
        assert_eq!(w.pending_tab(), Some(first));
        w.close_confirmed(first);
        assert!(w.advance_close());
        assert_eq!(w.tabs.len(), 1);
        assert!(!w.active().editor.has_unsaved_changes());
        assert!(
            !w.advance_close(),
            "window-close acceptance is consumed once"
        );
    }
    #[test]
    fn tab_switching_keeps_history_and_shared_settings_are_applied_without_pixels_changing() {
        let dir = tempfile::tempdir().unwrap();
        let a = image(dir.path(), "a.png", [20, 30, 40, 255]);
        let b = image(dir.path(), "b.png", [60, 70, 80, 255]);
        let mut w = workspace(dir.path());
        let first = w.open(&a).unwrap();
        w.active_mut().editor.import_layer_file(&b).unwrap();
        assert_eq!(w.active().editor.document().layers().len(), 2);
        let second = w.open(&b).unwrap();
        let second_pixels = w.active().editor.document().clone();
        let mut settings = serde_json::to_value(w.active().editor.settings()).unwrap();
        settings["request"]["model"] = "local/shared-model".into();
        w.active_mut()
            .editor
            .apply_settings(serde_json::from_value(settings).unwrap())
            .unwrap();
        w.synchronize_settings();
        w.select(first);
        assert_eq!(w.active().editor.settings().model(), "local/shared-model");
        let ctx = egui::Context::default();
        let mods = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: mods,
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: mods,
                }],
                ..Default::default()
            },
            |ui| {
                w.active_mut().editor.show(ui);
            },
        );
        assert_eq!(
            w.active().editor.document().layers().len(),
            1,
            "Undo stayed in its original tab"
        );
        assert!(!w.active().editor.has_unsaved_changes());
        assert_eq!(w.tab(second).unwrap().editor.document(), &second_pixels);
        w.cycle(true);
        assert_eq!(w.active().id, second);
        w.cycle(false);
        assert_eq!(w.active().id, first);
    }
}
