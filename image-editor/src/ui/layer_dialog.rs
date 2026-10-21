//! Layer-only transforms and photo correction, committed as a single undo step.
use super::{EditTarget, ImageEditor};
use crate::{ColorCorrection, EditError, Layer, PhotoGeometry, Raster};

pub(super) struct LayerDialog {
    kind: Option<bool>, // true: color, false: transform
    source: Option<Layer>,
    target: EditTarget,
    mask_source: Option<Vec<u8>>,
    index: usize,
    correction: ColorCorrection,
    angle: f64,
    photo: PhotoGeometry,
    center: [f64; 2],
    scale: [f64; 2],
    proportional: bool,
    inline: bool,
    bounds: [f64; 4],
    drag: Option<TransformDrag>,
    preview: Option<(Raster, egui::TextureHandle, egui::TextureHandle)>,
    error: String,
}
struct TransformDrag {
    handle: usize,
    start: egui::Pos2,
    angle: f64,
    scale: [f64; 2],
}
impl Default for LayerDialog {
    fn default() -> Self {
        Self {
            kind: None,
            target: EditTarget::Raster,
            mask_source: None,
            source: None,
            index: 0,
            correction: ColorCorrection::default(),
            angle: 0.0,
            photo: PhotoGeometry::default(),
            center: [0.0; 2],
            scale: [100.0; 2],
            proportional: true,
            inline: false,
            bounds: [0.0; 4],
            drag: None,
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
        self.finish_pasted_object();
        let mask = self.edit_target() == EditTarget::Mask;
        let mask_document = mask.then(|| {
            crate::Document::from_raster(self.document.mask_raster()).expect("valid mask")
        });
        let source = mask_document.as_ref().unwrap_or(&self.document);
        self.changed = true;
        let raster = source.layers()[source.active()].raster();
        let mut bounds = [raster.width(), raster.height(), 0, 0];
        for (i, p) in raster.pixels().chunks_exact(4).enumerate() {
            if (if mask { p[0] } else { p[3] }) > 0 {
                let x = u32::try_from(i % raster.width() as usize).expect("canvas");
                let y = u32::try_from(i / raster.width() as usize).expect("canvas");
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        if bounds[2] == 0 {
            bounds = [0, 0, raster.width(), raster.height()];
        }
        self.layer_dialog = LayerDialog {
            kind: Some(color),
            bounds: bounds.map(f64::from),
            target: self.edit_target(),
            mask_source: mask.then(|| self.document.mask().to_vec()),
            index: self.document.active(),
            center: [
                f64::from(bounds[0] + bounds[2]) / 2.0,
                f64::from(bounds[1] + bounds[3]) / 2.0,
            ],
            source: Some(source.layers()[source.active()].clone()),
            ..Default::default()
        };
    }
    /// Begin on-canvas transform with ribbon controls and no blocking dialog.
    pub fn start_canvas_transform(&mut self) {
        self.select_manual_tool(super::ManualTool::Move);
        self.open_layer_dialog(false);
        self.layer_dialog.inline = true;
        self.ribbon_ui.tab = super::ribbon_ui::RibbonTab::Tool;
        self.ribbon_ui.collapsed = false;
    }
    pub(super) fn layer_dialog_open(&self) -> bool {
        self.layer_dialog.kind.is_some()
    }
    pub(super) fn inline_transform_shortcuts(&mut self, ui: &egui::Ui) {
        if self.inline_transform() && self.ribbon_ui.tab != super::ribbon_ui::RibbonTab::Tool {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.layer_dialog = LayerDialog::default();
                self.changed = true;
            } else if !ui.ctx().egui_wants_keyboard_input()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                self.commit_layer_dialog();
            }
        }
    }
    pub(super) fn inline_transform(&self) -> bool {
        self.layer_dialog.inline && self.layer_dialog.kind == Some(false)
    }
    pub(super) fn transform_ribbon(&mut self, ui: &mut egui::Ui) {
        use super::ribbon_ui::group;
        let mut apply = false;
        let mut cancel = false;
        let g = &mut self.layer_dialog;
        let before = g.scale;
        ui.horizontal_top(|ui| {
            group(ui, "Поворот", 155.0, |ui| {
                ui.add(egui::DragValue::new(&mut g.angle).speed(0.05).suffix("°"));
                if ui.button("Сбросить угол").clicked() {
                    g.angle = 0.0;
                }
                ui.small("Shift: шаг15°");
            });
            group(ui, "Размер", 230.0, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Ширина");
                    ui.add(
                        egui::DragValue::new(&mut g.scale[0])
                            .range(1.0..=1600.0)
                            .suffix("%"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Высота");
                    ui.add(
                        egui::DragValue::new(&mut g.scale[1])
                            .range(1.0..=1600.0)
                            .suffix("%"),
                    );
                });
                ui.checkbox(&mut g.proportional, "Пропорции");
            });
            group(ui, "Размещение", 190.0, |ui| {
                apply = ui.button("Применить · Enter").clicked();
                cancel = ui.button("Отмена · Esc").clicked();
                ui.small("Маркеры на холсте");
            });
        });
        if g.proportional {
            if g.scale[0].to_bits() != before[0].to_bits() {
                g.scale[1] = (before[1] * g.scale[0] / before[0]).clamp(1.0, 1600.0);
            } else if g.scale[1].to_bits() != before[1].to_bits() {
                g.scale[0] = (before[0] * g.scale[1] / before[1]).clamp(1.0, 1600.0);
            }
        }
        if !g.error.is_empty() {
            ui.colored_label(egui::Color32::LIGHT_RED, &g.error);
        }
        apply |= ui.input(|i| i.key_pressed(egui::Key::Enter));
        cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
        if cancel {
            self.layer_dialog = LayerDialog::default();
            self.changed = true;
        } else if apply {
            self.commit_layer_dialog();
        }
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::too_many_lines,
        clippy::many_single_char_names
    )]
    pub(super) fn layer_transform_handles(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if !self.inline_transform() {
            return;
        }
        let g = &mut self.layer_dialog;
        if self.navigation.interacting {
            g.drag = None;
            return;
        }
        let center =
            response.rect.min + egui::vec2(g.center[0] as f32, g.center[1] as f32) * self.zoom;
        let (sin, cos) = g.angle.to_radians().sin_cos();
        let point = |x: f64, y: f64| {
            let dx = (x - g.center[0]) * g.scale[0] / 100.0;
            let dy = (y - g.center[1]) * g.scale[1] / 100.0;
            center
                + egui::vec2((dx * cos - dy * sin) as f32, (dx * sin + dy * cos) as f32) * self.zoom
        };
        let [l, t, r, b] = g.bounds;
        let points = [
            point(l, t),
            point(l.midpoint(r), t),
            point(r, t),
            point(r, t.midpoint(b)),
            point(r, b),
            point(l.midpoint(r), b),
            point(l, b),
            point(l, t.midpoint(b)),
        ];
        let mut rotate = points[1] + egui::vec2(sin as f32, -cos as f32) * 26.0;
        let clip = ui.clip_rect().shrink(8.0);
        rotate.x = rotate.x.clamp(clip.left(), clip.right());
        rotate.y = rotate.y.clamp(clip.top(), clip.bottom());
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        painter.add(egui::Shape::closed_line(
            vec![points[0], points[2], points[4], points[6]],
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        ));
        painter.line_segment(
            [points[1], rotate],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
        for (index, pos) in points
            .into_iter()
            .chain(std::iter::once(rotate))
            .enumerate()
        {
            let rect = egui::Rect::from_center_size(pos, egui::vec2(12.0, 12.0));
            let handle = ui.interact(
                rect,
                ui.id().with(("transform-handle", index)),
                egui::Sense::drag(),
            );
            if index == 8 {
                painter.circle_filled(rect.center(), 4.0, super::ACCENT);
                painter.circle_stroke(
                    rect.center(),
                    4.0,
                    egui::Stroke::new(1.0, egui::Color32::WHITE),
                );
            } else {
                painter.rect_filled(rect.shrink(2.0), 1.0, super::ACCENT);
                painter.rect_stroke(
                    rect.shrink(2.0),
                    1.0,
                    egui::Stroke::new(1.0, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            if handle.hovered() {
                ui.ctx().set_cursor_icon(if index == 8 {
                    egui::CursorIcon::Grab
                } else {
                    egui::CursorIcon::ResizeNwSe
                });
            }
            if handle.drag_started() {
                if let Some(start) = ui.input(|i| i.pointer.press_origin()) {
                    g.drag = Some(TransformDrag {
                        handle: index,
                        start,
                        angle: g.angle,
                        scale: g.scale,
                    });
                }
            }
        }
        if let Some(drag) = &g.drag {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                let a = drag.start - center;
                let b = pos - center;
                let shift = ui.input(|i| i.modifiers.shift);
                if drag.handle == 8 {
                    let angle =
                        drag.angle + f64::from(b.y.atan2(b.x) - a.y.atan2(a.x)).to_degrees();
                    g.angle = if shift {
                        (angle / 15.0).round() * 15.0
                    } else {
                        angle
                    };
                } else {
                    let (s, c) = drag.angle.to_radians().sin_cos();
                    let local = |p: egui::Vec2| {
                        [
                            f64::from(p.x) * c + f64::from(p.y) * s,
                            -f64::from(p.x) * s + f64::from(p.y) * c,
                        ]
                    };
                    let start = local(a);
                    let now = local(b);
                    let x = drag.handle % 2 == 0 || matches!(drag.handle, 3 | 7);
                    let y = drag.handle % 2 == 0 || matches!(drag.handle, 1 | 5);
                    if shift || g.proportional {
                        let ratio = f64::from(b.length() / a.length().max(0.01));
                        g.scale = drag.scale.map(|v| (v * ratio).clamp(1.0, 1600.0));
                    } else {
                        for (axis, enabled) in [x, y].into_iter().enumerate() {
                            if enabled && start[axis].abs() > 0.01 {
                                g.scale[axis] =
                                    (drag.scale[axis] * now[axis] / start[axis]).clamp(1.0, 1600.0);
                            }
                        }
                    }
                }
                ui.ctx().request_repaint();
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                g.drag = None;
            }
        }
    }
    pub(super) fn mask_transform_preview(&self) -> bool {
        self.layer_dialog.kind == Some(false) && self.layer_dialog.target == EditTarget::Mask
    }
    pub(super) fn layer_preview(&self) -> Option<(f64, [f64; 2], PhotoGeometry, [f64; 2])> {
        let g = &self.layer_dialog;
        (g.kind == Some(false)
            && (g.target == EditTarget::Mask || self.document.active() == g.index))
            .then_some((g.angle, g.scale.map(|v| v / 100.0), g.photo, g.center))
    }
    pub(super) fn layer_window(&mut self, ctx: &egui::Context) {
        if self.layer_dialog.inline {
            return;
        }
        let Some(color) = self.layer_dialog.kind else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(match (color, self.layer_dialog.target) {
            (true, EditTarget::Raster) => "Цветокоррекция слоя",
            (false, EditTarget::Raster) => "Трансформация слоя",
            (true, EditTarget::Mask) => "Коррекция маски",
            (false, EditTarget::Mask) => "Трансформация маски",
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
            ui.small(if g.target == EditTarget::Mask {
                "Только маска · слои сохраняются · отмена одним шагом"
            } else {
                "Только текущий слой · отмена одним шагом"
            });
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
            self.changed = true;
        } else if apply {
            ctx.request_repaint();
            self.commit_layer_dialog();
        }
    }
    fn commit_layer_dialog(&mut self) {
        let g = &self.layer_dialog;
        let mask = g.target == EditTarget::Mask;
        let stale = self.edit_target() != g.target
            || if mask {
                g.mask_source.as_deref() != Some(self.document.mask())
                    || g.source.as_ref().is_none_or(|l| {
                        l.raster().width() != self.document.width()
                            || l.raster().height() != self.document.height()
                    })
            } else {
                self.document.active() != g.index
                    || self.document.layers().get(g.index) != g.source.as_ref()
            };
        if stale {
            self.layer_dialog.error =
                "Содержимое или режим изменились. Откройте окно заново.".into();
            return;
        }
        let mut next = self.document.clone();
        let operation = |d: &mut crate::Document| {
            if g.kind == Some(true) {
                d.correct_color(g.correction)
            } else {
                d.transform_photo_layer_about(
                    g.angle,
                    g.scale.map(|v| v / 100.0),
                    g.photo,
                    g.center,
                )
            }
        };
        let result = if mask {
            next.edit_mask(operation)
        } else {
            operation(&mut next)
        };
        match result {
            Ok(()) => {
                self.edit(|d| {
                    *d = next;
                    Ok(())
                });
                self.layer_dialog = LayerDialog::default();
                self.changed = true;
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
    #[test]
    fn mask_transform_and_correction_are_transactional_and_keep_locked_layers() {
        for color in [false, true] {
            let mut document =
                crate::Document::from_raster(Raster::solid(16, 16, [80, 120, 160, 255]).unwrap())
                    .unwrap();
            document.select_rectangle([4.0, 4.0], [9.0, 11.0]).unwrap();
            document.set_layer_locked(true);
            let mut editor = ImageEditor::new(document);
            editor.set_edit_target(EditTarget::Mask);
            let before = editor.document.clone();
            editor.open_layer_dialog(color);
            editor.layer_dialog.angle = 24.0;
            editor.layer_dialog.correction.brightness = -0.4;
            assert_eq!(editor.document, before);
            if !color {
                assert!(editor.mask_transform_preview());
                assert!(editor.layer_preview().is_some());
            }
            editor.commit_layer_dialog();
            assert!(editor.layer_dialog.kind.is_none());
            assert_eq!(editor.document.layers(), before.layers());
            assert_ne!(editor.document.mask(), before.mask());
            editor.restore_history(false);
            assert_eq!(editor.document, before);
        }
    }
    #[test]
    fn mask_transform_full_ui_builds_single_overlay_mesh_and_cancels_on_mode_switch() {
        let ctx = egui::Context::default();
        let mut document = crate::Document::new(16, 16).unwrap();
        document.select_rectangle([4.0, 4.0], [9.0, 11.0]).unwrap();
        let mut editor = ImageEditor::new(document);
        editor.set_edit_target(EditTarget::Mask);
        editor.open_layer_transform_dialog();
        editor.layer_dialog.angle = 8.0;
        let before = editor.document.clone();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                editor.show(ui);
            },
        );
        assert_eq!(editor.transform_preview.layers.len(), 1);
        assert_eq!(editor.document, before);
        editor.set_edit_target(EditTarget::Raster);
        assert!(!editor.mask_transform_preview());
        assert!(editor.layer_dialog.kind.is_none());
        assert_eq!(editor.document, before);
    }
    fn handle_frame(
        ctx: &egui::Context,
        editor: &mut ImageEditor,
        events: Vec<egui::Event>,
    ) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let (_, response) = ui.allocate_exact_size(
                        egui::vec2(128.0, 128.0),
                        egui::Sense::click_and_drag(),
                    );
                    rect = response.rect;
                    editor.layer_transform_handles(ui, &response);
                    editor.transform_ribbon(ui);
                });
            },
        );
        rect
    }
    #[test]
    #[allow(clippy::cast_possible_truncation)] // Small fixed test canvas coordinates.
    fn canvas_handles_scale_and_rotate_without_mutation_then_apply_once_or_cancel() {
        let events = |position, pressed| {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        for rotate in [false, true] {
            let ctx = egui::Context::default();
            let mut doc = crate::Document::new(64, 64).unwrap();
            doc.draw_shape(
                [16.0, 24.0],
                [48.0, 56.0],
                crate::Shape::Rectangle,
                1.0,
                true,
                [255; 4],
                crate::BrushStyle::default(),
            )
            .unwrap();
            let mut editor = ImageEditor::new(doc);
            editor.zoom = 2.0;
            editor.start_canvas_transform();
            let before = editor.document.clone();
            let rect = handle_frame(&ctx, &mut editor, vec![]);
            let g = &editor.layer_dialog;
            let from = if rotate {
                rect.min
                    + egui::vec2(
                        (g.center[0] * 2.0) as f32,
                        (g.bounds[1] * 2.0) as f32 - 26.0,
                    )
            } else {
                rect.min + egui::vec2((g.bounds[2] * 2.0) as f32, (g.bounds[3] * 2.0) as f32)
            };
            let to = from
                + if rotate {
                    egui::vec2(10.0, 0.0)
                } else {
                    egui::vec2(16.0, 16.0)
                };
            handle_frame(&ctx, &mut editor, events(from, true));
            handle_frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(to)]);
            handle_frame(&ctx, &mut editor, events(to, false));
            assert_eq!(editor.document, before);
            if rotate {
                assert!(
                    editor.layer_dialog.angle > 0.0 && editor.layer_dialog.angle < 15.0,
                    "{}",
                    editor.layer_dialog.angle
                );
            } else {
                assert!(
                    editor.layer_dialog.scale[0] > 140.0 && editor.layer_dialog.scale[0] < 160.0,
                    "{:?}",
                    editor.layer_dialog.scale
                );
            }
            let key = if rotate {
                egui::Key::Escape
            } else {
                egui::Key::Enter
            };
            handle_frame(
                &ctx,
                &mut editor,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            if rotate {
                assert_eq!(editor.document, before);
                assert!(!editor.inline_transform());
            } else {
                assert_ne!(editor.document, before);
                editor.restore_history(false);
                assert_eq!(editor.document, before);
            }
        }
    }
}
