//! Canvas navigation without modifying document pixels.
use super::ImageEditor;
use egui::{Ui, Vec2};

pub(super) const SCROLL_ID: &str = "image-canvas-scroll";
#[derive(Default)]
pub(super) struct Navigation {
    pub offset: Vec2,
    button: Option<egui::PointerButton>,
    last: Option<egui::Pos2>,
    pub interacting: bool,
}
impl ImageEditor {
    pub(super) fn pause_canvas_navigation(&mut self) {
        self.navigation.button = None;
        self.navigation.last = None;
        self.navigation.interacting = false;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub(super) fn draw_pixel_grid(&self, ui: &Ui, rect: egui::Rect) {
        if !self.pixel_grid || self.zoom < 6.0 {
            return;
        }
        let visible = rect.intersect(ui.clip_rect());
        if !visible.is_positive() {
            return;
        }
        let painter = ui.painter().with_clip_rect(visible);
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(90));
        let x0 = ((visible.left() - rect.left()) / self.zoom).ceil().max(0.0) as u32;
        let x1 = ((visible.right() - rect.left()) / self.zoom)
            .floor()
            .max(0.0) as u32;
        let y0 = ((visible.top() - rect.top()) / self.zoom).ceil().max(0.0) as u32;
        let y1 = ((visible.bottom() - rect.top()) / self.zoom)
            .floor()
            .max(0.0) as u32;
        for x in x0..=x1.min(self.document.width()) {
            let x = rect.left() + x as f32 * self.zoom;
            painter.line_segment(
                [
                    egui::pos2(x, visible.top()),
                    egui::pos2(x, visible.bottom()),
                ],
                stroke,
            );
        }
        for y in y0..=y1.min(self.document.height()) {
            let y = rect.top() + y as f32 * self.zoom;
            painter.line_segment(
                [
                    egui::pos2(visible.left(), y),
                    egui::pos2(visible.right(), y),
                ],
                stroke,
            );
        }
    }
    pub(super) fn canvas_pan(&mut self, ui: &Ui) {
        use egui::{CursorIcon, PointerButton};
        let hand = self.manual_ui.tool == super::ManualTool::Hand;
        let move_object = self.has_pasted_object();
        let nav = &mut self.navigation;
        nav.interacting = nav.button.is_some();
        let inside = ui.rect_contains_pointer(ui.available_rect_before_wrap());
        ui.input(|input| {
            let pointer = &input.pointer;
            if nav.button.is_none() && inside {
                let button = if pointer.button_pressed(PointerButton::Secondary) {
                    Some(PointerButton::Secondary)
                } else if hand && !move_object && pointer.button_pressed(PointerButton::Primary) {
                    Some(PointerButton::Primary)
                } else {
                    None
                };
                if let Some(button) = button {
                    nav.button = Some(button);
                    nav.last = pointer.interact_pos();
                    nav.interacting = true;
                }
            }
            if let Some(button) = nav.button {
                if let Some(position) = pointer.interact_pos() {
                    if let Some(last) = nav.last {
                        nav.offset += position - last;
                    }
                    nav.last = Some(position);
                }
                if !pointer.button_down(button) {
                    // A stationary secondary click retains the Zoom tool's zoom-out action.
                    if button == PointerButton::Secondary && pointer.button_clicked(button) {
                        nav.interacting = false;
                    }
                    nav.button = None;
                    nav.last = None;
                }
            }
        });
        if nav.interacting {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            ui.ctx().request_repaint();
        } else if hand && inside {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
    }

    pub(super) fn canvas_wheel_zoom(&mut self, ui: &Ui, viewport: Vec2) -> Option<Vec2> {
        if !ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            return None;
        }
        let (ctrl, factor, pointer) =
            ui.input(|i| (i.modifiers.ctrl, i.zoom_delta(), i.pointer.hover_pos()));
        let pointer = pointer?;
        if !ctrl || !factor.is_finite() || (factor - 1.0).abs() < f32::EPSILON {
            return None;
        }
        let previous = self.zoom;
        self.zoom = (previous * factor).clamp(0.02, 8.0);
        let image = self.preview_canvas_size();
        let offset = egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(SCROLL_ID))
            .unwrap_or_default()
            .offset;
        let cursor = pointer - ui.available_rect_before_wrap().min;
        let old_margin = (viewport - image * previous).max(Vec2::ZERO) * 0.5;
        let source = (cursor + offset - old_margin - self.navigation.offset) / previous;
        let margin = (viewport - image * self.zoom).max(Vec2::ZERO) * 0.5;
        let limit = (image * self.zoom - viewport).max(Vec2::ZERO);
        ui.ctx().request_repaint();
        let desired = source * self.zoom + margin + self.navigation.offset - cursor;
        let offset = desired.clamp(Vec2::ZERO, limit);
        self.navigation.offset += offset - desired;
        Some(offset)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        editor: &mut ImageEditor,
        ctrl: bool,
        delta: f32,
        position: egui::Pos2,
    ) -> Vec2 {
        let modifiers = if ctrl {
            egui::Modifiers::CTRL
        } else {
            egui::Modifiers::NONE
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 240.0),
            )),
            modifiers,
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0.0, delta),
                    modifiers,
                },
            ],
            ..Default::default()
        };
        let mut offset = Vec2::ZERO;
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let viewport = ui.available_size();
                let next = editor.canvas_wheel_zoom(ui, viewport);
                let mut area = egui::ScrollArea::both()
                    .id_salt(SCROLL_ID)
                    .auto_shrink([false, false]);
                if let Some(next) = next {
                    area = area.scroll_offset(next);
                }
                let out = area.show(ui, |ui| {
                    ui.allocate_space(Vec2::splat(1024.0) * editor.zoom);
                });
                offset = out.state.offset;
            });
        });
        offset
    }
    fn pan_frame(ctx: &egui::Context, editor: &mut ImageEditor, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 240.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                editor.canvas_pan(ui);
                let response = ui.allocate_rect(
                    ui.available_rect_before_wrap(),
                    egui::Sense::click_and_drag(),
                );
                editor.manual_canvas(ui, &response);
            });
        });
    }
    fn button(
        position: egui::Pos2,
        button: egui::PointerButton,
        pressed: bool,
    ) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn hand_and_secondary_drag_pan_small_canvas_without_editing_or_switching_tools() {
        use egui::PointerButton::{Primary, Secondary};
        for (tool, mouse) in [
            (super::super::ManualTool::Hand, Primary),
            (super::super::ManualTool::Brush, Secondary),
            (super::super::ManualTool::Zoom, Secondary),
            (super::super::ManualTool::Select, Secondary),
        ] {
            let ctx = egui::Context::default();
            let mut editor = ImageEditor::new(crate::Document::new(32, 32).unwrap());
            editor.manual_ui.tool = tool;
            let original = editor.document.clone();
            let start = egui::pos2(100.0, 100.0);
            let end = egui::pos2(380.0, 280.0);
            pan_frame(&ctx, &mut editor, vec![]);
            pan_frame(&ctx, &mut editor, button(start, mouse, true));
            // Continue dragging beyond the viewport, then release there.
            pan_frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
            assert_eq!(editor.navigation.offset, end - start);
            assert!(editor.navigation.interacting);
            pan_frame(&ctx, &mut editor, button(end, mouse, false));
            let offset = editor.navigation.offset;
            pan_frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(start)]);
            assert_eq!(editor.navigation.offset, offset);
            assert!(!editor.navigation.interacting);
            assert!(editor.manual_ui.tool == tool);
            assert_eq!(editor.document, original);
            assert!(!editor.has_unsaved_changes());
            assert_eq!(editor.zoom.to_bits(), 1.0f32.to_bits());
        }
    }
    #[test]
    fn secondary_drag_started_outside_viewport_is_ignored() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::default();
        pan_frame(&ctx, &mut editor, vec![]);
        pan_frame(
            &ctx,
            &mut editor,
            button(
                egui::pos2(400.0, 300.0),
                egui::PointerButton::Secondary,
                true,
            ),
        );
        pan_frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(egui::pos2(100.0, 100.0))],
        );
        assert_eq!(editor.navigation.offset, Vec2::ZERO);
        assert!(!editor.navigation.interacting);
    }
    #[test]
    fn pixel_grid_is_optional_zoom_gated_and_preserved_in_preferences() {
        let mut editor = ImageEditor::new(crate::Document::new(32, 32).unwrap());
        let original = editor.document.clone();
        for (enabled, zoom, visible) in [(false, 6.0, false), (true, 5.0, false), (true, 6.0, true)]
        {
            editor.pixel_grid = enabled;
            editor.zoom = zoom;
            let ctx = egui::Context::default();
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                editor.draw_pixel_grid(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::splat(32.0 * zoom)),
                );
            });
            let lines = output
                .shapes
                .iter()
                .filter(|s| matches!(s.shape, egui::Shape::LineSegment { .. }))
                .count();
            assert_eq!(lines > 0, visible);
        }
        assert_eq!(editor.document, original);
        assert!(!editor.has_unsaved_changes());
        let saved = serde_json::to_string(&editor.settings()).unwrap();
        let mut restored = ImageEditor::default();
        restored
            .apply_settings(serde_json::from_str(&saved).unwrap())
            .unwrap();
        restored.replace(crate::Document::new(4, 4).unwrap());
        assert!(restored.pixel_grid);
    }
    #[test]
    fn ctrl_wheel_changes_only_view_and_keeps_source_under_pointer() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::new(crate::Document::new(1024, 1024).unwrap());
        let original = editor.document.clone();
        let pointer = egui::pos2(150.0, 120.0);
        frame(&ctx, &mut editor, false, 0.0, pointer);
        let offset = frame(&ctx, &mut editor, true, 100.0, pointer);
        assert!(editor.zoom > 1.0);
        // CentralPanel has an 8-point margin. No padding is needed for this large image.
        let before = pointer - egui::pos2(8.0, 8.0);
        let after = (before + offset) / editor.zoom;
        assert!((before - after).length() < 0.01, "{before:?} vs {after:?}");
        assert_eq!(editor.document, original);
        assert!(!editor.has_unsaved_changes());
    }
    #[test]
    fn zoom_keeps_anchor_after_panning_small_canvas() {
        let ctx = egui::Context::default();
        let mut editor = ImageEditor::new(crate::Document::new(128, 128).unwrap());
        editor.navigation.offset = egui::vec2(40.0, -20.0);
        let pointer = egui::pos2(150.0, 120.0);
        let viewport = egui::vec2(304.0, 224.0);
        let cursor = pointer - egui::pos2(8.0, 8.0);
        let before = cursor - (viewport - Vec2::splat(128.0)) * 0.5 - editor.navigation.offset;
        frame(&ctx, &mut editor, true, 100.0, pointer);
        let margin = (viewport - Vec2::splat(128.0) * editor.zoom).max(Vec2::ZERO) * 0.5;
        let after = (cursor - margin - editor.navigation.offset) / editor.zoom;
        assert!((before - after).length() < 0.01);
    }
    #[test]
    fn plain_wheel_and_wheel_outside_canvas_do_not_zoom() {
        for (ctrl, position) in [
            (false, egui::pos2(150.0, 120.0)),
            (true, egui::pos2(500.0, 120.0)),
        ] {
            let ctx = egui::Context::default();
            let mut editor = ImageEditor::default();
            frame(&ctx, &mut editor, false, 0.0, position);
            frame(&ctx, &mut editor, ctrl, 100.0, position);
            assert_eq!(editor.zoom.to_bits(), 1.0f32.to_bits());
        }
    }
    #[test]
    fn wheel_down_decreases_zoom_and_limits_are_respected() {
        for (initial, delta, expected) in [
            (1.0, -100.0, None),
            (8.0, 100.0, Some(8.0_f32)),
            (0.02, -100.0, Some(0.02_f32)),
        ] {
            let ctx = egui::Context::default();
            let mut editor = ImageEditor {
                zoom: initial,
                ..Default::default()
            };
            let pointer = egui::pos2(150.0, 120.0);
            frame(&ctx, &mut editor, false, 0.0, pointer);
            frame(&ctx, &mut editor, true, delta, pointer);
            if let Some(expected) = expected {
                assert_eq!(editor.zoom.to_bits(), expected.to_bits());
            } else {
                assert!(editor.zoom < initial);
            }
        }
    }
}
