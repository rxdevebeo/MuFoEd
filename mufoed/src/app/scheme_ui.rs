//! In-document diagram editing; the worker applies the confirmed scene atomically.
use eframe::egui;

#[derive(Clone)]
pub(crate) struct SchemeDialog {
    pub(super) tab: u64,
    pub(super) plan: Box<crate::worker::scheme::Plan>,
    editor: diagram_editor::DiagramEditor,
    error: Option<String>,
}

impl std::fmt::Debug for SchemeDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemeDialog")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}
impl SchemeDialog {
    pub(crate) fn new(
        tab: u64,
        plan: Box<crate::worker::scheme::Plan>,
        language: &str,
    ) -> Result<Self, String> {
        let locale = diagram_editor::Locale::new(language);
        let mut source = plan.diagram.clone();
        if plan.is_new() {
            for node in &mut source.nodes {
                node.text = locale.text(&node.text).to_owned();
            }
        }
        let mut editor = diagram_editor::DiagramEditor::new(source)?;
        editor.set_locale(locale);
        Ok(Self {
            tab,
            plan,
            editor,
            error: None,
        })
    }
    pub(crate) fn show(&mut self, ctx: &egui::Context, i18n: &crate::i18n::I18n) -> (bool, bool) {
        self.editor
            .set_locale(diagram_editor::Locale::new(i18n.language()));
        let locale = self.editor.locale().clone();
        let size = ctx.input(|i| i.content_rect().size());
        let width = (size.x - 64.0).clamp(600.0, 1140.0);
        let height = (size.y - 80.0).clamp(360.0, 760.0);
        let mut apply = false;
        let mut close = false;
        egui::Modal::new(egui::Id::new("scheme-editor")).show(ctx, |ui| {
            crate::theme::style_buttons(ui);
            ui.set_width(width);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);
            ui.heading(i18n.t("scheme.title"));
            ui.label(i18n.t("scheme.storage_note"));
            ui.horizontal(|ui| {
                if ui.button(i18n.t("scheme.load_json")).clicked() {
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
                                .replace(diagram_editor::Diagram::from_json(&text)?)?;
                            Ok(())
                        })();
                        self.error = result.err();
                    }
                }
                if ui.button(i18n.t("scheme.save_json")).clicked() {
                    self.editor.commit();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .set_file_name("diagram.json")
                        .save_file()
                    {
                        self.error = self
                            .editor
                            .diagram()
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
                .id_salt("scheme-editor-scroll")
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
                    !self.editor.diagram().nodes.is_empty(),
                    i18n.t("button.apply"),
                )
                .clicked()
                {
                    self.editor.commit();
                    match self.editor.diagram().validate() {
                        Ok(()) => {
                            self.plan.diagram = self.editor.diagram().clone();
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
    fn find(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<egui::Rect> {
        fn text(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| text(s, label)),
                _ => None,
            }
        }
        shapes.iter().find_map(|s| text(&s.shape, label))
    }
    #[test]
    fn modal_buttons_fit_small_window_and_apply_or_cancel() {
        for (size, label, expected) in [
            (egui::vec2(720.0, 480.0), "Применить", (false, true)),
            (egui::vec2(1280.0, 860.0), "Отмена", (false, false)),
        ] {
            let ctx = egui::Context::default();
            let mut i18n = crate::i18n::I18n::load(&[]);
            i18n.set_language("ru");
            let mut dialog =
                SchemeDialog::new(1, Box::new(crate::worker::scheme::preview_plan()), "ru")
                    .unwrap();
            let mut state = (true, false);
            let mut frame = |events| {
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        state = dialog.show(ui.ctx(), &i18n);
                    },
                )
            };
            frame(vec![]);
            frame(vec![]);
            let output = frame(vec![]);
            let rect = find(&output.shapes, label).expect("localized action is visible");
            assert!(
                egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(rect),
                "{size:?}: {rect:?}"
            );
            let p = rect.center();
            let button = |pressed| egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            frame(vec![egui::Event::PointerMoved(p), button(true)]);
            frame(vec![button(false)]);
            assert_eq!(state, expected);
        }
    }
}
