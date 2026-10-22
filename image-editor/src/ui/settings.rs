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
    #[serde(default = "default_random_seed")]
    random_seed: bool,
    #[serde(default)]
    automatic_vae: Option<std::path::PathBuf>,
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
fn default_random_seed() -> bool {
    true
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
        request.seed = self
            .generation_ui
            .seed_text
            .trim()
            .parse()
            .unwrap_or(request.seed);
        EditorSettings {
            version: 1,
            pixel_grid: self.pixel_grid,
            random_seed: self.generation_ui.random_seed,
            automatic_vae: self
                .generation_ui
                .automatic_vae
                .clone()
                .filter(|p| self.request.vae.as_ref() == Some(p)),
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
        if self.background_active() {
            return Err(EditError::Invalid(
                "Wait for this document's model operations before restoring settings".into(),
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
        self.set_generation_mode(mode);
        self.generation_ui.random_seed = settings.random_seed;
        self.generation_ui.automatic_vae = settings
            .automatic_vae
            .filter(|p| self.request.vae.as_ref() == Some(p));
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
        self.finish_manual_gesture();
        self.settings_open = true;
        if self.active_tab == super::StudioTab::Upscale {
            self.settings_tab = 3;
        }
        if std::env::var_os("IMAGE_STUDIO_SHOT").is_some() {
            if let Some(tab) = std::env::var("IMAGE_STUDIO_SHOT_SETTINGS_TAB")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&v| v < 4)
            {
                self.settings_tab = tab;
            }
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
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_width(ui.available_width())
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
            let response = egui::Window::new("Настройки Image Studio")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(
                    (ctx.content_rect().width() - 64.0).clamp(340.0, 700.0),
                    (ctx.content_rect().height() - 100.0).clamp(300.0, 560.0),
                ))
                .show(ctx, |ui| {
                    ui.push_id("settings-window", |ui| {
                        actions = self.settings_ui(ui);
                    });
                });
            #[cfg(test)]
            if let Some(window) = &response {
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("settings-rect-test"), window.response.rect)
                });
            }
            #[cfg(not(test))]
            let _ = response;
            self.settings_open =
                open && !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
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
    fn fixed_seed_and_random_mode_roundtrip_with_legacy_default() {
        let mut e = ImageEditor::default();
        e.generation_ui.random_seed = false;
        e.generation_ui.seed_text = u64::MAX.to_string();
        let saved = e.settings();
        let mut next = ImageEditor::default();
        next.apply_settings(saved.clone()).unwrap();
        assert!(!next.generation_ui.random_seed);
        assert_eq!(next.generation_ui.seed_text, u64::MAX.to_string());
        assert_eq!(next.request.seed, u64::MAX);
        let mut value = serde_json::to_value(saved).unwrap();
        value.as_object_mut().unwrap().remove("random_seed");
        let old = serde_json::from_value(value).unwrap();
        next.apply_settings(old).unwrap();
        assert!(next.generation_ui.random_seed);
    }
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
    #[test]
    fn settings_outer_size_and_position_do_not_change_across_tabs() {
        let ctx = egui::Context::default();
        super::super::apply_studio_style(&ctx);
        let mut e = ImageEditor {
            settings_open: true,
            ..Default::default()
        };
        let mut expected = None;
        for tab in [0, 1, 2, 3, 0] {
            e.settings_tab = tab;
            for _ in 0..3 {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1100.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        e.settings_window(ui.ctx());
                    },
                );
            }
            let rect = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("settings-rect-test")))
                .unwrap();
            assert!(
                rect.width() < 800.0 && rect.height() < 650.0,
                "{tab}: {rect:?}"
            );
            if let Some(previous) = expected {
                assert_eq!(rect, previous, "tab {tab}");
            } else {
                expected = Some(rect);
            }
        }
    }
}
