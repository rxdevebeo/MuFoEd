//! Retouch gesture state; all sampling uses canvas coordinates after zoom/pan.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use super::{EditTarget, ImageEditor, ManualTool};
use crate::{Document, RetouchMode, RetouchStroke};
use egui::{Color32, Pos2, Response, Stroke, Ui, Vec2};
#[derive(Default)]
pub(super) struct RetouchUi {
    pub(super) source: Option<[f32; 2]>,
    pub(super) stroke: Option<RetouchStroke>,
    key: Option<[usize; 3]>,
    destination: Option<[f32; 2]>,
}
impl ImageEditor {
    pub(super) fn retouch_canvas(
        &mut self,
        ui: &Ui,
        response: &Response,
        point: Option<[f32; 2]>,
        position: Option<Pos2>,
        down: bool,
    ) {
        let key = [
            self.document.active(),
            self.document.width() as usize,
            self.document.height() as usize,
        ];
        if self.manual_ui.retouch.key.is_some_and(|old| old != key) {
            self.finish_manual_stroke();
            self.manual_ui.retouch = RetouchUi::default();
        }
        let alt = ui.input(|i| i.modifiers.alt);
        if alt && response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            self.finish_manual_stroke();
            if let Some(p) = point.filter(|p| {
                p[0] >= 0.0
                    && p[1] >= 0.0
                    && p[0] < self.document.width() as f32
                    && p[1] < self.document.height() as f32
            }) {
                self.manual_ui.retouch.source = Some(p);
                self.manual_ui.retouch.key = Some(key);
                self.manual_ui.retouch.destination = None;
                self.status = "Источник ретуши задан".into();
            }
            self.manual_ui.ignore_until_release = down;
            return;
        }
        let engaged = (response.is_pointer_button_down_on() && down)
            || response.dragged_by(egui::PointerButton::Primary);
        if !alt && (engaged || (!down && self.manual_ui.stroke_before.is_some())) {
            if let (Some(p), Some(source)) = (point, self.manual_ui.retouch.source) {
                if self.manual_ui.retouch.stroke.is_none() {
                    let style = self.brush_style();
                    let mode = if self.manual_ui.tool == ManualTool::Heal {
                        RetouchMode::Heal
                    } else {
                        RetouchMode::Clone
                    };
                    let brush = if self.edit_target() == EditTarget::Mask {
                        Document::from_raster(self.document.mask_raster())
                            .and_then(|d| d.begin_retouch(source, p, self.radius, mode, style))
                    } else {
                        self.document
                            .begin_retouch(source, p, self.radius, mode, style)
                    };
                    match brush {
                        Ok(brush) => {
                            self.manual_ui.retouch.stroke = Some(brush);
                            self.manual_ui.retouch.destination = Some(p);
                            self.manual_ui.stroke_before =
                                Some((self.document.clone(), self.unsaved));
                        }
                        Err(e) => {
                            self.status = e.to_string();
                            return;
                        }
                    }
                }
                let from = self.manual_ui.last.unwrap_or(p);
                let stroke = self
                    .manual_ui
                    .retouch
                    .stroke
                    .as_ref()
                    .expect("initialized sample");
                let result = if self.edit_target() == EditTarget::Mask {
                    self.document
                        .edit_mask(|d| d.retouch_segment(stroke, from, p))
                } else {
                    self.document.retouch_segment(stroke, from, p)
                };
                match result {
                    Ok(()) => {
                        self.changed = true;
                        self.unsaved = true;
                    }
                    Err(e) => self.status = e.to_string(),
                }
                self.manual_ui.last = Some(p);
                ui.ctx().request_repaint();
            } else if engaged {
                self.status = "Alt+ЛКМ: сначала задайте источник ретуши".into();
            }
        }
        if !down {
            self.finish_manual_stroke();
        }
        self.retouch_overlay(ui, response, point, position);
    }
    fn retouch_overlay(
        &self,
        ui: &Ui,
        response: &Response,
        point: Option<[f32; 2]>,
        position: Option<Pos2>,
    ) {
        let painter = ui
            .painter()
            .with_clip_rect(response.rect.intersect(ui.clip_rect()));
        if response.hovered() {
            if let Some(pos) = position {
                for (width, color) in [(2.0, Color32::BLACK), (1.0, Color32::WHITE)] {
                    painter.circle_stroke(pos, self.radius * self.zoom, Stroke::new(width, color));
                }
            }
        }
        if let Some(mut source) = self.manual_ui.retouch.source {
            if self.manual_ui.retouch.stroke.is_some() {
                if let (Some(at), Some(p)) = (self.manual_ui.retouch.destination, point) {
                    source = [source[0] + p[0] - at[0], source[1] + p[1] - at[1]];
                }
            }
            let pos = response.rect.min + Vec2::new(source[0], source[1]) * self.zoom;
            for (width, color) in [(3.0, Color32::BLACK), (1.0, Color32::WHITE)] {
                let stroke = Stroke::new(width, color);
                painter.circle_stroke(pos, self.radius * self.zoom, stroke);
                painter.line_segment(
                    [pos - Vec2::new(7.0, 0.0), pos + Vec2::new(7.0, 0.0)],
                    stroke,
                );
                painter.line_segment(
                    [pos - Vec2::new(0.0, 7.0), pos + Vec2::new(0.0, 7.0)],
                    stroke,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Raster, SelectionOp};
    fn frame(
        ctx: &egui::Context,
        e: &mut ImageEditor,
        origin: Pos2,
        mods: egui::Modifiers,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(500.0, 400.0),
            )),
            modifiers: mods,
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                e.manual_shortcuts(ui);
                let response = ui.interact(
                    egui::Rect::from_min_size(origin, Vec2::splat(32.0 * e.zoom)),
                    egui::Id::new("retouch-test-canvas"),
                    egui::Sense::click_and_drag(),
                );
                e.manual_canvas(ui, &response);
            });
        });
    }
    fn pointer(pos: Pos2, pressed: bool, mods: egui::Modifiers) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: mods,
            },
        ]
    }
    fn editor(mask: bool) -> ImageEditor {
        let mut pixels = vec![0; 32 * 32 * 4];
        for (i, p) in pixels.chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(if i % 32 < 8 {
                &[220, 30, 10, 255]
            } else {
                &[20, 20, 20, 255]
            });
        }
        let mut d = Document::from_raster(Raster::new(32, 32, pixels).unwrap()).unwrap();
        if mask {
            d.set_mask(
                (0..1024)
                    .map(|i| if i % 32 < 8 { 200 } else { 0 })
                    .collect(),
            )
            .unwrap();
            d.set_layer_locked(true);
        }
        let mut e = ImageEditor::new(d);
        if mask {
            e.set_edit_target(EditTarget::Mask);
        }
        e.zoom = 2.0;
        e.radius = 0.5;
        e.select_manual_tool(ManualTool::Clone);
        e
    }
    #[test]
    fn alt_source_pan_zoom_release_endpoint_and_single_undo_for_both_targets() {
        for mask in [false, true] {
            let ctx = egui::Context::default();
            let mut e = editor(mask);
            let original = e.document.clone();
            let origin = egui::pos2(63.0, 41.0);
            let at = |x: f32, y: f32| origin + Vec2::new(x, y) * 2.0;
            let alt = egui::Modifiers {
                alt: true,
                ..Default::default()
            };
            let none = egui::Modifiers::NONE;
            frame(&ctx, &mut e, origin, none, vec![]);
            frame(&ctx, &mut e, origin, alt, pointer(at(2.5, 4.5), true, alt));
            frame(&ctx, &mut e, origin, alt, pointer(at(2.5, 4.5), false, alt));
            assert_eq!(e.document, original);
            assert!(!e.history.undo(&mut e.document));
            assert_eq!(e.manual_ui.retouch.source, Some([2.5, 4.5]));
            frame(
                &ctx,
                &mut e,
                origin,
                none,
                pointer(at(20.5, 4.5), true, none),
            );
            frame(
                &ctx,
                &mut e,
                origin,
                none,
                vec![egui::Event::PointerMoved(at(22.5, 4.5))],
            );
            frame(
                &ctx,
                &mut e,
                origin,
                none,
                pointer(at(24.5, 4.5), false, none),
            );
            if mask {
                assert_eq!(e.document.mask()[4 * 32 + 24], 200);
                assert_eq!(e.document.layers(), original.layers());
            } else {
                assert_eq!(
                    e.document.layers()[0].raster().pixel(24, 4),
                    Some([220, 30, 10, 255])
                );
            }
            assert!(e.manual_ui.retouch.stroke.is_none());
            let changed = e.document.clone();
            e.restore_history(false);
            assert_eq!(e.document, original);
            assert!(!e.history.undo(&mut e.document));
            e.restore_history(true);
            assert_eq!(e.document, changed);
        }
    }
    #[test]
    fn retouch_escape_layer_lock_no_source_and_mode_change_do_not_leak_gestures() {
        let ctx = egui::Context::default();
        let mut e = editor(false);
        let origin = egui::pos2(30.0, 40.0);
        let none = egui::Modifiers::NONE;
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let at = |x: f32| origin + Vec2::new(x, 4.5) * 2.0;
        let original = e.document.clone();
        frame(&ctx, &mut e, origin, none, vec![]);
        frame(&ctx, &mut e, origin, none, pointer(at(20.5), true, none));
        frame(&ctx, &mut e, origin, none, pointer(at(20.5), false, none));
        assert_eq!(e.document, original);
        frame(&ctx, &mut e, origin, alt, pointer(at(2.5), true, alt));
        frame(&ctx, &mut e, origin, alt, pointer(at(2.5), false, alt));
        frame(&ctx, &mut e, origin, none, pointer(at(20.5), true, none));
        assert_ne!(e.document, original);
        frame(
            &ctx,
            &mut e,
            origin,
            none,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: none,
            }],
        );
        frame(&ctx, &mut e, origin, none, pointer(at(24.5), false, none));
        assert_eq!(e.document, original);
        assert!(!e.history.undo(&mut e.document));
        e.document.set_layer_locked(true);
        let locked = e.document.clone();
        frame(&ctx, &mut e, origin, none, pointer(at(20.5), true, none));
        frame(&ctx, &mut e, origin, none, pointer(at(24.5), false, none));
        assert_eq!(e.document, locked);
        e.set_edit_target(EditTarget::Mask);
        assert!(e.manual_ui.retouch.source.is_none());
    }
    #[test]
    fn wand_modes_sample_correct_target_and_combine_without_painting() {
        for mask in [false, true] {
            let ctx = egui::Context::default();
            let mut e = editor(mask);
            e.select_manual_tool(ManualTool::ColorSelect);
            e.manual_ui.merged_sample = false;
            e.ribbon_ui.selection_op = SelectionOp::Replace;
            let origin = egui::pos2(45.0, 55.0);
            let pos = origin + Vec2::new(2.5, 4.5) * 2.0;
            let none = egui::Modifiers::NONE;
            let before = e.document.clone();
            frame(&ctx, &mut e, origin, none, vec![]);
            frame(&ctx, &mut e, origin, none, pointer(pos, true, none));
            frame(&ctx, &mut e, origin, none, pointer(pos, false, none));
            assert_eq!(
                e.document.selection(),
                (0..1024)
                    .map(|i| if i % 32 < 8 { 255 } else { 0 })
                    .collect::<Vec<_>>()
            );
            assert_eq!(e.document.mask(), before.mask());
            assert_eq!(e.document.layers(), before.layers());
            e.restore_history(false);
            assert_eq!(e.document, before);
        }
    }
}
