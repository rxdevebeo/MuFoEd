//! The grid of previews, its keyboard and mouse handling, and the status bar.

// Layout arithmetic on small positive point sizes.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use egui::{pos2, vec2, Align2, Color32, FontId, Key, Modifiers, Rect, Sense, Stroke, StrokeKind};

use super::{Command, Gallery, SlotState};

/// Padding around a preview inside its cell.
const PAD: f32 = 6.0;
/// Gap between cells.
const GAP: f32 = 6.0;

/// The preview side in pixels for a cell of `points` at the screen's scale:
/// one of a few steps, so resizing the grid does not decode everything again.
fn preview_side(pixels: f32) -> u32 {
    [128, 192, 256, 384, 512, 768]
        .into_iter()
        .find(|&step| step as f32 >= pixels)
        .unwrap_or(768)
}

impl Gallery {
    pub(super) fn grid_ui(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        ui.horizontal(|ui| {
            ui.label(locale.text("Filter"));
            let edit = egui::TextEdit::singleline(&mut self.filter)
                .hint_text(locale.text("Part of the name"))
                .desired_width(200.0);
            if ui.add(edit).changed() {
                self.resort();
            }
            if !self.filter.is_empty() && ui.small_button("✕").clicked() {
                self.filter.clear();
                self.resort();
            }
        });
        ui.separator();
        if self.folder.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.heading(locale.text("No folder is open"));
                if ui.button(locale.text("Open folder…")).clicked() {
                    self.execute(Command::OpenFolder);
                }
            });
            return;
        }
        if self.view.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                if self.scanning.is_some() {
                    ui.spinner();
                } else if let Some(message) = &self.message {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                } else {
                    ui.label(locale.text("No pictures here"));
                }
            });
            return;
        }
        self.keyboard(ui);
        self.grid(ui);
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let thumb = self.settings.thumb_size;
        let label = if self.settings.show_names {
            ui.text_style_height(&egui::TextStyle::Small) + 4.0
        } else {
            0.0
        };
        let cell = vec2(thumb + 2.0 * PAD, thumb + 2.0 * PAD + label);
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        let width = ui.available_width();
        let columns = (((width + GAP) / (cell.x + GAP)).floor() as usize).max(1);
        self.columns = columns;
        let rows = self.view.len().div_ceil(columns);
        let side = preview_side(thumb * ctx.pixels_per_point());
        let mut area = egui::ScrollArea::vertical()
            .id_salt("image-gallery-grid")
            .auto_shrink([false, false]);
        if self.scroll_to_focus {
            self.scroll_to_focus = false;
            if let Some(focus) = self.focus {
                let pitch = cell.y + GAP;
                let top = (focus / columns) as f32 * pitch;
                let (offset, height) = self.viewport;
                if top < offset {
                    area = area.vertical_scroll_offset(top);
                } else if top + cell.y > offset + height {
                    area = area.vertical_scroll_offset(top + cell.y - height);
                }
            }
        }
        let output = area.show_rows(ui, cell.y, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    for column in 0..columns {
                        let position = row * columns + column;
                        if position >= self.view.len() {
                            break;
                        }
                        self.want_preview(&ctx, position, side);
                        self.cell_ui(ui, position, cell);
                    }
                });
            }
        });
        self.viewport = (output.state.offset.y, output.inner_rect.height());
    }

    // One form, top to bottom.
    #[allow(clippy::too_many_lines)]
    fn cell_ui(&mut self, ui: &mut egui::Ui, position: usize, cell: egui::Vec2) {
        let Some(entry) = self.entry_at(position) else {
            return;
        };
        let path = entry.path.clone();
        let name = entry.name.clone();
        let format = entry.format;
        let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
        let selected = self.selection.contains(&path);
        let visuals = ui.visuals().clone();
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, 4.0, visuals.selection.bg_fill.gamma_multiply(0.6));
        } else if response.hovered() {
            painter.rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
        }
        if self.focus == Some(position) {
            painter.rect_stroke(
                rect.shrink(1.0),
                4.0,
                visuals.selection.stroke,
                StrokeKind::Inside,
            );
        }
        let thumb = self.settings.thumb_size;
        let frame = Rect::from_min_size(rect.min + vec2(PAD, PAD), vec2(thumb, thumb));
        let mut tooltip = name.clone();
        match self.thumbs.get(&path).map(|slot| &slot.state) {
            Some(SlotState::Ready(texture)) => {
                let size = texture.size_vec2();
                let scale = (frame.width() / size.x)
                    .min(frame.height() / size.y)
                    .min(1.0);
                let shown = Rect::from_center_size(frame.center(), size * scale);
                checkerboard(&painter, shown);
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(texture.id(), shown, uv, Color32::WHITE);
                painter.rect_stroke(
                    shown,
                    0.0,
                    Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.3)),
                    StrokeKind::Outside,
                );
            }
            Some(SlotState::Failed(error)) => {
                painter.rect_stroke(
                    frame.shrink(8.0),
                    4.0,
                    Stroke::new(1.0, visuals.error_fg_color),
                    StrokeKind::Inside,
                );
                painter.text(
                    frame.center(),
                    Align2::CENTER_CENTER,
                    format.name(),
                    FontId::proportional(14.0),
                    visuals.error_fg_color,
                );
                tooltip = format!("{name}\n{error}");
            }
            Some(SlotState::Loading) | None => {
                painter.rect_filled(frame.shrink(8.0), 4.0, visuals.faint_bg_color);
                painter.text(
                    frame.center(),
                    Align2::CENTER_CENTER,
                    format.name(),
                    FontId::proportional(13.0),
                    visuals.weak_text_color(),
                );
            }
        }
        if let Some(info) = self.info_of(&path) {
            tooltip = format!(
                "{tooltip}\n{} × {} · {}",
                info.width,
                info.height,
                info.format.name()
            );
        }
        if self.settings.show_names {
            let colour = if selected {
                visuals.strong_text_color()
            } else {
                visuals.text_color()
            };
            let mut job =
                egui::text::LayoutJob::simple_singleline(name, FontId::proportional(12.0), colour);
            job.wrap = egui::text::TextWrapping::truncate_at_width(cell.x - 2.0 * PAD);
            let galley = painter.layout_job(job);
            let at = pos2(
                rect.center().x - galley.size().x / 2.0,
                frame.max.y + PAD / 2.0,
            );
            painter.galley(at, galley, colour);
        }
        let response = response.on_hover_text(tooltip);
        if response.clicked() {
            let modifiers = ui.input(|input| input.modifiers);
            self.click(position, modifiers);
        }
        if response.double_clicked() {
            self.open_viewer(position);
        }
        if response.secondary_clicked() && !selected {
            self.click(position, Modifiers::NONE);
        }
        response.context_menu(|ui| self.context_menu(ui));
    }

    fn click(&mut self, position: usize, modifiers: Modifiers) {
        let Some(path) = self.entry_at(position).map(|entry| entry.path.clone()) else {
            return;
        };
        if modifiers.shift {
            let anchor = self.anchor.unwrap_or(position);
            if !modifiers.command {
                self.selection.clear();
            }
            let (low, high) = (anchor.min(position), anchor.max(position));
            for index in low..=high {
                if let Some(entry) = self.entry_at(index) {
                    self.selection.insert(entry.path.clone());
                }
            }
        } else if modifiers.command {
            if !self.selection.remove(&path) {
                self.selection.insert(path);
            }
            self.anchor = Some(position);
        } else {
            self.selection.clear();
            self.selection.insert(path);
            self.anchor = Some(position);
        }
        self.focus = Some(position);
        self.rename_plan = None;
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        let item = |ui: &mut egui::Ui, key: &str| ui.button(locale.text(key)).clicked();
        let mut command = None;
        if item(ui, "View") {
            command = Some(Command::OpenViewer);
        }
        if item(ui, "Open in editor") {
            command = Some(Command::OpenInEditor);
        }
        ui.separator();
        if item(ui, "Rotate left") {
            command = Some(Command::RotateLeft);
        }
        if item(ui, "Rotate right") {
            command = Some(Command::RotateRight);
        }
        if item(ui, "Flip horizontally") {
            command = Some(Command::FlipHorizontal);
        }
        ui.separator();
        if item(ui, "Run recipe") {
            command = Some(Command::RunRecipe);
        }
        if item(ui, "Copy to…") {
            command = Some(Command::CopyTo);
        }
        if item(ui, "Move to…") {
            command = Some(Command::MoveTo);
        }
        ui.separator();
        if let Some(path) = self.focus_path() {
            if item(ui, "Show in folder") {
                self.events.push(super::GalleryEvent::Reveal(path.clone()));
                ui.close();
            }
            if item(ui, "Copy path") {
                ui.ctx().copy_text(path.display().to_string());
                ui.close();
            }
        }
        if let Some(command) = command {
            self.execute(command);
            ui.close();
        }
    }

    fn keyboard(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx();
        if ctx.egui_wants_keyboard_input() || self.view.is_empty() {
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::A)) {
            self.execute(Command::SelectAll);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::R)) {
            self.execute(Command::RotateRight);
            return;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::L)) {
            self.execute(Command::RotateLeft);
            return;
        }
        let (step, modifiers, enter, escape, refresh) = ctx.input(|input| {
            let columns = self.columns.max(1).cast_signed();
            let page = columns
                * (self.viewport.1 / (self.settings.thumb_size + 2.0 * PAD + GAP)).max(1.0)
                    as isize;
            let step = if input.key_pressed(Key::ArrowRight) {
                Some(1)
            } else if input.key_pressed(Key::ArrowLeft) {
                Some(-1)
            } else if input.key_pressed(Key::ArrowDown) {
                Some(columns)
            } else if input.key_pressed(Key::ArrowUp) {
                Some(-columns)
            } else if input.key_pressed(Key::PageDown) {
                Some(page)
            } else if input.key_pressed(Key::PageUp) {
                Some(-page)
            } else if input.key_pressed(Key::Home) {
                Some(isize::MIN / 2)
            } else if input.key_pressed(Key::End) {
                Some(isize::MAX / 2)
            } else {
                None
            };
            (
                step,
                input.modifiers,
                input.key_pressed(Key::Enter),
                input.key_pressed(Key::Escape),
                input.key_pressed(Key::F5),
            )
        });
        if refresh {
            self.execute(Command::Refresh);
        }
        if escape {
            self.execute(Command::SelectNone);
        }
        if enter {
            self.execute(Command::OpenViewer);
        }
        let Some(step) = step else {
            return;
        };
        let last = self.view.len() - 1;
        let current = self.focus.unwrap_or(0).cast_signed();
        let target = current
            .saturating_add(step)
            .clamp(0, last.cast_signed())
            .cast_unsigned();
        if modifiers.shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.focus.unwrap_or(0));
            }
            self.click(target, Modifiers::SHIFT);
        } else if modifiers.command {
            self.focus = Some(target);
        } else {
            self.click(target, Modifiers::NONE);
        }
        self.scroll_to_focus = true;
    }

    pub(super) fn status_ui(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        let mut cancel = false;
        ui.horizontal_centered(|ui| {
            let shown = self.view.len();
            let selected = self.selection.len();
            ui.label(locale.format("{} pictures", &[shown.to_string()]));
            if selected > 0 {
                ui.label("·");
                ui.label(locale.format("{} selected", &[selected.to_string()]));
            }
            if self.scanning.is_some() {
                ui.spinner();
                ui.label(locale.text("Reading the folder…"));
            }
            if let Some(running) = &self.job {
                let (done, total) = running.job.progress();
                ui.separator();
                let fraction = if total == 0 {
                    1.0
                } else {
                    done as f32 / total as f32
                };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(220.0)
                        .text(format!("{}: {done} / {total}", locale.text(&running.title))),
                );
                let cancelling = running.job.is_cancelled();
                cancel = ui
                    .add_enabled(!cancelling, egui::Button::new(locale.text("Cancel")))
                    .clicked();
            } else if let Some(download) = &self.download {
                let state = download.state();
                ui.separator();
                let fraction = if state.total == 0 {
                    0.0
                } else {
                    state.received as f32 / state.total as f32
                };
                let mebibytes = |bytes: u64| bytes / (1 << 20);
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(220.0)
                        .text(format!(
                            "{}: {} / {} MiB",
                            locale.text("Model download"),
                            mebibytes(state.received),
                            mebibytes(state.total)
                        )),
                );
                cancel = ui.button(locale.text("Cancel")).clicked();
            } else if let Some(message) = &self.message {
                ui.separator();
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
        });
        if cancel {
            self.execute(Command::CancelJob);
        }
    }
}

/// A light and dark checkerboard behind transparent pictures.
fn checkerboard(painter: &egui::Painter, rect: Rect) {
    const SQUARE: f32 = 8.0;
    painter.rect_filled(rect, 0.0, Color32::from_gray(235));
    let columns = (rect.width() / SQUARE).ceil() as usize;
    let rows = (rect.height() / SQUARE).ceil() as usize;
    let clip = painter.with_clip_rect(rect);
    for row in 0..rows {
        for column in (row % 2..columns).step_by(2) {
            let min = rect.min + vec2(column as f32 * SQUARE, row as f32 * SQUARE);
            clip.rect_filled(
                Rect::from_min_size(min, vec2(SQUARE, SQUARE)),
                0.0,
                Color32::from_gray(205),
            );
        }
    }
}
