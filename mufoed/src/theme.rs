//! Colours and fonts in the manner of Word and WPS: a blue accent, a light
//! ribbon over a grey desk on which the pages lie.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily};
use strict_ooxml_render_svg::font::builtin::face_source;

/// The word processor blue.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(43, 87, 154);
/// A lighter blue for hovered and selected ribbon parts.
pub(crate) const ACCENT_LIGHT: Color32 = Color32::from_rgb(205, 222, 245);

/// The colours that differ between light and dark.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) title_bar: Color32,
    pub(crate) tab_active: Color32,
    pub(crate) tab_text: Color32,
    pub(crate) ribbon: Color32,
    pub(crate) ribbon_line: Color32,
    pub(crate) desk: Color32,
    pub(crate) status: Color32,
    pub(crate) status_text: Color32,
    pub(crate) group_label: Color32,
    pub(crate) hover: Color32,
    pub(crate) pressed: Color32,
    pub(crate) selection: Color32,
    pub(crate) caret: Color32,
    /// Face and outline of ribbon buttons and tab headers.
    pub(crate) button: Color32,
    pub(crate) button_border: Color32,
    pub(crate) tab_border: Color32,
}

pub(crate) fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            title_bar: Color32::from_rgb(32, 32, 34),
            tab_active: Color32::from_rgb(48, 48, 52),
            tab_text: Color32::from_gray(225),
            ribbon: Color32::from_rgb(48, 48, 52),
            ribbon_line: Color32::from_gray(70),
            desk: Color32::from_rgb(30, 30, 30),
            status: Color32::from_rgb(36, 36, 40),
            status_text: Color32::from_gray(200),
            group_label: Color32::from_gray(160),
            hover: Color32::from_rgb(70, 74, 82),
            pressed: Color32::from_rgb(60, 88, 130),
            selection: Color32::from_rgba_unmultiplied(80, 140, 230, 90),
            caret: Color32::BLACK,
            button: Color32::from_rgb(58, 58, 63),
            button_border: Color32::from_gray(84),
            tab_border: Color32::from_gray(78),
        }
    } else {
        Palette {
            title_bar: Color32::from_rgb(230, 232, 236),
            tab_active: Color32::from_rgb(250, 250, 250),
            tab_text: Color32::from_gray(40),
            ribbon: Color32::from_rgb(250, 250, 250),
            ribbon_line: Color32::from_gray(218),
            desk: Color32::from_rgb(230, 230, 230),
            status: Color32::from_rgb(243, 243, 243),
            status_text: Color32::from_gray(70),
            group_label: Color32::from_gray(110),
            hover: Color32::from_rgb(226, 232, 242),
            pressed: ACCENT_LIGHT,
            selection: Color32::from_rgba_unmultiplied(60, 120, 220, 80),
            caret: Color32::BLACK,
            button: Color32::WHITE,
            button_border: Color32::from_gray(208),
            tab_border: Color32::from_gray(200),
        }
    }
}

/// Family names for bold and italic interface text.
pub(crate) const BOLD: &str = "bold";
pub(crate) const ITALIC: &str = "italic";

