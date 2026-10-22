//! Image documents embedded in the MuFoEd window, using the shared raster workspace.
use crate::{
    document_tabs::{TabId, Workspace},
    preferences, recovery_host,
};
use eframe::egui;
use image_editor::{
    ui::{FileAction, ImageEditor},
    EditError,
};
use std::path::{Path, PathBuf};

pub(crate) const IMAGE_EXTENSIONS: &[&str] = &[
    "psd", "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "json",
];
pub(crate) fn open_extensions() -> Vec<&'static str> {
    crate::formats::OPEN_EXTENSIONS
        .iter()
        .chain(IMAGE_EXTENSIONS)
        .copied()
        .collect()
}
pub(crate) fn supports(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}
struct RecoveryPrompt {
    editor: ImageEditor,
    recovery: recovery_host::RecoveryHost,
}
pub(crate) struct ImageTabInfo {
    pub id: TabId,
    pub name: String,
    pub source: Option<PathBuf>,
    pub modified: bool,
    pub busy: bool,
    pub staged: bool,
}
pub(crate) struct ImageStudio {
    preferences: preferences::Preferences,
    workspace: Workspace,
    startup: Option<RecoveryPrompt>,
    recovery_directory: PathBuf,
    close_error: String,
    exiting: bool,
    exit_cancelled: bool,
    open_requests: Vec<PathBuf>,
    listed: Vec<TabId>,
    focus_request: Option<TabId>,
    pending_saves: Vec<(TabId, bool)>,
}
impl ImageStudio {
    pub(crate) fn new() -> Self {
        Self::with_preferences(preferences::path())
    }
    fn with_preferences(preferences_path: PathBuf) -> Self {
        let recovery_directory = recovery_host::path(&preferences_path);
        let mut editor = ImageEditor::default();
        match preferences::load(&preferences_path) {
            Ok(Some(settings)) => {
                if let Some(cache) = settings.cache_dir() {
                    std::env::set_var("IMAGE_STUDIO_CACHE", cache);
                    std::env::set_var("PLAKAT_CACHE_DIR", cache);
                }
                if let Err(error) = editor.apply_settings(settings) {
                    editor.set_status(error.to_string());
                }
            }
            Ok(None) => {}
            Err(error) => editor.set_status(format!("Настройки: {error}")),
        }
        let preferences = preferences::Preferences::new(preferences_path, editor.settings());
        let workspace = Workspace::new(recovery_directory.clone(), editor);
        let prompt_editor = workspace.fresh_editor();
        let recovery = recovery_host::RecoveryHost::new(
            &recovery_directory,
            prompt_editor.project_epoch(),
            None,
        );
        let startup = recovery.prompt_pending().then_some(RecoveryPrompt {
            editor: prompt_editor,
            recovery,
        });
        Self {
            preferences,
            workspace,
            startup,
            recovery_directory,
            close_error: String::new(),
            exiting: false,
            exit_cancelled: false,
            open_requests: Vec::new(),
            listed: Vec::new(),
            focus_request: None,
            pending_saves: Vec::new(),
        }
    }
    fn editor_mut(&mut self) -> &mut ImageEditor {
        &mut self.workspace.active_mut().editor
    }
    pub(crate) fn open(&mut self, path: &Path) -> Result<TabId, EditError> {
        let result = self.workspace.open(path);
        if let Ok(id) = result {
            self.register(id);
        }
        if let Err(error) = &result {
            self.editor_mut().set_status(error.to_string());
        }
        result
    }
    fn register(&mut self, id: TabId) {
        if !self.listed.contains(&id) {
            self.listed.push(id);
        }
        self.focus_request = Some(id);
    }
    pub(crate) fn new_document(&mut self) -> TabId {
        self.workspace.new_document();
        let id = self.workspace.active().id;
        self.register(id);
        id
    }
    pub(crate) fn tabs(&self) -> Vec<ImageTabInfo> {
        self.listed
            .iter()
            .filter_map(|&id| {
                self.workspace.tab(id).map(|t| ImageTabInfo {
                    id,
                    name: t.name(),
                    source: t.source().map(Path::to_path_buf),
                    modified: t.editor.has_unsaved_changes(),
                    busy: t.editor.background_active(),
                    staged: t.editor.has_staged_result(),
                })
            })
            .collect()
    }
    pub(crate) fn take_focus_request(&mut self) -> Option<TabId> {
        self.focus_request.take()
    }
    pub(crate) fn select(&mut self, id: TabId) {
        self.workspace.select(id);
    }
    pub(crate) fn request_save(&mut self, id: TabId, save_as: bool) {
        self.pending_saves.push((id, save_as));
    }
    pub(crate) fn close_tab(&mut self, id: TabId) -> bool {
        self.workspace.request_close(vec![id], false);
        self.workspace.advance_close();
        if self.workspace.close_pending() {
            self.focus_request = Some(id);
        }
        self.workspace.close_pending()
    }
    pub(crate) fn resume_after_shutdown_error(&mut self, error: &str) {
        self.workspace.cancel_shutdown();
        if let Some(prompt) = &mut self.startup {
            prompt.recovery.cancel_close();
        }
        self.exiting = false;
        self.editor_mut()
            .set_status(format!("Завершение моделей: {error}"));
    }
    pub(crate) fn prepare_switch(&mut self) {
        self.editor_mut().prepare_for_host_action();
    }
    pub(crate) fn commands_enabled(&self) -> bool {
        !self.exiting
            && self.startup.is_none()
            && !self.workspace.close_pending()
            && self.workspace.active().editor.document_commands_enabled()
    }
    pub(crate) fn title(&self) -> String {
        format!("{} — MuFoEd", self.workspace.active().name())
    }
    pub(crate) fn take_open_requests(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.open_requests)
    }
    pub(crate) fn take_exit_cancelled(&mut self) -> bool {
        std::mem::take(&mut self.exit_cancelled)
    }
    pub(crate) fn closing_id(&self) -> Option<TabId> {
        self.workspace.pending_tab()
    }
    pub(crate) fn request_exit(&mut self) {
        self.workspace.request_close(self.workspace.ids(), true);
        if self.workspace.advance_close() {
            self.begin_shutdown();
        }
    }
    pub(crate) fn exit_ready(&self) -> bool {
        self.exiting
            && self.workspace.ready_to_close()
            && self
                .startup
                .as_ref()
                .is_none_or(|p| p.recovery.ready_to_close())
    }
    fn begin_shutdown(&mut self) {
        self.workspace.synchronize_settings();
        let settings = self.workspace.settings.clone();
        if let Err(error) = self
            .preferences
            .poll(settings)
            .and_then(|()| self.preferences.flush())
        {
            self.editor_mut().set_status(format!("Настройки: {error}"));
        }
        self.workspace.begin_shutdown();
        if let Some(prompt) = &mut self.startup {
            prompt.recovery.close();
            prompt.editor.cancel_background();
        }
        self.exiting = true;
        // Only MuFoEd shuts down the process-wide runtime, after every document agrees.
    }
    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        self.workspace.poll(ctx);
        self.listed.retain(|&id| self.workspace.tab(id).is_some());
        if self.exiting {
            if let Some(prompt) = &mut self.startup {
                prompt.recovery.poll(&mut prompt.editor);
            }
        }
        if let Err(error) = self.preferences.poll(self.workspace.settings.clone()) {
            self.editor_mut().set_status(error.to_string());
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.exiting {
            ui.centered_and_justified(|ui| {
                ui.label("Завершение фоновых операций…");
            });
            return;
        }
        for (id, save_as) in std::mem::take(&mut self.pending_saves) {
            if let Err(error) = self.save_tab(id, save_as, frame) {
                self.editor_mut().set_status(error.to_string());
            }
        }
        if self.startup.is_some() {
            self.poll_startup(&ctx);
        } else if !self.workspace.close_pending() {
            let id = self.workspace.active().id;
            let actions = ui
                .push_id(("image-studio", id), |ui| self.editor_mut().show(ui))
                .inner;
            for action in actions {
                if let Err(error) = self.file_action(action, frame) {
                    self.editor_mut().set_status(error.to_string());
                }
            }
        }
        if self.workspace.close_pending() {
            self.close_dialog(&ctx, frame);
        }
    }
    fn poll_startup(&mut self, ctx: &egui::Context) {
        let Some(mut prompt) = self.startup.take() else {
            return;
        };
        prompt.recovery.poll(&mut prompt.editor);
        let epoch = prompt.editor.project_epoch();
        if prompt.recovery.prompt_pending() {
            prompt.recovery.show_prompt(ctx, &mut prompt.editor);
        }
        if prompt.editor.project_epoch() != epoch {
            let editor = self.workspace.fresh_editor();
            let mut recovery = recovery_host::RecoveryHost::new_tab(
                &self.recovery_directory,
                editor.project_epoch(),
                None,
            );
            prompt.recovery.transfer_pending_to(&mut recovery);
            self.workspace
                .insert_recovered(prompt.editor, prompt.recovery);
            self.register(self.workspace.active().id);
            if recovery.prompt_pending() {
                self.startup = Some(RecoveryPrompt { editor, recovery });
            }
        } else if prompt.recovery.prompt_pending() {
            self.startup = Some(prompt);
        }
    }
    fn save_tab(
        &mut self,
        id: TabId,
        save_as: bool,
        frame: &eframe::Frame,
    ) -> Result<bool, EditError> {
        let Some(tab) = self.workspace.tab(id) else {
            return Ok(false);
        };
        let existing = (!save_as)
            .then(|| tab.project().map(Path::to_path_buf))
            .flatten();
        let path = if let Some(path) = existing {
            Some(path)
        } else {
            let name = PathBuf::from(tab.name()).with_extension("psd");
            let mut dialog = rfd::FileDialog::new()
                .set_parent(frame)
                .set_title(format!("Сохранить {}", tab.name()))
                .set_file_name(name.to_string_lossy())
                .add_filter("Photoshop PSD", &["psd"]);
            if let Some(parent) = tab.source().and_then(Path::parent) {
                dialog = dialog.set_directory(parent);
            }
            dialog.save_file().map(|p| {
                if p.extension().is_none() {
                    p.with_extension("psd")
                } else {
                    p
                }
            })
        };
        let Some(path) = path else {
            return Ok(false);
        };
        self.workspace.save_to(id, &path)?;
        Ok(true)
    }
    fn save_all(&mut self, frame: &eframe::Frame) -> Result<(), EditError> {
        for id in self.workspace.ids() {
            if self
                .workspace
                .tab(id)
                .is_some_and(|t| t.editor.has_unsaved_changes())
                && !self.save_tab(id, false, frame)?
            {
                break;
            }
        }
        Ok(())
    }
    fn close_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.workspace.advance_close() {
            self.begin_shutdown();
            return;
        }
        let Some(id) = self.workspace.pending_tab() else {
            return;
        };
        let Some(tab) = self.workspace.tab(id) else {
            return;
        };
        let mut choice = 0;
        let response = egui::Modal::new(egui::Id::new("close-document-prompt")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading(format!("Закрыть «{}»?", tab.name()));
            if tab.editor.has_unsaved_changes() {
                ui.label("Сохранить изменения перед закрытием?");
            }
            if tab.editor.background_active() {
                ui.label("Фоновые операции этой вкладки будут отменены.");
            }
            if tab.editor.has_staged_result() {
                ui.label("Есть неприменённый результат обработки. Нажмите «Отмена», чтобы вернуться и принять его; при закрытии он будет потерян.");
            }
            if !self.close_error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &self.close_error);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(!tab.editor.has_staged_result(),egui::Button::new("Сохранить")).clicked() {
                    choice = 1;
                }
                if ui.button("Не сохранять").clicked() {
                    choice = 2;
                }
                if ui.button("Отмена").clicked() {
                    choice = 3;
                }
            });
        });
        if choice == 0 && response.should_close() {
            choice = 3;
        }
        match choice {
            1 => match self.save_tab(id, false, frame) {
                Ok(true) => {
                    self.workspace.close_confirmed(id);
                    self.close_error.clear();
                }
                Ok(false) => {}
                Err(error) => self.close_error = error.to_string(),
            },
            2 => {
                self.workspace.close_confirmed(id);
                self.close_error.clear();
            }
            3 => {
                self.workspace.cancel_close();
                self.exit_cancelled = true;
                self.close_error.clear();
            }
            _ => {}
        }
    }
    fn preset_file_dialog(&mut self, import: bool, frame: &eframe::Frame) -> Result<(), EditError> {
        let dialog = rfd::FileDialog::new()
            .set_parent(frame)
            .add_filter("Generation presets", &["json"]);
        let path = if import {
            dialog.pick_file()
        } else {
            dialog
                .set_file_name("image-studio-presets.json")
                .save_file()
        };
        if let Some(path) = path {
            if import {
                let count = self.editor_mut().import_presets(path)?;
                self.editor_mut()
                    .set_status(format!("Импортировано пресетов: {count}"));
            } else {
                self.editor_mut().export_presets(path)?;
                self.editor_mut().set_status("Пресеты экспортированы");
            }
        }
        Ok(())
    }
    fn file_action(&mut self, action: FileAction, frame: &eframe::Frame) -> Result<(), EditError> {
        match action {
            FileAction::ImportPresets | FileAction::ExportPresets => {
                self.preset_file_dialog(matches!(action, FileAction::ImportPresets), frame)?;
            }
            FileAction::UltraSharpModel => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_directory("D:/Models")
                    .add_filter("Upscaler weights", &["safetensors", "pth", "pt"])
                    .pick_file()
                {
                    self.editor_mut().set_ultrasharp_model(&path);
                }
            }
            FileAction::AuraSrModel => {
                if let Some(path) = rfd::FileDialog::new().set_parent(frame).pick_folder() {
                    self.editor_mut().set_aurasr_model(&path);
                }
            }
            FileAction::SegmentationModel => {
                if let Some(path) = rfd::FileDialog::new().set_parent(frame).pick_folder() {
                    self.editor_mut().set_segmentation_model(&path)?;
                }
            }
            FileAction::BackgroundModel => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter("BiRefNet ONNX", &["onnx"])
                    .pick_file()
                {
                    self.editor_mut().set_background_model(&path);
                }
            }
            FileAction::AddAsset => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_directory("D:/Models")
                    .add_filter("Safetensors", &["safetensors"])
                    .pick_file()
                {
                    self.editor_mut().add_asset(&path)?;
                }
            }
            FileAction::OpenImage | FileAction::OpenProject => {
                if let Some(paths) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter("MuFoEd", &open_extensions())
                    .add_filter("Images / PSD", IMAGE_EXTENSIONS)
                    .pick_files()
                {
                    self.open_requests.extend(paths);
                }
            }
            FileAction::ImportLayer => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter(
                        "Растровое изображение",
                        &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"],
                    )
                    .pick_file()
                {
                    self.editor_mut().import_layer_file(&path)?;
                }
            }
            FileAction::NewDocument => {
                self.new_document();
            }
            FileAction::CloseDocument => {
                self.close_tab(self.workspace.active().id);
            }
            FileAction::SaveProject | FileAction::SaveProjectAs => {
                self.save_tab(
                    self.workspace.active().id,
                    action == FileAction::SaveProjectAs,
                    frame,
                )?;
            }
            FileAction::SaveAll => self.save_all(frame)?,
            FileAction::ExportPng => {
                let name = self.workspace.active().name();
                let name = std::path::Path::new(&name).with_extension("png");
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_file_name(name.to_string_lossy())
                    .add_filter("PNG", &["png"])
                    .save_file()
                {
                    self.editor_mut().prepare_for_host_action();
                    self.editor_mut().document().flatten().save_png(path)?;
                    self.editor_mut()
                        .set_status("PNG экспортирован; PSD-проект сохраняется отдельно");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_paths_route_images_without_stealing_text_documents() {
        for name in ["a.PNG", "b.psd", "photo.JPEG", "legacy.json", "x.tiff"] {
            assert!(supports(Path::new(name)));
        }
        for name in ["a.docx", "b.tex", "c.pdf", "a.txt", "no-extension"] {
            assert!(!supports(Path::new(name)));
        }
    }
    #[test]
    fn every_advertised_extension_has_an_existing_open_handler() {
        for extension in open_extensions() {
            let name = format!("document.{extension}");
            let path = Path::new(&name);
            assert!(
                supports(path) || crate::formats::kind_by_extension(path).is_some(),
                "{extension}"
            );
        }
    }
    #[test]
    fn close_cancellation_keeps_the_shared_runtime_available() {
        let dir = tempfile::tempdir().unwrap();
        let mut studio = ImageStudio::with_preferences(dir.path().join("settings.json"));
        studio
            .workspace
            .active_mut()
            .editor
            .restore_recovered(image_editor::Document::new(16, 16).unwrap());
        studio.request_exit();
        assert!(studio.workspace.close_pending());
        assert!(!studio.exit_ready());
        studio.workspace.cancel_close();
        assert!(studio.commands_enabled());
        assert!(!studio.exiting);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn image_save_reopen_and_text_route_share_one_host_contract() {
        let dir = tempfile::tempdir().unwrap();
        let mut studio = ImageStudio::with_preferences(dir.path().join("settings.json"));
        let input = dir.path().join("photo.png");
        image_editor::Raster::solid(8, 6, [10, 20, 30, 255])
            .unwrap()
            .save_png(&input)
            .unwrap();
        studio.open(&input).unwrap();
        let id = studio.workspace.active().id;
        let psd = dir.path().join("photo.psd");
        studio.workspace.save_to(id, &psd).unwrap();
        studio.open(&psd).unwrap();
        assert_eq!(studio.workspace.tabs.len(), 1);
        assert_eq!(
            studio
                .workspace
                .active()
                .editor
                .document()
                .flatten()
                .pixel(0, 0),
            Some([10, 20, 30, 255])
        );
    }
    #[test]
    fn clean_exit_waits_for_local_recovery_without_shutting_down_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let mut studio = ImageStudio::with_preferences(dir.path().join("settings.json"));
        studio.request_exit();
        let ctx = egui::Context::default();
        for _ in 0..100 {
            studio.poll(&ctx);
            if studio.exit_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(studio.exit_ready());
        studio.resume_after_shutdown_error("test");
        assert!(!studio.exit_ready());
        assert!(studio.commands_enabled());
    }
}

#[cfg(test)]
mod shared_tab_tests {
    use super::*;
    #[test]
    fn hidden_placeholder_is_not_a_user_tab_and_last_close_adds_no_phantom() {
        let dir = tempfile::tempdir().unwrap();
        let mut studio = ImageStudio::with_preferences(dir.path().join("settings.json"));
        assert!(studio.tabs().is_empty());
        let first = studio.new_document();
        let second = studio.new_document();
        assert_eq!(
            studio.tabs().iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![first, second]
        );
        assert!(!studio.close_tab(first));
        assert_eq!(studio.tabs().len(), 1);
        assert!(!studio.close_tab(second));
        assert!(studio.tabs().is_empty());
        let third = studio.new_document();
        assert_ne!(third, second);
        assert_eq!(studio.tabs().len(), 1);
    }
    #[test]
    fn duplicate_image_focus_and_dirty_tab_close_keep_other_pixels_and_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut studio = ImageStudio::with_preferences(dir.path().join("settings.json"));
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        image_editor::Raster::solid(8, 6, [10, 20, 30, 255])
            .unwrap()
            .save_png(&a)
            .unwrap();
        image_editor::Raster::solid(4, 3, [80, 90, 100, 255])
            .unwrap()
            .save_png(&b)
            .unwrap();
        let first = studio.open(&a).unwrap();
        let second = studio.open(&b).unwrap();
        assert_eq!(studio.open(&a).unwrap(), first);
        assert_eq!(studio.tabs().len(), 2);
        studio
            .editor_mut()
            .restore_recovered(image_editor::Document::new(12, 12).unwrap());
        assert!(studio.close_tab(first));
        assert!(studio.closing_id() == Some(first));
        studio.workspace.cancel_close();
        studio.select(second);
        assert_eq!(
            studio
                .workspace
                .active()
                .editor
                .document()
                .flatten()
                .pixel(0, 0),
            Some([80, 90, 100, 255])
        );
        let psd = dir.path().join("renamed.psd");
        studio.workspace.save_to(second, &psd).unwrap();
        let names = studio.tabs();
        assert_eq!(
            names.iter().find(|t| t.id == second).unwrap().name,
            "renamed.psd"
        );
        assert!(names.iter().find(|t| t.id == first).unwrap().modified);
    }
}
