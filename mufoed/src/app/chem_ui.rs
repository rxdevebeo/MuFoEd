//! In-document chemistry editing; the worker applies the confirmed document
//! atomically through the drawing.
use eframe::egui;

#[derive(Clone)]
pub(crate) struct ChemDialog {
    pub(super) tab: u64,
    pub(super) plan: Box<crate::worker::chem::Plan>,
    editor: chem_editor::ChemEditor,
    error: Option<String>,
}

impl std::fmt::Debug for ChemDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChemDialog")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl ChemDialog {
    pub(crate) fn new(
        tab: u64,
        plan: Box<crate::worker::chem::Plan>,
        language: &str,
    ) -> Result<Self, String> {
        let mut editor = chem_editor::ChemEditor::new(plan.document.clone())?;
        editor.set_locale(chem_editor::Locale::new(language));
        Ok(Self {
            tab,
            plan,
            editor,
            error: None,
        })
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, i18n: &crate::i18n::I18n) -> (bool, bool) {
        self.editor
            .set_locale(chem_editor::Locale::new(i18n.language()));
        let locale = self.editor.locale().clone();
        let size = ctx.input(|i| i.content_rect().size());
        let width = (size.x - 64.0).clamp(600.0, 1140.0);
        let height = (size.y - 80.0).clamp(360.0, 760.0);
        let mut apply = false;
        let mut close = false;
        egui::Modal::new(egui::Id::new("chemistry-editor")).show(ctx, |ui| {
            crate::theme::style_buttons(ui);
            ui.set_width(width);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);
            ui.heading(i18n.t("chemistry.title"));
            ui.label(i18n.t("chemistry.storage_note"));
            ui.horizontal(|ui| {
                if ui.button(i18n.t("chemistry.load_json")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    {
                        use std::io::Read as _;
                        let result = (|| -> Result<(), String> {
                            let mut text = String::new();
                            std::fs::File::open(path)
                                .map_err(|e| e.to_string())?
                                .take(8 * 1024 * 1024 + 1)
                                .read_to_string(&mut text)
                                .map_err(|e| e.to_string())?;
                            self.editor
                                .replace(chem_editor::ChemDocument::from_json(&text)?)?;
                            Ok(())
                        })();
                        self.error = result.err();
                    }
                }
                if ui.button(i18n.t("chemistry.save_json")).clicked() {
                    self.editor.commit();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .set_file_name("chemistry.json")
                        .save_file()
                    {
                        self.error = self
                            .editor
                            .document()
                            .to_json()
                            .and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string()))
                            .err();
                    }
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, locale.diagnostic(error));
            }
            if !self.plan.issues.is_empty() {
                ui.small(
                    self.plan
                        .issues
                        .iter()
                        .map(|s| locale.diagnostic(s))
                        .collect::<Vec<_>>()
                        .join("; "),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt("chemistry-editor-scroll")
                .max_height(height - 130.0)
                .show(ui, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, height - 130.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            self.editor.show(ui);
                        },
                    );
                });
            ui.separator();
            ui.horizontal(|ui| {
                if crate::dialog::primary_enabled(
                    ui,
                    !self.editor.document().objects.is_empty(),
                    i18n.t("button.apply"),
                )
                .clicked()
                {
                    self.editor.commit();
                    match self.editor.document().validate() {
                        Ok(()) => {
                            self.plan.document = self.editor.document().clone();
                            apply = true;
                            close = true;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                if crate::dialog::button(ui, i18n.t("button.cancel")).clicked() {
                    close = true;
                }
            });
        });
        (!close, apply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_opens_on_the_sample_and_renders_a_frame() {
        let ctx = egui::Context::default();
        let i18n = crate::i18n::I18n::load(&[]);
        let mut dialog =
            ChemDialog::new(1, Box::new(crate::worker::chem::preview_plan()), "en").unwrap();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                let _ = dialog.show(ui.ctx(), &i18n);
            },
        );
    }
}