/// Interface fonts: Selawik (metric-compatible with Segoe UI, the face Word's
/// interface uses) from the renderer's own bundle, with egui's defaults and
/// then Arimo behind it for the symbols Selawik lacks.
pub(crate) fn install_fonts(ctx: &egui::Context) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, family: &str, bold: bool, italic: bool| -> bool {
        let Some(source) = face_source(family, bold, italic) else {
            return false;
        };
        // A variable face is shown at its default instance.
        let data = FontData::from_static(source.data);
        fonts
            .font_data
            .insert(name.to_owned(), std::sync::Arc::new(data));
        true
    };
    let regular = add("ui-regular", "Selawik", false, false);
    let bold = add("ui-bold", "Selawik", true, false);
    let italic = add("ui-italic", "Arimo", false, true);
    let mut families = vec![];
    if regular {
        families.push("ui-regular".to_owned());
    }
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    families.extend(defaults.iter().cloned());
    // Arimo carries the geometric marks Selawik and the egui faces lack
    // (the find arrows, the symbol-dialog arrows). It stays last, so it
    // only fills those holes.
    let symbols = add("ui-symbols", "Arimo", false, false);
    if symbols {
        families.push("ui-symbols".to_owned());
    }
    fonts.families.insert(FontFamily::Proportional, families);
    let mut bold_family = Vec::new();
    if bold {
        bold_family.push("ui-bold".to_owned());
    }
    bold_family.extend(defaults.iter().cloned());
    if symbols {
        bold_family.push("ui-symbols".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Name(BOLD.into()), bold_family);
    let mut italic_family = Vec::new();
    if italic {
        italic_family.push("ui-italic".to_owned());
    }
    italic_family.extend(defaults.iter().cloned());
    if symbols {
        italic_family.push("ui-symbols".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Name(ITALIC.into()), italic_family);
    ctx.set_fonts(fonts.clone());
    fonts
}

/// A button at rest uses a face and an outline, the same way a hovered one does.
///
/// egui's inactive button has no stroke, so in a dialog or a panel it reads as
/// disabled until the pointer reaches it.
pub(crate) fn style_buttons(ui: &mut egui::Ui) {
    let palette = palette(ui.visuals().dark_mode);
    let widgets = &mut ui.visuals_mut().widgets;
    let radius = egui::CornerRadius::same(4);
    widgets.inactive.weak_bg_fill = palette.button;
    widgets.inactive.bg_fill = palette.button;
    widgets.inactive.bg_stroke = egui::Stroke::new(1.0, palette.button_border);
    widgets.inactive.corner_radius = radius;
    widgets.hovered.weak_bg_fill = palette.hover;
    widgets.hovered.bg_fill = palette.hover;
    widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.7));
    widgets.hovered.corner_radius = radius;
    widgets.active.weak_bg_fill = palette.pressed;
    widgets.active.bg_fill = palette.pressed;
    widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    widgets.active.corner_radius = radius;
    widgets.open.corner_radius = radius;
}

/// Draw a dedicated focus ring for text inputs after all panels and dialogs.
/// egui uses selection.stroke for both the input frame and selected text;
/// keeping that white preserves readable text selection and button styling.
pub(crate) fn paint_input_focus(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|memory| memory.focused()) else {
        return;
    };
    if egui::text_edit::TextEditState::load(ctx, id).is_none() {
        return;
    }
    let Some(response) = ctx
        .read_response(id)
        .filter(|r| r.enabled() && r.has_focus() && r.interact_rect.is_positive())
    else {
        return;
    };
    let color = if ctx.global_style().visuals.dark_mode {
        Color32::from_rgb(120, 180, 255)
    } else {
        ACCENT
    };
    ctx.layer_painter(response.layer_id)
        .with_clip_rect(response.interact_rect.expand(2.0))
        .rect_stroke(
            response.rect,
            4.0,
            egui::Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
}

/// egui visuals tuned toward the office look.
pub(crate) fn apply(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let palette = palette(dark);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = if dark {
        Color32::from_rgb(120, 170, 240)
    } else {
        ACCENT
    };
    visuals.widgets.hovered.weak_bg_fill = palette.hover;
    visuals.widgets.hovered.bg_fill = palette.hover;
    visuals.widgets.active.weak_bg_fill = palette.pressed;
    visuals.window_corner_radius = egui::CornerRadius::same(6);
    visuals.panel_fill = palette.ribbon;
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 3.0);
        style.interaction.tooltip_delay = 0.35;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_ring_marks_text_input_in_both_themes_without_restyling_buttons() {
        for dark in [false, true] {
            let ctx = egui::Context::default();
            apply(&ctx, dark);
            let selected_text = ctx.global_style().visuals.selection.stroke;
            let mut value = "Text".to_owned();
            let mut field_rect = egui::Rect::NOTHING;
            for focus_input in [true, false] {
                let mut button_stroke = egui::Stroke::NONE;
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(600.0, 400.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            style_buttons(ui);
                            button_stroke = ui.visuals().widgets.inactive.bg_stroke;
                            let input = ui
                                .add(egui::TextEdit::singleline(&mut value).id_salt("focus-field"));
                            field_rect = input.rect;
                            let button = ui.button("Button");
                            if focus_input {
                                input.request_focus();
                            } else {
                                button.request_focus();
                            }
                            paint_input_focus(ui.ctx());
                            assert_eq!(ui.visuals().widgets.inactive.bg_stroke, button_stroke);
                        },
                    );
                }
                let color = if dark {
                    Color32::from_rgb(120, 180, 255)
                } else {
                    ACCENT
                };
                let marked=output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.rect==field_rect && r.stroke==egui::Stroke::new(2.0,color)));
                assert_eq!(marked, focus_input);
                assert_eq!(
                    button_stroke,
                    egui::Stroke::new(1.0, palette(dark).button_border)
                );
                assert_eq!(ctx.global_style().visuals.selection.stroke, selected_text);
            }
        }
    }
}
