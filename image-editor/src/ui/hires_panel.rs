//! Optional two-pass generation, distinct from historical same-size postprocessing.
use super::ImageEditor;
use crate::generation::{HiresFix, Mode};
impl ImageEditor {
    pub(super) fn disable_hires_for_source_mode(&mut self) {
        if self.request.mode != Mode::TextToImage {
            if let Some(post) = &mut self.request.postprocess {
                if post.hires.take().is_some() {
                    post.denoise = post.denoise.min(0.5);
                    self.status =
                        "Hires Fix отключён для img2img/inpaint; выбрана обработка без увеличения"
                            .into();
                }
            }
        }
    }
    fn select_post_mode(&mut self, mode: u8, base: [u32; 2]) {
        if mode == 2 {
            self.request.output_size = Some(base);
        }
        if mode == 0 {
            self.request.postprocess = None;
        } else {
            let mut post = self.request.postprocess.take().unwrap_or_default();
            if !self.upscale_ui.ultrasharp.is_empty() {
                post.ultrasharp = self.upscale_ui.ultrasharp.clone().into();
            }
            post.hires = if mode == 2 {
                post.denoise = 0.3;
                Some(HiresFix {
                    target_size: scaled(base, 3, 2),
                    guidance: self.request.guidance,
                })
            } else {
                post.denoise = post.denoise.min(0.5);
                None
            };
            self.request.postprocess = Some(post);
        }
    }
    pub(super) fn hires_ui(&mut self, ui: &mut egui::Ui) {
        let base = self.request.size(&self.document);
        let mut mode =
            self.request
                .postprocess
                .as_ref()
                .map_or(0, |p| if p.hires.is_some() { 2 } else { 1 });
        let previous = mode;
        egui::ComboBox::from_id_salt("generation-post-mode")
            .width(ui.available_width())
            .selected_text(match mode {
                0 => "Без постобработки",
                1 => "Img2Img → UltraSharp · 1×",
                _ => "Hires Fix · UltraSharp → Img2Img",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, 0, "Без постобработки");
                ui.selectable_value(&mut mode, 1, "Img2Img → UltraSharp · без увеличения");
                ui.add_enabled_ui(self.request.mode == Mode::TextToImage, |ui| {
                    ui.selectable_value(&mut mode, 2, "Hires Fix · UltraSharp → Img2Img");
                });
            });
        if mode != previous {
            self.select_post_mode(mode, base);
        }
        let Some(post) = &mut self.request.postprocess else {
            return;
        };
        let Some(hires) = &mut post.hires else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            for (n, d, label) in [(3, 2, "1,5×"), (2, 1, "2×"), (3, 1, "3×"), (4, 1, "4×")] {
                let size = scaled(base, n, d);
                if ui
                    .add_enabled(
                        u64::from(size[0]) * u64::from(size[1]) <= 16_777_216,
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    hires.target_size = size;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut hires.target_size[0])
                    .range(8..=16384)
                    .speed(8)
                    .prefix("W "),
            );
            ui.label("×");
            ui.add(
                egui::DragValue::new(&mut hires.target_size[1])
                    .range(8..=16384)
                    .speed(8)
                    .prefix("H "),
            );
        });
        ui.add(
            egui::Slider::new(&mut post.denoise, 0.0..=1.0)
                .step_by(0.01)
                .text("Denoise · 2-й проход"),
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut post.steps)
                    .range(1..=150)
                    .prefix("Steps "),
            );
            ui.add(
                egui::DragValue::new(&mut hires.guidance)
                    .range(0.0..=30.0)
                    .speed(0.1)
                    .prefix("CFG "),
            );
        });
        ui.small(format!(
            "{}×{} → {}×{} · та же модель, LoRA, VAE и seed",
            base[0], base[1], hires.target_size[0], hires.target_size[1]
        ));
        ui.small("Размеры кратны 8, увеличение до 4×. Веса UltraSharp — в Настройках → Генерация.");
    }
}
fn scaled(base: [u32; 2], n: u32, d: u32) -> [u32; 2] {
    base.map(|v| (v.saturating_mul(n) / d).saturating_add(4) / 8 * 8)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::Postprocess;
    #[test]
    fn source_modes_disable_hires_without_changing_model_settings() {
        let mut editor = ImageEditor::default();
        editor.request.postprocess = Some(Postprocess {
            hires: Some(HiresFix::default()),
            denoise: 0.8,
            ..Default::default()
        });
        let model = editor.request.model.clone();
        editor.set_generation_mode(Mode::ImageToImage);
        let post = editor.request.postprocess.as_ref().unwrap();
        assert!(post.hires.is_none());
        assert!(post.denoise <= 0.5);
        assert_eq!(editor.request.model, model);
    }
}
