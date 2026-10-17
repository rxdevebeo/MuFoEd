//! Paragraph formatting shared by the dialog and worker; only changed fields are applied.
use crate::i18n::I18n;
use eframe::egui::{self, pos2, vec2, Color32, RichText, Stroke};
use strict_ooxml::model::props::ParagraphProperties;
use strict_ooxml::model::values::{Justification, LineSpacingRule, TriState, Twips};
use strict_ooxml_render_svg::style::ComputedParagraph;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Format {
    pub(crate) alignment: Justification,
    pub(crate) outline: Option<u8>,
    pub(crate) left: i32,
    pub(crate) right: i32,
    pub(crate) first: i32,
    pub(crate) before: i32,
    pub(crate) after: i32,
    pub(crate) line: i32,
    pub(crate) rule: LineSpacingRule,
    pub(crate) contextual: bool,
    pub(crate) widow: bool,
    pub(crate) keep_next: bool,
    pub(crate) keep_lines: bool,
    pub(crate) page_break: bool,
    pub(crate) no_numbers: bool,
    pub(crate) no_hyphens: bool,
}

impl Format {
    pub(crate) fn from_computed(p: &ComputedParagraph) -> Self {
        let twips = |value: f64| (value * 20.0).round() as i32;
        Self {
            alignment: p.alignment,
            outline: p.outline_level.filter(|n| *n < 9),
            left: twips(p.indent_start_pt),
            right: twips(p.indent_end_pt),
            first: twips(p.first_line_pt),
            before: twips(p.space_before_pt),
            after: twips(p.space_after_pt),
            line: p.line_pt.map_or(240, twips),
            rule: p.line_rule,
            contextual: p.contextual_spacing,
            widow: p.widow_control,
            keep_next: p.keep_next,
            keep_lines: p.keep_lines,
            page_break: p.page_break_before,
            no_numbers: p.suppress_line_numbers,
            no_hyphens: p.suppress_auto_hyphens,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Patch {
    pub(crate) before: Format,
    pub(crate) after: Format,
}

impl Patch {
    pub(crate) fn apply(&self, p: &mut ParagraphProperties) {
        let a = &self.before;
        let b = &self.after;
        if a.alignment != b.alignment {
            p.alignment = Some(b.alignment);
        }
        if a.outline != b.outline {
            p.outline_level = Some(b.outline.unwrap_or(9));
        }
        if (a.left, a.right, a.first) != (b.left, b.right, b.first) {
            let indent = p.indentation.get_or_insert_with(Default::default);
            if a.left != b.left {
                indent.start = Some(Twips(b.left));
                indent.start_chars = None;
            }
            if a.right != b.right {
                indent.end = Some(Twips(b.right));
                indent.end_chars = None;
            }
            if a.first != b.first {
                indent.first_line = Some(Twips(b.first.max(0)));
                indent.hanging = Some(Twips((-b.first).max(0)));
                indent.first_line_chars = None;
                indent.hanging_chars = None;
            }
        }
        if (a.before, a.after, a.line, a.rule) != (b.before, b.after, b.line, b.rule) {
            let spacing = p.spacing.get_or_insert_with(Default::default);
            if a.before != b.before {
                spacing.before = Some(Twips(b.before));
                spacing.before_lines = None;
                spacing.before_autospacing = Some(false);
            }
            if a.after != b.after {
                spacing.after = Some(Twips(b.after));
                spacing.after_lines = None;
                spacing.after_autospacing = Some(false);
            }
            if (a.line, a.rule) != (b.line, b.rule) {
                spacing.line = Some(Twips(b.line));
                spacing.line_rule = Some(b.rule);
            }
        }
        let flag = |old, new, target: &mut TriState| {
            if old != new {
                *target = if new { TriState::On } else { TriState::Off };
            }
        };
        flag(a.contextual, b.contextual, &mut p.contextual_spacing);
        flag(a.widow, b.widow, &mut p.widow_control);
        flag(a.keep_next, b.keep_next, &mut p.keep_next);
        flag(a.keep_lines, b.keep_lines, &mut p.keep_lines);
        flag(a.page_break, b.page_break, &mut p.page_break_before);
        flag(a.no_numbers, b.no_numbers, &mut p.suppress_line_numbers);
        flag(a.no_hyphens, b.no_hyphens, &mut p.suppress_auto_hyphens);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Dialog {
    original: Format,
    current: Format,
    tab: bool,
}

impl Dialog {
    pub(crate) fn new(format: Format) -> Self {
        Self {
            original: format.clone(),
            current: format,
            tab: false,
        }
    }

    /// Returns a patch on OK/Apply, and whether to close.
    pub(crate) fn show(&mut self, ctx: &egui::Context, i18n: &I18n) -> (Option<Patch>, bool) {
        let t = |key| i18n.t(key);
        let mut apply = false;
        let mut close = false;
        let response = crate::dialog::modal(ctx, "paragraph").show(ctx, |ui| {
            ui.set_width(500.0);
            crate::dialog::title(ui, t("paragraph.title"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, false, t("paragraph.indents_spacing"));
                ui.selectable_value(&mut self.tab, true, t("paragraph.line_page"));
            });
            ui.separator();
            let p = &mut self.current;
            if self.tab {
                for (key, value) in [
                    ("paragraph.widow", &mut p.widow),
                    ("paragraph.keep_next", &mut p.keep_next),
                    ("paragraph.keep_lines", &mut p.keep_lines),
                    ("paragraph.page_break", &mut p.page_break),
                    ("paragraph.no_numbers", &mut p.no_numbers),
                    ("paragraph.no_hyphens", &mut p.no_hyphens),
                ] {
                    ui.checkbox(value, t(key));
                }
                ui.add_space(45.0);
            } else {
                crate::dialog::form("paragraph-fields").show(ui, |ui| {
                    ui.label(t("paragraph.alignment"));
                    egui::ComboBox::from_id_salt("paragraph-alignment")
                        .selected_text(t(alignment_key(p.alignment)))
                        .show_ui(ui, |ui| {
                            for (value, key) in [
                                (Justification::Start, "paragraph.left"),
                                (Justification::Center, "paragraph.center"),
                                (Justification::End, "paragraph.right"),
                                (Justification::Both, "paragraph.justify"),
                            ] {
                                ui.selectable_value(&mut p.alignment, value, t(key));
                            }
                        });
                    ui.end_row();
                    ui.label(t("paragraph.outline"));
                    egui::ComboBox::from_id_salt("paragraph-outline")
                        .selected_text(p.outline.map_or_else(
                            || t("paragraph.body").to_owned(),
                            |n| (n + 1).to_string(),
                        ))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut p.outline, None, t("paragraph.body"));
                            for n in 0..9 {
                                ui.selectable_value(&mut p.outline, Some(n), (n + 1).to_string());
                            }
                        });
                    ui.end_row();
                    ui.label(RichText::new(t("paragraph.indentation")).strong());
                    ui.label("");
                    ui.end_row();
                    for (key, value) in [
                        ("paragraph.left_indent", &mut p.left),
                        ("paragraph.right_indent", &mut p.right),
                    ] {
                        ui.label(t(key));
                        number(ui, value, 566.929, -20.0..=40.0, t("paragraph.cm"));
                        ui.end_row();
                    }
                    ui.label(t("paragraph.special"));
                    ui.horizontal(|ui| {
                        let mut mode = p.first.signum();
                        egui::ComboBox::from_id_salt("paragraph-special")
                            .width(120.0)
                            .selected_text(t(match mode {
                                1 => "paragraph.first_line",
                                -1 => "paragraph.hanging",
                                _ => "paragraph.none",
                            }))
                            .show_ui(ui, |ui| {
                                for (value, key) in [
                                    (0, "paragraph.none"),
                                    (1, "paragraph.first_line"),
                                    (-1, "paragraph.hanging"),
                                ] {
                                    ui.selectable_value(&mut mode, value, t(key));
                                }
                            });
                        if mode != p.first.signum() {
                            p.first = mode * p.first.abs().max(709);
                        }
                        if mode != 0 {
                            let mut amount = p.first.abs();
                            number(ui, &mut amount, 566.929, 0.01..=40.0, t("paragraph.cm"));
                            p.first = mode * amount;
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new(t("paragraph.spacing")).strong());
                    ui.label("");
                    ui.end_row();
                    for (key, value) in [
                        ("paragraph.before", &mut p.before),
                        ("paragraph.after", &mut p.after),
                    ] {
                        ui.label(t(key));
                        number(ui, value, 20.0, 0.0..=600.0, t("paragraph.pt"));
                        ui.end_row();
                    }
                    ui.label(t("paragraph.line_spacing"));
                    ui.horizontal(|ui| {
                        let old = match p.rule {
                            LineSpacingRule::Exact => 4,
                            LineSpacingRule::AtLeast => 3,
                            LineSpacingRule::Auto => match p.line {
                                240 => 0,
                                360 => 1,
                                480 => 2,
                                _ => 5,
                            },
                        };
                        let mut mode = old;
                        let keys = [
                            "paragraph.single",
                            "paragraph.one_half",
                            "paragraph.double",
                            "paragraph.at_least",
                            "paragraph.exactly",
                            "paragraph.multiple",
                        ];
                        egui::ComboBox::from_id_salt("paragraph-line")
                            .width(120.0)
                            .selected_text(t(keys[mode]))
                            .show_ui(ui, |ui| {
                                for (i, key) in keys.iter().enumerate() {
                                    ui.selectable_value(&mut mode, i, t(key));
                                }
                            });
                        if mode != old {
                            p.rule = match mode {
                                3 => LineSpacingRule::AtLeast,
                                4 => LineSpacingRule::Exact,
                                _ => LineSpacingRule::Auto,
                            };
                            p.line = match mode {
                                0 | 3 | 4 => 240,
                                1 => 360,
                                2 => 480,
                                _ => 288,
                            };
                        }
                        if mode >= 3 {
                            number(
                                ui,
                                &mut p.line,
                                if mode == 5 { 240.0 } else { 20.0 },
                                if mode == 5 { 0.1..=10.0 } else { 1.0..=600.0 },
                                if mode == 5 { "" } else { t("paragraph.pt") },
                            );
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut p.contextual, t("paragraph.contextual"));
            }
            ui.add_space(6.0);
            ui.label(t("paragraph.preview"));
            preview(ui, p);
            ui.label(
                RichText::new(t("paragraph.selection_hint"))
                    .weak()
                    .size(11.0),
            );
            crate::dialog::buttons(ui, |ui| {
                if crate::dialog::primary(ui, t("button.ok")).clicked() {
                    apply = true;
                    close = true;
                }
                if crate::dialog::button(ui, t("button.cancel")).clicked() {
                    close = true;
                }
                if ui
                    .add_enabled(
                        self.current != self.original,
                        egui::Button::new(t("paragraph.apply")),
                    )
                    .clicked()
                {
                    apply = true;
                }
            });
        });
        let patch = (apply && self.current != self.original).then(|| Patch {
            before: self.original.clone(),
            after: self.current.clone(),
        });
        if apply {
            self.original.clone_from(&self.current);
        }
        (patch, close || response.should_close())
    }
}

fn alignment_key(value: Justification) -> &'static str {
    match value {
        Justification::Center => "paragraph.center",
        Justification::End => "paragraph.right",
        Justification::Both | Justification::Justify | Justification::Distribute => {
            "paragraph.justify"
        }
        _ => "paragraph.left",
    }
}

fn number(
    ui: &mut egui::Ui,
    value: &mut i32,
    scale: f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) {
    let mut display = *value as f32 / scale;
    if ui
        .add(
            egui::DragValue::new(&mut display)
                .range(range)
                .speed(0.05)
                .max_decimals(2)
                .suffix(format!(" {suffix}")),
        )
        .changed()
    {
        *value = (display * scale).round() as i32;
    }
}

fn preview(ui: &mut egui::Ui, p: &Format) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 86.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, Color32::WHITE);
    let scale = (rect.width() - 40.0) / 9000.0;
    let left = rect.left() + 20.0 + p.left as f32 * scale;
    let right = rect.right() - 20.0 - p.right as f32 * scale;
    let gap = if p.rule == LineSpacingRule::Auto {
        (p.line as f32 / 240.0 * 9.0).clamp(5.0, 18.0)
    } else {
        (p.line as f32 / 40.0).clamp(5.0, 18.0)
    };
    for line in 0..3 {
        let start = left
            + if line == 0 {
                p.first as f32 * scale
            } else {
                0.0
            };
        let width = (right - start).max(10.0) * if line == 2 { 0.62 } else { 1.0 };
        let x = match p.alignment {
            Justification::Center => start + ((right - start) - width) / 2.0,
            Justification::End => right - width,
            _ => start,
        };
        let y = rect.top() + 15.0 + (p.before as f32 * scale).min(12.0) + line as f32 * gap;
        ui.painter().with_clip_rect(rect).line_segment(
            [pos2(x, y), pos2(x + width, y)],
            Stroke::new(3.0, Color32::from_gray(110)),
        );
    }
}
