//! Named generation presets; model files remain external references.
use super::{FileAction, ImageEditor};
use crate::{generation::Request, EditError};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
};
const MAX_PRESETS: usize = 64;
const MAX_BYTES: usize = 2 * 1024 * 1024;

/// A reusable generation configuration, with optional explicitly saved prompt text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationPreset {
    name: String,
    request: Request,
    model_name: String,
    random_seed: bool,
    include_prompts: bool,
}
impl GenerationPreset {
    /// Name shown in the preset library.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Model, adapters and generation parameters; model files are referenced, not embedded.
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub(super) fn validate(&self) -> Result<(), EditError> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 96
            || self.name.chars().any(char::is_control)
            || self.model_name.len() > 4096
            || self.request.prompt.len() > 16384
            || self.request.negative.len() > 16384
        {
            return Err(EditError::Invalid(
                "Invalid preset name or text length".into(),
            ));
        }
        let r = &self.request;
        let mut probe = r.clone();
        probe.prompt = "preset validation".into();
        probe.mode = crate::generation::Mode::TextToImage;
        probe.loras.clear();
        probe.vae = None;
        probe.validate(&crate::Document::new(64, 64)?)?;
        if r.model.len() > 4096
            || r.loras.len() > 16
            || r.loras
                .iter()
                .any(|l| !l.weight.is_finite() || !(-2.0..=2.0).contains(&l.weight))
            || r.postprocess
                .as_ref()
                .is_some_and(|p| p.validate_parameters().is_err())
        {
            return Err(EditError::Invalid(
                "Invalid preset generation parameters".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct PresetFile {
    version: u32,
    presets: Vec<GenerationPreset>,
}
#[derive(Default)]
pub(super) struct PresetUi {
    pub(super) open: bool,
    name: String,
    include_prompts: bool,
    message: String,
}

pub(super) fn validate_library(presets: &[GenerationPreset]) -> Result<(), EditError> {
    if presets.len() > MAX_PRESETS || serde_json::to_vec(presets)?.len() > MAX_BYTES {
        return Err(EditError::Invalid(
            "Preset library exceeds 64 entries or 2 MiB".into(),
        ));
    }
    let mut names = std::collections::HashSet::new();
    for preset in presets {
        preset.validate()?;
        if !names.insert(preset.name.trim()) {
            return Err(EditError::Invalid(format!(
                "Duplicate preset: {}",
                preset.name
            )));
        }
    }
    Ok(())
}
impl ImageEditor {
    /// Open the centered generation preset library.
    pub fn open_presets(&mut self) {
        self.finish_manual_gesture();
        self.preset_ui.open = true;
    }
    /// Presets available to an embedding host; persisted by the settings snapshot.
    pub fn presets(&self) -> &[GenerationPreset] {
        &self.presets
    }
    /// Capture current model/LoRA/VAE, sampler, size, seed mode and generation parameters.
    /// Existing names require explicit overwrite. Prompts are saved only when requested.
    pub fn save_preset(
        &mut self,
        name: &str,
        include_prompts: bool,
        overwrite: bool,
    ) -> Result<(), EditError> {
        let mut request = self.request.clone();
        request.cache_dir = None;
        request.seed =
            self.generation_ui.seed_text.trim().parse().map_err(|_| {
                EditError::Invalid("Enter a valid seed before saving a preset".into())
            })?;
        if !include_prompts {
            request.prompt.clear();
            request.negative.clear();
        }
        let preset = GenerationPreset {
            name: name.trim().into(),
            request,
            model_name: self.generation_ui.model_name.clone(),
            random_seed: self.generation_ui.random_seed,
            include_prompts,
        };
        preset.validate()?;
        let mut next = self.presets.clone();
        if let Some(index) = next.iter().position(|p| p.name == preset.name) {
            if !overwrite {
                return Err(EditError::Invalid(
                    "Preset already exists; use overwrite".into(),
                ));
            }
            next[index] = preset;
        } else {
            next.push(preset);
        }
        validate_library(&next)?;
        self.presets = next;
        Ok(())
    }
    /// Apply a preset atomically, keeping the host's fixed startup cache and document pixels.
    pub fn apply_preset(&mut self, index: usize) -> Result<(), EditError> {
        if self.settings_busy() {
            return Err(EditError::Invalid(
                "Wait for model operations before applying a preset".into(),
            ));
        }
        let preset = self
            .presets
            .get(index)
            .ok_or_else(|| EditError::Invalid("Preset not found".into()))?;
        preset.validate()?;
        let mut request = preset.request.clone();
        request.cache_dir.clone_from(&self.request.cache_dir);
        if !preset.include_prompts {
            request.prompt.clone_from(&self.request.prompt);
            request.negative.clone_from(&self.request.negative);
        }
        self.generation_ui.model_name.clone_from(&preset.model_name);
        self.generation_ui.random_seed = preset.random_seed;
        self.generation_ui.seed_text = request.seed.to_string();
        self.request = request;
        // A preset explicitly pins its VAE together with its checkpoint.
        self.generation_ui.automatic_vae = None;
        self.set_generation_mode(self.request.mode);
        Ok(())
    }
    /// Import a JSON preset library without replacing existing names. Missing weights remain selectable.
    pub fn import_presets(&mut self, path: impl AsRef<Path>) -> Result<usize, EditError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            return Err(EditError::Invalid("Preset file exceeds 2 MiB".into()));
        }
        let mut file: PresetFile = serde_json::from_slice(&bytes)?;
        if file.version != 1 {
            return Err(EditError::Invalid("Unsupported preset version".into()));
        }
        for preset in &mut file.presets {
            preset.request.cache_dir = None;
            preset.name = preset.name.trim().into();
        }
        let count = file.presets.len();
        let mut next = self.presets.clone();
        next.extend(file.presets);
        validate_library(&next)?;
        self.presets = next;
        Ok(count)
    }
    /// Atomically export the named preset library to JSON; no model weights or credentials are included.
    pub fn export_presets(&self, path: impl AsRef<Path>) -> Result<(), EditError> {
        validate_library(&self.presets)?;
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let bytes = serde_json::to_vec_pretty(&PresetFile {
            version: 1,
            presets: self.presets.clone(),
        })?;
        if bytes.len() > MAX_BYTES {
            return Err(EditError::Invalid(
                "Exported preset file exceeds 2 MiB".into(),
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
    pub(super) fn preset_window(&mut self, ctx: &egui::Context) -> Vec<FileAction> {
        let mut actions = Vec::new();
        if !self.preset_ui.open {
            return actions;
        }
        let mut open = true;
        egui::Window::new("Пресеты генерации")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(620.0)
            .show(ctx, |ui| {
                self.preset_controls(ui, &mut actions);
            });
        self.preset_ui.open =
            open && !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        actions
    }
    fn preset_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        ui.label("Модель · LoRA и веса · VAE · сэмплер · размер · seed · параметры генерации");
        ui.horizontal(|ui| {
            ui.label("Название");
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_ui.name)
                    .desired_width(390.0)
                    .hint_text("Например: Портрет · детали и глаза"),
            );
        });
        ui.checkbox(
            &mut self.preset_ui.include_prompts,
            "Сохранить также prompt и negative prompt",
        );
        let exists = self
            .presets
            .iter()
            .any(|p| p.name == self.preset_ui.name.trim());
        if ui
            .add_enabled(
                !self.preset_ui.name.trim().is_empty(),
                egui::Button::new(if exists {
                    "Перезаписать пресет"
                } else {
                    "Сохранить текущие параметры"
                }),
            )
            .clicked()
        {
            self.preset_ui.message = match self.save_preset(
                &self.preset_ui.name.clone(),
                self.preset_ui.include_prompts,
                exists,
            ) {
                Ok(()) => "Пресет сохранён".into(),
                Err(e) => e.to_string(),
            };
        }
        ui.separator();
        let (load, remove) = self.preset_list(ui);
        if let Some(index) = load {
            self.preset_ui.message = match self.apply_preset(index) {
                Ok(()) => "Пресет применён".into(),
                Err(e) => e.to_string(),
            };
        }
        if let Some(index) = remove {
            self.presets.remove(index);
            self.preset_ui.message = "Пресет удалён".into();
        }
        self.preset_file_controls(ui, actions);
    }
    fn preset_list(&self, ui: &mut egui::Ui) -> (Option<usize>, Option<usize>) {
        let mut load = None;
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("generation-presets")
            .max_height(270.0)
            .min_scrolled_height(140.0)
            .show(ui, |ui| {
                if self.presets.is_empty() {
                    ui.weak("Сохранённых пресетов пока нет");
                }
                for (index, preset) in self.presets.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width((ui.available_width() - 170.0).max(120.0));
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&preset.name).strong())
                                        .truncate(),
                                )
                                .on_hover_text(&preset.name);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!(
                                            "{} · LoRA: {} · {} шагов",
                                            if preset.model_name.is_empty() {
                                                &preset.request.model
                                            } else {
                                                &preset.model_name
                                            },
                                            preset
                                                .request
                                                .loras
                                                .iter()
                                                .filter(|l| l.enabled)
                                                .count(),
                                            preset.request.steps
                                        ))
                                        .small(),
                                    )
                                    .truncate(),
                                );
                            });
                            if ui
                                .add_enabled(!self.settings_busy(), egui::Button::new("Применить"))
                                .clicked()
                            {
                                load = Some(index);
                            }
                            if ui.button("Удалить").clicked() {
                                remove = Some(index);
                            }
                        });
                        ui.separator();
                    });
                }
            });
        (load, remove)
    }
    fn preset_file_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        ui.horizontal(|ui| {
            if ui.button("Импорт JSON…").clicked() {
                actions.push(FileAction::ImportPresets);
            }
            if ui
                .add_enabled(!self.presets.is_empty(), egui::Button::new("Экспорт JSON…"))
                .clicked()
            {
                actions.push(FileAction::ExportPresets);
            }
        });
        ui.label(&self.preset_ui.message);
        ui.small("Веса моделей не копируются. Пресеты автоматически сохраняются между запусками.");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_mode_preset_cannot_restore_incompatible_hires() {
        let mut e = ImageEditor::default();
        e.request.mode = crate::generation::Mode::ImageToImage;
        e.request.output_size = Some([512; 2]);
        e.request.postprocess = Some(crate::generation::Postprocess {
            hires: Some(crate::generation::HiresFix {
                target_size: [1024; 2],
                ..Default::default()
            }),
            denoise: 0.8,
            ..Default::default()
        });
        e.save_preset("source mode", false, false).unwrap();
        e.request.postprocess = None;
        e.apply_preset(0).unwrap();
        let post = e.request.postprocess.as_ref().unwrap();
        assert!(post.hires.is_none());
        assert!(post.denoise <= 0.5);
        assert_eq!(e.request.mode, crate::generation::Mode::ImageToImage);
    }
    #[test]
    fn hires_settings_and_preset_roundtrip_keep_both_pass_sizes_and_refine_cfg() {
        use crate::generation::{HiresFix, Postprocess};
        let mut editor = ImageEditor::default();
        editor.request.output_size = Some([512, 512]);
        editor.request.postprocess = Some(Postprocess {
            hires: Some(HiresFix {
                target_size: [768, 768],
                guidance: 4.5,
            }),
            denoise: 0.65,
            steps: 30,
            ..Default::default()
        });
        editor.save_preset("Hires 1.5x", false, false).unwrap();
        let settings = serde_json::to_string(&editor.settings()).unwrap();
        let mut restored = ImageEditor::default();
        restored
            .apply_settings(serde_json::from_str(&settings).unwrap())
            .unwrap();
        restored.apply_preset(0).unwrap();
        assert_eq!(restored.request.postprocess, editor.request.postprocess);
        assert_eq!(restored.request.size(&restored.document), [512, 512]);
        assert_eq!(restored.request.final_size(&restored.document), [768, 768]);
        assert!(!restored.has_unsaved_changes());
    }
    #[test]
    fn preset_retains_loras_settings_and_seed_but_not_document_or_startup_cache() {
        let mut editor = ImageEditor::default();
        editor.request.model = "local/custom".into();
        editor.request.cache_dir = Some("host/cache".into());
        editor.request.loras = vec![
            crate::generation::Lora {
                path: "missing/add-detail-xl.safetensors".into(),
                weight: 0.6,
                enabled: true,
            },
            crate::generation::Lora {
                path: "missing/PerfectEyesXL.safetensors".into(),
                weight: 0.7,
                enabled: true,
            },
        ];
        editor.request.steps = 50;
        editor.generation_ui.seed_text = "18446744073709551615".into();
        editor.request.prompt = "private prompt".into();
        editor.save_preset("Eyes", false, false).unwrap();
        assert!(editor.presets[0].request.prompt.is_empty());
        assert!(editor.presets[0].request.cache_dir.is_none());
        editor.request.loras.clear();
        editor.request.steps = 5;
        editor.request.prompt = "new prompt".into();
        let doc = editor.document.clone();
        editor.apply_preset(0).unwrap();
        assert_eq!(editor.request.steps, 50);
        assert_eq!(editor.request.loras.len(), 2);
        assert_eq!(editor.request.seed, u64::MAX);
        assert_eq!(editor.request.prompt, "new prompt");
        assert_eq!(editor.request.cache_dir, Some("host/cache".into()));
        assert_eq!(editor.document, doc);
        let settings = serde_json::to_vec(&editor.settings()).unwrap();
        let mut restored = ImageEditor::default();
        restored
            .apply_settings(serde_json::from_slice(&settings).unwrap())
            .unwrap();
        assert_eq!(restored.presets, editor.presets);
        assert_eq!(restored.request.loras, editor.request.loras);
        restored.replace(crate::Document::new(128, 64).unwrap());
        assert_eq!(restored.presets, editor.presets);
    }
    #[test]
    fn explicit_prompts_export_import_and_collision_rejection_are_atomic() {
        let mut editor = ImageEditor::default();
        editor.request.prompt = "saved text".into();
        editor.save_preset("Text", true, false).unwrap();
        assert!(editor.save_preset("Text", false, false).is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        editor.export_presets(&path).unwrap();
        let mut other = ImageEditor::default();
        assert_eq!(other.import_presets(&path).unwrap(), 1);
        other.apply_preset(0).unwrap();
        assert_eq!(other.request.prompt, "saved text");
        let before = other.presets.clone();
        assert!(other.import_presets(&path).is_err());
        assert_eq!(other.presets, before);
        std::fs::write(&path, b"invalid json").unwrap();
        assert!(other.import_presets(&path).is_err());
        assert_eq!(other.presets, before);
    }
}
