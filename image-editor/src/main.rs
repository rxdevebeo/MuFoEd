//! Standalone review host for the image-editor crate.
use image_editor::{
    ui::{FileAction, ImageEditor},
    Document, EditError, Raster,
};

fn main() -> eframe::Result {
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
        Box::new(|cc| {
            image_editor::ui::apply_studio_style(&cc.egui_ctx);
            let mut editor = ImageEditor::default();
            if std::env::var_os("IMAGE_STUDIO_SHOT").is_some() {
                match std::env::var("IMAGE_STUDIO_SHOT_DIALOG").as_deref() {
                    Ok("memory") => editor.open_memory_panel(),
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
                    Ok(doc) => editor.replace(doc),
                    Err(e) => editor.set_status(e.to_string()),
                }
            }
            Ok(Box::new(Demo {
                editor,
                frames: 0,
                shot: std::env::var_os("IMAGE_STUDIO_SHOT").map(Into::into),
                pending_open: None,
                confirm_close: false,
                releasing: None,
                close_ready: false,
            }))
        }),
    )
}
struct Demo {
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
        drop(self.editor.shutdown_models());
        self.releasing = Some(mufoed_model_runtime::Runtime::global().shutdown_async());
    }
    fn poll_shutdown(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.releasing {
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    self.close_ready = true;
                    self.releasing = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Ok(Err(error)) => {
                    self.editor.set_status(format!("Model shutdown: {error}"));
                    self.releasing = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.editor.set_status("Model shutdown worker stopped");
                    self.releasing = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.editor
                        .set_status("Завершение вычислений и освобождение моделей…");
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
    }
    fn file_action(&mut self, action: FileAction) -> Result<(), EditError> {
        match action {
            FileAction::UltraSharpModel => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("D:/Models")
                    .add_filter("Upscaler weights", &["safetensors", "pth", "pt"])
                    .pick_file()
                {
                    self.editor.set_ultrasharp_model(&path);
                }
            }
            FileAction::AuraSrModel => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.editor.set_aurasr_model(&path);
                }
            }
            FileAction::SegmentationModel => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.editor.set_segmentation_model(&path)?;
                }
            }
            FileAction::BackgroundModel => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("BiRefNet ONNX", &["onnx"])
                    .pick_file()
                {
                    self.editor.set_background_model(&path);
                }
            }
            FileAction::AddAsset => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("D:/Models")
                    .add_filter("Safetensors", &["safetensors"])
                    .pick_file()
                {
                    self.editor.add_asset(&path)?;
                }
            }
            FileAction::OpenImage => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "Raster image",
                        &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"],
                    )
                    .pick_file()
                {
                    self.editor
                        .replace(Document::from_raster(Raster::open(path)?)?);
                }
            }
            FileAction::OpenProject => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Photoshop PSD / legacy project", &["psd", "json"])
                    .pick_file()
                {
                    self.editor.replace(Document::open_project(path)?);
                }
            }
            FileAction::SaveProject => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("image-project.psd")
                    .add_filter("Photoshop PSD", &["psd"])
                    .save_file()
                {
                    self.editor.document().save_project(path)?;
                    self.editor.mark_saved();
                    self.editor.set_status("PSD-проект сохранён");
                }
            }
            FileAction::ExportPng => {
                if let Some(path) = rfd::FileDialog::new()
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        for action in self.editor.show(ui) {
            if matches!(action, FileAction::OpenImage | FileAction::OpenProject)
                && self.editor.has_unsaved_changes()
            {
                self.pending_open = Some(action);
                continue;
            }
            if let Err(e) = self.file_action(action) {
                self.editor.set_status(e.to_string());
            }
        }
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
            egui::Window::new("Unsaved project").collapsible(false).show(ui.ctx(), |ui| {
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
                    if let Err(e) = self.file_action(action) {
                        self.editor.set_status(e.to_string());
                    }
                }
            }
        }
        if let Some(path) = &self.shot {
            self.frames += 1;
            if self.frames >= 12 && !self.editor.resource_operation_active() {
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
