//! Reusable model settings, independent of the demo host's persistence location.
use super::{FileAction, ImageEditor};
use crate::{generation::Request, EditError};
use serde::{Deserialize, Serialize};

/// Serializable model preferences; contains no Civitai credentials or project pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditorSettings {
    version: u32,
    #[serde(default)]
    pixel_grid: bool,
    #[serde(default)]
    presets: Vec<super::GenerationPreset>,
    #[serde(deserialize_with = "generation_preferences")]
    request: Request,
    model_name: String,
    executable: String,
    use_library: bool,
    background: (String, String, bool),
    segmentation: (String, bool),
    upscale: (String, String, String, f32, f32, u32, bool),
}
// Preferences adopt the new default, while historical document recipes keep DDIM.
fn generation_preferences<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Request, D::Error> {
    let mut value = serde_json::Value::deserialize(deserializer)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| serde::de::Error::custom("Generation preferences must be an object"))?;
    if !object.contains_key("sampler") {
        object.insert(
            "sampler".into(),
            serde_json::to_value(crate::generation::Sampler::Dpmpp2mKarras)
                .map_err(serde::de::Error::custom)?,
        );
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}
impl EditorSettings {
    /// Cache containing prepared generation models; configure the backend before starting threads.
    pub fn cache_dir(&self) -> Option<&std::path::Path> {
        self.request.cache_dir.as_deref()
    }
    /// Selected Plakat alias or prepared repository identifier.
    pub fn model(&self) -> &str {
        &self.request.model
    }
}
impl ImageEditor {
    /// Snapshot preferences for persistence by `MuFoEd` or another embedding host.
    /// Prompts and per-document generation mode are deliberately not stored.
    pub fn settings(&self) -> EditorSettings {
        let mut request = self.request.clone();
        request.prompt.clear();
        request.negative.clear();
        request.mode = crate::generation::Mode::TextToImage;
        EditorSettings {
            version: 1,
            pixel_grid: self.pixel_grid,
            presets: self.presets.clone(),
            request,
            model_name: self.generation_ui.model_name.clone(),
            executable: self.executable.clone(),
            use_library: self.use_library,
            background: (
                self.model_ui.variant.clone(),
                self.model_ui.model.clone(),
                self.model_ui.auto_device,
            ),
            segmentation: (
                self.segmentation_ui.model.clone(),
                self.segmentation_ui.auto_device,
            ),
            upscale: (
                self.upscale_ui.variant.clone(),
                self.upscale_ui.ultrasharp.clone(),
                self.upscale_ui.aurasr.clone(),
                self.upscale_ui.ultrasharp_weight,
                self.upscale_ui.aura_weight,
                self.upscale_ui.tile,
                self.upscale_ui.auto_device,
            ),
        }
    }
    /// Restore preferences before inference. Missing model files remain selected and can be repaired in settings.
    pub fn apply_settings(&mut self, settings: EditorSettings) -> Result<(), EditError> {
        settings.validate()?;
        if self.settings_busy() {
            return Err(EditError::Invalid(
                "Wait for active model operations before restoring settings".into(),
            ));
        }
        let prompt = self.request.prompt.clone();
        let negative = self.request.negative.clone();
        let mode = self.request.mode;
        self.pixel_grid = settings.pixel_grid;
        self.presets = settings.presets;
        self.request = settings.request;
        self.generation_ui.seed_text = self.request.seed.to_string();
        self.request.prompt = prompt;
        self.request.negative = negative;
        self.request.mode = mode;
        self.generation_ui.model_name = settings.model_name;
        self.executable = settings.executable;
        self.use_library = settings.use_library && cfg!(feature = "plakat");
        (
            self.model_ui.variant,
            self.model_ui.model,
            self.model_ui.auto_device,
        ) = settings.background;
        (self.segmentation_ui.model, self.segmentation_ui.auto_device) = settings.segmentation;
        (
            self.upscale_ui.variant,
            self.upscale_ui.ultrasharp,
            self.upscale_ui.aurasr,
            self.upscale_ui.ultrasharp_weight,
            self.upscale_ui.aura_weight,
            self.upscale_ui.tile,
            self.upscale_ui.auto_device,
        ) = settings.upscale;
        Ok(())
    }
    /// Open the common model settings window.
    pub fn open_settings(&mut self) {
        self.settings_open = true;
        if self.active_tab == super::StudioTab::Upscale {
            self.settings_tab = 3;
        }
    }
    /// Render preferences inside the host's own settings page; returns native file picker requests.
    pub fn settings_ui(&mut self, ui: &mut egui::Ui) -> Vec<FileAction> {
        let mut actions = Vec::new();
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["Генерация", "Удаление фона", "Выделение", "Апскейл"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.settings_tab, i, *label);
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("common-model-settings")
            .max_height(480.0)
            .min_scrolled_height(480.0)
            .show(ui, |ui| match self.settings_tab {
                0 => {
                    ui.add_enabled_ui(!self.settings_busy(), |ui| {
                        ui.label("Сэмплер");
                        egui::ComboBox::from_id_salt("generation-sampler")
                            .selected_text(match self.request.sampler {
                                crate::generation::Sampler::Ddim => "DDIM",
                                crate::generation::Sampler::Dpmpp2mKarras => "DPM++ 2M Karras",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.request.sampler,
                                    crate::generation::Sampler::Dpmpp2mKarras,
                                    "DPM++ 2M Karras",
                                );
                                ui.selectable_value(
                                    &mut self.request.sampler,
                                    crate::generation::Sampler::Ddim,
                                    "DDIM",
                                );
                            });

                        self.generation_panel(ui, &mut actions);
                        self.generation_backend_settings(ui, &mut actions);
                    });
                }
                1 => self.background_settings(ui, &mut actions),
                2 => self.segmentation_settings(ui, &mut actions),
                _ => self.upscale_settings(ui, &mut actions),
            });
        actions
    }
    pub(super) fn settings_busy(&self) -> bool {
        self.resource_operation_active()
            || self.plakat_active()
            || self.model_operation_active()
            || self.segmentation_operation_active()
            || self.upscale_operation_active()
    }
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) -> Vec<FileAction> {
        let mut actions = Vec::new();
        if self.settings_open && self.generation_ui.dialog.is_none() {
            let mut open = true;
            egui::Window::new("Настройки Image Studio")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(620.0)
                .show(ctx, |ui| {
                    ui.push_id("settings-window", |ui| {
                        actions = self.settings_ui(ui);
                    });
                });
            self.settings_open = open;
        }
        actions
    }
}
impl EditorSettings {
    /// Validate persisted configuration before using identifiers, dimensions, and memory-sensitive controls.
    pub fn validate(&self) -> Result<(), EditError> {
        super::preset_panel::validate_library(&self.presets)?;
        let r = &self.request;
        let mut probe = r.clone();
        probe.prompt = "configuration validation".into();
        probe.mode = crate::generation::Mode::TextToImage;
        probe.loras.clear();
        probe.vae = None;
        probe.validate(&crate::Document::new(64, 64)?)?;
        if self.version != 1
            || r.loras.len() > 16
            || r.loras
                .iter()
                .any(|l| !l.weight.is_finite() || !(-2.0..=2.0).contains(&l.weight))
            || r.postprocess
                .as_ref()
                .is_some_and(|p| p.validate_parameters().is_err())
            || r.model.len() > 4096
            || self.model_name.len() > 4096
            || !["tiny", "general", "matting"].contains(&self.background.0.as_str())
            || !["blend", "ultrasharp", "aurasr"].contains(&self.upscale.0.as_str())
            || ![128, 256, 512].contains(&self.upscale.5)
            || [self.upscale.3, self.upscale.4]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(EditError::Invalid(
                "Invalid image editor preferences".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_preferences_adopt_dpm_but_old_recipes_remain_ddim() {
        use crate::generation::Sampler;
        let editor = ImageEditor::default();
        let mut value = serde_json::to_value(editor.settings()).unwrap();
        value["request"].as_object_mut().unwrap().remove("sampler");
        let recipe: Request = serde_json::from_value(value["request"].clone()).unwrap();
        assert_eq!(recipe.sampler, Sampler::Ddim);
        let settings: EditorSettings = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(settings.request.sampler, Sampler::Dpmpp2mKarras);
        value["request"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<EditorSettings>(value).is_err());
    }
    #[test]
    fn model_preferences_survive_restart_and_project_replacement() {
        let mut editor = ImageEditor::default();
        editor.request.model = "local/last-prepared-sdxl".into();
        editor.request.family = Some(crate::assets::Family::Sdxl);
        editor.request.output_size = Some([1024, 768]);
        editor.request.prompt = "Private project prompt".into();
        editor.generation_ui.model_name = "My checkpoint".into();
        editor.upscale_ui.ultrasharp_weight = 0.7;
        let saved = editor.settings();
        let bytes = serde_json::to_vec(&saved).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("Private project prompt"));
        let mut restored = ImageEditor::default();
        restored
            .apply_settings(serde_json::from_slice(&bytes).unwrap())
            .unwrap();
        assert_eq!(restored.settings(), saved);
        assert_eq!(restored.request.model, "local/last-prepared-sdxl");
        restored.replace(crate::Document::new(128, 64).unwrap());
        assert_eq!(restored.settings(), saved);
    }
    #[test]
    fn missing_weights_remain_selected_and_invalid_settings_do_not_mutate_editor() {
        let mut editor = ImageEditor::default();
        let mut saved = editor.settings();
        saved.request.vae = Some("missing/vae.safetensors".into());
        saved.request.loras.push(crate::generation::Lora {
            path: "missing/lora.safetensors".into(),
            weight: 0.7,
            enabled: true,
        });
        editor.apply_settings(saved.clone()).unwrap();
        assert_eq!(editor.settings(), saved);
        saved.version = 99;
        assert!(editor.apply_settings(saved).is_err());
        assert_eq!(editor.request.vae, Some("missing/vae.safetensors".into()));
    }
}
