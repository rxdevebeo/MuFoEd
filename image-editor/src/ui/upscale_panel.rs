use super::{Document, EditError, FileAction, ImageEditor, Raster, TextureHandle};
use crate::model_tools::ModelJob;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) struct UpscaleUi {
    root: PathBuf,
    variant: String,
    ultrasharp: String,
    aurasr: String,
    ultrasharp_weight: f32,
    aura_weight: f32,
    tile: u32,
    auto_device: bool,
    job: Option<ModelJob>,
    source: Option<Raster>,
    download: Option<String>,
    candidate: Option<(Raster, Raster)>,
    texture: Option<TextureHandle>,
    result_label: String,
}
impl Default for UpscaleUi {
    fn default() -> Self {
        let root = std::env::var_os("IMAGE_STUDIO_CACHE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_owned))
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".runtime"))
            .join("Upscale");
        let local = Path::new("D:/Models/4xUltrasharp_4xUltrasharpV10.pt");
        Self {
            ultrasharp: if local.is_file() {
                local.display().to_string()
            } else {
                root.join("ultrasharp/4x-UltraSharp.safetensors")
                    .display()
                    .to_string()
            },
            aurasr: root.join("aurasr").display().to_string(),
            root,
            variant: "blend".into(),
            ultrasharp_weight: 0.65,
            aura_weight: 0.35,
            tile: 256,
            auto_device: true,
            job: None,
            source: None,
            download: None,
            candidate: None,
            texture: None,
            result_label: String::new(),
        }
    }
}
fn parse_percent(value: &str) -> Option<f64> {
    value
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<f64>()
        .ok()
        .map(|n| n / 100.0)
}
fn normalized_weights(a: f32, b: f32) -> Result<[f32; 2], EditError> {
    if !a.is_finite()
        || !b.is_finite()
        || !(0.0..=1.0).contains(&a)
        || !(0.0..=1.0).contains(&b)
        || a + b <= 0.0
    {
        return Err(EditError::Invalid(
            "Set at least one positive upscaler weight".into(),
        ));
    }
    Ok([a / (a + b), b / (a + b)])
}
impl UpscaleUi {
    pub(super) fn reset_project(&mut self) {
        self.job = None;
        self.source = None;
        self.download = None;
        self.candidate = None;
        self.texture = None;
    }
}
impl ImageEditor {
    /// Select local `UltraSharp` tensor weights.
    pub fn set_ultrasharp_model(&mut self, path: &Path) {
        self.upscale_ui.ultrasharp = path.display().to_string();
        if let Some(post) = &mut self.request.postprocess {
            path.clone_into(&mut post.ultrasharp);
        }
    }
    /// Select local `AuraSR` v2 safetensors/config directory.
    pub fn set_aurasr_model(&mut self, path: &Path) {
        self.upscale_ui.aurasr = path.display().to_string();
    }
    pub(super) fn upscale_operation_active(&self) -> bool {
        self.upscale_ui.job.is_some()
    }
    fn upscale_download(&mut self, kind: &str) {
        let target = self.upscale_ui.root.join(kind);
        if target.exists() {
            self.set_downloaded_upscaler(kind);
            return;
        }
        let args: Vec<OsString> = vec![
            "download".into(),
            "--variant".into(),
            kind.into(),
            "--model".into(),
            target.into_os_string(),
        ];
        match ModelJob::start_with_runtime(
            "upscale_tool.py",
            &args,
            None,
            Some(&self.upscale_ui.root),
            &self.runtime,
        ) {
            Ok(job) => {
                self.upscale_ui.job = Some(job);
                self.upscale_ui.download = Some(kind.into());
                self.upscale_ui.source = None;
                self.status = format!("Downloading {kind}…");
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    fn set_downloaded_upscaler(&mut self, kind: &str) {
        if kind == "ultrasharp" {
            self.upscale_ui.ultrasharp = self
                .upscale_ui
                .root
                .join("ultrasharp/4x-UltraSharp.safetensors")
                .display()
                .to_string();
        } else {
            self.upscale_ui.aurasr = self.upscale_ui.root.join("aurasr").display().to_string();
        }
        if kind == "ultrasharp" {
            if let Some(post) = &mut self.request.postprocess {
                post.ultrasharp = self.upscale_ui.ultrasharp.clone().into();
            }
        }
    }
    fn upscale_start(&mut self) {
        let source = self.document.flatten();
        if u64::from(source.width()) * u64::from(source.height()) * 16 > 16_777_216 {
            self.status =
                "4× output exceeds 16,777,216 pixels; crop or resize the source first".into();
            return;
        }
        let weights = match if self.upscale_ui.variant == "blend" {
            normalized_weights(
                self.upscale_ui.ultrasharp_weight,
                self.upscale_ui.aura_weight,
            )
        } else {
            Ok(if self.upscale_ui.variant == "ultrasharp" {
                [1.0, 0.0]
            } else {
                [0.0, 1.0]
            })
        } {
            Ok(w) => w,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        self.upscale_ui.result_label = match self.upscale_ui.variant.as_str() {
            "ultrasharp" => "4× · UltraSharp".into(),
            "aurasr" => "4× · AuraSR v2".into(),
            _ => format!(
                "4× · UltraSharp {:.0}% + AuraSR v2 {:.0}%",
                weights[0] * 100.0,
                weights[1] * 100.0
            ),
        };
        let model = if self.upscale_ui.variant == "aurasr" {
            &self.upscale_ui.aurasr
        } else {
            &self.upscale_ui.ultrasharp
        };
        let args: Vec<OsString> = vec![
            "upscale".into(),
            "--variant".into(),
            self.upscale_ui.variant.clone().into(),
            "--model".into(),
            model.into(),
            "--aura-model".into(),
            self.upscale_ui.aurasr.clone().into(),
            "--ultrasharp-weight".into(),
            weights[0].to_string().into(),
            "--aura-weight".into(),
            weights[1].to_string().into(),
            "--tile".into(),
            self.upscale_ui.tile.to_string().into(),
            "--device".into(),
            if self.upscale_ui.auto_device {
                "auto"
            } else {
                "cpu"
            }
            .into(),
        ];
        match ModelJob::start_with_runtime(
            "upscale_tool.py",
            &args,
            Some(&source),
            None,
            &self.runtime,
        ) {
            Ok(job) => {
                self.upscale_ui.job = Some(job);
                self.upscale_ui.source = Some(source);
                self.upscale_ui.download = None;
                self.status = "Upscaling 4×; models run sequentially to limit VRAM…".into();
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    pub(super) fn upscale_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        egui::CollapsingHeader::new("UltraSharp / AuraSR v2")
            .default_open(true)
            .show(ui, |ui| {
                let busy = self.upscale_operation_active()
                    || self.model_operation_active()
                    || self.segmentation_operation_active()
                    || self.plakat_active();
                ui.add_enabled_ui(!busy, |ui| {
                    egui::ComboBox::from_id_salt("upscale-method")
                        .selected_text(match self.upscale_ui.variant.as_str() {
                            "ultrasharp" => "4x-UltraSharp only",
                            "aurasr" => "AuraSR v2 only",
                            _ => "Mix UltraSharp + AuraSR v2",
                        })
                        .show_ui(ui, |ui| {
                            for (id, label) in [
                                ("blend", "Mix UltraSharp + AuraSR v2"),
                                ("ultrasharp", "4x-UltraSharp only"),
                                ("aurasr", "AuraSR v2 only"),
                            ] {
                                ui.selectable_value(&mut self.upscale_ui.variant, id.into(), label);
                            }
                        });
                    self.upscale_model_controls(ui, actions);
                    ui.add_enabled_ui(self.upscale_ui.variant == "blend", |ui| {
                        ui.label("Mix weights (normalized to 100%)");
                        ui.add(
                            egui::Slider::new(&mut self.upscale_ui.ultrasharp_weight, 0.0..=1.0)
                                .text("UltraSharp")
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                                .custom_parser(parse_percent),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.upscale_ui.aura_weight, 0.0..=1.0)
                                .text("AuraSR v2")
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                                .custom_parser(parse_percent),
                        );
                    });
                    if self.upscale_ui.variant == "blend" {
                        if let Ok([a, b]) = normalized_weights(
                            self.upscale_ui.ultrasharp_weight,
                            self.upscale_ui.aura_weight,
                        ) {
                            ui.small(format!(
                                "Result: UltraSharp {:.0}% · AuraSR v2 {:.0}%",
                                a * 100.0,
                                b * 100.0
                            ));
                        }
                    }
                    ui.checkbox(&mut self.upscale_ui.auto_device, "Use CUDA if available");
                    egui::ComboBox::from_id_salt("upscale-tile")
                        .selected_text(format!("UltraSharp tile: {} px", self.upscale_ui.tile))
                        .show_ui(ui, |ui| {
                            for n in [128, 256, 512] {
                                ui.selectable_value(
                                    &mut self.upscale_ui.tile,
                                    n,
                                    format!("{n} px"),
                                );
                            }
                        });
                    ui.small(format!(
                        "{} × {} -> {} × {} · source composite",
                        self.document.width(),
                        self.document.height(),
                        self.document.width() * 4,
                        self.document.height() * 4
                    ));
                    if ui
                        .add_enabled(
                            self.upscale_ui.candidate.is_none(),
                            egui::Button::new("Upscale 4×"),
                        )
                        .clicked()
                    {
                        self.upscale_start();
                    }
                });
                if let Some(job) = &self.upscale_ui.job {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Upscale model operation…");
                    });
                    if let Some((n, total)) = job.progress() {
                        ui.label(format!("{} / {} MiB", n / 1_048_576, total / 1_048_576));
                    }
                    if ui.button("Cancel upscale operation").clicked() {
                        self.upscale_ui.job = None;
                        self.upscale_ui.source = None;
                        self.upscale_ui.download = None;
                        self.status = "Upscale operation cancelled".into();
                    }
                }
            });
    }
    fn upscale_model_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        if self.upscale_ui.variant != "aurasr" {
            ui.label("4x-UltraSharp");
            ui.add(
                egui::TextEdit::singleline(&mut self.upscale_ui.ultrasharp)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui.button("Load weights…").clicked() {
                    actions.push(FileAction::UltraSharpModel);
                }
                if ui.button("Download / use cached").clicked() {
                    self.upscale_download("ultrasharp");
                }
            });
            ui.hyperlink_to(
                "Model / license",
                "https://huggingface.co/Kim2091/UltraSharp",
            );
        }
        if self.upscale_ui.variant != "ultrasharp" {
            ui.label("AuraSR v2");
            ui.add(
                egui::TextEdit::singleline(&mut self.upscale_ui.aurasr)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui.button("Load folder…").clicked() {
                    actions.push(FileAction::AuraSrModel);
                }
                if ui.button("Download / use cached").clicked() {
                    self.upscale_download("aurasr");
                }
            });
            ui.hyperlink_to("Model / license", "https://huggingface.co/fal/AuraSR-v2");
        }
    }
    pub(super) fn poll_upscale(&mut self, context: &egui::Context) {
        if let Some(job) = &mut self.upscale_ui.job {
            match job.poll() {
                Ok(Some(output)) => {
                    self.upscale_ui.job = None;
                    if let Some(kind) = self.upscale_ui.download.take() {
                        self.set_downloaded_upscaler(&kind);
                        self.status = "Upscaler downloaded; SHA256 verified".into();
                    } else if let (Some(raster), Some(source)) =
                        (output.raster, self.upscale_ui.source.take())
                    {
                        if [raster.width(), raster.height()]
                            == [source.width() * 4, source.height() * 4]
                        {
                            self.upscale_ui.candidate = Some((raster, source));
                            self.upscale_ui.texture = None;
                            self.status =
                                "Upscale ready; review before accepting as a new image".into();
                        } else {
                            self.status = "Upscale output size mismatch".into();
                        }
                    } else {
                        self.status = "Upscaler returned no PNG".into();
                    }
                }
                Err(e) => {
                    self.upscale_ui.job = None;
                    self.upscale_ui.source = None;
                    self.upscale_ui.download = None;
                    self.status = e.to_string();
                }
                Ok(None) => context.request_repaint_after(Duration::from_millis(100)),
            }
        }
    }
    pub(super) fn upscale_preview(&mut self, context: &egui::Context) {
        let Some((raster, _)) = &self.upscale_ui.candidate else {
            return;
        };
        if self.upscale_ui.texture.is_none() {
            self.upscale_ui.texture = Some(context.load_texture(
                "upscaled-image",
                egui::ColorImage::from_rgba_unmultiplied(
                    [raster.width() as usize, raster.height() as usize],
                    raster.pixels(),
                ),
                egui::TextureOptions::LINEAR,
            ));
        }
        let mut accept = false;
        let mut discard = false;
        egui::Window::new("4× upscale · preview")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(&self.upscale_ui.result_label);
                ui.label(format!("{} × {}", raster.width(), raster.height()));
                ui.add(
                    egui::Image::new(self.upscale_ui.texture.as_ref().expect("upscale texture"))
                        .max_size(egui::vec2(700.0, 500.0)),
                );
                ui.small(
                    "Accept opens the result as a new image. Undo restores the source layers.",
                );
                ui.horizontal(|ui| {
                    accept = ui.button("Accept as new image").clicked();
                    discard = ui.button("Discard").clicked();
                });
            });
        if accept {
            let label = self.upscale_ui.result_label.clone();
            let (raster, source) = self.upscale_ui.candidate.take().expect("upscale candidate");
            self.edit(|doc| {
                if doc.flatten() != source {
                    return Err(EditError::Invalid(
                        "Source changed during upscale; run it again".into(),
                    ));
                }
                *doc = Document::from_raster(raster)?;
                doc.set_layer(label, true, 255);
                Ok(())
            });
            self.upscale_ui.texture = None;
        }
        if discard {
            self.upscale_ui.candidate = None;
            self.upscale_ui.texture = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(clippy::float_cmp)] // Exact defaults and exactly representable reference ratios.
    fn blend_weights_default_to_65_35_and_normalize_or_reject() {
        let defaults = UpscaleUi::default();
        assert_eq!(
            normalized_weights(defaults.ultrasharp_weight, defaults.aura_weight).unwrap(),
            [0.65, 0.35]
        );
        assert_eq!(normalized_weights(0.6, 0.2).unwrap(), [0.75, 0.25]);
        assert_eq!(normalized_weights(1.0, 0.0).unwrap(), [1.0, 0.0]);
        assert!(normalized_weights(0.0, 0.0).is_err());
        assert!(normalized_weights(f32::NAN, 0.5).is_err());
        assert_eq!(parse_percent("65%"), Some(0.65));
        assert_eq!(parse_percent(" 35 "), Some(0.35));
        assert!(parse_percent("invalid").is_none());
    }
    #[test]
    fn changing_source_retains_upscaler_paths_weights_and_tab() {
        let mut editor = ImageEditor::default();
        editor.select_tab(super::super::StudioTab::Upscale);
        editor.upscale_ui.ultrasharp = "local/weights.safetensors".into();
        editor.upscale_ui.variant = "aurasr".into();
        editor.replace(Document::new(8, 8).unwrap());
        assert_eq!(editor.upscale_ui.ultrasharp, "local/weights.safetensors");
        assert_eq!(editor.upscale_ui.variant, "aurasr");
        assert!(editor.active_tab == super::super::StudioTab::Upscale);
    }
}
