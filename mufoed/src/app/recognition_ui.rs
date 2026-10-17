//! Review the recognized MathML before replacing its source picture.
use eframe::egui;

#[derive(Clone)]
pub(crate) struct RecognitionDialog {
    pub(super) tab: u64,
    pub(super) plan: Box<crate::worker::FormulaRecognition>,
    preview: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for RecognitionDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecognitionDialog")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl RecognitionDialog {
    pub(crate) fn new(
        ctx: &egui::Context,
        tab: u64,
        plan: Box<crate::worker::FormulaRecognition>,
    ) -> Self {
        let preview = strict_ooxml::math_from_mathml(&plan.mathml)
            .ok()
            .and_then(|expression| crate::formula::preview(&expression))
            .map(|(w, h, rgba)| {
                ctx.load_texture(
                    "recognized-formula",
                    egui::ColorImage::from_rgba_premultiplied([w, h], &rgba),
                    egui::TextureOptions::LINEAR,
                )
            });
        Self { tab, plan, preview }
    }

    pub(crate) fn show(&self, ctx: &egui::Context, i18n: &crate::i18n::I18n) -> (bool, bool) {
        let mut confirmed = false;
        let response = crate::dialog::modal(ctx, "formula-recognition").show(ctx, |ui| {
            ui.set_width(560.0);
            crate::dialog::title(ui, i18n.t("recognition.title"));
            ui.label(i18n.t("recognition.explanation"));
            if let Some(texture) = &self.preview {
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .show(ui, |ui| {
                        ui.add(egui::Image::new(texture).max_width(540.0).max_height(180.0));
                    });
            }
            ui.label("MathML");
            egui::ScrollArea::both().max_height(220.0).show(ui, |ui| {
                let mut source = self.plan.mathml.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut source)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(535.0)
                        .desired_rows(7),
                );
            });
            if ui.button(i18n.t("recognition.copy")).clicked() {
                ctx.copy_text(self.plan.mathml.clone());
            }
            crate::dialog::buttons(ui, |ui| {
                if crate::dialog::primary(ui, i18n.t("recognition.convert")).clicked() {
                    confirmed = true;
                    ui.close();
                }
                if crate::dialog::button(ui, i18n.t("button.cancel")).clicked() {
                    ui.close();
                }
            });
        });
        (response.should_close() || confirmed, confirmed)
    }
}
