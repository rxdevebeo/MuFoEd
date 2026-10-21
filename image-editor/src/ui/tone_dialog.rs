//! Centered transactional levels/curves; document pixels stay unchanged until Apply.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use super::{EditTarget, ImageEditor};
use crate::{Curves, Document, Histogram, Levels, Raster, ToneAdjustment};
use egui::{Color32, Pos2, Rect, Stroke, TextureHandle, Ui, Vec2};

pub(super) struct ToneDialog {
    before: Document,
    target: EditTarget,
    mask_source: Option<Raster>,
    curves_mode: bool,
    levels: Levels,
    curves: Curves,
    channel: usize,
    selected: Option<usize>,
    dragging: bool,
    logarithmic: bool,
    histogram: Histogram,
    last: Option<ToneAdjustment>,
    preview: Option<Raster>,
    textures: Option<[TextureHandle; 2]>,
    error: String,
}
impl ImageEditor {
    /// Open levels for the current raster/mask with histogram and before/after.
    pub fn open_levels_dialog(&mut self) {
        self.open_tone_dialog(false);
    }
    /// Open interactive smooth curves for the current raster/mask.
    pub fn open_curves_dialog(&mut self) {
        self.open_tone_dialog(true);
    }
    fn open_tone_dialog(&mut self, curves_mode: bool) {
        self.finish_manual_stroke();
        self.finish_pasted_object();
        if self.edit_target() == EditTarget::Raster
            && self.document.layers()[self.document.active()].locked()
        {
            self.status = "Слой заблокирован".into();
            return;
        }
        self.layer_dialog = super::layer_dialog::LayerDialog::default();
        self.geometry = super::geometry_dialog::GeometryDialog::default();
        self.changed = true;
        let target = self.edit_target();
        let mask_source = (target == EditTarget::Mask).then(|| self.document.mask_raster());
        let source = mask_source
            .as_ref()
            .unwrap_or_else(|| self.document.layers()[self.document.active()].raster());
        let histogram =
            Histogram::from_raster(source, self.document.selection()).expect("validated document");
        self.tone_dialog = Some(ToneDialog {
            before: self.document.clone(),
            target,
            mask_source,
            curves_mode,
            levels: Levels::default(),
            curves: Curves::default(),
            channel: 0,
            selected: None,
            dragging: false,
            logarithmic: true,
            histogram,
            last: None,
            preview: None,
            textures: None,
            error: String::new(),
        });
    }
    pub(super) fn tone_window(&mut self, ctx: &egui::Context) {
        let Some(g) = self.tone_dialog.as_mut() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let response=egui::Modal::new(egui::Id::new("tone-dialog")).show(ctx,|ui| {
            ui.set_width((ctx.content_rect().width()-60.0).clamp(340.0,620.0));
            ui.heading(match (g.curves_mode,g.target) {
                (true,EditTarget::Raster)=>"Кривые слоя",(false,EditTarget::Raster)=>"Уровни слоя",
                (true,EditTarget::Mask)=>"Кривые маски",(false,EditTarget::Mask)=>"Уровни маски",
            });
            ui.small(if g.before.selection().is_empty(){"Вся область · один Undo после применения"}else{"Только ручное выделение · мягкие края учитываются"});
            egui::ScrollArea::vertical().id_salt("tone-content").max_height((ctx.content_rect().height()-130.0).max(180.0)).show(ui,|ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("tone-channel").selected_text(channel_label(g.channel)).show_ui(ui,|ui| {
                        for c in 0..if g.target==EditTarget::Mask {1}else{4} {
                            if ui.selectable_value(&mut g.channel,c,channel_label(c)).changed() {g.selected=None;g.dragging=false;}
                        }
                    });
                    ui.checkbox(&mut g.logarithmic,"Логарифмическая гистограмма");
                });
                ui.horizontal_top(|ui| {
                    g.graph(ui);
                    ui.vertical(|ui| {ui.spacing_mut().slider_width=105.0;g.controls(ui);});
                });
                ui.small(if g.curves_mode {"ЛКМ — добавить/двигать точку · ПКМ — удалить · крайние точки фиксированы по X"}else{"Гистограмма исходника: RGB — яркость, R/G/B — соответствующий канал"});
                if ui.button("Сбросить всё").clicked() {g.reset();}
                g.refresh(ctx);
                if let Some(textures)=&g.textures {
                    ui.separator();
                    ui.horizontal(|ui| {
                        for (label,texture) in ["До","После"].into_iter().zip(textures) {
                            ui.vertical(|ui| {ui.strong(label);let size=texture.size_vec2();ui.image((texture.id(),size*(220.0/size.x.max(size.y)).min(1.0)));});
                        }
                    });
                }
            });
            if !g.error.is_empty(){ui.colored_label(Color32::LIGHT_RED,&g.error);}
            ui.separator();
            ui.horizontal(|ui| {
                apply=ui.add_enabled(g.preview.is_some() && g.error.is_empty(),egui::Button::new("Применить").fill(super::ACCENT)).clicked();
                cancel=ui.button("Отмена · Esc").clicked();
            });
        });
        if cancel || response.should_close() {
            self.tone_dialog = None;
        } else if apply {
            self.commit_tone_dialog();
        }
    }
    fn commit_tone_dialog(&mut self) {
        let Some(g) = self.tone_dialog.as_mut() else {
            return;
        };
        if self.document != g.before || self.manual_ui.target != g.target {
            g.error = "Документ или режим изменились. Откройте окно заново.".into();
            return;
        }
        let adjustment = g.adjustment();
        let target = g.target;
        let mut next = self.document.clone();
        let result = if target == EditTarget::Mask {
            next.edit_mask(|d| d.correct_tone(&adjustment))
        } else {
            next.correct_tone(&adjustment)
        };
        match result {
            Ok(()) => {
                self.tone_dialog = None;
                self.edit(|d| {
                    *d = next;
                    Ok(())
                });
            }
            Err(e) => {
                if let Some(g) = &mut self.tone_dialog {
                    g.error = e.to_string();
                }
            }
        }
    }
}
impl ToneDialog {
    fn source(&self) -> &Raster {
        self.mask_source
            .as_ref()
            .unwrap_or_else(|| self.before.layers()[self.before.active()].raster())
    }
    fn adjustment(&self) -> ToneAdjustment {
        if self.curves_mode {
            ToneAdjustment::Curves(self.curves.clone())
        } else {
            ToneAdjustment::Levels(self.levels.clone())
        }
    }
    fn reset(&mut self) {
        self.levels = Levels::default();
        self.curves = Curves::default();
        self.selected = None;
        self.dragging = false;
    }
    fn refresh(&mut self, ctx: &egui::Context) {
        let adjustment = self.adjustment();
        if self.last.as_ref() == Some(&adjustment) {
            return;
        }
        match adjustment.apply(self.source(), self.before.selection()) {
            Ok(raster) => {
                let thumbnail = if self.target == EditTarget::Raster {
                    self.before.flatten_preview(Some(&raster)).thumbnail(256)
                } else {
                    raster.thumbnail(256)
                }
                .expect("bounded preview");
                let image = color_image(&thumbnail);
                if let Some(textures) = &mut self.textures {
                    textures[1].set(image, egui::TextureOptions::LINEAR);
                } else {
                    let original = if self.target == EditTarget::Raster {
                        self.before.flatten().thumbnail(256)
                    } else {
                        self.source().thumbnail(256)
                    }
                    .expect("bounded preview");
                    self.textures = Some([
                        ctx.load_texture(
                            "tone-before",
                            color_image(&original),
                            egui::TextureOptions::LINEAR,
                        ),
                        ctx.load_texture("tone-after", image, egui::TextureOptions::LINEAR),
                    ]);
                }
                self.preview = Some(raster);
                self.error.clear();
            }
            Err(e) => {
                self.preview = None;
                self.error = e.to_string();
            }
        }
        self.last = Some(adjustment);
    }
    fn controls(&mut self, ui: &mut Ui) {
        if self.curves_mode {
            ui.strong("Точки кривой");
            if let Some(i) = self.selected {
                let p = &mut self.curves.channels[self.channel].points;
                let min = if i == 0 { 0 } else { p[i - 1][0] + 1 };
                let max = if i == p.len() - 1 {
                    255
                } else {
                    p[i + 1][0] - 1
                };
                ui.add_enabled(
                    i > 0 && i < p.len() - 1,
                    egui::DragValue::new(&mut p[i][0])
                        .range(min..=max)
                        .prefix("Вход "),
                );
                ui.add(
                    egui::DragValue::new(&mut p[i][1])
                        .range(0..=255)
                        .prefix("Выход "),
                );
                if ui
                    .add_enabled(i > 0 && i < p.len() - 1, egui::Button::new("Удалить точку"))
                    .clicked()
                {
                    p.remove(i);
                    self.selected = None;
                }
            } else {
                ui.weak("Выберите точку на графике");
            }
        } else {
            let c = &mut self.levels.channels[self.channel];
            ui.strong("Входные уровни");
            ui.horizontal(|ui| {
                let max_black = c.input[1] - 1;
                ui.add(
                    egui::DragValue::new(&mut c.input[0])
                        .range(0..=max_black)
                        .prefix("Чёрный "),
                );
                let min_white = c.input[0] + 1;
                ui.add(
                    egui::DragValue::new(&mut c.input[1])
                        .range(min_white..=255)
                        .prefix("Белый "),
                );
            });
            ui.add(
                egui::Slider::new(&mut c.gamma, 0.1..=10.0)
                    .logarithmic(true)
                    .text("Гамма"),
            );
            ui.strong("Выходные уровни");
            ui.horizontal(|ui| {
                let max_black = c.output[1];
                ui.add(
                    egui::DragValue::new(&mut c.output[0])
                        .range(0..=max_black)
                        .prefix("Чёрный "),
                );
                let min_white = c.output[0];
                ui.add(
                    egui::DragValue::new(&mut c.output[1])
                        .range(min_white..=255)
                        .prefix("Белый "),
                );
            });
        }
        if ui.button("Сбросить канал").clicked() {
            self.levels.channels[self.channel] = crate::LevelChannel::default();
            self.curves.channels[self.channel] = crate::ToneCurve::default();
            self.selected = None;
            self.dragging = false;
        }
    }
    fn graph(&mut self, ui: &mut Ui) -> Rect {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(304.0, 185.0), egui::Sense::click_and_drag());
        let plot = rect.shrink(9.0);
        if self.curves_mode {
            self.curve_input(ui, &response, plot);
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(plot, 0.0, Color32::from_gray(24));
        let bins = &self.histogram.bins[self.channel];
        let value = |v: u64| {
            if self.logarithmic {
                (v as f64).ln_1p()
            } else {
                v as f64
            }
        };
        let max = bins.iter().copied().map(value).fold(1.0, f64::max);
        let color = match self.channel {
            1 => Color32::from_rgb(180, 75, 75),
            2 => Color32::from_rgb(70, 160, 95),
            3 => Color32::from_rgb(75, 120, 200),
            _ => Color32::from_gray(100),
        };
        for (i, &bin) in bins.iter().enumerate() {
            let x = plot.left() + i as f32 / 255.0 * plot.width();
            let y = plot.bottom() - (value(bin) / max) as f32 * plot.height();
            painter.line_segment(
                [Pos2::new(x, plot.bottom()), Pos2::new(x, y)],
                Stroke::new((plot.width() / 256.0).max(1.0), color),
            );
        }
        for i in 0..=4 {
            let t = i as f32 / 4.0;
            painter.line_segment(
                [
                    Pos2::new(plot.left() + plot.width() * t, plot.top()),
                    Pos2::new(plot.left() + plot.width() * t, plot.bottom()),
                ],
                Stroke::new(0.5, Color32::from_gray(65)),
            );
            painter.line_segment(
                [
                    Pos2::new(plot.left(), plot.top() + plot.height() * t),
                    Pos2::new(plot.right(), plot.top() + plot.height() * t),
                ],
                Stroke::new(0.5, Color32::from_gray(65)),
            );
        }
        if self.curves_mode {
            painter.line_segment(
                [plot.left_bottom(), plot.right_top()],
                Stroke::new(1.0, Color32::from_gray(110)),
            );
            let curve = &self.curves.channels[self.channel];
            if let Ok(lut) = curve.lookup() {
                painter.add(egui::Shape::line(
                    lut.iter()
                        .enumerate()
                        .map(|(x, y)| point(plot, [x as u8, *y]))
                        .collect(),
                    Stroke::new(2.0, Color32::WHITE),
                ));
            }
            for (i, &p) in curve.points.iter().enumerate() {
                painter.circle_filled(
                    point(plot, p),
                    4.5,
                    if self.selected == Some(i) {
                        super::ACCENT
                    } else {
                        Color32::WHITE
                    },
                );
            }
        } else {
            for &x in &self.levels.channels[self.channel].input {
                let x = plot.left() + f32::from(x) / 255.0 * plot.width();
                painter.line_segment(
                    [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                    Stroke::new(1.5, Color32::WHITE),
                );
            }
        }
        rect
    }
    fn curve_input(&mut self, ui: &Ui, response: &egui::Response, plot: Rect) {
        let Some(pos) = response.interact_pointer_pos() else {
            if !ui.input(|i| i.pointer.primary_down()) {
                self.dragging = false;
            }
            return;
        };
        let p = &mut self.curves.channels[self.channel].points;
        let nearest = p
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, point(plot, p).distance(pos)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, d)| *d <= 9.0)
            .map(|(i, _)| i);
        if response.secondary_clicked() {
            if let Some(i) = nearest.filter(|i| *i > 0 && *i < p.len() - 1) {
                p.remove(i);
                self.selected = None;
            }
            self.dragging = false;
            return;
        }
        if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            self.selected = nearest;
            if self.selected.is_none() && p.len() < 32 {
                let sample = unproject(plot, pos);
                let x = sample[0].clamp(1, 254);
                let i = p.partition_point(|v| v[0] < x);
                if p[i][0] != x {
                    p.insert(i, [x, sample[1]]);
                }
                self.selected = Some(i);
            }
            self.dragging = self.selected.is_some();
        }
        if self.dragging {
            if let Some(i) = self.selected {
                let sample = unproject(plot, pos);
                p[i] = [
                    if i == 0 {
                        0
                    } else if i == p.len() - 1 {
                        255
                    } else {
                        sample[0].clamp(p[i - 1][0] + 1, p[i + 1][0] - 1)
                    },
                    sample[1],
                ];
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = false;
        }
    }
}
fn channel_label(channel: usize) -> &'static str {
    match channel {
        1 => "Красный · R",
        2 => "Зелёный · G",
        3 => "Синий · B",
        _ => "Общий · RGB",
    }
}
fn point(rect: Rect, p: [u8; 2]) -> Pos2 {
    Pos2::new(
        rect.left() + f32::from(p[0]) / 255.0 * rect.width(),
        rect.bottom() - f32::from(p[1]) / 255.0 * rect.height(),
    )
}
fn unproject(rect: Rect, p: Pos2) -> [u8; 2] {
    [
        ((p.x - rect.left()) / rect.width() * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8,
        ((rect.bottom() - p.y) / rect.height() * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8,
    ]
}
fn color_image(r: &Raster) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([r.width() as usize, r.height() as usize], r.pixels())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor(mask: bool) -> ImageEditor {
        let mut d = Document::from_raster(
            Raster::new(
                3,
                1,
                vec![20, 40, 60, 255, 100, 120, 140, 128, 90, 80, 70, 0],
            )
            .unwrap(),
        )
        .unwrap();
        d.set_mask(vec![20, 100, 200]).unwrap();
        d.set_selection(vec![0, 128, 255]).unwrap();
        if mask {
            d.set_layer_locked(true);
        }
        let mut e = ImageEditor::new(d);
        if mask {
            e.set_edit_target(EditTarget::Mask);
        }
        e
    }
    fn frame(ctx: &egui::Context, e: &mut ImageEditor, events: Vec<egui::Event>) {
        let modifiers = events
            .iter()
            .find_map(|event| match event {
                egui::Event::Key { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 700.0))),
                modifiers,
                events,
                ..Default::default()
            },
            |ui| {
                e.show(ui);
            },
        );
    }
    #[test]
    fn exact_preview_apply_psd_and_one_undo_for_levels_curves_and_both_targets() {
        for mask in [false, true] {
            for curves in [false, true] {
                let ctx = egui::Context::default();
                let mut e = editor(mask);
                let original = e.document.clone();
                e.open_tone_dialog(curves);
                let g = e.tone_dialog.as_mut().unwrap();
                g.levels.channels[0].output = [100, 220];
                g.curves.channels[0].points = vec![[0, 20], [100, 180], [255, 240]];
                g.refresh(&ctx);
                let expected = g.preview.clone().unwrap();
                assert_eq!(e.document, original);
                assert!(!e.unsaved);
                assert!(!e.history.undo(&mut e.document));
                e.commit_tone_dialog();
                assert!(e.tone_dialog.is_none());
                assert_ne!(e.document, original);
                if mask {
                    assert_eq!(e.document.mask_raster(), expected);
                    assert_eq!(e.document.layers(), original.layers());
                } else {
                    assert_eq!(e.document.layers()[0].raster(), &expected);
                    assert_eq!(e.document.mask(), original.mask());
                }
                assert_eq!(
                    Document::from_psd(&e.document.to_psd().unwrap()).unwrap(),
                    e.document
                );
                let applied = e.document.clone();
                e.restore_history(false);
                assert_eq!(e.document, original);
                assert!(!e.history.undo(&mut e.document));
                e.restore_history(true);
                assert_eq!(e.document, applied);
            }
        }
    }
    #[test]
    fn reset_neutral_cancel_lock_and_stale_changes_keep_document_safe() {
        let ctx = egui::Context::default();
        let mut e = editor(false);
        let original = e.document.clone();
        e.open_levels_dialog();
        let g = e.tone_dialog.as_mut().unwrap();
        g.levels.channels[0].gamma = 2.0;
        g.refresh(&ctx);
        g.reset();
        g.refresh(&ctx);
        assert_eq!(g.preview.as_ref().unwrap(), original.layers()[0].raster());
        e.commit_tone_dialog();
        assert_eq!(e.document, original);
        assert!(!e.unsaved);
        assert!(!e.history.undo(&mut e.document));
        e.open_curves_dialog();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        frame(
            &ctx,
            &mut e,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(e.tone_dialog.is_none());
        assert_eq!(e.document, original);
        assert!(!e.unsaved);
        e.open_levels_dialog();
        e.tone_dialog.as_mut().unwrap().levels.channels[0].gamma = 2.0;
        e.document.set_mask(vec![17; 3]).unwrap();
        let changed = e.document.clone();
        e.commit_tone_dialog();
        assert_eq!(e.document, changed);
        assert!(!e.tone_dialog.as_ref().unwrap().error.is_empty());
        e.set_edit_target(EditTarget::Mask);
        assert!(e.tone_dialog.is_none());
        e.set_edit_target(EditTarget::Raster);
        e.document.set_layer_locked(true);
        e.open_levels_dialog();
        assert!(e.tone_dialog.is_none());
        e.set_edit_target(EditTarget::Mask);
        e.open_curves_dialog();
        assert!(e.tone_dialog.is_some());
    }
    #[test]
    fn modal_blocks_global_undo_and_drawing_until_cancelled() {
        let ctx = egui::Context::default();
        let mut e = editor(false);
        e.history.checkpoint(&e.document);
        e.document.set_mask(vec![42; 3]).unwrap();
        let before = e.document.clone();
        e.open_levels_dialog();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        frame(
            &ctx,
            &mut e,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        assert_eq!(e.document, before);
        assert!(e.tone_dialog.is_some());
        e.tone_dialog = None;
        frame(
            &ctx,
            &mut e,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        frame(
            &ctx,
            &mut e,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        assert_ne!(
            e.document, before,
            "same shortcut must work after closing the modal"
        );
    }
    fn graph_frame(ctx: &egui::Context, e: &mut ImageEditor, events: Vec<egui::Event>) -> Rect {
        let mut rect = Rect::NOTHING;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    rect = e.tone_dialog.as_mut().unwrap().graph(ui);
                });
            },
        );
        rect.shrink(9.0)
    }
    fn pointer(pos: Pos2, pressed: bool, button: egui::PointerButton) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                pressed,
                button,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn curve_pointer_add_drag_delete_preserves_order_and_source() {
        let ctx = egui::Context::default();
        let mut e = editor(false);
        let original = e.document.clone();
        e.open_curves_dialog();
        let rect = graph_frame(&ctx, &mut e, vec![]);
        let pos = point(rect, [128, 180]);
        graph_frame(
            &ctx,
            &mut e,
            pointer(pos, true, egui::PointerButton::Primary),
        );
        graph_frame(
            &ctx,
            &mut e,
            pointer(pos, false, egui::PointerButton::Primary),
        );
        assert_eq!(
            e.tone_dialog.as_ref().unwrap().curves.channels[0].points,
            [[0, 0], [128, 180], [255, 255]]
        );
        graph_frame(
            &ctx,
            &mut e,
            pointer(pos, true, egui::PointerButton::Primary),
        );
        let dest = point(rect, [96, 140]);
        graph_frame(&ctx, &mut e, vec![egui::Event::PointerMoved(dest)]);
        graph_frame(
            &ctx,
            &mut e,
            pointer(dest, false, egui::PointerButton::Primary),
        );
        assert_eq!(
            e.tone_dialog.as_ref().unwrap().curves.channels[0].points[1],
            [96, 140]
        );
        graph_frame(
            &ctx,
            &mut e,
            pointer(dest, true, egui::PointerButton::Secondary),
        );
        graph_frame(
            &ctx,
            &mut e,
            pointer(dest, false, egui::PointerButton::Secondary),
        );
        assert_eq!(
            e.tone_dialog.as_ref().unwrap().curves.channels[0].points,
            [[0, 0], [255, 255]]
        );
        assert_eq!(e.document, original);
    }
}
