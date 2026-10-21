//! Compact MuFoEd-style ribbon with one contextual tool page.
use super::{manual_panel::LayerAction, EditTarget, FileAction, ImageEditor, ManualTool, PANEL};
use crate::{Document, SelectionOp, SelectionRefine};
use egui::{Ui, Vec2};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum RibbonTab {
    #[default]
    Home,
    Image,
    Selection,
    View,
    Tool,
}
pub(super) struct RibbonUi {
    pub tab: RibbonTab,
    pub collapsed: bool,
    pub ai_visible: bool,
    pub selection_op: SelectionOp,
    radius: u32,
    feather: f32,
}
impl Default for RibbonUi {
    fn default() -> Self {
        Self {
            tab: RibbonTab::Home,
            collapsed: false,
            ai_visible: true,
            selection_op: SelectionOp::Replace,
            radius: 4,
            feather: 2.0,
        }
    }
}
fn command(ui: &mut Ui, label: &str) -> bool {
    ui.add_sized([ui.available_width(), 24.0], egui::Button::new(label))
        .clicked()
}
pub(super) fn group(ui: &mut Ui, label: &str, width: f32, body: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.allocate_ui_with_layout(
            Vec2::new(width, 78.0),
            egui::Layout::top_down(egui::Align::Min),
            body,
        );
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(label).small().weak());
        });
    });
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 100.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_top(), rect.left_bottom()],
        egui::Stroke::new(1.0, super::BORDER),
    );
}
impl ImageEditor {
    pub(super) fn ribbon(&mut self, ui: &mut Ui, actions: &mut Vec<FileAction>) {
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::symmetric(12, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (RibbonTab::Home, "Главная"),
                        (RibbonTab::Image, "Изображение"),
                        (RibbonTab::Selection, "Выделение"),
                        (RibbonTab::View, "Вид"),
                        (
                            RibbonTab::Tool,
                            if self.inline_transform() {
                                "Трансформация"
                            } else {
                                self.manual_ui.tool.label()
                            },
                        ),
                    ] {
                        if ui
                            .selectable_label(self.ribbon_ui.tab == tab, label)
                            .clicked()
                        {
                            self.ribbon_ui.tab = tab;
                            self.ribbon_ui.collapsed = false;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(if self.ribbon_ui.collapsed {
                                "Развернуть"
                            } else {
                                "Свернуть"
                            })
                            .clicked()
                        {
                            self.ribbon_ui.collapsed = !self.ribbon_ui.collapsed;
                        }
                        let mut target = self.edit_target();
                        ui.selectable_value(&mut target, EditTarget::Mask, "Маска");
                        ui.selectable_value(&mut target, EditTarget::Raster, "Растр");
                        self.set_edit_target(target);
                    });
                });
                if self.ribbon_ui.collapsed {
                    return;
                }
                ui.separator();
                ui.set_min_height(110.0);
                if self.ribbon_ui.tab == RibbonTab::Tool {
                    if self.inline_transform() {
                        self.transform_ribbon(ui);
                    } else {
                        self.manual_options(ui);
                    }
                    return;
                }
                egui::ScrollArea::horizontal()
                    .id_salt("image-ribbon-scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal_top(|ui| match self.ribbon_ui.tab {
                            RibbonTab::Home => self.ribbon_home(ui, actions),
                            RibbonTab::Image => self.ribbon_image(ui),
                            RibbonTab::Selection => self.ribbon_selection(ui),
                            RibbonTab::View => self.ribbon_view(ui),
                            RibbonTab::Tool => {}
                        });
                    });
            });
    }
    fn ribbon_home(&mut self, ui: &mut Ui, actions: &mut Vec<FileAction>) {
        group(ui, "Файл", 125.0, |ui| {
            ui.menu_button("Открыть", |ui| {
                for (label, action) in [
                    ("Изображение…", FileAction::OpenImage),
                    ("Проект PSD…", FileAction::OpenProject),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(action);
                        ui.close();
                    }
                }
            });
            if command(ui, "Сохранить PSD") {
                actions.push(FileAction::SaveProject);
            }
            if command(ui, "Экспорт PNG") {
                actions.push(FileAction::ExportPng);
            }
        });
        group(ui, "Буфер обмена", 160.0, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Копировать").clicked() {
                    self.copy_pixels(false);
                }
                if ui.button("Вырезать").clicked() {
                    self.cut_pixels();
                }
            });
            ui.menu_button("Вставить", |ui| {
                if self.edit_target() == EditTarget::Mask {
                    if ui.button("В маску · Ctrl+V").clicked() {
                        self.paste_clipboard(false);
                        ui.close();
                    }
                } else {
                    if ui.button("Новым слоем · Ctrl+V").clicked() {
                        self.paste_clipboard(true);
                        ui.close();
                    }
                    if ui.button("В текущий · Ctrl+Shift+V").clicked() {
                        self.paste_clipboard(false);
                        ui.close();
                    }
                }
            });
            if command(ui, "Удалить · Delete") {
                self.delete_selected();
            }
        });
        group(ui, "Правка", 105.0, |ui| {
            if command(ui, "Отменить") {
                self.restore_history(false);
            }
            if command(ui, "Повторить") {
                self.restore_history(true);
            }
        });
        group(ui, "Слой", 150.0, |ui| {
            if command(ui, "Из файла…") {
                actions.push(FileAction::ImportLayer);
            }
            if command(ui, "Новый") {
                self.layer_action(LayerAction::New);
            }
            if command(ui, "Дублировать") {
                self.layer_action(LayerAction::Duplicate(self.document.active()));
            }
        });
        group(ui, "Цвет / покрытие", 245.0, |ui| {
            self.manual_colors(ui);
        });
    }
    fn ribbon_image(&mut self, ui: &mut Ui) {
        group(ui, "Холст", 160.0, |ui| {
            if command(ui, "Размер холста…") {
                self.open_canvas_dialog();
            }
            if command(ui, "Повернуть холст…") {
                self.open_rotation_dialog();
            }
            if command(ui, "Кадрирование") {
                self.select_manual_tool(ManualTool::Crop);
            }
        });
        group(ui, "Активный объект", 185.0, |ui| {
            if command(ui, "Трансформация") {
                self.start_canvas_transform();
            }
            if command(ui, "Отразить по горизонтали") {
                self.flip_target(true);
            }
            if command(ui, "Отразить по вертикали") {
                self.flip_target(false);
            }
        });
        group(ui, "Коррекция", 160.0, |ui| {
            if command(ui, "Цветокоррекция…") {
                self.open_color_dialog();
            }
            if command(ui, "Размытие…") {
                self.ribbon_ui.tab = RibbonTab::View;
                self.manual_properties_open();
            }
        });
        group(ui, "Документ", 165.0, |ui| {
            if command(ui, "Очистить холст") {
                self.edit(Document::clear_canvas);
            }
            if command(ui, "Оставить текущий слой") {
                self.edit(Document::retain_active_layer);
            }
        });
    }
    fn ribbon_selection(&mut self, ui: &mut Ui) {
        group(ui, "Выбор по цвету", 170.0, |ui| {
            if command(ui, "Волшебная палочка · W") {
                self.select_manual_tool(ManualTool::Wand);
            }
            if command(ui, "По всему холсту · Shift+W") {
                self.select_manual_tool(ManualTool::ColorSelect);
            }
            ui.small("Допуск — во вкладке инструмента");
        });
        group(ui, "Область", 160.0, |ui| {
            ui.horizontal(|ui| {
                for (tool, label) in [
                    (ManualTool::Select, "Прямоугольник"),
                    (ManualTool::Lasso, "Лассо"),
                ] {
                    if ui.button(label).clicked() {
                        self.select_manual_tool(tool);
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Всё").clicked() {
                    self.select_all_pixels();
                }
                if ui.button("Снять").clicked() {
                    self.clear_selection();
                }
                if ui.button("Инверсия").clicked() {
                    self.refine_area(SelectionRefine::Invert);
                }
            });
            if command(ui, "Непрозрачные пиксели слоя") {
                self.edit(Document::select_layer_alpha);
            }
        });
        group(ui, "Совмещение · Shift / Alt", 195.0, |ui| {
            for row in [
                [
                    (SelectionOp::Replace, "Заменить"),
                    (SelectionOp::Add, "Добавить"),
                ],
                [
                    (SelectionOp::Subtract, "Вычесть"),
                    (SelectionOp::Intersect, "Пересечь"),
                ],
            ] {
                ui.horizontal(|ui| {
                    for (op, label) in row {
                        ui.selectable_value(&mut self.ribbon_ui.selection_op, op, label);
                    }
                });
            }
        });
        group(ui, "Граница выделения", 245.0, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.ribbon_ui.feather)
                        .range(0.1..=64.0)
                        .suffix(" px"),
                );
                if ui.button("Растушевать").clicked() {
                    self.refine_area(SelectionRefine::Feather(self.ribbon_ui.feather));
                }
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.ribbon_ui.radius)
                        .range(1..=128)
                        .suffix(" px"),
                );
                if ui.button("Расширить").clicked() {
                    self.refine_area(SelectionRefine::Grow(self.ribbon_ui.radius));
                }
                if ui.button("Сжать").clicked() {
                    self.refine_area(SelectionRefine::Shrink(self.ribbon_ui.radius));
                }
            });
        });
        group(ui, "Маска ИИ", 165.0, |ui| {
            if command(ui, "Выделение → маска") {
                self.edit(Document::selection_to_mask);
                self.generation_ui.show_mask = true;
                self.changed = true;
            }
            if command(ui, "Маска → выделение") {
                self.edit(Document::mask_to_selection);
            }
            if command(ui, "Очистить маску") {
                self.edit(|d| {
                    d.clear_mask();
                    Ok(())
                });
            }
        });
    }
    fn ribbon_view(&mut self, ui: &mut Ui) {
        group(ui, "Панели", 190.0, |ui| {
            ui.checkbox(&mut self.ribbon_ui.ai_visible, "Панель ИИ");
            ui.checkbox(&mut self.pixel_grid, "Пиксельная сетка");
            if ui
                .checkbox(&mut self.generation_ui.show_mask, "Показывать маску ИИ")
                .changed()
            {
                self.changed = true;
            }
        });
        group(ui, "Масштаб", 160.0, |ui| {
            if command(ui, "По размеру окна") {
                self.manual_ui.fit_once = true;
            }
            if command(ui, "100%") {
                self.zoom = 1.0;
            }
            if command(ui, "200%") {
                self.zoom = 2.0;
            }
        });
        group(ui, "Приложение", 180.0, |ui| {
            if command(ui, "Настройки…") {
                self.open_settings();
            }
            if command(ui, "Память моделей…") {
                self.open_memory_panel();
            }
        });
    }
    pub(super) fn canvas_context_menu(&mut self, response: &egui::Response) {
        response.context_menu(|ui| {
            ui.label(if self.edit_target() == EditTarget::Mask {
                "Маска"
            } else {
                "Растр"
            });
            if ui.button("Копировать · Ctrl+C").clicked() {
                self.copy_pixels(false);
                ui.close();
            }
            if ui.button("Вырезать · Ctrl+X").clicked() {
                self.cut_pixels();
                ui.close();
            }
            if ui.button("Вставить · Ctrl+V").clicked() {
                self.paste_clipboard(true);
                ui.close();
            }
            if self.edit_target() == EditTarget::Raster
                && ui.button("Вставить в текущий · Ctrl+Shift+V").clicked()
            {
                self.paste_clipboard(false);
                ui.close();
            }
            ui.separator();
            if ui.button("Трансформация · Ctrl+T").clicked() {
                self.start_canvas_transform();
                ui.close();
            }
            if ui.button("Снять выделение · Ctrl+D").clicked() {
                self.clear_selection();
                ui.close();
            }
        });
    }
    fn refine_area(&mut self, refine: SelectionRefine) {
        self.edit(|d| d.refine_selection(refine));
    }
    fn flip_target(&mut self, horizontal: bool) {
        self.edit_manual(|d| d.flip_layer(horizontal));
    }
    pub(super) fn selection_operation(&self, ui: &Ui) -> SelectionOp {
        let mods = ui.input(|i| i.modifiers);
        match (mods.shift, mods.alt) {
            (true, true) => SelectionOp::Intersect,
            (true, false) => SelectionOp::Add,
            (false, true) => SelectionOp::Subtract,
            _ => self.ribbon_ui.selection_op,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ribbon_pages_leave_canvas_and_layers_visible_at_both_window_sizes() {
        for size in [egui::vec2(1440.0, 900.0), egui::vec2(1100.0, 700.0)] {
            for tab in [
                RibbonTab::Home,
                RibbonTab::Image,
                RibbonTab::Selection,
                RibbonTab::View,
                RibbonTab::Tool,
            ] {
                let ctx = egui::Context::default();
                super::super::apply_studio_style(&ctx);
                let mut editor = ImageEditor::new(
                    Document::from_raster(
                        crate::Raster::solid(256, 256, [90, 100, 110, 255]).unwrap(),
                    )
                    .unwrap(),
                );
                editor.ribbon_ui.tab = tab;
                for _ in 0..2 {
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ui| {
                            editor.show(ui);
                        },
                    );
                    let texture = editor.texture.as_ref().unwrap().id();
                    let visible = output
                        .shapes
                        .iter()
                        .filter_map(|shape| {
                            if let egui::Shape::Mesh(mesh) = &shape.shape {
                                if mesh.texture_id == texture {
                                    let bounds =
                                        mesh.vertices.iter().fold(egui::Rect::NOTHING, |r, v| {
                                            r.union(egui::Rect::from_min_max(v.pos, v.pos))
                                        });
                                    return Some(bounds.intersect(shape.clip_rect));
                                }
                            }
                            None
                        })
                        .any(|rect| rect.width() > 120.0 && rect.height() > 200.0);
                    assert!(visible, "canvas hidden at {size:?}, tab {}", tab as u8);
                    let row = ctx
                        .read_response(egui::Id::new(("layer-row", 0_usize)))
                        .expect("layer row");
                    assert!(
                        row.rect.top() > 100.0 && row.rect.bottom() < size.y - 60.0,
                        "layer row clipped: {:?}",
                        row.rect
                    );
                }
            }
        }
    }
}
