//! The large view of one picture: fit or zoom, pan, previous and next.

// Zoom arithmetic on positive sizes.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::path::PathBuf;

use egui::{pos2, vec2, Color32, Key, Rect, Sense, TextureHandle};

use super::{Command, Gallery, SlotState};
use crate::thumbs::{Answer, Info, Kind, Request, Stamp};

/// Largest texture side the viewer asks for.
const MAX_SIDE: usize = 8192;
/// Zoom limits, in screen pixels per picture pixel.
const ZOOM: std::ops::RangeInclusive<f32> = 0.02..=32.0;

pub(super) struct Viewer {
    pub(super) path: PathBuf,
    position: usize,
    stamp: Stamp,
    texture: Option<TextureHandle>,
    info: Option<Info>,
    error: Option<String>,
    /// Screen pixels per picture pixel; `None` fits the window.
    zoom: Option<f32>,
    pan: egui::Vec2,
    requested: bool,
}

impl Viewer {
    /// Asks for the picture again (it was changed on disk).
    pub(super) fn reload(&mut self) {
        self.requested = false;
        self.error = None;
    }

    pub(super) fn receive(&mut self, ctx: &egui::Context, answer: Answer) {
        if answer.request.path != self.path || answer.request.stamp != self.stamp {
            return;
        }
        match answer.result {
            Ok(pixels) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(pixels.size, &pixels.rgba);
                let name = format!("image-gallery-view:{}", self.path.display());
                self.texture = Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
                self.info = Some(pixels.info);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

impl Gallery {
    pub(super) fn open_viewer(&mut self, position: usize) {
        let Some(entry) = self.entry_at(position) else {
            return;
        };
        let path = entry.path.clone();
        let stamp = Stamp {
            size: entry.size,
            modified: entry.modified,
        };
        self.focus = Some(position);
        if !self.selection.contains(&path) {
            self.selection.clear();
            self.selection.insert(path.clone());
        }
        self.viewer = Some(Viewer {
            path,
            position,
            stamp,
            texture: None,
            info: None,
            error: None,
            zoom: None,
            pan: egui::Vec2::ZERO,
            requested: false,
        });
    }

    // One form, top to bottom.
    #[allow(clippy::too_many_lines)]
    pub(super) fn viewer_ui(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        let ctx = ui.ctx().clone();
        self.request_full(&ctx);
        let count = self.view.len();
        let Some(position) = self.viewer.as_ref().map(|viewer| viewer.position) else {
            return;
        };
        let mut go = None;
        ui.horizontal(|ui| {
            if ui.button(locale.text("← Previews")).clicked() {
                go = Some(None);
            }
            ui.separator();
            if ui
                .add_enabled(position > 0, egui::Button::new("◀"))
                .clicked()
            {
                go = Some(Some(position - 1));
            }
            if ui
                .add_enabled(position + 1 < count, egui::Button::new("▶"))
                .clicked()
            {
                go = Some(Some(position + 1));
            }
            ui.label(format!("{} / {count}", position + 1));
            ui.separator();
            if let Some(viewer) = &mut self.viewer {
                if ui
                    .selectable_label(viewer.zoom.is_none(), locale.text("Fit"))
                    .clicked()
                {
                    viewer.zoom = None;
                    viewer.pan = egui::Vec2::ZERO;
                }
                if ui
                    .selectable_label(viewer.zoom == Some(1.0), "100 %")
                    .clicked()
                {
                    viewer.zoom = Some(1.0);
                    viewer.pan = egui::Vec2::ZERO;
                }
                let name = viewer
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.separator();
                ui.strong(name);
                if let Some(info) = &viewer.info {
                    ui.weak(format!(
                        "{} × {} · {}",
                        info.width,
                        info.height,
                        info.format.name()
                    ));
                }
            }
        });
        let typing = ctx.egui_wants_keyboard_input();
        let keys = ctx.input(|input| {
            if typing {
                return None;
            }
            Some((
                input.key_pressed(Key::ArrowLeft) || input.key_pressed(Key::PageUp),
                input.key_pressed(Key::ArrowRight)
                    || input.key_pressed(Key::PageDown)
                    || input.key_pressed(Key::Space),
                input.key_pressed(Key::Escape),
                input.key_pressed(Key::Num0),
            ))
        });
        if let Some((previous, next, escape, fit)) = keys {
            if previous && position > 0 {
                go = Some(Some(position - 1));
            }
            if next && position + 1 < count {
                go = Some(Some(position + 1));
            }
            if escape {
                go = Some(None);
            }
            if fit {
                if let Some(viewer) = &mut self.viewer {
                    viewer.zoom = None;
                    viewer.pan = egui::Vec2::ZERO;
                }
            }
        }
        match go {
            Some(Some(target)) => {
                self.open_viewer(target);
                self.scroll_to_focus = true;
                return;
            }
            Some(None) => {
                self.execute(Command::CloseViewer);
                self.scroll_to_focus = true;
                return;
            }
            None => {}
        }
        self.canvas_ui(ui, &locale);
    }

    fn request_full(&mut self, ctx: &egui::Context) {
        let side = ctx.input(|input| input.max_texture_side).min(MAX_SIDE) as u32;
        let Some(viewer) = &self.viewer else {
            return;
        };
        if viewer.requested {
            return;
        }
        let request = Request {
            path: viewer.path.clone(),
            stamp: viewer.stamp,
            kind: Kind::Full(side),
            generation: 0,
        };
        let loader = self.loader(ctx);
        let request = Request {
            generation: loader.generation(),
            ..request
        };
        loader.request(request);
        if let Some(viewer) = &mut self.viewer {
            viewer.requested = true;
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui, locale: &crate::Locale) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(32));
        let preview = self
            .viewer
            .as_ref()
            .and_then(|viewer| self.thumbs.get(&viewer.path))
            .and_then(|slot| match &slot.state {
                SlotState::Ready(texture) => Some(texture.clone()),
                _ => None,
            });
        let ppp = ui.ctx().pixels_per_point();
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        let texture = viewer.texture.clone().or(preview);
        let Some(texture) = texture else {
            if let Some(error) = &viewer.error {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    error,
                    egui::FontId::proportional(15.0),
                    ui.visuals().error_fg_color,
                );
            } else {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    locale.text("Loading…"),
                    egui::FontId::proportional(15.0),
                    Color32::from_gray(200),
                );
            }
            return;
        };
        // Picture pixels: the full size when known, else the texture's.
        let pixels = viewer.info.as_ref().map_or(texture.size_vec2(), |info| {
            vec2(info.width as f32, info.height as f32)
        });
        let fit = (rect.width() / (pixels.x / ppp))
            .min(rect.height() / (pixels.y / ppp))
            .min(1.0);
        let zoom = viewer.zoom.unwrap_or(fit);
        // Wheel zoom around the pointer.
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y)
                + (ui.input(egui::InputState::zoom_delta) - 1.0) * 200.0;
            if scroll.abs() > 0.0 {
                let factor = (scroll * 0.0025).exp();
                let target = (zoom * factor).clamp(*ZOOM.start(), *ZOOM.end());
                if let Some(pointer) = response.hover_pos() {
                    let from_centre = pointer - rect.center() - viewer.pan;
                    viewer.pan = pointer - rect.center() - from_centre * (target / zoom);
                }
                viewer.zoom = Some(target);
            }
        }
        if response.dragged() {
            viewer.pan += response.drag_delta();
            if viewer.zoom.is_none() {
                viewer.zoom = Some(zoom);
            }
        }
        if response.double_clicked() {
            if viewer.zoom.is_some_and(|zoom| (zoom - 1.0).abs() < 1e-3) {
                viewer.zoom = None;
            } else {
                viewer.zoom = Some(1.0);
            }
            viewer.pan = egui::Vec2::ZERO;
        }
        let zoom = viewer.zoom.unwrap_or(fit);
        let size = pixels / ppp * zoom;
        let shown = Rect::from_center_size(rect.center() + viewer.pan, size);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(texture.id(), shown, uv, Color32::WHITE);
        let label = format!("{:.0} %", zoom * 100.0);
        painter.text(
            rect.right_bottom() - vec2(8.0, 8.0),
            egui::Align2::RIGHT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            Color32::from_gray(210),
        );
    }
}
