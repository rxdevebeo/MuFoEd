//! Centered canvas and rotation dialogs; edits commit only on Apply.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
use super::ImageEditor;
use crate::{CanvasMode, EditError};

pub(super) struct GeometryDialog {
    kind: Option<bool>, // false: canvas, true: rotation
    size: [u32; 2],
    source: [u32; 2],
    proportional: bool,
    mode: CanvasMode,
    anchor: [u8; 2],
    angle: f64,
    expand: bool,
    error: String,
}
impl Default for GeometryDialog {
    fn default() -> Self {
        Self {
            kind: None,
            size: [1, 1],
            source: [1, 1],
            proportional: true,
            mode: CanvasMode::Scale,
            anchor: [1, 1],
            angle: 0.0,
            expand: true,
            error: String::new(),
        }
    }
}
impl ImageEditor {
    /// Open a canvas sizing dialog without changing project pixels.
    pub fn open_canvas_dialog(&mut self) {
        self.finish_manual_gesture();
        self.geometry = GeometryDialog {
            kind: Some(false),
            size: [self.document.width(), self.document.height()],
            source: [self.document.width(), self.document.height()],
            ..Default::default()
        };
    }
    /// Open a clockwise rotation dialog, initially preserving all image corners.
    pub fn open_rotation_dialog(&mut self) {
        self.finish_manual_gesture();
        self.geometry = GeometryDialog {
            kind: Some(true),
            ..Default::default()
        };
    }
    pub(super) fn geometry_dialog_open(&self) -> bool {
        self.geometry.kind.is_some()
    }
    pub(super) fn rotation_preview(&self) -> Option<(f64, bool)> {
        (self.geometry.kind == Some(true)).then_some((self.geometry.angle, self.geometry.expand))
    }
    pub(super) fn geometry_window(&mut self, ctx: &egui::Context) {
        let Some(rotation) = self.geometry.kind else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(if rotation {
            "Поворот изображения"
        } else {
            "Размер холста"
        })
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(430.0)
        .show(ctx, |ui| {
            let g = &mut self.geometry;
            let before = (g.angle.to_bits(), g.expand);
            g.controls(ui, rotation);
            if before != (g.angle.to_bits(), g.expand) {
                ctx.request_repaint();
            }
            ui.separator();
            ui.small("Все слои и маска · отмена одним шагом");
            if !g.error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &g.error);
            }
            ui.horizontal(|ui| {
                apply = ui.button("Применить").clicked();
                cancel = ui.button("Отмена").clicked();
            });
        });
        cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if !open || cancel {
            ctx.request_repaint();
            self.geometry.kind = None;
        } else if apply {
            ctx.request_repaint();
            let mut next = self.document.clone();
            let g = &self.geometry;
            let result: Result<(), EditError> = if rotation {
                next.rotate(g.angle, g.expand)
            } else {
                next.resize_canvas(g.size[0], g.size[1], g.mode, g.anchor)
            };
            match result {
                Ok(()) => {
                    self.edit(|d| {
                        *d = next;
                        Ok(())
                    });
                    self.geometry.kind = None;
                    self.manual_ui.fit_once = true;
                }
                Err(e) => self.geometry.error = e.to_string(),
            }
        }
    }
}
impl GeometryDialog {
    #[allow(clippy::too_many_lines)] // One cohesive immediate-mode form.
    fn controls(&mut self, ui: &mut egui::Ui, rotation: bool) {
        let g = self;
        if rotation {
            ui.horizontal(|ui| {
                ui.label("Угол по часовой стрелке");
                ui.add(
                    egui::DragValue::new(&mut g.angle)
                        .range(-360.0..=360.0)
                        .speed(0.01)
                        .fixed_decimals(2)
                        .suffix("°"),
                );
            });
            ui.small("Предпросмотр на холсте · окно можно перетащить за заголовок");
            ui.checkbox(&mut g.expand, "Расширить холст, сохранив углы изображения");
            ui.small("Свободные области будут прозрачными. Без расширения края обрезаются.");
        } else {
            ui.label(format!(
                "Исходный размер: {} × {} px",
                g.source[0], g.source[1]
            ));
            ui.horizontal(|ui| {
                ui.label("Ширина");
                if ui
                    .add(
                        egui::DragValue::new(&mut g.size[0])
                            .range(1..=16384)
                            .suffix(" px"),
                    )
                    .changed()
                    && g.proportional
                {
                    g.size[1] = ratio(g.size[0], g.source[1], g.source[0]);
                }
                ui.label("Высота");
                if ui
                    .add(
                        egui::DragValue::new(&mut g.size[1])
                            .range(1..=16384)
                            .suffix(" px"),
                    )
                    .changed()
                    && g.proportional
                {
                    g.size[0] = ratio(g.size[1], g.source[0], g.source[1]);
                }
            });
            if ui
                .checkbox(&mut g.proportional, "Сохранять пропорции размеров")
                .changed()
                && g.proportional
            {
                g.size[1] = ratio(g.size[0], g.source[1], g.source[0]);
            }
            egui::ComboBox::from_id_salt("canvas-mode")
                .selected_text(mode_label(g.mode))
                .show_ui(ui, |ui| {
                    for mode in [
                        CanvasMode::Scale,
                        CanvasMode::Fit,
                        CanvasMode::Fill,
                        CanvasMode::Bounds,
                    ] {
                        ui.selectable_value(&mut g.mode, mode, mode_label(mode));
                    }
                });
            ui.small(match g.mode {
                CanvasMode::Scale => "Масштабировать все слои до указанного размера.",
                CanvasMode::Fit => "Вписать изображение с сохранением пропорций; поля прозрачные.",
                CanvasMode::Fill => {
                    "Заполнить холст с сохранением пропорций; лишние края обрезать."
                }
                CanvasMode::Bounds => {
                    "Обрезать края или добавить прозрачные поля без изменения масштаба."
                }
            });
            ui.label("Привязка изображения");
            egui::Grid::new("canvas-anchor")
                .spacing([4.0, 4.0])
                .show(ui, |ui| {
                    for y in 0..3 {
                        for x in 0..3 {
                            if ui
                                .add_sized(
                                    [34.0, 28.0],
                                    egui::Button::new(if g.anchor == [x, y] { "+" } else { "·" })
                                        .selected(g.anchor == [x, y]),
                                )
                                .clicked()
                            {
                                g.anchor = [x, y];
                            }
                        }
                        ui.end_row();
                    }
                });
        }
    }
}
fn ratio(value: u32, numerator: u32, denominator: u32) -> u32 {
    ((u64::from(value) * u64::from(numerator) + u64::from(denominator) / 2)
        / u64::from(denominator))
    .clamp(1, 16384) as u32
}
fn mode_label(mode: CanvasMode) -> &'static str {
    match mode {
        CanvasMode::Scale => "Масштабировать изображение",
        CanvasMode::Fit => "Вписать с полями",
        CanvasMode::Fill => "Заполнить и обрезать",
        CanvasMode::Bounds => "Только границы холста",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiny_canvas_rotation_changes_gpu_vertices_without_touching_document() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::new(crate::Document::new(64, 32).unwrap());
        let original = editor.document.clone();
        editor.open_rotation_dialog();
        let mut corners = Vec::new();
        for angle in [0.0, 0.05] {
            editor.geometry.angle = angle;
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                editor.texture = Some(ui.ctx().load_texture(
                    "canvas",
                    egui::ColorImage::filled([64, 32], egui::Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                ));
                let rect = egui::Rect::from_center_size(
                    egui::pos2(200.0, 200.0),
                    editor.preview_canvas_size(),
                );
                assert!(editor.paint_transform_preview(ui, ui.painter(), rect));
            });
            let corner = out
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Mesh(mesh) = &s.shape {
                        Some(mesh.vertices[0].pos)
                    } else {
                        None
                    }
                })
                .unwrap();
            corners.push(corner);
        }
        assert!((corners[0] - corners[1]).length() > 0.01);
        assert_eq!(editor.document, original);
        assert!(!editor.unsaved);
        assert!(!editor.history.undo(&mut editor.document));
    }
}
