//! Standalone review host for the image-editor crate.
mod preferences;
mod recovery_host;
use image_editor::{
    ui::{FileAction, ImageEditor},
    Document, EditError, Raster,
};

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
    // Configure Plakat once, before eframe or any inference worker starts threads.
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
                    if let Err(e) = editor.apply_settings(settings) {
                        editor.set_status(format!("Настройки: {e}"));
                    }
                }
                Err(e) => editor.set_status(format!("Не удалось прочитать настройки: {e}")),
                Ok(None) => {}
            }

            let mut source = None;
            if let Some(path) = std::env::args_os().nth(1) {
                let path = std::path::Path::new(&path);
                let result = if path.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("json") || e.eq_ignore_ascii_case("psd")
                }) {
                    Document::open_project(path)
                } else {
                    Raster::open(path).and_then(Document::from_raster)
                };
                match result {
                    Ok(doc) => {
                        editor.replace(doc);
                        source = Some(path.to_path_buf());
                    }
                    Err(e) => editor.set_status(e.to_string()),
                }
            }
            open_shot_dialog(&mut editor);
            let preferences = preferences::Preferences::new(preferences_path, editor.settings());
            let mut recovery = recovery_host::RecoveryHost::new(
                &recovery_directory,
                editor.project_epoch(),
                source,
            );
            // Isolated crash QA only: makes the command-line project dirty without altering pixels.
            if std::env::var("IMAGE_STUDIO_RECOVERY_PROBE").as_deref() == Ok("draft") {
                editor.restore_recovered(editor.document().clone());
                let source = recovery.source().map(std::path::Path::to_path_buf);
                recovery.bind_source(editor.project_epoch(), source);
            }
            Ok(Box::new(Demo {
                preferences,
                recovery,
                editor,
                frames: 0,
                shot: std::env::var_os("IMAGE_STUDIO_SHOT").map(Into::into),
                pending_open: None,
                confirm_close: false,
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
            Ok("wand") => editor.select_manual_tool(image_editor::ui::ManualTool::Wand),
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

struct Demo {
    recovery: recovery_host::RecoveryHost,
    models_released: bool,
    preferences: preferences::Preferences,
    editor: ImageEditor,
    frames: u32,
    shot: Option<std::path::PathBuf>,
    pending_open: Option<FileAction>,
    confirm_close: bool,
    releasing: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    close_ready: bool,
}
impl Demo {
    fn begin_shutdown(&mut self) {
        self.recovery.close();
        if let Err(e) = self
            .preferences
            .poll(self.editor.settings())
            .and_then(|()| self.preferences.flush())
        {
            self.editor
                .set_status(format!("Не удалось сохранить настройки: {e}"));
        }
        drop(self.editor.shutdown_models());
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
                    self.editor.set_status(format!("Model shutdown: {error}"));
                    self.releasing = None;
                    self.recovery.cancel_close();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.editor.set_status("Model shutdown worker stopped");
                    self.releasing = None;
                    self.recovery.cancel_close();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.editor
                        .set_status("Завершение вычислений и освобождение моделей…");
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if self.models_released && self.recovery.ready_to_close() {
            self.close_ready = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
                let count = self.editor.import_presets(path)?;
                self.editor
                    .set_status(format!("Импортировано пресетов: {count}"));
            } else {
                self.editor.export_presets(path)?;
                self.editor.set_status("Пресеты экспортированы");
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
                    self.editor.set_ultrasharp_model(&path);
                }
            }
            FileAction::AuraSrModel => {
                if let Some(path) = rfd::FileDialog::new().set_parent(frame).pick_folder() {
                    self.editor.set_aurasr_model(&path);
                }
            }
            FileAction::SegmentationModel => {
                if let Some(path) = rfd::FileDialog::new().set_parent(frame).pick_folder() {
                    self.editor.set_segmentation_model(&path)?;
                }
            }
            FileAction::BackgroundModel => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter("BiRefNet ONNX", &["onnx"])
                    .pick_file()
                {
                    self.editor.set_background_model(&path);
                }
            }
            FileAction::AddAsset => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_directory("D:/Models")
                    .add_filter("Safetensors", &["safetensors"])
                    .pick_file()
                {
                    self.editor.add_asset(&path)?;
                }
            }
            FileAction::OpenImage | FileAction::ImportLayer => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter(
                        "Raster image",
                        &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"],
                    )
                    .pick_file()
                {
                    if matches!(action, FileAction::ImportLayer) {
                        self.editor.import_layer_file(&path)?;
                    } else {
                        self.editor
                            .replace(Document::from_raster(Raster::open(&path)?)?);
                        self.recovery
                            .bind_source(self.editor.project_epoch(), Some(path));
                    }
                }
            }
            FileAction::OpenProject => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter("Photoshop PSD / legacy project", &["psd", "json"])
                    .pick_file()
                {
                    self.editor.replace(Document::open_project(&path)?);
                    self.recovery
                        .bind_source(self.editor.project_epoch(), Some(path));
                }
            }
            FileAction::SaveProject => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_file_name("image-project.psd")
                    .add_filter("Photoshop PSD", &["psd"])
                    .save_file()
                {
                    self.editor.document().save_project(&path)?;
                    self.editor.mark_saved();
                    self.recovery
                        .bind_source(self.editor.project_epoch(), Some(path));
                    self.editor.set_status("PSD-проект сохранён");
                }
            }
            FileAction::ExportPng => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_file_name("image.png")
                    .add_filter("PNG", &["png"])
                    .save_file()
                {
                    self.editor.document().flatten().save_png(path)?;
                    self.editor.set_status("PNG exported");
                }
            }
        }
        Ok(())
    }
}
impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.recovery.poll(&mut self.editor);
        if self.releasing.is_some() || self.models_released {
            // Once close is accepted, no new edit can race the final recovery cleanup.
            ui.centered_and_justified(|ui| {
                ui.label("Завершение фоновых операций…");
            });
        } else if self.recovery.prompt_pending() {
            self.recovery.show_prompt(ui.ctx(), &mut self.editor);
        } else {
            for action in self.editor.show(ui) {
                if matches!(action, FileAction::OpenImage | FileAction::OpenProject)
                    && self.editor.has_unsaved_changes()
                {
                    self.pending_open = Some(action);
                    continue;
                }
                if let Err(e) = self.file_action(action, frame) {
                    self.editor.set_status(e.to_string());
                }
            }
        }
        if let Err(e) = self.preferences.poll(self.editor.settings()) {
            self.editor
                .set_status(format!("Не удалось сохранить настройки: {e}"));
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
        if ui.input(|i| i.viewport().close_requested()) && !self.close_ready {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.editor.has_unsaved_changes() {
                self.confirm_close = true;
            } else if self.releasing.is_none() {
                self.begin_shutdown();
            }
        }
        self.poll_shutdown(ui.ctx());
        if self.pending_open.is_some() || self.confirm_close {
            let mut discard = false;
            let mut cancel = false;
            egui::Window::new("Unsaved project").anchor(egui::Align2::CENTER_CENTER,egui::Vec2::ZERO).collapsible(false).show(ui.ctx(), |ui| {
                ui.label("This project contains unsaved changes. Save it before continuing, or discard the changes.");
                discard = ui.button("Discard changes and continue").clicked(); cancel = ui.button("Cancel").clicked();
            });
            if cancel {
                self.pending_open = None;
                self.confirm_close = false;
            }
            if discard {
                if self.confirm_close {
                    self.editor.mark_saved();
                    self.confirm_close = false;
                    self.begin_shutdown();
                } else if let Some(action) = self.pending_open.take() {
                    if let Err(e) = self.file_action(action, frame) {
                        self.editor.set_status(e.to_string());
                    }
                }
            }
        }
        if let Some(path) = &self.shot {
            self.frames += 1;
            if self.frames >= 12
                && !self.editor.resource_operation_active()
                && self.recovery.screenshot_ready()
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
                    .and_then(|raster| raster.save_png(path));
                if let Err(e) = result {
                    self.editor.set_status(e.to_string());
                }
                self.shot = None;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
    }
}
