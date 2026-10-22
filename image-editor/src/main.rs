//! Standalone tabbed host for the embeddable image editor.
mod document_tabs;
mod preferences;
mod recovery_host;
use document_tabs::{TabAction, TabId, Workspace};
use image_editor::{
    ui::{FileAction, ImageEditor},
    EditError, Raster,
};
use std::path::{Path, PathBuf};

fn main() -> eframe::Result {
    let preferences_path = preferences::path();
    let recovery_directory = recovery_host::path(&preferences_path);
    let loaded_settings = preferences::load(&preferences_path);
    if let Ok(Some(settings)) = &loaded_settings {
        if let Some(cache) = settings.cache_dir() {
            std::env::set_var("IMAGE_STUDIO_CACHE", cache);
            std::env::set_var("PLAKAT_CACHE_DIR", cache);
        }
    }
    #[cfg(windows)]
    std::env::set_var("PLAKAT_NO_SDPA", "1");
    if std::env::var_os("PLAKAT_CACHE_DIR").is_none() {
        if let Some(cache) = std::env::var_os("IMAGE_STUDIO_CACHE") {
            std::env::set_var("PLAKAT_CACHE_DIR", cache);
        }
    }
    let shot_size = std::env::var_os("IMAGE_STUDIO_SHOT")
        .and_then(|_| {
            std::env::var("IMAGE_STUDIO_SHOT_SIZE")
                .ok()?
                .split_once('x')
                .and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))
        })
        .filter(|s| {
            s.iter()
                .all(|v| v.is_finite() && (700.0..=3840.0).contains(v))
        })
        .unwrap_or([1440.0, 900.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(shot_size)
            .with_min_inner_size([1100.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Image Studio",
        options,
        Box::new(move |cc| {
            image_editor::ui::apply_studio_style(&cc.egui_ctx);
            let mut editor = ImageEditor::default();
            match loaded_settings {
                Ok(Some(settings)) => {
                    if let Err(error) = editor.apply_settings(settings) {
                        editor.set_status(format!("Настройки: {error}"));
                    }
                }
                Err(error) => editor.set_status(format!("Не удалось прочитать настройки: {error}")),
                Ok(None) => {}
            }
            let preferences = preferences::Preferences::new(preferences_path, editor.settings());
            let mut workspace = Workspace::new(recovery_directory.clone(), editor);
            for path in std::env::args_os().skip(1) {
                if let Err(error) = workspace.open(Path::new(&path)) {
                    workspace.active_mut().editor.set_status(error.to_string());
                }
            }
            open_shot_dialog(&mut workspace.active_mut().editor);
            if std::env::var("IMAGE_STUDIO_RECOVERY_PROBE").as_deref() == Ok("draft") {
                let tab = workspace.active_mut();
                tab.editor.restore_recovered(tab.editor.document().clone());
                let source = tab.recovery.source().map(Path::to_path_buf);
                tab.recovery.bind_source(tab.editor.project_epoch(), source);
            }
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
            Ok(Box::new(Demo {
                preferences,
                workspace,
                startup,
                recovery_directory,
                frames: 0,
                shot: std::env::var_os("IMAGE_STUDIO_SHOT").map(Into::into),
                shot_started: std::time::Instant::now(),
                dropped: Vec::new(),
                close_error: String::new(),
                releasing: None,
                close_ready: false,
                models_released: false,
            }))
        }),
    )
}

fn open_shot_dialog(editor: &mut ImageEditor) {
    if std::env::var_os("IMAGE_STUDIO_SHOT").is_some() {
        if std::env::var("IMAGE_STUDIO_SHOT_TARGET").as_deref() == Ok("mask") {
            editor.set_edit_target(image_editor::ui::EditTarget::Mask);
        }
        match std::env::var("IMAGE_STUDIO_SHOT_DIALOG").as_deref() {
            Ok("color") => editor.open_color_dialog(),
            Ok("layer-mask") => {
                if let Err(e) = editor
                    .set_mask_target(image_editor::MaskTarget::Layer(editor.document().active()))
                {
                    editor.set_status(e.to_string());
                }
            }
            Ok("levels") => editor.open_levels_dialog(),
            Ok("curves") => editor.open_curves_dialog(),
            Ok("layer") => editor.open_layer_transform_dialog(),
            Ok("hf") => {
                if let Ok(repo) = std::env::var("IMAGE_STUDIO_SHOT_HF_REPO") {
                    editor.open_huggingface_repository(
                        &repo,
                        "main",
                        image_editor::assets::Kind::Model,
                    );
                } else {
                    editor.open_huggingface_dialog(image_editor::assets::Kind::Model);
                }
            }
            Ok("presets") => editor.open_presets(),
            Ok("settings") => editor.open_settings(),
            Ok("canvas") => editor.open_canvas_dialog(),
            Ok("rotate") => editor.open_rotation_dialog(),
            Ok("memory") => editor.open_memory_panel(),
            Ok("retouch") => editor.select_manual_tool(image_editor::ui::ManualTool::Heal),
            Ok("wand") => {
                if let Some(point) =
                    std::env::var("IMAGE_STUDIO_SHOT_WAND_POINT")
                        .ok()
                        .and_then(|v| {
                            v.split_once(',').and_then(|(x, y)| {
                                Some([x.parse::<u32>().ok()?, y.parse::<u32>().ok()?])
                            })
                        })
                {
                    let mut doc = editor.document().clone();
                    let sample = doc.flatten();
                    match doc.select_by_color(
                        &sample,
                        point,
                        32,
                        true,
                        image_editor::SelectionOp::Replace,
                    ) {
                        Ok(()) => editor.replace(doc),
                        Err(e) => editor.set_status(e.to_string()),
                    }
                }
                editor.select_manual_tool(image_editor::ui::ManualTool::Wand);
            }
            Ok("ribbon-transform") => {
                editor.start_canvas_transform();
            }
            Ok("edit") => editor.select_tab(image_editor::ui::StudioTab::Edit),
            Ok("upscale") => editor.select_tab(image_editor::ui::StudioTab::Upscale),
            Ok("img2img") => {
                editor.set_generation_mode(image_editor::generation::Mode::ImageToImage);
            }
            Ok("model") => editor.open_resource_dialog(image_editor::assets::Kind::Model),
            Ok("lora") => editor.open_resource_dialog(image_editor::assets::Kind::Lora),
            Ok("civitai") => editor.open_civitai_dialog(image_editor::assets::Kind::Model),
            _ => {}
        }
    }
}

struct RecoveryPrompt {
    editor: ImageEditor,
    recovery: recovery_host::RecoveryHost,
}
struct Demo {
    preferences: preferences::Preferences,
    workspace: Workspace,
    startup: Option<RecoveryPrompt>,
    recovery_directory: PathBuf,
    frames: u32,
    shot: Option<PathBuf>,
    shot_started: std::time::Instant,
    dropped: Vec<PathBuf>,
    close_error: String,
    releasing: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    close_ready: bool,
    models_released: bool,
}
impl Demo {
    fn editor_mut(&mut self) -> &mut ImageEditor {
        &mut self.workspace.active_mut().editor
    }
    fn begin_shutdown(&mut self) {
        self.workspace.synchronize_settings();
        let settings = self.workspace.settings.clone();
        if let Err(error) = self
            .preferences
            .poll(settings)
            .and_then(|()| self.preferences.flush())
        {
            self.editor_mut()
                .set_status(format!("Не удалось сохранить настройки: {error}"));
        }
        self.workspace.begin_shutdown();
        if let Some(prompt) = &mut self.startup {
            prompt.recovery.close();
            prompt.editor.cancel_background();
        }
        self.releasing = Some(mufoed_model_runtime::Runtime::global().shutdown_async());
    }
    fn poll_shutdown(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.releasing {
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    self.models_released = true;
                    self.releasing = None;
                }
                Ok(Err(error)) => {
                    self.editor_mut()
                        .set_status(format!("Завершение моделей: {error}"));
                    self.releasing = None;
                    self.workspace.cancel_shutdown();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.editor_mut()
                        .set_status("Поток завершения моделей остановился");
                    self.releasing = None;
                    self.workspace.cancel_shutdown();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if self.models_released
            && self.workspace.ready_to_close()
            && self
                .startup
                .as_ref()
                .is_none_or(|p| p.recovery.ready_to_close())
        {
            self.close_ready = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
            if recovery.prompt_pending() {
                self.startup = Some(RecoveryPrompt { editor, recovery });
            }
        } else if prompt.recovery.prompt_pending() {
            self.startup = Some(prompt);
        }
    }
    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        let mut errors = Vec::new();
        for path in paths {
            if let Err(error) = self.workspace.open(&path) {
                errors.push(format!("{}: {error}", path.display()));
            }
        }
        if !errors.is_empty() {
            self.editor_mut().set_status(errors.join("; "));
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
    fn tab_action(&mut self, action: TabAction, frame: &eframe::Frame) -> Result<(), EditError> {
        match action {
            TabAction::Select(id) => self.workspace.select(id),
            TabAction::New => self.workspace.new_document(),
            TabAction::Close(id) => self.workspace.request_close(vec![id], false),
            TabAction::CloseOthers(id) => {
                let ids = self
                    .workspace
                    .ids()
                    .into_iter()
                    .filter(|&i| i != id)
                    .collect();
                self.workspace.request_close(ids, false);
            }
            TabAction::CloseAll => self.workspace.request_close(self.workspace.ids(), false),
            TabAction::Save(id) | TabAction::SaveAs(id) => {
                self.save_tab(id, matches!(action, TabAction::SaveAs(_)), frame)?;
            }
            TabAction::SaveAll => self.save_all(frame)?,
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
                    .add_filter(
                        "Изображения и проекты",
                        &[
                            "psd", "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "json",
                        ],
                    )
                    .pick_files()
                {
                    self.open_paths(paths);
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
            FileAction::NewDocument => self.workspace.new_document(),
            FileAction::CloseDocument => self
                .workspace
                .request_close(vec![self.workspace.active().id], false),
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
    fn screenshot(&mut self, ui: &mut egui::Ui) {
        if let Some(path) = self.shot.clone() {
            self.frames += 1;
            if self.frames >= 12
                && self.shot_started.elapsed() >= std::time::Duration::from_millis(350)
                && !self.workspace.active().editor.resource_operation_active()
                && self
                    .startup
                    .as_ref()
                    .is_none_or(|p| p.recovery.screenshot_ready())
            {
                ui.ctx()
                    .send_viewport_cmd(
                        egui::ViewportCommand::Screenshot(egui::UserData::default()),
                    );
            }
            let shot = ui.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = shot {
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(egui::Color32::to_array)
                    .collect();
                let result = u32::try_from(image.size[0])
                    .ok()
                    .zip(u32::try_from(image.size[1]).ok())
                    .ok_or_else(|| EditError::Invalid("Screenshot dimensions overflow".into()))
                    .and_then(|(w, h)| Raster::new(w, h, bytes))
                    .and_then(|raster| raster.save_png(&path));
                if let Err(e) = result {
                    self.editor_mut().set_status(e.to_string());
                }
                self.shot = None;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.workspace.poll(&ctx);
        if self.releasing.is_some() || self.models_released {
            if let Some(prompt) = &mut self.startup {
                prompt.recovery.poll(&mut prompt.editor);
            }
            ui.centered_and_justified(|ui| {
                ui.label("Завершение фоновых операций…");
            });
        } else {
            self.dropped.extend(
                ui.input_mut(|i| std::mem::take(&mut i.raw.dropped_files))
                    .into_iter()
                    .filter_map(|f| f.path),
            );
            let enabled = self.startup.is_none()
                && !self.workspace.close_pending()
                && self.workspace.active().editor.document_commands_enabled();
            if let Some(action) = self.workspace.tab_bar(ui, enabled) {
                if let Err(error) = self.tab_action(action, frame) {
                    self.editor_mut().set_status(error.to_string());
                }
                ui.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
            if enabled {
                self.workspace.keyboard_navigation(ui);
            }
            if self.startup.is_some() {
                self.poll_startup(&ctx);
            } else if !self.workspace.close_pending() {
                if enabled && !self.dropped.is_empty() {
                    let paths = std::mem::take(&mut self.dropped);
                    self.open_paths(paths);
                }
                let id = self.workspace.active().id;
                let actions = ui.push_id(id, |ui| self.editor_mut().show(ui)).inner;
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
        self.workspace.synchronize_settings();
        if let Err(error) = self.preferences.poll(self.workspace.settings.clone()) {
            self.editor_mut().set_status(format!("Настройки: {error}"));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — Image Studio",
            if self.workspace.active().editor.has_unsaved_changes() {
                "● "
            } else {
                ""
            },
            self.workspace.active().name()
        )));
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        if ui.input(|i| i.viewport().close_requested()) && !self.close_ready {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.releasing.is_none() && !self.models_released {
                self.workspace.request_close(self.workspace.ids(), true);
                if self.workspace.advance_close() {
                    self.begin_shutdown();
                }
            }
        }
        self.poll_shutdown(&ctx);
        self.screenshot(ui);
    }
}
