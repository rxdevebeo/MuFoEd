//! Layer-only transforms and photo correction, committed as a single undo step.
use super::ImageEditor;
use crate::{ColorCorrection, EditError, Layer, PhotoGeometry, Raster};

pub(super) struct LayerDialog {
    kind: Option<bool>, // true: color, false: transform
    source: Option<Layer>,
    index: usize,
    correction: ColorCorrection,
    angle: f64,
    photo: PhotoGeometry,
    center: [f64; 2],
    scale: [f64; 2],
    proportional: bool,
    preview: Option<(Raster, egui::TextureHandle, egui::TextureHandle)>,
    error: String,
}
impl Default for LayerDialog {
    fn default() -> Self {
        Self {
            kind: None,
            source: None,
            index: 0,
            correction: ColorCorrection::default(),
            angle: 0.0,
            photo: PhotoGeometry::default(),
            center: [0.0; 2],
            scale: [100.0; 2],
            proportional: true,
            preview: None,
            error: String::new(),
        }
    }
}
impl ImageEditor {
    /// Import a centered raster layer without replacing the document; one undo step.
    pub fn import_layer_file(&mut self, path: &std::path::Path) -> Result<(), EditError> {
        self.finish_manual_stroke();
        let raster = Raster::open(path)?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut next = self.document.clone();
        next.import_layer(name, &raster)?;
        self.edit(|d| {
            *d = next;
            Ok(())
        });
        self.status = "Слой загружен · крупное изображение вписано в холст".into();
        Ok(())
    }
    /// Open a color correction dialog for the current layer.
    pub fn open_color_dialog(&mut self) {
        self.open_layer_dialog(true);
    }
    /// Open a layer transform dialog; the canvas and other layers keep their size.
    pub fn open_layer_transform_dialog(&mut self) {
        self.open_layer_dialog(false);
    }
    pub(super) fn open_layer_dialog(&mut self, color: bool) {
        self.finish_manual_stroke();
        self.layer_dialog = LayerDialog {
            kind: Some(color),
            index: self.document.active(),
            center: self.document.active_layer_center(),
            source: Some(self.document.layers()[self.document.active()].clone()),
            ..Default::default()
        };
    }
    pub(super) fn layer_preview(&self) -> Option<(f64, [f64; 2], PhotoGeometry, [f64; 2])> {
        let g = &self.layer_dialog;
        (g.kind == Some(false) && self.document.active() == g.index).then_some((
            g.angle,
            g.scale.map(|v| v / 100.0),
            g.photo,
            g.center,
        ))
    }
    pub(super) fn layer_window(&mut self, ctx: &egui::Context) {
        let Some(color) = self.layer_dialog.kind else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(if color {
            "Цветокоррекция слоя"
        } else {
            "Трансформация слоя"
        })
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(470.0)
        .show(ctx, |ui| {
            let g = &mut self.layer_dialog;
            let before = (g.angle.to_bits(), g.scale.map(f64::to_bits), g.photo);
            if let Some(source) = &g.source {
                ui.label(source.name());
            }
            egui::ScrollArea::vertical()
                .id_salt("layer-dialog-controls")
                .max_height((ctx.content_rect().height() - 240.0).max(180.0))
                .show(ui, |ui| {
                    if color {
                        g.color_controls(ui);
                    } else {
                        g.transform_controls(ui);
                    }
                });
            if before != (g.angle.to_bits(), g.scale.map(f64::to_bits), g.photo) {
                ctx.request_repaint();
            }
            ui.separator();
            ui.small("Только текущий слой · отмена одним шагом");
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
            self.layer_dialog = LayerDialog::default();
        } else if apply {
            ctx.request_repaint();
            self.commit_layer_dialog();
        }
    }
    fn commit_layer_dialog(&mut self) {
        let g = &self.layer_dialog;
        if self.document.active() != g.index
            || self.document.layers().get(g.index) != g.source.as_ref()
        {
            self.layer_dialog.error = "Слой изменился. Закройте окно и откройте его заново.".into();
            return;
        }
        let mut next = self.document.clone();
        let result = if g.kind == Some(true) {
            next.correct_color(g.correction)
        } else {
            next.transform_photo_layer(g.angle, g.scale.map(|v| v / 100.0), g.photo)
        };
        match result {
            Ok(()) => {
                self.edit(|d| {
                    *d = next;
                    Ok(())
                });
                self.layer_dialog = LayerDialog::default();
            }
            Err(e) => self.layer_dialog.error = e.to_string(),
        }
    }
}
impl LayerDialog {
    fn transform_controls(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::DragValue::new(&mut self.angle)
                .range(-360.0..=360.0)
                .speed(0.01)
                .fixed_decimals(2)
                .prefix("Угол ")
                .suffix("°"),
        );
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::DragValue::new(&mut self.scale[0])
                        .range(1.0..=1600.0)
                        .prefix("Ширина ")
                        .suffix("%"),
                )
                .changed()
                && self.proportional
            {
                self.scale[1] = self.scale[0];
            }
            if ui
                .add(
                    egui::DragValue::new(&mut self.scale[1])
                        .range(1.0..=1600.0)
                        .prefix("Высота ")
                        .suffix("%"),
                )
                .changed()
                && self.proportional
            {
                self.scale[0] = self.scale[1];
            }
        });
        if ui
            .checkbox(&mut self.proportional, "Сохранять пропорции")
            .changed()
            && self.proportional
        {
            self.scale[1] = self.scale[0];
        }
        ui.small("Предпросмотр на холсте · окно можно перетащить за заголовок");
        ui.small("Центр — середина содержимого слоя. Холст не расширяется; выходящие за него края обрезаются.");
        egui::CollapsingHeader::new("Перспектива и объектив").default_open(true).show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.photo.horizontal,-0.4..=0.4).step_by(0.001).text("Перспектива X"));
            ui.add(egui::Slider::new(&mut self.photo.vertical,-0.4..=0.4).step_by(0.001).text("Перспектива Y"));
            ui.add(egui::Slider::new(&mut self.photo.shear,-0.5..=0.5).step_by(0.001).text("Скос"));
            ui.add(egui::Slider::new(&mut self.photo.radial,-0.5..=0.5).step_by(0.001).text("Дисторсия k₁"));
            ui.add(egui::Slider::new(&mut self.photo.radial2,-0.25..=0.25).step_by(0.001).text("Дисторсия k₂"));
            ui.small("Вручную выровняйте вертикали, горизонтали и кривизну линий. Профили объективов не используются.");
        });
        if ui.button("Сбросить").clicked() {
            self.photo = PhotoGeometry::default();
            self.angle = 0.0;
            self.scale = [100.0; 2];
        }
    }
    fn color_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.correction;
        let c = &mut self.correction;
        ui.add(egui::Slider::new(&mut c.exposure, -5.0..=5.0).text("Экспозиция · EV"));
        ui.add(egui::Slider::new(&mut c.brightness, -1.0..=1.0).text("Яркость"));
        ui.add(egui::Slider::new(&mut c.contrast, 0.0..=3.0).text("Контраст"));
        ui.add(egui::Slider::new(&mut c.saturation, 0.0..=3.0).text("Насыщенность"));
        ui.add(egui::Slider::new(&mut c.hue, -180.0..=180.0).text("Цветовой тон"));
        ui.add(egui::Slider::new(&mut c.temperature, -1.0..=1.0).text("Температура"));
        ui.add(egui::Slider::new(&mut c.gamma, 0.1..=5.0).text("Гамма"));
        if ui.button("Сбросить").clicked() {
            self.correction = ColorCorrection::default();
        }
        if self.preview.is_none() {
            if let Some(source) = &self.source {
                match source.raster().thumbnail(220) {
                    Ok(raster) => {
                        let original = preview_texture(ui.ctx(), "color-original", &raster);
                        let corrected = preview_texture(ui.ctx(), "color-corrected", &raster);
                        self.preview = Some((raster, original, corrected));
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
        }
        if let Some((raster, original, corrected)) = &mut self.preview {
            if before != self.correction {
                match self.correction.apply(raster) {
                    Ok(result) => {
                        *corrected = preview_texture(ui.ctx(), "color-corrected", &result);
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
            ui.horizontal(|ui| {
                for (label, texture) in [("До", original), ("После", corrected)] {
                    ui.vertical(|ui| {
                        ui.label(label);
                        let size = texture.size_vec2();
                        ui.image((texture.id(), size * (200.0 / size.x.max(size.y)).min(1.0)));
                    });
                }
            });
        }
    }
}
fn preview_texture(ctx: &egui::Context, name: &str, raster: &Raster) -> egui::TextureHandle {
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [raster.width() as usize, raster.height() as usize],
            raster.pixels(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preview(ctx: &egui::Context, editor: &mut ImageEditor) -> egui::Pos2 {
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(256.0, 256.0));
            let painter = ui.painter().with_clip_rect(rect);
            assert!(editor.paint_transform_preview(ui, &painter, rect));
        });
        out.shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Mesh(mesh) = &s.shape {
                    (mesh.vertices.len() > 4).then(|| mesh.vertices[0].uv)
                } else {
                    None
                }
            })
            .expect("warped mesh")
    }
    #[test]
    fn small_rotation_updates_mesh_without_mutation_and_commits_from_original_once() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::new(
            crate::Document::from_raster(Raster::solid(16, 16, [140, 70, 30, 255]).unwrap())
                .unwrap(),
        );
        let original = editor.document.clone();
        editor.open_layer_transform_dialog();
        let uv0 = preview(&ctx, &mut editor);
        let texture = editor.transform_preview.layers[0].id();
        editor.layer_dialog.angle = 0.05;
        let uv1 = preview(&ctx, &mut editor);
        assert!((uv1 - uv0).length() > 0.0001);
        assert_eq!(editor.transform_preview.layers[0].id(), texture);
        editor.layer_dialog.angle = 0.1;
        let _ = preview(&ctx, &mut editor);
        assert_eq!(editor.document, original);
        assert!(!editor.unsaved);
        let mut expected = original.clone();
        expected.transform_layer(0.1, [1.0; 2]).unwrap();
        editor.commit_layer_dialog();
        assert_eq!(editor.document, expected);
        assert!(editor.history.undo(&mut editor.document));
        assert_eq!(editor.document, original);
        assert!(!editor.history.undo(&mut editor.document));
    }
    #[test]
    fn escape_cancels_preview_and_stale_source_is_not_overwritten() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::default();
        let original = editor.document.clone();
        editor.open_layer_transform_dialog();
        editor.layer_dialog.angle = 0.05;
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| {
                editor.layer_window(ui.ctx());
            },
        );
        assert!(!editor.transform_preview_active());
        assert_eq!(editor.document, original);
        assert!(!editor.unsaved);
        editor.open_layer_transform_dialog();
        editor.document.set_layer("Changed".into(), true, 255);
        let changed = editor.document.clone();
        editor.commit_layer_dialog();
        assert_eq!(editor.document, changed);
        assert!(!editor.layer_dialog.error.is_empty());
    }
}
