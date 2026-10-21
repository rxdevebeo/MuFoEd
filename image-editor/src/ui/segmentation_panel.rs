use super::{Color32, Document, EditError, FileAction, ImageEditor, Mode, Raster, TextureHandle};
use crate::model_tools::ModelJob;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) struct SegmentationUi {
    pub(super) model: String,
    job: Option<ModelJob>,
    source: Option<Raster>,
    candidate: Option<(Raster, Raster)>,
    texture: Option<TextureHandle>,
    points: Vec<[f32; 3]>,
    point_mode: Option<bool>,
    pub(super) auto_device: bool,
}
impl Default for SegmentationUi {
    fn default() -> Self {
        let root = std::env::var_os("IMAGE_STUDIO_CACHE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_owned))
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".runtime"));
        Self {
            model: root.join("SAM2/sam2.1_hiera_large").display().to_string(),
            job: None,
            source: None,
            candidate: None,
            texture: None,
            points: Vec::new(),
            point_mode: None,
            auto_device: true,
        }
    }
}
impl SegmentationUi {
    pub(super) fn reset_project(&mut self) {
        self.job = None;
        self.source = None;
        self.candidate = None;
        self.texture = None;
        self.points.clear();
        self.point_mode = None;
    }
}
impl ImageEditor {
    /// Select an existing native Transformers SAM2 model directory.
    pub fn set_segmentation_model(&mut self, path: &Path) -> Result<(), EditError> {
        if !path.join("config.json").is_file() || !path.join("model.safetensors").is_file() {
            return Err(EditError::Invalid(
                "Select a SAM2 folder with config.json and model.safetensors".into(),
            ));
        }
        self.segmentation_ui.model = path.display().to_string();
        Ok(())
    }
    pub(super) fn segmentation_operation_active(&self) -> bool {
        self.segmentation_ui.job.is_some()
    }
    fn segmentation_start(&mut self, download: bool) {
        let model = PathBuf::from(&self.segmentation_ui.model);
        if !download && !self.segmentation_ui.points.iter().any(|p| p[2] > 0.5) {
            self.status = "Place at least one positive point on the object".into();
            return;
        }
        let source = (!download).then(|| self.document.flatten());
        let mut args: Vec<OsString> = vec![
            if download { "download" } else { "segment" }.into(),
            "--model".into(),
            model.as_os_str().to_owned(),
            "--device".into(),
            if self.segmentation_ui.auto_device {
                "auto"
            } else {
                "cpu"
            }
            .into(),
        ];
        args.extend([
            "--points".into(),
            serde_json::to_string(&self.segmentation_ui.points)
                .expect("finite UI points")
                .into(),
        ]);
        match ModelJob::start_with_runtime(
            "sam2_tool.py",
            &args,
            source.as_ref(),
            if download { model.parent() } else { None },
            &self.runtime,
        ) {
            Ok(job) => {
                self.segmentation_ui.job = Some(job);
                self.segmentation_ui.source = source;
                self.segmentation_ui.point_mode = None;
                self.status = if download {
                    "Downloading SAM 2.1 Hiera Large (856 MiB)…"
                } else {
                    "SAM 2.1 is segmenting the selected object…"
                }
                .into();
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    pub(super) fn segmentation_settings(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut Vec<FileAction>,
    ) {
        ui.add_enabled_ui(!self.settings_busy(), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.segmentation_ui.model)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui.button("Load folder…").clicked() {
                    actions.push(FileAction::SegmentationModel);
                }
                if ui
                    .add_enabled(
                        !Path::new(&self.segmentation_ui.model).exists(),
                        egui::Button::new("Download model"),
                    )
                    .clicked()
                {
                    self.segmentation_start(true);
                }
            });
            ui.checkbox(
                &mut self.segmentation_ui.auto_device,
                "Use CUDA if available",
            );
        });
        ui.hyperlink_to(
            "Модель и лицензия · Meta",
            "https://huggingface.co/facebook/sam2.1-hiera-large",
        );
    }
    pub(super) fn segmentation_panel(&mut self, ui: &mut egui::Ui, _actions: &mut Vec<FileAction>) {
        egui::CollapsingHeader::new("Выделение объекта")
            .default_open(true)
            .show(ui, |ui| {
                let busy = self.segmentation_operation_active()
                    || self.model_operation_active()
                    || self.plakat_active()
                    || self.upscale_operation_active();
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.segmentation_ui.point_mode,
                            Some(true),
                            "+ Объект",
                        );
                        ui.selectable_value(
                            &mut self.segmentation_ui.point_mode,
                            Some(false),
                            "− Фон",
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Убрать точку").clicked() {
                            self.segmentation_ui.points.pop();
                        }
                        if ui.button("Сбросить").clicked() {
                            self.segmentation_ui.points.clear();
                        }
                        if ui.button("Кисть маски").clicked() {
                            self.segmentation_ui.point_mode = None;
                        }
                    });
                    ui.small(format!(
                        "Точек: {} · укажите объект на холсте",
                        self.segmentation_ui.points.len()
                    ));
                    if ui
                        .add_enabled(
                            !self.segmentation_ui.points.is_empty()
                                && Path::new(&self.segmentation_ui.model)
                                    .join("model.safetensors")
                                    .is_file()
                                && self.segmentation_ui.candidate.is_none(),
                            egui::Button::new("Выделить объект"),
                        )
                        .clicked()
                    {
                        self.segmentation_start(false);
                    }
                    if ui
                        .add_enabled(
                            self.document.mask().iter().any(|v| *v > 0),
                            egui::Button::new("Вырезать по маске"),
                        )
                        .clicked()
                    {
                        self.edit(cutout_selection);
                    }
                });
                if let Some(job) = &self.segmentation_ui.job {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("SAM 2.1 operation…");
                    });
                    if let Some((n, total)) = job.progress() {
                        ui.label(format!("{} / {} MiB", n / 1_048_576, total / 1_048_576));
                    }
                    if ui.button("Cancel SAM operation").clicked() {
                        self.segmentation_ui.job = None;
                        self.segmentation_ui.source = None;
                        self.status = "SAM operation cancelled".into();
                    }
                }
            });
    }
    pub(super) fn poll_segmentation(&mut self, context: &egui::Context) {
        if let Some(job) = &mut self.segmentation_ui.job {
            match job.poll() {
                Ok(Some(output)) => {
                    self.segmentation_ui.job = None;
                    if let Some(source) = self.segmentation_ui.source.take() {
                        if let Some(mask) = output.raster.filter(|r| {
                            r.width() == source.width() && r.height() == source.height()
                        }) {
                            self.segmentation_ui.candidate = Some((mask, source));
                            self.segmentation_ui.texture = None;
                            self.status = "SAM mask ready; review before applying".into();
                        } else {
                            self.status = "SAM returned no mask at source resolution".into();
                        }
                    } else {
                        self.status = "SAM 2.1 Hiera Large downloaded and SHA256 verified".into();
                    }
                }
                Err(e) => {
                    self.segmentation_ui.job = None;
                    self.segmentation_ui.source = None;
                    self.status = e.to_string();
                }
                Ok(None) => context.request_repaint_after(Duration::from_millis(100)),
            }
        }
    }
    pub(super) fn segmentation_preview(&mut self, context: &egui::Context) {
        let Some((mask, _)) = &self.segmentation_ui.candidate else {
            return;
        };
        if self.segmentation_ui.texture.is_none() {
            self.segmentation_ui.texture = Some(context.load_texture(
                "sam-selection",
                egui::ColorImage::from_rgba_unmultiplied(
                    [mask.width() as usize, mask.height() as usize],
                    mask.pixels(),
                ),
                egui::TextureOptions::LINEAR,
            ));
        }
        let mut accept = false;
        let mut discard = false;
        egui::Window::new("SAM 2.1 · object mask")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.add(
                    egui::Image::new(self.segmentation_ui.texture.as_ref().expect("SAM preview"))
                        .max_size(egui::vec2(650.0, 500.0)),
                );
                ui.small("White = selected object / area to regenerate");
                ui.horizontal(|ui| {
                    accept = ui.button("Use as editing / inpaint mask").clicked();
                    discard = ui.button("Discard").clicked();
                });
            });
        if accept {
            let (mask, source) = self
                .segmentation_ui
                .candidate
                .take()
                .expect("SAM candidate");
            self.edit(|doc| apply_mask(doc, &mask, &source));
            self.request.mode = Mode::Inpaint;
            self.active_tab = super::StudioTab::Edit;
            self.request.mask_invert = false;
            self.select_manual_tool(super::ManualTool::Mask);
            self.segmentation_ui.point_mode = None;
            self.segmentation_ui.texture = None;
        }
        if discard {
            self.segmentation_ui.candidate = None;
            self.segmentation_ui.texture = None;
        }
    }
    pub(super) fn segmentation_point_mode(&self) -> bool {
        self.segmentation_ui.point_mode.is_some()
    }
    pub(super) fn stop_segmentation_points(&mut self) {
        self.segmentation_ui.point_mode = None;
    }
    pub(super) fn clear_segmentation_points(&mut self) {
        self.segmentation_ui.points.clear();
        self.segmentation_ui.point_mode = None;
    }
    #[allow(clippy::cast_precision_loss)] // UI canvas coordinates and zoom are bounded floats.
    pub(super) fn segmentation_canvas(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if self.active_tab != super::StudioTab::Edit {
            return;
        }
        if response.clicked() && !self.segmentation_operation_active() {
            if let (Some(positive), Some(position)) = (
                self.segmentation_ui.point_mode,
                response.interact_pointer_pos(),
            ) {
                let x = (position.x - response.rect.left()) / self.zoom;
                let y = (position.y - response.rect.top()) / self.zoom;
                if x >= 0.0
                    && y >= 0.0
                    && x < self.document.width() as f32
                    && y < self.document.height() as f32
                    && self.segmentation_ui.points.len() < 128
                {
                    self.segmentation_ui
                        .points
                        .push([x, y, if positive { 1.0 } else { 0.0 }]);
                }
            }
        }
        let painter = ui
            .painter()
            .with_clip_rect(response.rect.intersect(ui.clip_rect()));
        for [x, y, label] in &self.segmentation_ui.points {
            let position = response.rect.min + egui::vec2(*x, *y) * self.zoom;
            painter.circle_filled(
                position,
                5.0,
                if *label > 0.5 {
                    Color32::from_rgb(80, 170, 255)
                } else {
                    Color32::from_rgb(240, 90, 90)
                },
            );
            painter.circle_stroke(position, 6.0, egui::Stroke::new(1.5, Color32::WHITE));
        }
    }
}
fn apply_mask(doc: &mut Document, mask: &Raster, source: &Raster) -> Result<(), EditError> {
    if doc.flatten() != *source || [doc.width(), doc.height()] != [mask.width(), mask.height()] {
        return Err(EditError::Invalid(
            "Canvas changed during SAM inference; segment again".into(),
        ));
    }
    doc.set_mask(mask.pixels().chunks_exact(4).map(|p| p[0]).collect())
}
fn cutout_selection(doc: &mut Document) -> Result<(), EditError> {
    let layer = doc.layers()[doc.active()].clone();
    let mut pixels = layer.raster().pixels().to_vec();
    for (pixel, mask) in pixels.chunks_exact_mut(4).zip(doc.mask()) {
        pixel[3] = u8::try_from((u32::from(pixel[3]) * u32::from(*mask) + 127) / 255)
            .expect("bounded alpha");
    }
    let raster = Raster::new(doc.width(), doc.height(), pixels)?;
    doc.set_layer(layer.name().into(), false, layer.opacity());
    doc.add_layer("Selection · cutout".into(), raster)?;
    doc.set_layer("Selection · cutout".into(), true, layer.opacity());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segmentation_mask_preserves_pixels_and_rejects_stale_canvas() {
        let mut doc = Document::new(8, 8).unwrap();
        let source = doc.flatten();
        let mask = Raster::solid(8, 8, [255; 4]).unwrap();
        apply_mask(&mut doc, &mask, &source).unwrap();
        assert_eq!(doc.flatten(), source);
        assert!(doc.mask().iter().all(|v| *v == 255));
        assert!(apply_mask(&mut doc, &mask, &Raster::solid(8, 8, [20; 4]).unwrap()).is_err());
        assert!(doc.set_mask(vec![0; 63]).is_err());
        assert!(doc.mask().iter().all(|v| *v == 255));
    }
    #[test]
    fn selection_cutout_keeps_rgb_and_original_alpha_and_is_undoable() {
        let mut doc =
            Document::from_raster(Raster::solid(8, 8, [20, 30, 40, 80]).unwrap()).unwrap();
        let mut mask = vec![0; 64];
        mask[0] = 255;
        mask[1] = 128;
        doc.set_mask(mask).unwrap();
        let before = doc.clone();
        let mut editor = ImageEditor::new(doc);
        editor.edit(cutout_selection);
        let pixels = editor.document.layers()[1].raster().pixels();
        assert_eq!(&pixels[..4], &[20, 30, 40, 80]);
        assert_eq!(&pixels[4..8], &[20, 30, 40, 40]);
        assert_eq!(&pixels[8..12], &[20, 30, 40, 0]);
        assert!(editor.history.undo(&mut editor.document));
        assert_eq!(editor.document, before);
    }
}
