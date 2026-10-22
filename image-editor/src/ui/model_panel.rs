use super::{Color32, Document, EditError, FileAction, ImageEditor, TextureHandle};
use crate::{model_tools::ModelJob, Layer, Raster};
use std::{ffi::OsString, path::PathBuf, time::Duration};

enum Operation {
    DownloadBackground,
    RemoveBackground,
}
struct Source {
    index: usize,
    layer: Layer,
}
struct Cutout {
    raster: Raster,
    source: Source,
}
pub(super) struct ModelUi {
    root: PathBuf,
    pub(super) variant: String,
    pub(super) model: String,
    pub(super) auto_device: bool,
    job: Option<ModelJob>,
    operation: Option<Operation>,
    source: Option<Source>,
    candidate: Option<Cutout>,
    texture: Option<TextureHandle>,
}
impl Default for ModelUi {
    fn default() -> Self {
        let root = std::env::var_os("IMAGE_STUDIO_CACHE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(std::path::Path::to_owned))
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".runtime"))
            .join("BiRefNet");
        Self {
            model: root.join("tiny.onnx").display().to_string(),
            root,
            variant: "tiny".into(),
            auto_device: false,
            job: None,
            operation: None,
            source: None,
            candidate: None,
            texture: None,
        }
    }
}
impl ModelUi {
    pub(super) fn has_result(&self) -> bool {
        self.candidate.is_some()
    }
    pub(super) fn reset_project(&mut self) {
        self.job = None;
        self.operation = None;
        self.source = None;
        self.candidate = None;
        self.texture = None;
    }
}
impl ImageEditor {
    /// Select existing `BiRefNet` ONNX weights, without downloading or running them.
    pub fn set_background_model(&mut self, path: &std::path::Path) {
        self.model_ui.model = path.display().to_string();
    }
    pub(super) fn model_operation_active(&self) -> bool {
        self.model_ui.job.is_some()
    }
    fn background_start(&mut self, download: bool) {
        let model = PathBuf::from(&self.model_ui.model);
        let mut args: Vec<OsString> = vec![
            if download { "download" } else { "remove" }.into(),
            "--model".into(),
            model.as_os_str().to_owned(),
            "--variant".into(),
            self.model_ui.variant.clone().into(),
        ];
        let source = if download {
            None
        } else {
            Some(Source {
                index: self.document.active(),
                layer: self.document.layers()[self.document.active()].clone(),
            })
        };
        args.extend([
            "--device".into(),
            if self.model_ui.auto_device {
                "auto"
            } else {
                "cpu"
            }
            .into(),
        ]);
        match ModelJob::start_with_runtime(
            "birefnet.py",
            &args,
            source.as_ref().map(|s| s.layer.raster()),
            if download { model.parent() } else { None },
            &self.runtime,
        ) {
            Ok(job) => {
                self.model_ui.job = Some(job);
                self.model_ui.operation = Some(if download {
                    Operation::DownloadBackground
                } else {
                    Operation::RemoveBackground
                });
                self.model_ui.source = source;
                self.status = if download {
                    "Downloading official BiRefNet model…"
                } else {
                    "Removing background from active layer…"
                }
                .into();
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    pub(super) fn background_settings(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        let busy = self.model_operation_active()
            || self.segmentation_operation_active()
            || self.plakat_active()
            || self.upscale_operation_active();
        let previous = self.model_ui.variant.clone();
        ui.add_enabled_ui(!busy, |ui| {
            egui::ComboBox::from_id_salt("birefnet-variant")
                .selected_text(match self.model_ui.variant.as_str() {
                    "general" => "General · 928 MiB",
                    "matting" => "Matting · 928 MiB",
                    _ => "General Tiny · 214 MiB",
                })
                .show_ui(ui, |ui| {
                    for (id, label) in [
                        ("tiny", "General Tiny · 214 MiB"),
                        ("general", "General · 928 MiB"),
                        ("matting", "Matting · 928 MiB"),
                    ] {
                        ui.selectable_value(&mut self.model_ui.variant, id.into(), label);
                    }
                });
            if previous != self.model_ui.variant {
                self.model_ui.model = self
                    .model_ui
                    .root
                    .join(format!("{}.onnx", self.model_ui.variant))
                    .display()
                    .to_string();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.model_ui.model).desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui.button("Load ONNX…").clicked() {
                    actions.push(FileAction::BackgroundModel);
                }
                if ui
                    .add_enabled(
                        !PathBuf::from(&self.model_ui.model).is_file(),
                        egui::Button::new("Download model"),
                    )
                    .clicked()
                {
                    self.background_start(true);
                }
            });
            ui.checkbox(
                &mut self.model_ui.auto_device,
                "Use GPU (ONNX provider if available)",
            );
        });
        ui.hyperlink_to(
            "Official BiRefNet models",
            "https://github.com/ZhengPeng7/BiRefNet/releases/tag/v1",
        );
        ui.small("Active layer → transparent cutout. Accepting hides the original layer; Undo restores it.");
    }
    pub(super) fn model_tools_panel(&mut self, ui: &mut egui::Ui, _actions: &mut Vec<FileAction>) {
        egui::CollapsingHeader::new("Удаление фона")
            .default_open(true)
            .show(ui, |ui| {
                let busy = self.model_ui.job.is_some()
                    || self.segmentation_operation_active()
                    || self.plakat_active()
                    || self.upscale_operation_active();
                if ui
                    .add_enabled(
                        !busy
                            && !self.document.layers()[self.document.active()].locked()
                            && self.model_ui.candidate.is_none()
                            && PathBuf::from(&self.model_ui.model).is_file(),
                        egui::Button::new("Удалить фон"),
                    )
                    .clicked()
                {
                    self.background_start(false);
                }
            });
        if let Some(job) = &self.model_ui.job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Model tool is running…");
            });
            if let Some((n, total)) = job.progress() {
                ui.label(format!("{} / {} MiB", n / 1_048_576, total / 1_048_576));
            }
            if ui.button("Cancel model operation").clicked() {
                self.model_ui.job = None;
                self.model_ui.operation = None;
                self.model_ui.source = None;
                self.status = "Model operation cancelled".into();
            }
        }
    }
    pub(super) fn poll_model_tools(&mut self, context: &egui::Context) {
        if let Some(job) = &mut self.model_ui.job {
            match job.poll() {
                Ok(Some(output)) => {
                    self.model_ui.job = None;
                    match (
                        self.model_ui.operation.take(),
                        output.raster,
                        self.model_ui.source.take(),
                    ) {
                        (Some(Operation::RemoveBackground), Some(raster), Some(source)) => {
                            if raster.width() != source.layer.raster().width()
                                || raster.height() != source.layer.raster().height()
                            {
                                self.status = "BiRefNet returned a different size".into();
                            } else {
                                self.model_ui.candidate = Some(Cutout { raster, source });
                                self.model_ui.texture = None;
                                self.status =
                                    "Background removed; review transparency before accepting"
                                        .into();
                            }
                        }
                        (Some(Operation::DownloadBackground), _, _) => {
                            self.status =
                                "BiRefNet weights downloaded and ONNX graph validated".into();
                        }
                        _ => self.status = "Model tool returned no result".into(),
                    }
                }
                Err(e) => {
                    self.model_ui.job = None;
                    self.model_ui.operation = None;
                    self.model_ui.source = None;
                    self.status = e.to_string();
                }
                Ok(None) => context.request_repaint_after(Duration::from_millis(100)),
            }
        }
    }
    pub(super) fn background_preview(&mut self, context: &egui::Context) {
        let Some(candidate) = &self.model_ui.candidate else {
            return;
        };
        if self.model_ui.texture.is_none() {
            let r = &candidate.raster;
            self.model_ui.texture = Some(context.load_texture(
                "background-cutout",
                egui::ColorImage::from_rgba_unmultiplied(
                    [r.width() as usize, r.height() as usize],
                    r.pixels(),
                ),
                egui::TextureOptions::LINEAR,
            ));
        }
        let mut accept = false;
        let mut discard = false;
        egui::Window::new("BiRefNet · transparent cutout")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.add(
                    egui::Image::new(self.model_ui.texture.as_ref().expect("cutout texture"))
                        .bg_fill(Color32::from_gray(70))
                        .max_size(egui::vec2(650.0, 500.0)),
                );
                ui.horizontal(|ui| {
                    accept = ui.button("Accept cutout as layer").clicked();
                    discard = ui.button("Discard").clicked();
                });
            });
        if accept {
            let candidate = self.model_ui.candidate.take().expect("cutout");
            self.edit(|doc| accept_cutout(doc, candidate));
            self.model_ui.texture = None;
        }
        if discard {
            self.model_ui.candidate = None;
            self.model_ui.texture = None;
        }
    }
}
fn accept_cutout(doc: &mut Document, candidate: Cutout) -> Result<(), EditError> {
    if doc.layers().get(candidate.source.index) != Some(&candidate.source.layer) {
        return Err(EditError::Invalid(
            "Source layer changed; remove background again".into(),
        ));
    }
    doc.select(candidate.source.index)?;
    doc.add_layer_result("BiRefNet · cutout".into(), candidate.raster)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn birefnet_keeps_source_mask_blend_and_stack_position() {
        let mut doc =
            Document::from_raster(Raster::solid(8, 8, [90, 100, 110, 255]).unwrap()).unwrap();
        doc.add_layer_mask(true).unwrap();
        doc.set_layer_mask_enabled(false).unwrap();
        doc.set_blend_mode(crate::LayerBlendMode::Screen).unwrap();
        let source = Source {
            index: 0,
            layer: doc.layers()[0].clone(),
        };
        doc.add_layer(
            "above".into(),
            Raster::solid(8, 8, [50, 60, 70, 90]).unwrap(),
        )
        .unwrap();
        let before = doc.flatten();
        let raster = source.layer.raster().clone();
        accept_cutout(&mut doc, Cutout { raster, source }).unwrap();
        assert_eq!(doc.active(), 1);
        assert_eq!(doc.layers()[2].name(), "above");
        assert_eq!(doc.layers()[1].mask(), doc.layers()[0].mask());
        assert_eq!(doc.layers()[1].blend_mode(), crate::LayerBlendMode::Screen);
        assert_eq!(doc.flatten(), before);
    }
    #[test]
    fn accepting_cutout_hides_original_and_rejects_stale_source() {
        let mut doc = Document::new(8, 8).unwrap();
        let source = Source {
            index: 0,
            layer: doc.layers()[0].clone(),
        };
        let raster = Raster::solid(8, 8, [20, 30, 40, 0]).unwrap();
        accept_cutout(&mut doc, Cutout { raster, source }).unwrap();
        assert!(!doc.layers()[0].visible());
        assert_eq!(doc.layers().len(), 2);
        assert!(doc.flatten().pixels().chunks_exact(4).all(|p| p[3] == 0));
        let source = Source {
            index: 1,
            layer: doc.layers()[1].clone(),
        };
        doc.set_layer("changed".into(), true, 128);
        assert!(accept_cutout(
            &mut doc,
            Cutout {
                raster: Raster::solid(8, 8, [0; 4]).unwrap(),
                source
            }
        )
        .is_err());
    }
}
