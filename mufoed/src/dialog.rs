//! How the dialogs look: room around and between their parts, a title, and
//! a row of buttons at the bottom right with the main one filled.

use eframe::egui::{self, vec2, Color32, RichText, Stroke};

/// The smallest width of a dialog, so short ones do not look cramped.
const MIN_WIDTH: f32 = 340.0;

/// A modal dialog with room inside its frame.
pub(crate) fn modal(ctx: &egui::Context, id: &str) -> egui::Modal {
    let frame = egui::Frame::popup(&ctx.global_style())
        .inner_margin(egui::Margin::same(24))
        .corner_radius(egui::CornerRadius::same(10));
    egui::Modal::new(egui::Id::new(id.to_owned())).frame(frame)
}

/// The dialog's spacing and its title; first in each dialog.
pub(crate) fn title(ui: &mut egui::Ui, text: &str) {
    let spacing = ui.spacing_mut();
    spacing.item_spacing = vec2(10.0, 10.0);
    spacing.button_padding = vec2(16.0, 5.0);
    spacing.interact_size = vec2(72.0, 26.0);
    // Fields and boxes with an edge, so they stand out from the dialog.
    let visuals = ui.visuals_mut();
    let edge = if visuals.dark_mode {
        Color32::from_gray(90)
    } else {
        Color32::from_gray(190)
    };
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, edge);
    if ui.min_rect().width() < MIN_WIDTH {
        ui.set_min_width(MIN_WIDTH);
    }
    ui.label(RichText::new(text).size(18.0).strong());
    ui.add_space(6.0);
}

/// Labels on the left, their fields on the right, with room between rows.
pub(crate) fn form(id: &str) -> egui::Grid {
    egui::Grid::new(id).num_columns(2).spacing(vec2(20.0, 12.0))
}

/// A form's label for a field taller than one line, at its top.
pub(crate) fn label_top(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.add_space(4.0);
        ui.label(text);
    });
}

/// The row of buttons at the bottom, against the right edge.
pub(crate) fn buttons<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(10.0);
    let id = ui.id().with("dialog-buttons");
    // The row's width from the last frame, to push it to the right.
    let width: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
    let right = ui.min_rect().right();
    ui.horizontal(|ui| {
        let gap = (right - ui.cursor().min.x - width).max(0.0);
        ui.add_space(gap);
        let start = ui.cursor().min.x;
        let inner = add(ui);
        let used = (ui.cursor().min.x - start - ui.spacing().item_spacing.x).max(0.0);
        if (used - width).abs() > 0.5 {
            ui.data_mut(|data| data.insert_temp(id, used));
            ui.ctx().request_discard("dialog buttons measured");
        }
        inner
    })
    .inner
}

/// The dialog's main button, filled with the accent colour.
pub(crate) fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    primary_enabled(ui, true, text)
}

pub(crate) fn primary_enabled(ui: &mut egui::Ui, enabled: bool, text: &str) -> egui::Response {
    let fill = ui.visuals().selection.bg_fill;
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE))
            .fill(fill)
            .min_size(vec2(88.0, 0.0)),
    )
}

/// A plain button as wide as the main one.
pub(crate) fn button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(vec2(88.0, 0.0)))
}
